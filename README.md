# Dictate

[Download](https://github.com/wolfAC/dictate/releases/latest) · macOS, Windows, Linux

Offline push-to-talk dictation for macOS (Intel and Apple Silicon), Windows and Linux.
Hold a shortcut, speak, release, and the text appears in whatever app you're using.
Speech is transcribed on your computer with [whisper.cpp](https://github.com/ggml-org/whisper.cpp); audio never leaves it.

Built with Tauri 2 (Rust) and React.

## Using it

1. Open Dictate and download a speech model. **Base (English)** is a good start.
2. Hold **Ctrl+Shift**, speak, release. You can change the shortcut (any combination, or modifiers alone),
   or switch to press-to-toggle. Ctrl+Shift with another key, like Ctrl+Shift+T, still works as normal.
3. Voice commands run instead of being typed (turn them off under Output → Voice commands):

   | Say | Does |
   |---|---|
   | "open Chrome", "launch Spotify", "open settings" | Opens the app |
   | "open youtube.com" | Opens the website |
   | "search cats in Chrome", "search for flights to Goa", "google the weather", "look up …" | Google search, in the named browser or the default one |
   | "close Terminal", "quit Spotify" | Asks the app to quit |
   | "kill Terminal", "force quit Chrome" | Force-quits it |

   Open and quit commands only count as short phrases (six words or fewer), and if nothing matches
   the words are typed as usual. Uninstallers and setup tools are never launched by voice.
4. Under **Microphone**, allow access, pick which microphone to use (or keep the system default),
   and use **Test microphone** to check it hears you.
5. Tick **General → Open Dictate at login** to start it quietly in the menu bar when you log in.
6. Dictate checks for a new version when it starts and offers to install it (**General → Updates**).
   Updates install automatically on macOS, Windows and the Linux AppImage; for the `.deb` and `.rpm`,
   download the new package from Releases.
7. Closing the window keeps Dictate running in the tray / menu bar. Use **Quit** from there to exit.

### macOS

- Allow **Microphone** access when asked.
- Allow **Accessibility** access (System Settings → Privacy & Security → Accessibility) so Dictate can paste into other apps.

### Linux

- **X11**: works out of the box.
- **Wayland** (Fedora, Ubuntu 22.04+, and most current desktops): Wayland stops apps from grabbing global
  shortcuts or typing into other windows. To work around that:
  - Bind a shortcut in your desktop's keyboard settings that runs `dictate --toggle`.
    It talks to the running app and starts or stops dictation.
  - Install a typing helper so text is inserted automatically:
    `wtype` on Sway, Hyprland or KDE, or `ydotool` on GNOME (`sudo dnf install ydotool`, then enable `ydotoold`).
    Without one, text is copied to the clipboard for you to paste.

## Online models

Under **Speech model → Online**, Dictate can send recordings to any OpenAI-compatible transcription API
instead of running Whisper locally:

| Provider | Default model | Get a key |
|---|---|---|
| Groq | `whisper-large-v3-turbo` (fast, cheap) | https://console.groq.com/keys |
| OpenAI | `gpt-4o-mini-transcribe` | https://platform.openai.com/api-keys |
| Custom | any | your server's `/v1` base URL |

API keys are stored in the system keychain (macOS Keychain, Windows Credential Manager, Linux Secret
Service), never in the settings file. **Test connection** checks the key and model name. If an online request
fails, for example when you're offline, Dictate falls back to your downloaded local model.

## Models

On first run Dictate picks a local model based on your CPU, core count and memory, and marks it
**Recommended**. On a CPU, Base is the sweet spot for most machines; Small needs 8+ cores (or Apple Silicon).

Models are downloaded from Hugging Face on demand and stored in the app data folder:

| OS | Folder |
|---|---|
| macOS | `~/Library/Application Support/dev.dictate.desktop/models` |
| Windows | `%APPDATA%\dev.dictate.desktop\models` |
| Linux | `~/.local/share/dev.dictate.desktop/models` |

On CPU-only machines, `base.en` transcribes a sentence in about a second; `small.en` is more accurate but a few times slower.

## Development

Requirements: Node 20+, pnpm, Rust (stable), CMake, and a C/C++ compiler.
On Linux also install the [Tauri prerequisites](https://tauri.app/start/prerequisites/) plus ALSA headers
(`alsa-lib-devel` on Fedora, `libasound2-dev` on Debian/Ubuntu).

```sh
pnpm install
pnpm tauri dev                # run the app
pnpm tauri build --config '{"bundle":{"createUpdaterArtifacts":false}}'   # build installers for this OS
```

Release builds also make signed update files, which needs the updater key (`TAURI_SIGNING_PRIVATE_KEY`);
the `--config` override skips them for local builds.

On macOS, sign local builds with a stable identity, or macOS forgets the Accessibility and Microphone
permissions after every rebuild. A self-signed "Dictate Local Signing" certificate in the login keychain works:

```sh
APPLE_SIGNING_IDENTITY="Dictate Local Signing" pnpm tauri build --bundles app \
  --config '{"bundle":{"createUpdaterArtifacts":false}}'
```

Icons are generated from `app-icon.svg` with `pnpm tauri icon app-icon.svg` (then delete the
`android`/`ios` folders it creates). The macOS menu-bar icon is `src-tauri/icons/tray-template.png`.

Tests:

```sh
cd src-tauri
cargo test --lib
# End-to-end transcription test with a real model and a 16 kHz mono WAV:
DICTATE_TEST_MODEL=path/to/ggml-base.en.bin DICTATE_TEST_WAV=path/to/speech.wav \
  cargo test --lib -- --include-ignored
```

## Releases

Download the latest version from [Releases](https://github.com/wolfAC/dictate/releases/latest).

The macOS app is built and signed on the maintainer's Mac, so the signing key never goes to GitHub:

```sh
scripts/release.sh 0.2.0
```

It runs the tests, sets the version in `package.json`, `tauri.conf.json` and `Cargo.toml`, commits and tags `v0.2.0`,
builds the universal (Apple Silicon + Intel) app, makes `Dictate.dmg`, `Dictate.zip`, the update archive
`Dictate.app.tar.gz` and `latest.json`, pushes, and uploads them with `gh release create`. It refuses to run without the `Dictate Local Signing` certificate (or `APPLE_SIGNING_IDENTITY`),
because every release must be signed with the same one or users lose their Accessibility and Microphone permissions.
Back the certificate up (Keychain Access → export as `.p12`) and keep it safe.

Updates are signed with the updater key in `~/.tauri/dictate.key` (public half in `tauri.conf.json`). Installed copies
only accept updates signed with it, so back it up too: if it's lost, users have to reinstall by hand to get a new key.
The release workflow reads the same key from the `TAURI_SIGNING_PRIVATE_KEY` repository secret.

Publishing the release starts `.github/workflows/release.yml`, which builds the Windows (`.msi`, `.exe`) and Linux
(`.deb`, `.rpm`, `.AppImage`) installers and adds them, with their entries in `latest.json`, to the same release. Run that workflow manually to get them as
artifacts without releasing. `.github/workflows/ci.yml` type-checks the UI and runs the tests on macOS, Windows and Linux on every push, and builds
the Windows (`.msi`, `.exe`) and Linux (`.deb`, `.rpm`) installers as workflow artifacts.

`packaging/dictate.rb` is a Homebrew cask template for your own tap; `release.sh` prints the version and sha256 to put in it.

Builds aren't notarized. On macOS, right-click the app and choose **Open** the first time (or run
`xattr -dr com.apple.quarantine /Applications/Dictate.app`); on Windows, choose **More info → Run anyway** in SmartScreen.

## How it works

```
hotkey ─▶ cpal (mic) ─▶ resample to 16 kHz mono ─▶ whisper.cpp ─▶ clipboard + Cmd/Ctrl+V
                                                                  └▶ wtype / ydotool on Wayland
```

| File | Role |
|---|---|
| `src-tauri/src/lib.rs` | App state, hotkey, tray, overlay window, commands for the UI |
| `src-tauri/src/audio.rs` | Microphone capture and resampling |
| `src-tauri/src/transcribe.rs` | Loads the model and runs Whisper |
| `src-tauri/src/output.rs` | Inserts text into the focused app, per platform |
| `src-tauri/src/models.rs` | Model catalog and downloads |
| `src-tauri/src/settings.rs` | Settings persisted as JSON |
| `src/App.tsx` | Settings window |
| `src/Overlay.tsx` | "Listening…" indicator |
