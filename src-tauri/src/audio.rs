use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
        mpsc,
    },
    thread::JoinHandle,
};

use anyhow::{Context, anyhow};
use cpal::{
    FromSample, Sample, SampleFormat, SizedSample,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

/// Whisper expects 16 kHz mono f32 samples.
pub const WHISPER_RATE: u32 = 16_000;

/// Records from the default input device on a dedicated thread, because cpal
/// streams are not `Send` on every platform.
pub struct Recorder {
    stop_tx: mpsc::Sender<()>,
    thread: JoinHandle<()>,
    samples: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    /// Latest loudness (0.0–1.0, see `loudness`) stored as f32 bits, for the live meter.
    level: Arc<AtomicU32>,
}

impl Recorder {
    pub fn start() -> anyhow::Result<Self> {
        let samples = Arc::new(Mutex::new(Vec::with_capacity(WHISPER_RATE as usize * 30)));
        let level = Arc::new(AtomicU32::new(0));
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let (ready_tx, ready_rx) = mpsc::channel::<anyhow::Result<u32>>();

        let thread_samples = samples.clone();
        let thread_level = level.clone();
        let thread = std::thread::spawn(move || {
            let stream = match open_stream(thread_samples, thread_level) {
                Ok((stream, rate)) => {
                    let _ = ready_tx.send(Ok(rate));
                    stream
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            // Block until asked to stop (or the Recorder is dropped), then drop the stream.
            let _ = stop_rx.recv();
            drop(stream);
        });

        let sample_rate = ready_rx
            .recv()
            .map_err(|_| anyhow!("audio thread exited unexpectedly"))??;

        Ok(Self { stop_tx, thread, samples, sample_rate, level })
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    /// Stops recording and returns 16 kHz mono audio.
    pub fn stop(self) -> Vec<f32> {
        let _ = self.stop_tx.send(());
        let _ = self.thread.join();
        let samples = std::mem::take(&mut *self.samples.lock().unwrap());
        resample(&samples, self.sample_rate, WHISPER_RATE)
    }
}

fn open_stream(
    samples: Arc<Mutex<Vec<f32>>>,
    level: Arc<AtomicU32>,
) -> anyhow::Result<(cpal::Stream, u32)> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .context("no microphone found")?;
    let config = device
        .default_input_config()
        .context("could not read microphone config")?;
    let rate = config.sample_rate();
    log::info!(
        "microphone: {} ({} Hz, {} ch, {})",
        device.description().map(|d| d.to_string()).unwrap_or_else(|_| "unknown".into()),
        rate,
        config.channels(),
        config.sample_format()
    );
    let channels = config.channels() as usize;

    let stream = match config.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, config.into(), channels, samples, level),
        SampleFormat::I16 => build::<i16>(&device, config.into(), channels, samples, level),
        SampleFormat::I32 => build::<i32>(&device, config.into(), channels, samples, level),
        SampleFormat::U16 => build::<u16>(&device, config.into(), channels, samples, level),
        SampleFormat::U8 => build::<u8>(&device, config.into(), channels, samples, level),
        SampleFormat::I8 => build::<i8>(&device, config.into(), channels, samples, level),
        other => return Err(anyhow!("unsupported microphone sample format {other}")),
    }?;
    stream.play().context("could not start microphone")?;
    Ok((stream, rate))
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    samples: Arc<Mutex<Vec<f32>>>,
    level: Arc<AtomicU32>,
) -> anyhow::Result<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let stream = device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            let mut sum_sq = 0f32;
            let mut buf = samples.lock().unwrap();
            // Downmix interleaved frames to mono by averaging channels.
            for frame in data.chunks(channels) {
                let sum: f32 = frame.iter().map(|&s| f32::from_sample(s)).sum();
                let mono = sum / channels as f32;
                sum_sq += mono * mono;
                buf.push(mono);
            }
            let frames = (data.len() / channels).max(1);
            let rms = (sum_sq / frames as f32).sqrt();
            level.store(loudness(rms).to_bits(), Ordering::Relaxed);
        },
        |err| log::error!("microphone stream error: {err}"),
        None,
    )?;
    Ok(stream)
}

/// Maps RMS amplitude to 0.0–1.0 on a decibel scale, so quiet speech still moves
/// the meter: -60 dBFS (room noise) is 0, -20 dBFS (loud speech) is 1.
pub fn loudness(rms: f32) -> f32 {
    let db = 20.0 * rms.max(1e-6).log10();
    ((db + 60.0) / 40.0).clamp(0.0, 1.0)
}

/// Loudness of the loudest 50 ms stretch, to tell speech from a silent recording.
/// Whisper invents words like "you" when given silence, so silent clips are skipped.
pub fn peak_loudness(audio: &[f32]) -> f32 {
    audio
        .chunks(WHISPER_RATE as usize / 20)
        .map(|w| loudness((w.iter().map(|s| s * s).sum::<f32>() / w.len() as f32).sqrt()))
        .fold(0.0, f32::max)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MicPermission {
    Granted,
    Denied,
    /// Not asked yet; macOS prompts the first time we record.
    Undetermined,
}

#[cfg(target_os = "macos")]
#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {
    static AVMediaTypeAudio: &'static objc2::runtime::AnyObject;
}

/// macOS hands apps silence, without an error, when microphone access is denied
/// or hasn't been asked for yet.
#[cfg(target_os = "macos")]
pub fn mic_permission() -> MicPermission {
    use objc2::{class, msg_send};

    // AVAuthorizationStatus: 0 not determined, 1 restricted, 2 denied, 3 authorized.
    let status: isize =
        unsafe { msg_send![class!(AVCaptureDevice), authorizationStatusForMediaType: AVMediaTypeAudio] };
    match status {
        3 => MicPermission::Granted,
        0 => MicPermission::Undetermined,
        _ => MicPermission::Denied,
    }
}

/// Shows the macOS microphone permission prompt. Recording alone doesn't trigger it.
#[cfg(target_os = "macos")]
pub fn request_mic_permission() {
    use block2::RcBlock;
    use objc2::{class, msg_send, runtime::Bool};

    let done = RcBlock::new(|granted: Bool| log::info!("microphone permission granted: {}", granted.as_bool()));
    unsafe {
        let _: () = msg_send![
            class!(AVCaptureDevice),
            requestAccessForMediaType: AVMediaTypeAudio,
            completionHandler: &*done
        ];
    }
}

#[cfg(not(target_os = "macos"))]
pub fn mic_permission() -> MicPermission {
    MicPermission::Granted
}

#[cfg(not(target_os = "macos"))]
pub fn request_mic_permission() {}

/// Simple resampler, plenty for speech going into Whisper. Downsampling averages
/// each output window (a crude low-pass that avoids aliasing); upsampling
/// interpolates linearly.
pub fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() {
        return input.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let out_len = (input.len() as f64 / ratio).floor() as usize;
    if ratio > 1.0 {
        return (0..out_len)
            .map(|i| {
                let start = (i as f64 * ratio) as usize;
                let end = (((i + 1) as f64 * ratio) as usize).clamp(start + 1, input.len());
                input[start..end].iter().sum::<f32>() / (end - start) as f32
            })
            .collect();
    }
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * ratio;
            let idx = pos as usize;
            let frac = (pos - idx as f64) as f32;
            let a = input[idx];
            let b = *input.get(idx + 1).unwrap_or(&a);
            a + (b - a) * frac
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_halves_length() {
        let input: Vec<f32> = (0..48_000).map(|i| (i as f32 / 100.0).sin()).collect();
        let out = resample(&input, 48_000, 16_000);
        assert_eq!(out.len(), 16_000);
    }

    #[test]
    fn peak_loudness_finds_speech_in_silence() {
        let mut audio = vec![0.0; 16_000];
        assert_eq!(peak_loudness(&audio), 0.0);
        audio[8_000..8_800].iter_mut().for_each(|s| *s = 0.1); // 50 ms at -20 dB
        assert!(peak_loudness(&audio) > 0.9);
    }

    #[test]
    fn loudness_maps_decibels() {
        assert_eq!(loudness(0.0), 0.0);
        assert_eq!(loudness(0.001), 0.0); // -60 dB
        assert!((loudness(0.01) - 0.5).abs() < 1e-4); // -40 dB
        assert_eq!(loudness(0.5), 1.0);
    }

    #[test]
    fn resample_same_rate_is_identity() {
        let input = vec![0.1, 0.2, 0.3];
        assert_eq!(resample(&input, 16_000, 16_000), input);
    }
}
