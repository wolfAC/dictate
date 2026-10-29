//! Voice commands. Short phrases like these run instead of being typed:
//!   "open Chrome", "open settings", "open youtube.com"
//!   "search cats in Chrome", "search for cheap flights", "google the weather"
//!   "close Terminal", "quit Spotify", "kill Terminal", "force quit Chrome"
//! Anything that doesn't parse, or names an app that isn't installed/running, is typed as usual.

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Open(String),
    OpenUrl(String),
    /// `full_query` keeps any trailing "in X", for when X turns out not to be a browser.
    Search { query: String, browser: Option<String>, full_query: String },
    Quit { app: String, force: bool },
}

/// Open and quit commands longer than this are treated as dictation, so
/// "open the report and send it to Sam" is typed.
const MAX_WORDS: usize = 6;
const MAX_SEARCH_WORDS: usize = 20;
const FILLERS: [&str; 5] = ["up", "the", "app", "application", "please"];

pub fn parse(text: &str) -> Option<Action> {
    // Keep dots inside words so "youtube.com" survives; drop surrounding punctuation.
    let words: Vec<&str> = text
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|w| !w.is_empty())
        .collect();
    let lower: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
    let first = lower.first()?.as_str();
    let second = lower.get(1).map(String::as_str);

    match (first, second) {
        ("search" | "google" | "look", _) => parse_search(&words, &lower),
        ("force", Some("quit" | "close" | "kill")) => quit(&words[2..], true, words.len()),
        ("kill", _) => quit(&words[1..], true, words.len()),
        ("close" | "quit" | "exit", _) => quit(&words[1..], false, words.len()),
        ("open" | "launch" | "start" | "run", _) => {
            let name = target(&words[1..], words.len())?;
            if looks_like_url(&name) {
                let url = if name.contains("://") { name } else { format!("https://{}", name.to_lowercase()) };
                return Some(Action::OpenUrl(url));
            }
            Some(Action::Open(name))
        }
        _ => None,
    }
}

fn quit(rest: &[&str], force: bool, total: usize) -> Option<Action> {
    target(rest, total).map(|app| Action::Quit { app, force })
}

/// The app name after the verb, minus filler words.
fn target(rest: &[&str], total: usize) -> Option<String> {
    if total > MAX_WORDS {
        return None;
    }
    let name: Vec<&str> = rest
        .iter()
        .copied()
        .filter(|w| !FILLERS.contains(&w.to_lowercase().as_str()))
        .collect();
    (!name.is_empty()).then(|| name.join(" "))
}

fn looks_like_url(s: &str) -> bool {
    !s.contains(' ') && s.split('.').count() >= 2 && s.split('.').all(|part| !part.is_empty())
}

/// "search [for] <query> [in|on|using <browser>]", "google <query>", "look up <query>".
fn parse_search(words: &[&str], lower: &[String]) -> Option<Action> {
    if words.len() > MAX_SEARCH_WORDS {
        return None;
    }
    let mut start = 1;
    if lower[0] == "look" {
        if lower.get(1).map(String::as_str) != Some("up") {
            return None;
        }
        start = 2;
    }
    if lower.get(start).map(String::as_str) == Some("for") {
        start += 1;
    }
    let mut end = words.len();
    let mut browser = None;
    // A trailing "in Chrome" / "on Firefox" / "using Safari" (1–3 words) names the browser.
    if let Some(pos) = lower[start..]
        .iter()
        .rposition(|w| ["in", "on", "using", "with"].contains(&w.as_str()))
        .map(|p| p + start)
    {
        if pos > start && words.len() - pos - 1 >= 1 && words.len() - pos - 1 <= 3 {
            browser = Some(words[pos + 1..].join(" "));
            end = pos;
        }
    }
    let query = words[start..end].join(" ");
    let full_query = words[start..].join(" ");
    (!query.is_empty()).then_some(Action::Search { query, browser, full_query })
}

/// A Whisper prompt listing the command words and installed app names, so
/// "kill Chrome" isn't heard as "till Rome".
pub fn recognition_hint() -> String {
    let mut names: Vec<String> = installed_apps().into_iter().map(|a| a.name).collect();
    names.sort();
    names.dedup();
    let mut hint = String::from("Open Chrome. Search for the weather in Chrome. Close Terminal. Kill Safari. Apps:");
    for name in names {
        if hint.len() + name.len() > 700 {
            break; // Whisper only uses roughly the last 220 tokens of a prompt
        }
        hint.push(' ');
        hint.push_str(&name);
        hint.push(',');
    }
    hint
}

// ---------- Matching ----------

fn normalize(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

fn name_words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Similarity from 0 to 1 based on edit distance, to forgive slips like "Spotifi" or "Fire fox".
fn similarity(a: &str, b: &str) -> f32 {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + (ca != cb) as usize).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    1.0 - prev[b.len()] as f32 / a.len().max(b.len()) as f32
}

/// Index of the name that best matches what was said, if any is close enough.
pub fn best_match(query: &str, names: &[String]) -> Option<usize> {
    let q = normalize(query);
    let q_words = name_words(query);
    if q.is_empty() {
        return None;
    }
    names
        .iter()
        .enumerate()
        .filter_map(|(i, name)| {
            let n = normalize(name);
            let words = name_words(name);
            // For partial matches, prefer names mostly covered by what was said, so "chrome"
            // picks "Google Chrome" over "Chrome Remote Desktop Host Uninstaller".
            let coverage = q.len() as f32 / n.len().max(1) as f32;
            let score = if n == q {
                1.0
            } else if n.starts_with(&q) && q.len() >= 3 {
                0.8 + 0.15 * coverage // "Visual Studio" → "Visual Studio Code"
            } else if !q_words.is_empty() && words.windows(q_words.len()).any(|w| w == q_words.as_slice()) {
                0.79 + 0.15 * coverage // "chrome" → "Google Chrome", "settings" → "System Settings"
            } else {
                similarity(&q, &n) * 0.85
            };
            (score >= 0.7).then_some((score, i))
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, i)| i)
}

// ---------- Installed apps ----------

#[derive(Debug, Clone)]
pub struct App {
    pub name: String,
    pub path: PathBuf,
}

pub fn find_installed(query: &str) -> Option<App> {
    let apps = installed_apps();
    let names: Vec<String> = apps.iter().map(|a| a.name.clone()).collect();
    best_match(query, &names).map(|i| apps[i].clone())
}

/// Launches an app, optionally handing it a URL (used to search in a specific browser).
pub fn launch(app: &App, url: Option<&str>) -> anyhow::Result<()> {
    let mut cmd = if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.arg("-a").arg(&app.path);
        c
    } else if cfg!(target_os = "windows") {
        // Start-menu shortcuts (.lnk) launch through the shell.
        let mut c = Command::new("cmd");
        c.args(["/C", "start", ""]).arg(&app.path);
        c
    } else {
        let mut c = Command::new("gio");
        c.arg("launch").arg(&app.path);
        c
    };
    if let Some(url) = url {
        cmd.arg(url);
    }
    let status = cmd.status()?;
    anyhow::ensure!(status.success(), "launcher exited with {status}");
    Ok(())
}

pub fn open_url(url: &str) -> anyhow::Result<()> {
    let status = if cfg!(target_os = "macos") {
        Command::new("open").arg(url).status()?
    } else if cfg!(target_os = "windows") {
        Command::new("explorer").arg(url).status()? // explorer's exit code is unreliable
    } else {
        Command::new("xdg-open").arg(url).status()?
    };
    anyhow::ensure!(status.success() || cfg!(target_os = "windows"), "could not open {url}");
    Ok(())
}

pub fn search_url(query: &str) -> String {
    let encoded: String = query
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b' ' => "+".to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect();
    format!("https://www.google.com/search?q={encoded}")
}

/// Special targets that aren't ordinary installed apps.
pub fn system_target(name: &str) -> Option<&'static str> {
    if cfg!(target_os = "windows") && normalize(name) == "settings" {
        return Some("ms-settings:");
    }
    None
}

/// Never launched by voice: one misheard word shouldn't start an uninstaller.
fn is_unsafe_to_launch(name: &str) -> bool {
    let n = name.to_lowercase();
    ["uninstall", "setup", "installer", "updater", "repair"].iter().any(|w| n.contains(w))
}

pub fn installed_apps() -> Vec<App> {
    let mut apps = installed_apps_unfiltered();
    apps.retain(|a| !is_unsafe_to_launch(&a.name));
    apps
}

fn installed_apps_unfiltered() -> Vec<App> {
    let mut apps = Vec::new();
    if cfg!(target_os = "macos") {
        let home = std::env::var("HOME").unwrap_or_default();
        for dir in [
            "/Applications".into(),
            "/Applications/Utilities".into(),
            "/System/Applications".into(),
            "/System/Applications/Utilities".into(),
            format!("{home}/Applications"),
        ] {
            collect(Path::new(&dir), 1, &mut apps, &|p| file_name_with_extension(p, "app"));
        }
    } else if cfg!(target_os = "windows") {
        for var in ["ProgramData", "APPDATA"] {
            if let Ok(base) = std::env::var(var) {
                let dir = Path::new(&base).join(r"Microsoft\Windows\Start Menu\Programs");
                collect(&dir, 4, &mut apps, &|p| file_name_with_extension(p, "lnk"));
            }
        }
    } else {
        let home = std::env::var("HOME").unwrap_or_default();
        for dir in [
            "/usr/share/applications".into(),
            "/usr/local/share/applications".into(),
            "/var/lib/flatpak/exports/share/applications".into(),
            format!("{home}/.local/share/applications"),
            format!("{home}/.local/share/flatpak/exports/share/applications"),
        ] {
            collect(Path::new(&dir), 2, &mut apps, &desktop_entry_name);
        }
    }
    apps
}

/// Walks `dir` up to `depth` levels, naming entries with `name_of`. Doesn't descend into apps.
fn collect(dir: &Path, depth: usize, apps: &mut Vec<App>, name_of: &dyn Fn(&Path) -> Option<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(name) = name_of(&path) {
            apps.push(App { name, path });
        } else if depth > 1 && path.is_dir() {
            collect(&path, depth - 1, apps, name_of);
        }
    }
}

fn file_name_with_extension(path: &Path, ext: &str) -> Option<String> {
    (path.extension()? == ext).then_some(())?;
    Some(path.file_stem()?.to_string_lossy().into_owned())
}

/// Reads `Name=` from a visible Linux .desktop file.
fn desktop_entry_name(path: &Path) -> Option<String> {
    (path.extension()? == "desktop").then_some(())?;
    let content = std::fs::read_to_string(path).ok()?;
    let entry = content.split("\n[").next()?; // only the [Desktop Entry] group
    if entry.lines().any(|l| l.trim() == "NoDisplay=true" || l.trim() == "Hidden=true") {
        return None;
    }
    entry.lines().find_map(|l| l.strip_prefix("Name=")).map(|n| n.trim().to_string())
}

// ---------- Running apps ----------

/// Quits the running app that best matches `query`. Returns its name, or None if nothing matched.
pub fn quit_app(query: &str, force: bool) -> anyhow::Result<Option<String>> {
    platform::quit_app(query, force)
}

#[cfg(target_os = "macos")]
mod platform {
    use objc2_app_kit::{NSApplicationActivationPolicy, NSWorkspace};

    pub fn quit_app(query: &str, force: bool) -> anyhow::Result<Option<String>> {
        // Only apps with a Dock presence, so "kill finder" can't hit background helpers.
        let apps: Vec<_> = NSWorkspace::sharedWorkspace()
            .runningApplications()
            .iter()
            .filter(|a| a.activationPolicy() == NSApplicationActivationPolicy::Regular)
            .filter_map(|a| Some((a.localizedName()?.to_string(), a)))
            .collect();
        let names: Vec<String> = apps.iter().map(|(n, _)| n.clone()).collect();
        let Some(i) = super::best_match(query, &names) else {
            return Ok(None);
        };
        let (name, app) = &apps[i];
        let ok = if force { app.forceTerminate() } else { app.terminate() };
        anyhow::ensure!(ok, "{name} refused to quit");
        Ok(Some(name.clone()))
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use std::process::Command;

    pub fn quit_app(query: &str, force: bool) -> anyhow::Result<Option<String>> {
        let out = Command::new("tasklist").args(["/FO", "CSV", "/NH"]).output()?;
        let mut names: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| l.split(',').next())
            .map(|n| n.trim_matches('"').trim_end_matches(".exe").to_string())
            .collect();
        names.sort();
        names.dedup();
        let Some(i) = super::best_match(query, &names) else {
            return Ok(None);
        };
        let name = &names[i];
        let mut cmd = Command::new("taskkill");
        cmd.args(["/IM", &format!("{name}.exe"), "/T"]);
        if force {
            cmd.arg("/F");
        }
        anyhow::ensure!(cmd.status()?.success(), "could not close {name}");
        Ok(Some(name.clone()))
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::process::Command;

    pub fn quit_app(query: &str, force: bool) -> anyhow::Result<Option<String>> {
        let out = Command::new("ps").args(["-u", &whoami(), "-o", "comm="]).output()?;
        let mut names: Vec<String> = String::from_utf8_lossy(&out.stdout).lines().map(|l| l.trim().to_string()).collect();
        names.sort();
        names.dedup();
        let Some(i) = super::best_match(query, &names) else {
            return Ok(None);
        };
        let name = &names[i];
        let signal = if force { "-KILL" } else { "-TERM" };
        anyhow::ensure!(
            Command::new("pkill").args([signal, "-x", name]).status()?.success(),
            "could not close {name}"
        );
        Ok(Some(name.clone()))
    }

    fn whoami() -> String {
        std::env::var("USER").unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_open() {
        assert_eq!(parse("Open Safari."), Some(Action::Open("Safari".into())));
        assert_eq!(parse("Open up the Notes app."), Some(Action::Open("Notes".into())));
        assert_eq!(parse("open settings"), Some(Action::Open("settings".into())));
        assert_eq!(parse("Open YouTube.com."), Some(Action::OpenUrl("https://youtube.com".into())));
        assert_eq!(parse("Open."), None);
        assert_eq!(parse("Open the quarterly report and send it to the team"), None);
        assert_eq!(parse("Hello there."), None);
    }

    #[test]
    fn parses_quit() {
        assert_eq!(parse("Kill Terminal."), Some(Action::Quit { app: "Terminal".into(), force: true }));
        assert_eq!(parse("Close Spotify"), Some(Action::Quit { app: "Spotify".into(), force: false }));
        assert_eq!(parse("Force quit Chrome."), Some(Action::Quit { app: "Chrome".into(), force: true }));
    }

    #[test]
    fn parses_search() {
        let search = |text: &str| match parse(text) {
            Some(Action::Search { query, browser, full_query }) => Some((query, browser, full_query)),
            _ => None,
        };
        assert_eq!(
            search("Search cats in Chrome."),
            Some(("cats".into(), Some("Chrome".into()), "cats in Chrome".into()))
        );
        assert_eq!(
            search("Search for cheap flights to Goa."),
            Some(("cheap flights to Goa".into(), None, "cheap flights to Goa".into()))
        );
        assert_eq!(
            search("Google the weather in Chennai on Firefox"),
            Some(("the weather in Chennai".into(), Some("Firefox".into()), "the weather in Chennai on Firefox".into()))
        );
        assert_eq!(search("Look up rust tutorials"), Some(("rust tutorials".into(), None, "rust tutorials".into())));
        assert_eq!(parse("Look at this"), None);
    }

    #[test]
    fn matches_names() {
        let list = names(&[
            "Safari", "Notes", "Visual Studio Code", "Spotify", "System Settings", "Google Chrome",
            "Google Chrome Canary", "Chrome Remote Desktop Host Uninstaller", "Firefox", "Terminal",
            "gnome-terminal-server",
        ]);
        let m = |q: &str| best_match(q, &list).map(|i| list[i].as_str());
        assert_eq!(m("safari"), Some("Safari"));
        assert_eq!(m("Visual Studio"), Some("Visual Studio Code"));
        assert_eq!(m("Spotifi"), Some("Spotify"));
        assert_eq!(m("Fire fox"), Some("Firefox"));
        assert_eq!(m("chrome"), Some("Google Chrome"));
        assert_eq!(m("settings"), Some("System Settings"));
        assert_eq!(m("terminal"), Some("Terminal"));
        assert_eq!(m("the door"), None);
    }

    #[test]
    fn skips_uninstallers() {
        assert!(is_unsafe_to_launch("Chrome Remote Desktop Host Uninstaller"));
        assert!(is_unsafe_to_launch("Docker Setup"));
        assert!(!is_unsafe_to_launch("Google Chrome"));
    }

    #[test]
    fn encodes_search() {
        assert_eq!(search_url("cats & dogs"), "https://www.google.com/search?q=cats+%26+dogs");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn finds_installed_mac_apps() {
        let found = find_installed("safari").expect("Safari");
        assert_eq!(found.name, "Safari");
        assert!(installed_apps().iter().all(|a| !a.name.to_lowercase().contains("uninstall")));
        if std::path::Path::new("/Applications/Google Chrome.app").exists() {
            assert_eq!(find_installed("chrome").unwrap().name, "Google Chrome");
        }
        assert_eq!(find_installed("settings").unwrap().name, "System Settings");
        assert_eq!(find_installed("terminal").unwrap().name, "Terminal");
    }
}
