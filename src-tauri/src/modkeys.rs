//! Modifier-only shortcuts such as "Ctrl+Shift", which OS hotkey APIs can't register.
//! A background thread polls the physical keyboard state instead.

use std::{
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

/// How long the modifiers must be held, with no other key, before dictation starts.
/// Keeps ordinary shortcuts like Ctrl+Shift+T from triggering it.
const HOLD_DELAY: Duration = Duration::from_millis(200);
const POLL_INTERVAL: Duration = Duration::from_millis(15);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

impl Mods {
    /// Parses "Ctrl+Shift" style strings. Returns None if any part is not a modifier.
    pub fn parse(accel: &str) -> Option<Mods> {
        let mut mods = Mods::default();
        for part in accel.split('+') {
            match part.trim().to_ascii_lowercase().as_str() {
                "ctrl" | "control" => mods.ctrl = true,
                "alt" | "option" => mods.alt = true,
                "shift" => mods.shift = true,
                "super" | "cmd" | "command" | "meta" => mods.meta = true,
                _ => return None,
            }
        }
        (mods != Mods::default()).then_some(mods)
    }
}

/// Bitset of non-modifier keys currently down, indexed by platform key code (0–255).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Keys([u64; 4]);

impl Keys {
    fn set(&mut self, code: usize) {
        self.0[code / 64] |= 1 << (code % 64);
    }

    fn and(self, other: Keys) -> Keys {
        Keys(std::array::from_fn(|i| self.0[i] & other.0[i]))
    }

    /// True if any key is down here that isn't in `baseline`.
    fn any_new(self, baseline: Keys) -> bool {
        (0..4).any(|i| self.0[i] & !baseline.0[i] != 0)
    }
}

impl FromIterator<usize> for Keys {
    fn from_iter<I: IntoIterator<Item = usize>>(codes: I) -> Self {
        let mut keys = Keys::default();
        codes.into_iter().for_each(|c| keys.set(c));
        keys
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Pressed,
    Released,
    /// Another key was pressed after dictation started, so it was a regular shortcut.
    Cancelled,
}

/// The shortcut to watch for; `None` disables the watcher.
pub type Target = Arc<Mutex<Option<Mods>>>;

pub fn spawn(target: Target, mut on_event: impl FnMut(Event) + Send + 'static) {
    thread::spawn(move || {
        let Some(mut keyboard) = platform::Keyboard::new() else {
            log::warn!("modifier-only shortcuts are not supported on this system");
            return;
        };
        let mut last_state = (Mods::default(), false);
        // Keys already down before the modifiers were pressed. macOS can report a key as
        // permanently held (seen with key code 0), so only keys pressed later count.
        let mut baseline = Keys::default();
        let mut held_since: Option<Instant> = None;
        let mut active = false;
        // Set when another key joined the chord; wait for a full release before re-arming.
        let mut spoiled = false;

        loop {
            thread::sleep(POLL_INTERVAL);
            let Some(want) = *target.lock().unwrap() else {
                if active {
                    active = false;
                    on_event(Event::Cancelled);
                }
                held_since = None;
                continue;
            };
            let (mods, keys) = keyboard.state();
            if mods == Mods::default() {
                baseline = keys;
            } else {
                baseline = baseline.and(keys); // forget stuck keys once they're released
            }
            let other_key = keys.any_new(baseline);
            if (mods, other_key) != last_state {
                log::debug!("keys: {mods:?}, other key: {other_key}");
                last_state = (mods, other_key);
            }
            let exact = mods == want;

            if active {
                if other_key {
                    active = false;
                    spoiled = true;
                    on_event(Event::Cancelled);
                } else if !exact {
                    active = false;
                    on_event(Event::Released);
                }
                continue;
            }

            if mods == Mods::default() {
                spoiled = false;
                held_since = None;
                continue;
            }
            if spoiled || other_key || !exact {
                spoiled |= other_key;
                held_since = None;
                continue;
            }
            let since = *held_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= HOLD_DELAY {
                active = true;
                on_event(Event::Pressed);
            }
        }
    });
}

#[cfg(target_os = "macos")]
mod platform {
    use super::{Keys, Mods};

    // kCGEventSourceStateHIDSystemState: physical keys only, so our own synthetic
    // Cmd+V paste never looks like the user pressing something.
    const HID_STATE: i32 = 1;
    const FLAG_SHIFT: u64 = 0x0002_0000;
    const FLAG_CTRL: u64 = 0x0004_0000;
    const FLAG_ALT: u64 = 0x0008_0000;
    const FLAG_CMD: u64 = 0x0010_0000;
    // Virtual key codes for modifiers, caps lock and fn, which don't count as "other keys".
    const MODIFIER_KEYCODES: [u16; 10] = [54, 55, 56, 57, 58, 59, 60, 61, 62, 63];

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceFlagsState(state: i32) -> u64;
        fn CGEventSourceKeyState(state: i32, key: u16) -> bool;
    }

    // Reading the keyboard needs the Input Monitoring permission, separate from Accessibility.
    const LISTEN_EVENT: u32 = 1; // kIOHIDRequestTypeListenEvent
    const ACCESS_GRANTED: u32 = 0; // kIOHIDAccessTypeGranted

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOHIDCheckAccess(request: u32) -> u32;
        fn IOHIDRequestAccess(request: u32) -> bool;
    }

    /// Whether Input Monitoring is granted. With `prompt`, asks macOS to show its
    /// permission dialog and add Dictate to the Input Monitoring list.
    pub fn has_listen_permission(prompt: bool) -> bool {
        if unsafe { IOHIDCheckAccess(LISTEN_EVENT) } == ACCESS_GRANTED {
            return true;
        }
        prompt && unsafe { IOHIDRequestAccess(LISTEN_EVENT) }
    }

    pub struct Keyboard;

    impl Keyboard {
        pub fn new() -> Option<Self> {
            let granted = has_listen_permission(true);
            log::info!("input monitoring permission: {granted}");
            Some(Keyboard)
        }

        pub fn state(&mut self) -> (Mods, Keys) {
            let flags = unsafe { CGEventSourceFlagsState(HID_STATE) };
            let mods = Mods {
                ctrl: flags & FLAG_CTRL != 0,
                alt: flags & FLAG_ALT != 0,
                shift: flags & FLAG_SHIFT != 0,
                meta: flags & FLAG_CMD != 0,
            };
            let keys = (0u16..128)
                .filter(|k| !MODIFIER_KEYCODES.contains(k))
                .filter(|&k| unsafe { CGEventSourceKeyState(HID_STATE, k) })
                .map(usize::from)
                .collect();
            (mods, keys)
        }
    }
}

#[cfg(target_os = "macos")]
pub use platform::has_listen_permission;

#[cfg(not(target_os = "macos"))]
pub fn has_listen_permission(_prompt: bool) -> bool {
    true
}

#[cfg(target_os = "windows")]
mod platform {
    use super::{Keys, Mods};

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetAsyncKeyState(vkey: i32) -> i16;
    }

    fn down(vk: i32) -> bool {
        unsafe { GetAsyncKeyState(vk) as u16 & 0x8000 != 0 }
    }

    // Shift/Ctrl/Alt (generic and left/right), Win keys, Caps Lock.
    const MODIFIER_VKS: [i32; 12] = [0x10, 0x11, 0x12, 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0x5B, 0x5C, 0x14];

    pub struct Keyboard;

    impl Keyboard {
        pub fn new() -> Option<Self> {
            Some(Keyboard)
        }

        pub fn state(&mut self) -> (Mods, Keys) {
            let mods = Mods {
                ctrl: down(0x11),
                alt: down(0x12),
                shift: down(0x10),
                meta: down(0x5B) || down(0x5C),
            };
            // 0x01–0x06 are mouse buttons; start at Backspace.
            let keys = (0x08..=0xFE)
                .filter(|vk| !MODIFIER_VKS.contains(vk) && down(*vk))
                .map(|vk| vk as usize)
                .collect();
            (mods, keys)
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::{Keys, Mods};
    use x11rb::{protocol::xproto::ConnectionExt, rust_connection::RustConnection};

    /// X11 only. On Wayland apps can't read global key state; use `dictate --toggle` there.
    pub struct Keyboard {
        conn: RustConnection,
        /// Keycodes for Shift, Lock, Control, Mod1 (Alt), Mod2, Mod3, Mod4 (Super), Mod5.
        modifier_keycodes: Vec<Vec<u8>>,
    }

    impl Keyboard {
        pub fn new() -> Option<Self> {
            if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                return None;
            }
            let (conn, _) = x11rb::connect(None).ok()?;
            let mapping = conn.get_modifier_mapping().ok()?.reply().ok()?;
            let per = mapping.keycodes_per_modifier() as usize;
            let modifier_keycodes = mapping
                .keycodes
                .chunks(per.max(1))
                .map(|c| c.iter().copied().filter(|&k| k != 0).collect())
                .collect();
            Some(Self { conn, modifier_keycodes })
        }

        pub fn state(&mut self) -> (Mods, Keys) {
            let Ok(keys) = self.conn.query_keymap().map_err(|_| ()).and_then(|c| c.reply().map_err(|_| ())) else {
                return (Mods::default(), Keys::default());
            };
            let pressed = |k: u8| keys.keys[k as usize / 8] & (1 << (k % 8)) != 0;
            let group = |i: usize| self.modifier_keycodes.get(i).is_some_and(|ks| ks.iter().any(|&k| pressed(k)));
            let mods = Mods { shift: group(0), ctrl: group(2), alt: group(3), meta: group(6) };
            let all_modifiers: Vec<u8> = self.modifier_keycodes.iter().flatten().copied().collect();
            let down = (8u8..=255)
                .filter(|k| !all_modifiers.contains(k) && pressed(*k))
                .map(usize::from)
                .collect();
            (mods, down)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Mods;

    #[test]
    fn stuck_keys_are_ignored() {
        use super::Keys;
        let stuck: Keys = [0].into_iter().collect();
        let with_t: Keys = [0, 17].into_iter().collect();
        assert!(!stuck.any_new(stuck));
        assert!(with_t.any_new(stuck));
        assert_eq!(with_t.and(stuck), stuck);
    }

    #[test]
    fn parses_modifier_only() {
        assert_eq!(Mods::parse("Ctrl+Shift"), Some(Mods { ctrl: true, shift: true, ..Default::default() }));
        assert_eq!(Mods::parse("Super+Alt"), Some(Mods { meta: true, alt: true, ..Default::default() }));
        assert_eq!(Mods::parse("Ctrl+Shift+Space"), None);
        assert_eq!(Mods::parse(""), None);
    }
}
