use std::{fs, path::Path};

use serde::{Deserialize, Serialize};

use crate::vocab::Replacement;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Record while the hotkey is held down.
    Hold,
    /// Press once to start, again to stop.
    Toggle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Output {
    /// Put the text on the clipboard and send the paste shortcut.
    Paste,
    /// Simulate typing each character.
    Type,
    /// Only copy to the clipboard.
    Clipboard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineKind {
    /// whisper.cpp on this computer.
    Local,
    /// An OpenAI-compatible online API.
    Cloud,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub hotkey: String,
    pub mode: Mode,
    /// `audio::InputDevice` id; empty uses the system default microphone.
    pub microphone: String,
    pub model: String,
    /// Whisper language code, or "auto" to detect.
    pub language: String,
    pub output: Output,
    /// Put the previous clipboard contents back after pasting.
    pub restore_clipboard: bool,
    pub show_overlay: bool,
    /// Treat "open <app>" as a command to launch the app instead of typing it.
    pub voice_commands: bool,
    pub engine: EngineKind,
    /// `cloud::PROVIDERS` id. The API key itself is kept in the system keychain.
    pub cloud_provider: String,
    pub cloud_model: String,
    /// Only used by the "custom" provider.
    pub cloud_base_url: String,
    /// Names and terms the speech model should spell the user's way.
    pub dictionary: Vec<String>,
    /// Spoken phrases that type fixed text instead.
    pub replacements: Vec<Replacement>,
    pub history_enabled: bool,
    /// Delete history older than this many days; 0 keeps it forever.
    pub history_days: u32,
    /// Look for a new version on launch.
    pub check_updates: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: "Ctrl+Shift".into(),
            mode: Mode::Hold,
            microphone: String::new(),
            model: "base.en".into(),
            language: "en".into(),
            output: Output::Paste,
            restore_clipboard: true,
            show_overlay: true,
            voice_commands: true,
            engine: EngineKind::Local,
            cloud_provider: "groq".into(),
            cloud_model: "whisper-large-v3-turbo".into(),
            cloud_base_url: String::new(),
            dictionary: Vec::new(),
            replacements: Vec::new(),
            history_enabled: true,
            history_days: 30,
            check_updates: true,
        }
    }
}

impl Settings {
    /// Returns the saved settings, or None on first run.
    pub fn load(path: &Path) -> Option<Self> {
        fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok())
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}
