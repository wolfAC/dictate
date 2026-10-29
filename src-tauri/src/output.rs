use std::{process::Command, thread, time::Duration};

use anyhow::anyhow;
use enigo::{Direction, Enigo, Key, Keyboard, Settings as EnigoSettings};

use crate::settings::Output;

/// What actually happened, so the UI can tell the user to paste manually if needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Delivered {
    Inserted,
    Copied,
    /// A voice command opened an app; nothing was typed.
    Launched,
}

/// Inserts `text` into the focused app. On macOS this must run on the main thread.
/// Returns the previous clipboard text when it should be restored later.
pub fn insert(
    clipboard: &mut arboard::Clipboard,
    text: &str,
    output: Output,
) -> anyhow::Result<(Delivered, Option<String>)> {
    if output == Output::Clipboard {
        clipboard.set_text(text)?;
        return Ok((Delivered::Copied, None));
    }

    // Wayland blocks synthetic input from ordinary apps; use a helper tool if one is installed.
    if is_wayland() {
        if type_with_wayland_tool(text) {
            return Ok((Delivered::Inserted, None));
        }
        clipboard.set_text(text)?;
        return Ok((Delivered::Copied, None));
    }

    let mut enigo = Enigo::new(&EnigoSettings::default())
        .map_err(|e| anyhow!("could not access the keyboard: {e}"))?;

    match output {
        Output::Type => {
            enigo.text(text)?;
            Ok((Delivered::Inserted, None))
        }
        _ => {
            let previous = clipboard.get_text().ok();
            clipboard.set_text(text)?;
            // Give the clipboard a moment to propagate before pasting.
            thread::sleep(Duration::from_millis(40));
            let modifier = if cfg!(target_os = "macos") { Key::Meta } else { Key::Control };
            enigo.key(modifier, Direction::Press)?;
            enigo.key(Key::Unicode('v'), Direction::Click)?;
            enigo.key(modifier, Direction::Release)?;
            Ok((Delivered::Inserted, previous))
        }
    }
}

fn is_wayland() -> bool {
    cfg!(target_os = "linux")
        && (std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var("XDG_SESSION_TYPE").is_ok_and(|s| s == "wayland"))
}

/// Tries `wtype` (wlroots compositors, KDE) and then `ydotool` (any compositor, needs ydotoold).
fn type_with_wayland_tool(text: &str) -> bool {
    let attempts: [(&str, &[&str]); 2] = [("wtype", &["--"]), ("ydotool", &["type", "--"])];
    attempts.iter().any(|(cmd, args)| {
        Command::new(cmd)
            .args(*args)
            .arg(text)
            .status()
            .is_ok_and(|s| s.success())
    })
}

/// Which Wayland typing helpers are installed, for the settings screen.
pub fn wayland_status() -> Option<Vec<&'static str>> {
    if !is_wayland() {
        return None;
    }
    let found = ["wtype", "ydotool"]
        .into_iter()
        .filter(|cmd| {
            Command::new("which")
                .arg(cmd)
                .output()
                .is_ok_and(|o| o.status.success())
        })
        .collect();
    Some(found)
}

/// macOS needs Accessibility permission to send the paste keystroke.
#[cfg(target_os = "macos")]
pub fn has_input_permission(prompt: bool) -> bool {
    // Creating Enigo checks the permission and optionally shows the system prompt.
    let settings = EnigoSettings { open_prompt_to_get_permissions: prompt, ..Default::default() };
    Enigo::new(&settings).is_ok()
}

#[cfg(not(target_os = "macos"))]
pub fn has_input_permission(_prompt: bool) -> bool {
    true
}
