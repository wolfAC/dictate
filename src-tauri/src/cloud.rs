//! Online transcription through any OpenAI-compatible `/audio/transcriptions` endpoint
//! (OpenAI, Groq, or a custom server). API keys live in the system keychain.

use std::time::Duration;

use anyhow::{Context, anyhow};
use serde::Serialize;

use crate::audio::WHISPER_RATE;

const KEYCHAIN_SERVICE: &str = "dev.dictate.desktop";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: &'static str,
    pub name: &'static str,
    pub base_url: &'static str,
    pub models: &'static [&'static str],
    pub key_url: &'static str,
}

pub const PROVIDERS: &[Provider] = &[
    Provider {
        id: "openai",
        name: "OpenAI",
        base_url: "https://api.openai.com/v1",
        models: &["gpt-4o-mini-transcribe", "gpt-4o-transcribe", "gpt-transcribe", "whisper-1"],
        key_url: "https://platform.openai.com/api-keys",
    },
    Provider {
        id: "groq",
        name: "Groq",
        base_url: "https://api.groq.com/openai/v1",
        models: &["whisper-large-v3-turbo", "whisper-large-v3"],
        key_url: "https://console.groq.com/keys",
    },
    Provider {
        id: "custom",
        name: "Custom (OpenAI-compatible)",
        base_url: "",
        models: &[],
        key_url: "",
    },
];

pub fn provider(id: &str) -> Option<&'static Provider> {
    PROVIDERS.iter().find(|p| p.id == id)
}

// ---------- API keys ----------

fn entry(provider: &str) -> anyhow::Result<keyring::Entry> {
    keyring::Entry::new(KEYCHAIN_SERVICE, provider).map_err(|e| anyhow!("keychain unavailable: {e}"))
}

pub fn save_key(provider: &str, key: &str) -> anyhow::Result<()> {
    entry(provider)?.set_password(key.trim()).map_err(|e| anyhow!("could not save key: {e}"))
}

pub fn load_key(provider: &str) -> Option<String> {
    entry(provider).ok()?.get_password().ok().filter(|k| !k.is_empty())
}

pub fn delete_key(provider: &str) -> anyhow::Result<()> {
    match entry(provider)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(anyhow!("could not delete key: {e}")),
    }
}

// ---------- Requests ----------

fn client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder().timeout(Duration::from_secs(30)).build()?)
}

pub struct Request<'a> {
    pub base_url: &'a str,
    pub key: &'a str,
    pub model: &'a str,
    pub language: &'a str,
    pub prompt: Option<&'a str>,
}

/// Uploads 16 kHz mono audio and returns the transcript.
pub async fn transcribe(req: Request<'_>, audio: &[f32]) -> anyhow::Result<String> {
    let file = reqwest::multipart::Part::bytes(wav(audio))
        .file_name("dictation.wav")
        .mime_str("audio/wav")?;
    let mut form = reqwest::multipart::Form::new()
        .part("file", file)
        .text("model", req.model.to_string())
        .text("response_format", "json");
    if req.language != "auto" {
        form = form.text("language", req.language.to_string());
    }
    if let Some(prompt) = req.prompt {
        form = form.text("prompt", prompt.to_string());
    }

    let url = format!("{}/audio/transcriptions", req.base_url.trim_end_matches('/'));
    let resp = client()?.post(&url).bearer_auth(req.key).multipart(form).send().await?;
    let status = resp.status();
    let body = resp.text().await?;
    if !status.is_success() {
        return Err(anyhow!("{status}: {}", api_error(&body)));
    }
    let json: serde_json::Value = serde_json::from_str(&body).context("unexpected response")?;
    let text = json["text"].as_str().ok_or_else(|| anyhow!("response had no text"))?;
    Ok(crate::transcribe::clean(text))
}

/// Checks the key works and that the provider offers `model`.
pub async fn test(base_url: &str, key: &str, model: &str) -> anyhow::Result<String> {
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let resp = client()?.get(&url).bearer_auth(key).send().await?;
    let status = resp.status();
    let body = resp.text().await?;
    if !status.is_success() {
        return Err(anyhow!("{status}: {}", api_error(&body)));
    }
    let json: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
    let ids: Vec<&str> = json["data"].as_array().into_iter().flatten().filter_map(|m| m["id"].as_str()).collect();
    if ids.is_empty() || ids.contains(&model) {
        Ok(format!("Connected. {model} is ready."))
    } else {
        Err(anyhow!("the key works, but this provider has no model named \"{model}\""))
    }
}

/// Pulls the human-readable message out of an OpenAI-style error body.
fn api_error(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|j| j["error"]["message"].as_str().map(str::to_string))
        .unwrap_or_else(|| body.chars().take(200).collect())
}

/// Encodes 16 kHz mono f32 audio as a 16-bit PCM WAV file.
fn wav(audio: &[f32]) -> Vec<u8> {
    let data_len = (audio.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&WHISPER_RATE.to_le_bytes());
    out.extend_from_slice(&(WHISPER_RATE * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in audio {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_round_trips() {
        let audio: Vec<f32> = (0..1600).map(|i| (i as f32 / 20.0).sin() * 0.5).collect();
        let bytes = wav(&audio);
        let reader = hound::WavReader::new(std::io::Cursor::new(bytes)).unwrap();
        assert_eq!(reader.spec().sample_rate, WHISPER_RATE);
        assert_eq!(reader.spec().channels, 1);
        assert_eq!(reader.len(), 1600);
    }

    /// Hits the real Groq API with a bad key: `cargo test --lib cloud -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn reports_bad_keys() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let err = rt.block_on(test("https://api.groq.com/openai/v1", "bad-key", "whisper-large-v3-turbo")).unwrap_err();
        println!("models error: {err:#}");
        let req = Request {
            base_url: "https://api.groq.com/openai/v1",
            key: "bad-key",
            model: "whisper-large-v3-turbo",
            language: "en",
            prompt: Some("Open Chrome."),
        };
        let err = rt.block_on(transcribe(req, &vec![0.0; 16_000])).unwrap_err();
        println!("transcribe error: {err:#}");
        assert!(err.to_string().contains("401"));
    }

    /// Writes, reads and deletes a throwaway keychain entry.
    #[test]
    #[ignore]
    fn keychain_round_trip() {
        save_key("dictate-test", "secret-123").unwrap();
        assert_eq!(load_key("dictate-test").as_deref(), Some("secret-123"));
        delete_key("dictate-test").unwrap();
        assert_eq!(load_key("dictate-test"), None);
    }

    #[test]
    fn extracts_api_errors() {
        assert_eq!(api_error(r#"{"error":{"message":"Invalid API key"}}"#), "Invalid API key");
        assert_eq!(api_error("plain text"), "plain text");
    }
}
