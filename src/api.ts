import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Mode = "hold" | "toggle";
export type Output = "paste" | "type" | "clipboard";
export type Status = "idle" | "recording" | "transcribing";
export type EngineKind = "local" | "cloud";

export interface Settings {
  hotkey: string;
  mode: Mode;
  /** InputDevice id; empty for the system default. */
  microphone: string;
  model: string;
  language: string;
  output: Output;
  restoreClipboard: boolean;
  showOverlay: boolean;
  voiceCommands: boolean;
  engine: EngineKind;
  cloudProvider: string;
  cloudModel: string;
  cloudBaseUrl: string;
  dictionary: string[];
  replacements: Replacement[];
  historyEnabled: boolean;
  historyDays: number;
  checkUpdates: boolean;
}

export interface InputDevice {
  id: string;
  name: string;
  isDefault: boolean;
}

export interface UpdateInfo {
  version: string;
  notes: string | null;
}

export interface Replacement {
  from: string;
  to: string;
}

export interface HistoryEntry {
  id: number;
  at: number;
  text: string;
  kind: "dictation" | "command";
  model: string;
  durationMs: number;
}

export interface Provider {
  id: string;
  name: string;
  baseUrl: string;
  models: string[];
  keyUrl: string;
}

export interface Recommendation {
  model: string;
  reason: string;
}

export interface Model {
  id: string;
  name: string;
  file: string;
  sizeMb: number;
  englishOnly: boolean;
  description: string;
  downloaded: boolean;
  downloading: boolean;
}

export interface AppState {
  settings: Settings;
  models: Model[];
  status: Status;
  platform: "macos" | "windows" | "linux" | string;
  inputPermission: boolean;
  keyboardPermission: boolean;
  micPermission: "granted" | "denied" | "undetermined";
  recommendation: Recommendation;
  providers: Provider[];
  cloudKeys: string[];
  launchAtLogin: boolean;
  version: string;
  update: UpdateInfo | null;
  waylandTools: string[] | null;
  lastTranscript: string;
}

export interface DownloadProgress {
  id: string;
  downloaded: number;
  total: number;
}

export interface Transcript {
  text: string;
  delivered: "inserted" | "copied" | "launched";
}

export const api = {
  getState: () => invoke<AppState>("get_state"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  downloadModel: (id: string) => invoke<void>("download_model", { id }),
  deleteModel: (id: string) => invoke<void>("delete_model", { id }),
  toggleDictation: () => invoke<void>("toggle_dictation"),
  requestInputPermission: () => invoke<boolean>("request_input_permission"),
  requestKeyboardPermission: () => invoke<boolean>("request_keyboard_permission"),
  openMicSettings: () => invoke<void>("open_mic_settings"),
  requestMicPermission: () => invoke<void>("request_mic_permission"),
  openSoundSettings: () => invoke<void>("open_sound_settings"),
  listMicrophones: () => invoke<InputDevice[]>("list_microphones"),
  startMicTest: () => invoke<void>("start_mic_test"),
  stopMicTest: () => invoke<void>("stop_mic_test"),
  checkUpdate: () => invoke<UpdateInfo | null>("check_update"),
  installUpdate: () => invoke<void>("install_update"),
  saveCloudKey: (provider: string, key: string) => invoke<void>("save_cloud_key", { provider, key }),
  testCloud: () => invoke<string>("test_cloud"),
  setLaunchAtLogin: (enabled: boolean) => invoke<void>("set_launch_at_login", { enabled }),
  getHistory: () => invoke<HistoryEntry[]>("get_history"),
  deleteHistory: (id: number) => invoke<void>("delete_history", { id }),
  clearHistory: () => invoke<void>("clear_history"),
  pauseHotkey: () => invoke<void>("pause_hotkey"),
  resumeHotkey: () => invoke<void>("resume_hotkey"),
};

export const on = {
  status: (cb: (s: Status) => void): Promise<UnlistenFn> => listen<Status>("status", (e) => cb(e.payload)),
  error: (cb: (msg: string) => void) => listen<string>("error", (e) => cb(e.payload)),
  transcript: (cb: (t: Transcript) => void) => listen<Transcript>("transcript", (e) => cb(e.payload)),
  progress: (cb: (p: DownloadProgress) => void) =>
    listen<DownloadProgress>("download-progress", (e) => cb(e.payload)),
  history: (cb: () => void) => listen<null>("history", () => cb()),
  level: (cb: (level: number) => void) => listen<number>("level", (e) => cb(e.payload)),
  micLevel: (cb: (level: number) => void) => listen<number>("mic-level", (e) => cb(e.payload)),
  updateAvailable: (cb: (u: UpdateInfo) => void) => listen<UpdateInfo>("update-available", (e) => cb(e.payload)),
  updateProgress: (cb: (downloaded: number, total: number) => void) =>
    listen<[number, number]>("update-progress", (e) => cb(...e.payload)),
};
