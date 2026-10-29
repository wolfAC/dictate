//! A searchable log of past dictations, kept in `history.json` in the app's data folder.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

/// Oldest entries are dropped beyond this, whatever the retention setting.
const MAX_ENTRIES: usize = 5000;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Dictation,
    /// A voice command such as "open Chrome"; `text` describes what happened.
    Command,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: u64,
    /// Unix time in milliseconds.
    pub at: u64,
    pub text: String,
    pub kind: Kind,
    /// Speech model that produced it, e.g. "Base (English)" or "Groq · whisper-large-v3-turbo".
    pub model: String,
    pub duration_ms: u64,
}

pub struct History {
    path: PathBuf,
    entries: Vec<Entry>, // oldest first
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

impl History {
    /// Loads the history; a missing or unreadable file starts it empty.
    pub fn load(path: PathBuf) -> Self {
        let entries = fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { path, entries }
    }

    /// Newest first.
    pub fn entries(&self) -> Vec<Entry> {
        self.entries.iter().rev().cloned().collect()
    }

    pub fn add(&mut self, text: String, kind: Kind, model: String, duration_ms: u64, keep_days: u32) -> anyhow::Result<()> {
        let at = now_ms();
        // Unique even if two entries land in the same millisecond.
        let id = self.entries.last().map_or(at, |e| at.max(e.id + 1));
        self.entries.push(Entry { id, at, text, kind, model, duration_ms });
        self.prune(keep_days);
        self.save()
    }

    pub fn delete(&mut self, id: u64) -> anyhow::Result<()> {
        self.entries.retain(|e| e.id != id);
        self.save()
    }

    pub fn clear(&mut self) -> anyhow::Result<()> {
        self.entries.clear();
        self.save()
    }

    /// Drops entries older than `keep_days` (0 keeps everything). Returns whether anything changed.
    pub fn prune(&mut self, keep_days: u32) -> bool {
        let before = self.entries.len();
        if keep_days > 0 {
            let cutoff = now_ms().saturating_sub(keep_days as u64 * DAY_MS);
            self.entries.retain(|e| e.at >= cutoff);
        }
        let excess = self.entries.len().saturating_sub(MAX_ENTRIES);
        self.entries.drain(..excess);
        self.entries.len() != before
    }

    pub fn save(&self) -> anyhow::Result<()> {
        write_atomic(&self.path, &serde_json::to_vec(&self.entries)?)
    }
}

/// Writes to a temporary file first so a crash can't leave half a history behind.
fn write_atomic(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dictate-history-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir.join("history.json")
    }

    #[test]
    fn adds_persists_deletes_and_clears() {
        let path = temp_path("crud");
        let mut h = History::load(path.clone());
        h.add("first".into(), Kind::Dictation, "Base".into(), 1200, 0).unwrap();
        h.add("Opened Chrome".into(), Kind::Command, "Base".into(), 800, 0).unwrap();

        let reloaded = History::load(path.clone());
        let entries = reloaded.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].text, "Opened Chrome"); // newest first
        assert_ne!(entries[0].id, entries[1].id);

        h.delete(entries[1].id).unwrap();
        assert_eq!(History::load(path.clone()).entries().len(), 1);
        h.clear().unwrap();
        assert!(History::load(path).entries().is_empty());
    }

    #[test]
    fn prunes_old_entries() {
        let mut h = History::load(temp_path("prune"));
        h.entries.push(Entry {
            id: 1,
            at: now_ms() - 10 * DAY_MS,
            text: "old".into(),
            kind: Kind::Dictation,
            model: String::new(),
            duration_ms: 0,
        });
        h.add("new".into(), Kind::Dictation, String::new(), 0, 7).unwrap();
        let texts: Vec<_> = h.entries().into_iter().map(|e| e.text).collect();
        assert_eq!(texts, ["new"]);
    }
}
