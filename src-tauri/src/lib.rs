mod audio;
mod cloud;
mod commands;
mod history;
mod modkeys;
mod models;
mod output;
mod settings;
mod specs;
mod transcribe;
mod vocab;

use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Mutex, mpsc},
    thread,
    time::Duration,
};

use serde::Serialize;
use tauri::{
    AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, Wry,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_updater::UpdaterExt;

use audio::Recorder;
use output::Delivered;
use settings::{EngineKind, Mode, Settings};
use tauri_plugin_autostart::ManagerExt;
use transcribe::Engine;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Status {
    Idle,
    Recording,
    Transcribing,
}

struct AppState {
    settings: Mutex<Settings>,
    settings_path: PathBuf,
    models_dir: PathBuf,
    status: Mutex<Status>,
    recorder: Mutex<Option<Recorder>>,
    /// Open while the Microphone page shows the test meter.
    mic_test: Mutex<Option<Recorder>>,
    /// Found by the check on launch.
    update: Mutex<Option<UpdateInfo>>,
    engine: Mutex<Engine>,
    clipboard: Mutex<Option<arboard::Clipboard>>,
    downloading: Mutex<HashSet<String>>,
    last_transcript: Mutex<String>,
    history: Mutex<history::History>,
    tray_toggle: Mutex<Option<MenuItem<Wry>>>,
    /// Set when the shortcut is modifier-only (e.g. "Ctrl+Shift"); watched by `modkeys`.
    modifier_hotkey: modkeys::Target,
}

impl AppState {
    fn model_path(&self, id: &str) -> Option<PathBuf> {
        models::find(id).map(|m| models::path(&self.models_dir, m))
    }

    fn current_model_path(&self) -> Option<PathBuf> {
        let id = self.settings.lock().unwrap().model.clone();
        self.model_path(&id).filter(|p| p.exists())
    }
}

#[derive(Clone, Serialize)]
struct UpdateInfo {
    version: String,
    notes: Option<String>,
}

#[derive(Clone, Serialize)]
struct TranscriptEvent {
    text: String,
    delivered: Delivered,
}

fn emit_error(app: &AppHandle, message: impl Into<String>) {
    let message = message.into();
    log::error!("{message}");
    let _ = app.emit("error", message);
}

fn set_status(app: &AppHandle, status: Status) {
    let state = app.state::<AppState>();
    *state.status.lock().unwrap() = status;
    let _ = app.emit("status", status);

    if let Some(item) = state.tray_toggle.lock().unwrap().as_ref() {
        let label = match status {
            Status::Recording => "Stop dictation",
            _ => "Start dictation",
        };
        let _ = item.set_text(label);
        let _ = item.set_enabled(status != Status::Transcribing);
    }
    if let Some(tray) = app.tray_by_id("main") {
        let tooltip = match status {
            Status::Idle => "Dictate",
            Status::Recording => "Dictate — listening…",
            Status::Transcribing => "Dictate — transcribing…",
        };
        let _ = tray.set_tooltip(Some(tooltip));
    }

    let show_overlay = state.settings.lock().unwrap().show_overlay;
    if let Some(overlay) = app.get_webview_window("overlay") {
        if status == Status::Idle || !show_overlay {
            let _ = overlay.hide();
        } else {
            let _ = overlay.show();
        }
    }
}

fn start_recording(app: &AppHandle) {
    let state = app.state::<AppState>();
    if *state.status.lock().unwrap() != Status::Idle {
        log::info!("start ignored: already busy");
        return;
    }
    if audio::mic_permission() == audio::MicPermission::Undetermined {
        audio::request_mic_permission();
        emit_error(app, "Allow microphone access in the macOS prompt, then try again.");
        return;
    }
    if let Err(message) = engine_ready(&state) {
        emit_error(app, message);
        show_settings(app);
        return;
    }
    // Dictation needs the microphone the test meter is holding.
    state.mic_test.lock().unwrap().take();
    let microphone = state.settings.lock().unwrap().microphone.clone();
    match Recorder::start(&microphone) {
        Ok(rec) => {
            *state.recorder.lock().unwrap() = Some(rec);
            log::info!("recording started (microphone permission: {:?})", audio::mic_permission());
            set_status(app, Status::Recording);
            spawn_level_meter(app.clone());
        }
        Err(e) => emit_error(app, format!("Could not start the microphone: {e:#}")),
    }
}

/// Streams the microphone level to the overlay while recording.
fn spawn_level_meter(app: AppHandle) {
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_millis(33)); // ~30 fps; the overlay interpolates to 60
            let state = app.state::<AppState>();
            let level = match state.recorder.lock().unwrap().as_ref() {
                Some(rec) => rec.level(),
                None => break,
            };
            let _ = app.emit_to("overlay", "level", level);
        }
    });
}

fn stop_recording(app: &AppHandle) {
    let state = app.state::<AppState>();
    let Some(rec) = state.recorder.lock().unwrap().take() else {
        return;
    };
    set_status(app, Status::Transcribing);

    let app = app.clone();
    thread::spawn(move || {
        let audio = rec.stop();
        let secs = audio.len() as f32 / audio::WHISPER_RATE as f32;
        let peak = audio::peak_loudness(&audio);
        log::info!("recording stopped: {secs:.1}s, peak loudness {peak:.2}");
        // Below about -54 dB nothing was said (or the mic is muted/blocked).
        if peak < 0.15 && secs >= 0.3 {
            let message = match audio::mic_permission() {
                audio::MicPermission::Granted => {
                    "Didn't hear anything. Check that the right microphone is selected and not muted."
                }
                _ if cfg!(target_os = "windows") => {
                    "Dictate can't hear the microphone. Allow it in Settings → Privacy & security → Microphone."
                }
                _ => "Dictate can't hear the microphone. Allow it in System Settings → Privacy & Security → Microphone.",
            };
            emit_error(&app, message);
            if audio::mic_permission() != audio::MicPermission::Granted {
                show_settings(&app);
            }
            set_status(&app, Status::Idle);
            return;
        }
        let started = std::time::Instant::now();
        let result = transcribe(&app, &audio);
        log::info!("transcribed in {:.1}s: {result:?}", started.elapsed().as_secs_f32());
        let duration_ms = (secs * 1000.0) as u64;
        match result {
            Ok((text, _)) if text.is_empty() => {}
            Ok((text, model)) => {
                // Commands match what was said; replacements only apply to typed text.
                if let Some(message) = run_voice_command(&app, &text) {
                    record(&app, message, history::Kind::Command, model, duration_ms);
                } else {
                    let settings = app.state::<AppState>().settings.lock().unwrap().clone();
                    let text = vocab::apply(&text, &settings.dictionary, &settings.replacements);
                    record(&app, text.clone(), history::Kind::Dictation, model, duration_ms);
                    deliver(&app, text);
                }
            }
            Err(e) => emit_error(&app, format!("Transcription failed: {e:#}")),
        }
        set_status(&app, Status::Idle);
    });
}

/// Whether the selected engine can transcribe right now, or why not.
fn engine_ready(state: &AppState) -> Result<(), &'static str> {
    let settings = state.settings.lock().unwrap().clone();
    match settings.engine {
        EngineKind::Local if state.current_model_path().is_none() => Err("Download a speech model first."),
        EngineKind::Cloud if cloud::load_key(&settings.cloud_provider).is_none() => {
            Err("Add an API key for the online model first.")
        }
        EngineKind::Cloud if settings.cloud_provider == "custom" && settings.cloud_base_url.trim().is_empty() => {
            Err("Enter the server URL for the custom online model.")
        }
        _ => Ok(()),
    }
}

/// Returns the text and a label for the model that produced it.
fn transcribe(app: &AppHandle, audio: &[f32]) -> anyhow::Result<(String, String)> {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().unwrap().clone();
    let command_hint = settings.voice_commands.then(commands::recognition_hint);
    let hint = vocab::hint(&settings.dictionary, command_hint.as_deref());

    if settings.engine == EngineKind::Cloud {
        match transcribe_cloud(&settings, audio, hint.as_deref()) {
            Ok(text) => {
                let provider = cloud::provider(&settings.cloud_provider).map_or("Online", |p| p.name);
                return Ok((text, format!("{provider} · {}", settings.cloud_model)));
            }
            // Offline or the API failed: use the local model if one is downloaded.
            Err(e) if state.current_model_path().is_some() => {
                emit_error(app, format!("Online model failed ({e:#}). Used the local model instead."));
            }
            Err(e) => return Err(e.context("online model failed")),
        }
    }

    let path = state
        .current_model_path()
        .ok_or_else(|| anyhow::anyhow!("model not downloaded"))?;
    let mut engine = state.engine.lock().unwrap();
    engine.load(&path)?;
    let text = engine.transcribe(audio, &settings.language, hint.as_deref())?;
    let model = models::find(&settings.model).map_or(settings.model.clone(), |m| m.name.to_string());
    Ok((text, model))
}

/// Adds an entry to the history, if it's turned on.
fn record(app: &AppHandle, text: String, kind: history::Kind, model: String, duration_ms: u64) {
    let state = app.state::<AppState>();
    let (enabled, keep_days) = {
        let s = state.settings.lock().unwrap();
        (s.history_enabled, s.history_days)
    };
    if !enabled {
        return;
    }
    if let Err(e) = state.history.lock().unwrap().add(text, kind, model, duration_ms, keep_days) {
        log::error!("could not save history: {e:#}");
    }
    let _ = app.emit("history", ());
}

fn transcribe_cloud(settings: &Settings, audio: &[f32], hint: Option<&str>) -> anyhow::Result<String> {
    let key = cloud::load_key(&settings.cloud_provider).ok_or_else(|| anyhow::anyhow!("no API key saved"))?;
    let base_url = cloud_base_url(settings)?;
    let request = cloud::Request {
        base_url: &base_url,
        key: &key,
        model: &settings.cloud_model,
        language: &settings.language,
        prompt: hint,
    };
    tauri::async_runtime::block_on(cloud::transcribe(request, audio))
}

fn cloud_base_url(settings: &Settings) -> anyhow::Result<String> {
    let provider = cloud::provider(&settings.cloud_provider).ok_or_else(|| anyhow::anyhow!("unknown provider"))?;
    let url = if provider.id == "custom" { settings.cloud_base_url.trim() } else { provider.base_url };
    anyhow::ensure!(!url.is_empty(), "no server URL set");
    Ok(url.to_string())
}

fn deliver(app: &AppHandle, text: String) {
    let state = app.state::<AppState>();
    *state.last_transcript.lock().unwrap() = text.clone();
    let settings = state.settings.lock().unwrap().clone();

    // Keyboard simulation must happen on the main thread on macOS.
    let (tx, rx) = mpsc::channel();
    let main_app = app.clone();
    let main_text = text.clone();
    let _ = app.run_on_main_thread(move || {
        let state = main_app.state::<AppState>();
        let mut guard = state.clipboard.lock().unwrap();
        let result = match guard.as_mut() {
            Some(clipboard) => output::insert(clipboard, &main_text, settings.output),
            None => Err(anyhow::anyhow!("clipboard unavailable")),
        };
        let _ = tx.send(result);
    });

    match rx.recv() {
        Ok(Ok((delivered, previous))) => {
            log::info!("text {delivered:?} using {:?}", settings.output);
            let _ = app.emit("transcript", TranscriptEvent { text, delivered });
            if let (true, Some(previous)) = (settings.restore_clipboard, previous) {
                // Wait for the target app to read the clipboard before restoring it.
                thread::sleep(Duration::from_millis(400));
                if let Some(clipboard) = state.clipboard.lock().unwrap().as_mut() {
                    let _ = clipboard.set_text(previous);
                }
            }
        }
        Ok(Err(e)) => emit_error(app, format!("Could not insert text: {e:#}")),
        Err(_) => emit_error(app, "Could not insert text."),
    }
}

/// Runs voice commands like "open Chrome" or "kill Terminal".
/// Returns what happened, or None if the text should be typed instead.
fn run_voice_command(app: &AppHandle, text: &str) -> Option<String> {
    let state = app.state::<AppState>();
    if !state.settings.lock().unwrap().voice_commands {
        return None;
    }
    let action = commands::parse(text)?;
    log::info!("voice command: {action:?}");
    let result: anyhow::Result<Option<String>> = (|| {
        Ok(match action {
            commands::Action::Open(name) => {
                if let Some(target) = commands::system_target(&name) {
                    commands::open_url(target)?;
                    Some(format!("Opened {name}"))
                } else if let Some(found) = commands::find_installed(&name) {
                    commands::launch(&found, None)?;
                    Some(format!("Opened {}", found.name))
                } else {
                    None
                }
            }
            commands::Action::OpenUrl(url) => {
                commands::open_url(&url)?;
                Some(format!("Opened {url}"))
            }
            commands::Action::Search { query, browser, full_query } => {
                let browser = browser.as_deref().and_then(commands::find_installed);
                match browser {
                    Some(b) => {
                        commands::launch(&b, Some(&commands::search_url(&query)))?;
                        Some(format!("Searched “{query}” in {}", b.name))
                    }
                    None => {
                        commands::open_url(&commands::search_url(&full_query))?;
                        Some(format!("Searched “{full_query}”"))
                    }
                }
            }
            commands::Action::Quit { app: name, force } => commands::quit_app(&name, force)?
                .map(|quit| format!("{} {quit}", if force { "Force quit" } else { "Quit" })),
        })
    })();

    match result {
        Ok(Some(message)) => {
            log::info!("{message}");
            *state.last_transcript.lock().unwrap() = message.clone();
            let _ = app.emit("transcript", TranscriptEvent { text: message.clone(), delivered: Delivered::Launched });
            Some(message)
        }
        // Nothing matched (e.g. "open the door"): type it like normal dictation.
        Ok(None) => None,
        Err(e) => {
            let message = format!("Voice command failed: {e:#}");
            emit_error(app, message.clone());
            Some(message)
        }
    }
}

fn toggle_recording(app: &AppHandle) {
    let status = *app.state::<AppState>().status.lock().unwrap();
    match status {
        Status::Idle => start_recording(app),
        Status::Recording => stop_recording(app),
        Status::Transcribing => {}
    }
}

fn on_hotkey(app: &AppHandle, pressed: bool) {
    let mode = app.state::<AppState>().settings.lock().unwrap().mode;
    log::info!("hotkey {} ({mode:?} mode)", if pressed { "pressed" } else { "released" });
    match (mode, pressed) {
        (Mode::Hold, true) => start_recording(app),
        (Mode::Hold, false) => stop_recording(app),
        (Mode::Toggle, true) => toggle_recording(app),
        (Mode::Toggle, false) => {}
    }
}

/// Throws away the current recording without transcribing it.
fn cancel_recording(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.recorder.lock().unwrap().take().is_some() {
        set_status(app, Status::Idle);
    }
}

fn on_modifier_hotkey(app: &AppHandle, event: modkeys::Event) {
    log::info!("modifier shortcut: {event:?}");
    match event {
        modkeys::Event::Pressed => on_hotkey(app, true),
        modkeys::Event::Released => on_hotkey(app, false),
        // Another key joined the chord (e.g. Ctrl+Shift+T), so it was a regular shortcut.
        modkeys::Event::Cancelled => {
            if app.state::<AppState>().settings.lock().unwrap().mode == Mode::Hold {
                cancel_recording(app);
            }
        }
    }
}

/// Modifier-only shortcuts go to the `modkeys` watcher, which OS hotkey APIs can't handle;
/// everything else is registered as a normal global shortcut.
fn register_hotkey(app: &AppHandle, hotkey: &str) -> anyhow::Result<()> {
    let shortcuts = app.global_shortcut();
    shortcuts.unregister_all()?;
    let state = app.state::<AppState>();
    if let Some(mods) = modkeys::Mods::parse(hotkey) {
        *state.modifier_hotkey.lock().unwrap() = Some(mods);
        return Ok(());
    }
    *state.modifier_hotkey.lock().unwrap() = None;
    shortcuts.on_shortcut(hotkey, |app, _shortcut, event| {
        on_hotkey(app, event.state() == ShortcutState::Pressed);
    })?;
    Ok(())
}

/// Loads the selected model in the background so the first dictation is fast.
fn preload_model(app: &AppHandle) {
    let app = app.clone();
    thread::spawn(move || {
        let state = app.state::<AppState>();
        if let Some(path) = state.current_model_path() {
            if let Err(e) = state.engine.lock().unwrap().load(&path) {
                emit_error(&app, format!("{e:#}"));
            }
        }
    });
}

fn show_settings(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

// ---------- Commands for the settings UI ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelView {
    #[serde(flatten)]
    info: models::ModelInfo,
    downloaded: bool,
    downloading: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StateView {
    settings: Settings,
    models: Vec<ModelView>,
    status: Status,
    platform: &'static str,
    input_permission: bool,
    /// macOS Input Monitoring, needed for modifier-only shortcuts like Ctrl+Shift.
    keyboard_permission: bool,
    mic_permission: audio::MicPermission,
    recommendation: specs::Recommendation,
    providers: &'static [cloud::Provider],
    /// Providers that have an API key saved in the keychain.
    cloud_keys: Vec<&'static str>,
    launch_at_login: bool,
    version: String,
    update: Option<UpdateInfo>,
    /// Present only on Wayland: the typing helpers that are installed.
    wayland_tools: Option<Vec<&'static str>>,
    last_transcript: String,
}

/// English-only models are better for English; anything else needs a multilingual one.
fn english_preferred(settings: &Settings) -> bool {
    settings.language == "en"
}

#[tauri::command]
fn get_state(app: AppHandle, state: tauri::State<AppState>) -> StateView {
    let downloading = state.downloading.lock().unwrap().clone();
    // Clone first: a lock guard inside the struct literal would live until the end of the
    // expression and deadlock when the settings are locked again below.
    let settings = state.settings.lock().unwrap().clone();
    let english = english_preferred(&settings);
    StateView {
        settings,
        models: models::MODELS
            .iter()
            .map(|m| ModelView {
                info: m.clone(),
                downloaded: models::path(&state.models_dir, m).exists(),
                downloading: downloading.contains(m.id),
            })
            .collect(),
        status: *state.status.lock().unwrap(),
        platform: std::env::consts::OS,
        input_permission: output::has_input_permission(false),
        keyboard_permission: modkeys::has_listen_permission(false),
        mic_permission: audio::mic_permission(),
        recommendation: specs::recommend(specs::detect(), english),
        providers: cloud::PROVIDERS,
        cloud_keys: cloud::PROVIDERS.iter().map(|p| p.id).filter(|id| cloud::load_key(id).is_some()).collect(),
        launch_at_login: app.autolaunch().is_enabled().unwrap_or(false),
        version: app.package_info().version.to_string(),
        update: state.update.lock().unwrap().clone(),
        wayland_tools: output::wayland_status(),
        last_transcript: state.last_transcript.lock().unwrap().clone(),
    }
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    let state = app.state::<AppState>();
    let old = state.settings.lock().unwrap().clone();

    if settings.hotkey != old.hotkey {
        if let Err(e) = register_hotkey(&app, &settings.hotkey) {
            let _ = register_hotkey(&app, &old.hotkey);
            return Err(format!("Could not use \"{}\" as a shortcut: {e}", settings.hotkey));
        }
    }
    settings.save(&state.settings_path).map_err(|e| e.to_string())?;
    if settings.history_days != old.history_days && state.history.lock().unwrap().prune(settings.history_days) {
        let _ = state.history.lock().unwrap().save();
        let _ = app.emit("history", ());
    }
    let model_changed = settings.model != old.model;
    *state.settings.lock().unwrap() = settings;
    if model_changed {
        preload_model(&app);
    }
    Ok(())
}

#[tauri::command]
fn get_history(state: tauri::State<AppState>) -> Vec<history::Entry> {
    state.history.lock().unwrap().entries()
}

#[tauri::command]
fn delete_history(app: AppHandle, id: u64) -> Result<(), String> {
    app.state::<AppState>().history.lock().unwrap().delete(id).map_err(|e| e.to_string())?;
    let _ = app.emit("history", ());
    Ok(())
}

#[tauri::command]
fn clear_history(app: AppHandle) -> Result<(), String> {
    app.state::<AppState>().history.lock().unwrap().clear().map_err(|e| e.to_string())?;
    let _ = app.emit("history", ());
    Ok(())
}

#[derive(Clone, Serialize)]
struct DownloadProgress {
    id: String,
    downloaded: u64,
    total: u64,
}

#[tauri::command]
async fn download_model(app: AppHandle, id: String) -> Result<(), String> {
    let model = models::find(&id).ok_or("unknown model")?;
    let models_dir = {
        let state = app.state::<AppState>();
        if !state.downloading.lock().unwrap().insert(id.clone()) {
            return Ok(()); // already downloading
        }
        state.models_dir.clone()
    };

    let progress_app = app.clone();
    let result = models::download(&models_dir, model, |downloaded, total| {
        let _ = progress_app.emit(
            "download-progress",
            DownloadProgress { id: id.clone(), downloaded, total },
        );
    })
    .await;

    app.state::<AppState>().downloading.lock().unwrap().remove(&id);
    result.map_err(|e| format!("Download failed: {e:#}"))?;
    if app.state::<AppState>().settings.lock().unwrap().model == id {
        preload_model(&app);
    }
    Ok(())
}

#[tauri::command]
fn delete_model(state: tauri::State<AppState>, id: String) -> Result<(), String> {
    let path = state.model_path(&id).ok_or("unknown model")?;
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn save_cloud_key(provider: String, key: String) -> Result<(), String> {
    if key.trim().is_empty() {
        return cloud::delete_key(&provider).map_err(|e| e.to_string());
    }
    cloud::save_key(&provider, &key).map_err(|e| e.to_string())
}

#[tauri::command]
async fn test_cloud(app: AppHandle) -> Result<String, String> {
    let settings = app.state::<AppState>().settings.lock().unwrap().clone();
    let key = cloud::load_key(&settings.cloud_provider).ok_or("Save an API key first.")?;
    let base_url = cloud_base_url(&settings).map_err(|e| e.to_string())?;
    cloud::test(&base_url, &key, &settings.cloud_model).await.map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn set_launch_at_login(app: AppHandle, enabled: bool) -> Result<(), String> {
    let autolaunch = app.autolaunch();
    let result = if enabled { autolaunch.enable() } else { autolaunch.disable() };
    result.map_err(|e| format!("Could not change the login item: {e}"))
}

#[tauri::command]
fn toggle_dictation(app: AppHandle) {
    toggle_recording(&app);
}

/// Releases the global shortcut while the user records a new one, so the
/// current combination reaches the settings window instead of starting dictation.
#[tauri::command]
fn pause_hotkey(app: AppHandle) -> Result<(), String> {
    *app.state::<AppState>().modifier_hotkey.lock().unwrap() = None;
    app.global_shortcut().unregister_all().map_err(|e| e.to_string())
}

#[tauri::command]
fn resume_hotkey(app: AppHandle) -> Result<(), String> {
    let hotkey = app.state::<AppState>().settings.lock().unwrap().hotkey.clone();
    register_hotkey(&app, &hotkey).map_err(|e| format!("Could not register shortcut \"{hotkey}\": {e}"))
}

#[tauri::command]
fn request_input_permission() -> bool {
    output::has_input_permission(true)
}

#[tauri::command]
fn open_mic_settings(app: AppHandle) {
    let url = match std::env::consts::OS {
        "macos" => "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
        "windows" => "ms-settings:privacy-microphone",
        _ => return,
    };
    let _ = tauri_plugin_opener::OpenerExt::opener(&app).open_url(url, None::<&str>);
}

#[tauri::command]
fn request_mic_permission() {
    audio::request_mic_permission();
}

/// The system's sound settings, where the input volume is set.
#[tauri::command]
fn open_sound_settings(app: AppHandle) -> Result<(), String> {
    let url = match std::env::consts::OS {
        "macos" => "x-apple.systempreferences:com.apple.preference.sound?input",
        "windows" => "ms-settings:sound",
        _ => {
            // Linux has no common settings URL, so try each desktop's sound panel.
            let panels: [&[&str]; 3] =
                [&["gnome-control-center", "sound"], &["systemsettings", "kcm_pulseaudio"], &["pavucontrol"]];
            for panel in panels {
                if std::process::Command::new(panel[0]).args(&panel[1..]).spawn().is_ok() {
                    return Ok(());
                }
            }
            return Err("Couldn't find your desktop's sound settings.".into());
        }
    };
    tauri_plugin_opener::OpenerExt::opener(&app).open_url(url, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_microphones() -> Vec<audio::InputDevice> {
    audio::input_devices()
}

/// Opens the chosen microphone and streams its level to the settings window,
/// so the user can see it hears them without dictating.
#[tauri::command]
fn start_mic_test(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    if *state.status.lock().unwrap() != Status::Idle {
        return Err("Finish dictating first.".into());
    }
    if audio::mic_permission() == audio::MicPermission::Undetermined {
        audio::request_mic_permission();
        return Err("Allow microphone access in the macOS prompt, then try again.".into());
    }
    let microphone = state.settings.lock().unwrap().microphone.clone();
    // Replacing a running test (e.g. after switching microphones) drops the old one.
    let rec = Recorder::start(&microphone).map_err(|e| format!("Could not start the microphone: {e:#}"))?;
    let restart = state.mic_test.lock().unwrap().replace(rec).is_some();
    if !restart {
        let app = app.clone();
        thread::spawn(move || {
            loop {
                thread::sleep(Duration::from_millis(50));
                let state = app.state::<AppState>();
                let level = match state.mic_test.lock().unwrap().as_ref() {
                    Some(rec) => {
                        rec.clear();
                        rec.level()
                    }
                    None => break,
                };
                let _ = app.emit_to("main", "mic-level", level);
            }
        });
    }
    Ok(())
}

#[tauri::command]
fn stop_mic_test(state: tauri::State<AppState>) {
    state.mic_test.lock().unwrap().take();
}

async fn find_update(app: &AppHandle) -> Result<Option<UpdateInfo>, String> {
    let update = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?;
    let info = update.map(|u| UpdateInfo { version: u.version, notes: u.body });
    *app.state::<AppState>().update.lock().unwrap() = info.clone();
    Ok(info)
}

#[tauri::command]
async fn check_update(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    find_update(&app).await.map_err(|e| format!("Could not check for updates: {e}"))
}

/// Downloads and installs the new version, then restarts into it.
#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), String> {
    let fail = |e: tauri_plugin_updater::Error| format!("Update failed: {e}");
    let update = app.updater().map_err(fail)?.check().await.map_err(fail)?.ok_or("Dictate is up to date.")?;
    let mut downloaded = 0u64;
    let progress_app = app.clone();
    update
        .download_and_install(
            |chunk, total| {
                downloaded += chunk as u64;
                let _ = progress_app.emit("update-progress", (downloaded, total.unwrap_or(0)));
            },
            || {},
        )
        .await
        .map_err(fail)?;
    app.restart();
}

#[tauri::command]
fn request_keyboard_permission(app: AppHandle) -> bool {
    let granted = modkeys::has_listen_permission(true);
    if !granted {
        // The prompt only appears once; after that, send the user to the settings page.
        let _ = tauri_plugin_opener::OpenerExt::opener(&app).open_url(
            "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent",
            None::<&str>,
        );
    }
    granted
}

// ---------- Setup ----------

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let toggle = MenuItem::with_id(app, "toggle", "Start dictation", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Dictate", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&toggle, &settings, &PredefinedMenuItem::separator(app)?, &quit],
    )?;
    *app.state::<AppState>().tray_toggle.lock().unwrap() = Some(toggle);

    // macOS menu bar wants a monochrome template it can tint for light and dark mode;
    // other platforms get the full-colour app icon.
    let (icon, template) = if cfg!(target_os = "macos") {
        (tauri::image::Image::from_bytes(include_bytes!("../icons/tray-template.png"))?, true)
    } else {
        (app.default_window_icon().unwrap().clone(), false)
    };
    TrayIconBuilder::with_id("main")
        .icon(icon)
        .icon_as_template(template)
        .tooltip("Dictate")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "toggle" => toggle_recording(app),
            "settings" => show_settings(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

/// A small always-on-top pill that shows while listening. It must never take
/// focus, or the text would be pasted into it instead of the user's app.
fn build_overlay(app: &AppHandle) -> tauri::Result<()> {
    let (width, height) = (260.0, 64.0);
    let overlay = WebviewWindowBuilder::new(app, "overlay", WebviewUrl::App("index.html#overlay".into()))
        .title("Dictate")
        .inner_size(width, height)
        .decorations(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focusable(false)
        .focused(false)
        .visible(false)
        .shadow(false)
        // Transparent so the page can draw a rounded pill.
        .transparent(true)
        .build()?;

    if let Ok(Some(monitor)) = overlay.primary_monitor() {
        let scale = monitor.scale_factor();
        let size = monitor.size().to_logical::<f64>(scale);
        let pos = monitor.position().to_logical::<f64>(scale);
        let _ = overlay.set_position(tauri::LogicalPosition::new(
            pos.x + (size.width - width) / 2.0,
            pos.y + size.height - height - 96.0,
        ));
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Must be first: a second launch forwards its arguments here and exits.
        // `dictate --toggle` lets Wayland users bind dictation to a desktop shortcut.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if args.iter().any(|a| a == "--toggle") {
                toggle_recording(app);
            } else {
                show_settings(app);
            }
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .level_for("whisper_rs", log::LevelFilter::Warn)
                .level_for("dictate_lib::modkeys", log::LevelFilter::Debug)
                .build(),
        )
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        // Login launches start hidden in the tray/menu bar.
        .plugin({
            let autostart = tauri_plugin_autostart::Builder::new().args(["--hidden"]);
            // macos_launcher only exists in macOS builds of the plugin.
            #[cfg(target_os = "macos")]
            let autostart = autostart.macos_launcher(tauri_plugin_autostart::MacosLauncher::LaunchAgent);
            autostart.build()
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let config_dir = app.path().app_config_dir()?;
            let data_dir = app.path().app_data_dir()?;
            let settings_path = config_dir.join("settings.json");
            // First run: start with the model that suits this computer.
            let settings = Settings::load(&settings_path).unwrap_or_else(|| {
                let mut s = Settings::default();
                s.model = specs::recommend(specs::detect(), english_preferred(&s)).model.into();
                s
            });
            let hotkey = settings.hotkey.clone();
            let check_updates = settings.check_updates;
            let mut history = history::History::load(data_dir.join("history.json"));
            if history.prune(settings.history_days) {
                let _ = history.save();
            }

            app.manage(AppState {
                settings: Mutex::new(settings),
                settings_path,
                models_dir: data_dir.join("models"),
                status: Mutex::new(Status::Idle),
                recorder: Mutex::new(None),
                mic_test: Mutex::new(None),
                update: Mutex::new(None),
                engine: Mutex::new(Engine::default()),
                clipboard: Mutex::new(arboard::Clipboard::new().ok()),
                downloading: Mutex::new(HashSet::new()),
                last_transcript: Mutex::new(String::new()),
                history: Mutex::new(history),
                tray_toggle: Mutex::new(None),
                modifier_hotkey: Default::default(),
            });

            let handle = app.handle();
            build_tray(handle)?;
            build_overlay(handle)?;
            let watcher_app = handle.clone();
            let target = handle.state::<AppState>().modifier_hotkey.clone();
            modkeys::spawn(target, move |event| on_modifier_hotkey(&watcher_app, event));
            if let Err(e) = register_hotkey(handle, &hotkey) {
                emit_error(handle, format!("Could not register shortcut \"{hotkey}\": {e}"));
            }
            preload_model(handle);
            if check_updates {
                let app = handle.clone();
                tauri::async_runtime::spawn(async move {
                    match find_update(&app).await {
                        Ok(Some(update)) => {
                            log::info!("update available: {}", update.version);
                            let _ = app.emit("update-available", update);
                        }
                        Ok(None) => {}
                        Err(e) => log::warn!("update check failed: {e}"),
                    }
                });
            }
            // Ask up front, so the first dictation isn't silently empty.
            if audio::mic_permission() == audio::MicPermission::Undetermined {
                audio::request_mic_permission();
            }

            // Start hidden only when launched at login with --hidden.
            if !std::env::args().any(|a| a == "--hidden") {
                show_settings(handle);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the settings window keeps the app running in the tray.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            save_settings,
            get_history,
            delete_history,
            clear_history,
            download_model,
            delete_model,
            toggle_dictation,
            request_input_permission,
            save_cloud_key,
            test_cloud,
            set_launch_at_login,
            request_keyboard_permission,
            open_mic_settings,
            request_mic_permission,
            open_sound_settings,
            list_microphones,
            start_mic_test,
            stop_mic_test,
            check_update,
            install_update,
            pause_hotkey,
            resume_hotkey,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
