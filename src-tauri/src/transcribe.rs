use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::audio::WHISPER_RATE;

/// Keeps the loaded model around between dictations, since loading takes a while.
#[derive(Default)]
pub struct Engine {
    loaded: Option<(PathBuf, WhisperContext)>,
}

impl Engine {
    pub fn is_loaded(&self, model_path: &Path) -> bool {
        matches!(&self.loaded, Some((p, _)) if p == model_path)
    }

    pub fn load(&mut self, model_path: &Path) -> anyhow::Result<()> {
        if self.is_loaded(model_path) {
            return Ok(());
        }
        self.loaded = None; // free the old model before loading the new one
        // Route whisper.cpp's chatty stdout logging into `log` instead.
        whisper_rs::install_logging_hooks();
        let path = model_path
            .to_str()
            .ok_or_else(|| anyhow!("model path is not valid UTF-8"))?;
        let ctx = WhisperContext::new_with_params(path, WhisperContextParameters::default())
            .with_context(|| format!("could not load model {}", model_path.display()))?;
        self.loaded = Some((model_path.to_path_buf(), ctx));
        Ok(())
    }

    /// Transcribes 16 kHz mono audio. `language` is a Whisper code like "en", or "auto".
    /// `prompt` biases recognition toward expected words, such as app names.
    pub fn transcribe(&self, audio: &[f32], language: &str, prompt: Option<&str>) -> anyhow::Result<String> {
        let (_, ctx) = self.loaded.as_ref().ok_or_else(|| anyhow!("no model loaded"))?;

        // Ignore accidental taps: anything under ~0.3 s is not speech.
        if audio.len() < WHISPER_RATE as usize * 3 / 10 {
            return Ok(String::new());
        }
        // whisper.cpp needs at least 1 s of audio; pad short clips with silence.
        let mut audio = audio.to_vec();
        let min_len = WHISPER_RATE as usize * 11 / 10;
        if audio.len() < min_len {
            audio.resize(min_len, 0.0);
        }

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(8);
        params.set_n_threads(threads as i32);
        params.set_language(if language == "auto" { None } else { Some(language) });
        params.set_translate(false);
        params.set_no_context(true);
        params.set_no_timestamps(true);
        params.set_suppress_blank(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        if let Some(prompt) = prompt {
            params.set_initial_prompt(prompt);
        }

        let mut state = ctx.create_state()?;
        state.full(params, &audio)?;

        let text: String = state
            .as_iter()
            .filter_map(|segment| segment.to_str_lossy().ok().map(|s| s.into_owned()))
            .collect();
        Ok(clean(&text))
    }
}

/// Strips Whisper's non-speech markers like "[BLANK_AUDIO]" or "(wind blowing)"
/// and collapses whitespace.
pub fn clean(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth = 0usize;
    for c in text.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_removes_markers() {
        assert_eq!(clean(" [BLANK_AUDIO] "), "");
        assert_eq!(clean(" Hello (coughs) world. "), "Hello world.");
        assert_eq!(clean("Plain  text"), "Plain text");
    }

    /// Run with a model and a 16 kHz mono WAV:
    /// DICTATE_TEST_MODEL=... DICTATE_TEST_WAV=... cargo test -- --ignored
    #[test]
    #[ignore]
    fn transcribes_wav() {
        let model = std::env::var("DICTATE_TEST_MODEL").expect("DICTATE_TEST_MODEL");
        let wav = std::env::var("DICTATE_TEST_WAV").expect("DICTATE_TEST_WAV");
        let mut reader = hound::WavReader::open(wav).unwrap();
        let spec = reader.spec();
        let samples: Vec<f32> = reader
            .samples::<i16>()
            .map(|s| s.unwrap() as f32 / i16::MAX as f32)
            .collect();
        let audio = crate::audio::resample(&samples, spec.sample_rate, WHISPER_RATE);

        let mut engine = Engine::default();
        engine.load(Path::new(&model)).unwrap();
        let text = engine.transcribe(&audio, "en", None).unwrap();
        println!("transcript: {text}");
        assert!(!text.is_empty());
    }
}
