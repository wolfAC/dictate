import { useCallback, useEffect, useId, useRef, useState, type ReactNode } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, on, type AppState, type DownloadProgress, type Model, type Provider, type Settings } from "./api";
import { acceleratorFromEvent, formatAccelerator, heldModifiers } from "./hotkey";
import { Icon, type IconName } from "./icons";
import { Select } from "./Select";
import { DictionaryPage } from "./DictionaryPage";
import { HistoryPage } from "./HistoryPage";
import { Callout, Group, kbd, Row, Segmented, SwitchRow } from "./ui";
import "./App.css";

const LANGUAGES: [string, string][] = [
  ["auto", "Detect automatically"],
  ["en", "English"],
  ["es", "Spanish"],
  ["fr", "French"],
  ["de", "German"],
  ["it", "Italian"],
  ["pt", "Portuguese"],
  ["nl", "Dutch"],
  ["hi", "Hindi"],
  ["ta", "Tamil"],
  ["te", "Telugu"],
  ["ml", "Malayalam"],
  ["kn", "Kannada"],
  ["ar", "Arabic"],
  ["ru", "Russian"],
  ["zh", "Chinese"],
  ["ja", "Japanese"],
  ["ko", "Korean"],
];

const STATUS_TITLE = { idle: "Ready to dictate", recording: "Listening…", transcribing: "Transcribing…" };

type Page = "home" | "history" | "dictionary" | "shortcut" | "speech" | "output" | "general";

const PAGES: { id: Page; title: string; icon: IconName; description: string }[] = [
  { id: "home", title: "Home", icon: "mic", description: "Start dictating and check that everything is set up." },
  { id: "history", title: "History", icon: "history", description: "Everything you've dictated, searchable." },
  { id: "dictionary", title: "Dictionary", icon: "book", description: "Words to spell your way, and phrases that type something else." },
  { id: "shortcut", title: "Shortcut", icon: "keyboard", description: "The keys that start and stop dictation." },
  { id: "speech", title: "Speech", icon: "cpu", description: "Where your speech is turned into text, and in which language." },
  { id: "output", title: "Output", icon: "type", description: "How text reaches the app you're typing in." },
  { id: "general", title: "General", icon: "sliders", description: "Startup and app behaviour." },
];

function savedPage(): Page {
  try {
    const saved = localStorage.getItem("page");
    if (PAGES.some((p) => p.id === saved)) return saved as Page;
  } catch {
    // Fall through to the default page.
  }
  return "home";
}

const OUTPUT_NAME = { paste: "Paste", type: "Typing", clipboard: "Copy only" };

const OUTPUT_HELP = {
  paste: "Fastest. Briefly uses the clipboard.",
  type: "Types each character. Slower, leaves the clipboard alone.",
  clipboard: "Copies the text; you paste it yourself.",
};

export default function App() {
  const [state, setState] = useState<AppState | null>(null);
  const [progress, setProgress] = useState<Record<string, DownloadProgress>>({});
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [page, setPage] = useState<Page>(savedPage);
  const content = useRef<HTMLElement>(null);

  function goTo(next: Page) {
    setPage(next);
    content.current?.scrollTo(0, 0);
    try {
      localStorage.setItem("page", next);
    } catch {
      // Storage can be unavailable; the page just won't be remembered.
    }
  }

  const refresh = useCallback(() => api.getState().then(setState), []);

  useEffect(() => {
    refresh();
    const subs = [
      on.status((status) => setState((s) => (s ? { ...s, status } : s))),
      on.error((msg) => setError(msg)),
      on.progress((p) => setProgress((all) => ({ ...all, [p.id]: p }))),
      on.transcript((t) => {
        setState((s) => (s ? { ...s, lastTranscript: t.text } : s));
        setNotice(t.delivered === "copied" ? "Copied to clipboard — paste it with Ctrl+V." : null);
      }),
    ];
    // Permissions can change while the window is hidden; re-check when it regains focus.
    window.addEventListener("focus", refresh);
    return () => {
      subs.forEach((p) => p.then((unlisten) => unlisten()));
      window.removeEventListener("focus", refresh);
    };
  }, [refresh]);

  // Notices are informational, so they go away on their own; errors stay until dismissed.
  useEffect(() => {
    if (!notice) return;
    const t = setTimeout(() => setNotice(null), 5000);
    return () => clearTimeout(t);
  }, [notice]);

  if (!state) return null;
  const { settings, models } = state;
  const selected = models.find((m) => m.id === settings.model);
  const isMac = state.platform === "macos";
  const englishOnly = settings.engine === "local" && !!selected?.englishOnly;

  async function update(patch: Partial<Settings>) {
    const next = { ...settings, ...patch };
    setState((s) => (s ? { ...s, settings: next } : s));
    try {
      await api.saveSettings(next);
      setError(null);
    } catch (e) {
      setError(String(e));
      refresh();
    }
  }

  async function download(id: string) {
    setState((s) => s && { ...s, models: s.models.map((m) => (m.id === id ? { ...m, downloading: true } : m)) });
    try {
      await api.downloadModel(id);
    } catch (e) {
      setError(String(e));
    }
    setProgress(({ [id]: _, ...rest }) => rest);
    refresh();
  }

  async function remove(id: string) {
    await api.deleteModel(id).catch((e) => setError(String(e)));
    refresh();
  }

  const provider = state.providers.find((p) => p.id === settings.cloudProvider);
  const missingKey = settings.engine === "cloud" && !state.cloudKeys.includes(settings.cloudProvider);
  const setupIssues = [
    settings.engine === "local" && !selected?.downloaded,
    missingKey,
    isMac && !state.inputPermission,
    state.micPermission === "denied",
    isMac && !state.keyboardPermission && !/\+(Key|Digit|F\d|Space)/.test(settings.hotkey),
  ].filter(Boolean).length;

  const pages: Record<Page, ReactNode> = {
    home: (
      <>
        <Hero state={state} onError={setError} onShowHistory={() => goTo("history")} />

        {settings.engine === "local" && !selected?.downloaded && (
          <Callout icon="download" action={<button onClick={() => goTo("speech")}>Choose a model</button>}>
            <strong>Download a speech model to get started.</strong> It runs on this computer, so your audio never
            leaves it.
          </Callout>
        )}

        {missingKey && (
          <Callout tone="warn" action={<button onClick={() => goTo("speech")}>Add key</button>}>
            <strong>Add your {provider?.name ?? "provider"} API key</strong> to use online transcription. Until then,
            Dictate uses your local model.
          </Callout>
        )}

        {isMac && !state.inputPermission && (
          <Callout
            tone="warn"
            action={<button onClick={() => api.requestInputPermission().then(refresh)}>Open Settings</button>}
          >
            <strong>Allow Accessibility access</strong> so Dictate can paste into other apps: System Settings →
            Privacy & Security → Accessibility.
          </Callout>
        )}

        {state.micPermission === "denied" && (
          <Callout tone="warn" action={<button onClick={() => api.openMicSettings()}>Open Settings</button>}>
            <strong>Microphone access is off</strong>, so Dictate only hears silence. Turn it on in System Settings
            → Privacy & Security → Microphone.
          </Callout>
        )}

        {isMac && !state.keyboardPermission && !/\+(Key|Digit|F\d|Space)/.test(settings.hotkey) && (
          <Callout
            tone="warn"
            action={<button onClick={() => api.requestKeyboardPermission().then(refresh)}>Open Settings</button>}
          >
            <strong>Allow Input Monitoring</strong> so Dictate can tell when you hold {kbd(settings.hotkey)}. Then
            quit and reopen Dictate.
          </Callout>
        )}

        {state.waylandTools && <WaylandHelp tools={state.waylandTools} />}

        <div className="tiles">
          <Tile icon="keyboard" title="Shortcut" onClick={() => goTo("shortcut")}>
            {kbd(settings.hotkey)} <span className="muted">· {settings.mode === "hold" ? "Hold" : "Toggle"}</span>
          </Tile>
          <Tile icon={settings.engine === "local" ? "cpu" : "cloud"} title="Speech" onClick={() => goTo("speech")}>
            {settings.engine === "local" ? (selected?.name ?? "No model") : (provider?.name ?? "Online")}
            <span className="muted"> · {LANGUAGES.find(([c]) => c === settings.language)?.[1] ?? settings.language}</span>
          </Tile>
          <Tile icon="type" title="Output" onClick={() => goTo("output")}>
            {OUTPUT_NAME[settings.output]}
            <span className="muted"> · commands {settings.voiceCommands ? "on" : "off"}</span>
          </Tile>
        </div>
      </>
    ),
    history: <HistoryPage settings={settings} update={update} onError={setError} />,
    dictionary: <DictionaryPage settings={settings} update={update} />,
    shortcut: (
      <Group title="Keys">
        <Row label="Shortcut" help="Click, then press new keys. Modifier pairs like Ctrl+Shift work too.">
          <HotkeyInput value={settings.hotkey} onChange={(hotkey) => update({ hotkey })} onError={setError} />
        </Row>
        <Row
          label="Activation"
          help={settings.mode === "hold" ? "Talk while holding the keys." : "Press once to start, again to stop."}
        >
          <Segmented
            label="Activation"
            value={settings.mode}
            options={[
              ["hold", "Hold"],
              ["toggle", "Toggle"],
            ]}
            onChange={(mode) => update({ mode })}
          />
        </Row>
      </Group>
    ),
    speech: (
      <>
        <Group title="Engine">
          <Row
            label="Engine"
            help={
              settings.engine === "local"
                ? "Private and free. Works offline."
                : "Runs on a provider's servers. Needs an API key."
            }
          >
            <Segmented
              label="Engine"
              value={settings.engine}
              options={[
                ["local", "On device"],
                ["cloud", "Online"],
              ]}
              onChange={(engine) => update({ engine })}
            />
          </Row>
          <Row
            label="Language"
            help={englishOnly ? "This model is English-only. Pick a multilingual one for others." : undefined}
          >
            <Select
              label="Language"
              value={settings.language}
              options={LANGUAGES}
              disabled={englishOnly}
              onChange={(language) => update({ language })}
            />
          </Row>
        </Group>
        {settings.engine === "cloud" ? (
          <Group title="Online provider">
            <CloudSettings state={state} update={update} refresh={refresh} onError={setError} />
          </Group>
        ) : (
          <Group title="Models">
            <ModelList
              models={models}
              selectedId={settings.model}
              recommendation={state.recommendation}
              progress={progress}
              onSelect={(m) => update({ model: m.id, language: m.englishOnly ? "en" : settings.language })}
              onDownload={download}
              onDelete={remove}
            />
          </Group>
        )}
      </>
    ),
    output: (
      <>
        <Group title="Text">
          <Row label="Insert text by" help={OUTPUT_HELP[settings.output]}>
            <Segmented
              label="Insert text by"
              value={settings.output}
              options={[
                ["paste", "Paste"],
                ["type", "Typing"],
                ["clipboard", "Copy only"],
              ]}
              onChange={(output) => update({ output })}
            />
          </Row>
          <SwitchRow
            label="Restore clipboard"
            help="Put back what you had copied after pasting."
            checked={settings.restoreClipboard}
            disabled={settings.output !== "paste"}
            onChange={(restoreClipboard) => update({ restoreClipboard })}
          />
          <SwitchRow
            label="Listening indicator"
            help="Show a small voice meter while recording."
            checked={settings.showOverlay}
            onChange={(showOverlay) => update({ showOverlay })}
          />
        </Group>
        <Group title="Voice commands">
          <SwitchRow
            label="Voice commands"
            help="Say “open Chrome”, “search for … in Safari” or “quit Terminal” instead of typing."
            checked={settings.voiceCommands}
            onChange={(voiceCommands) => update({ voiceCommands })}
          />
        </Group>
      </>
    ),
    general: (
      <Group title="Startup">
        <SwitchRow
          label="Open at login"
          help="Starts quietly in the menu bar."
          checked={state.launchAtLogin}
          onChange={async (enabled) => {
            setState((s) => s && { ...s, launchAtLogin: enabled });
            await api.setLaunchAtLogin(enabled).catch((err) => setError(String(err)));
            refresh();
          }}
        />
      </Group>
    ),
  };

  const current = PAGES.find((p) => p.id === page)!;

  return (
    <div className="shell">
      <nav className="sidebar" aria-label="Sections">
        <div className="brand">
          <svg className="logo" viewBox="0 0 32 32" aria-hidden="true">
            <rect width="32" height="32" rx="9" />
            <path d="M9 14v4M12.5 11v10M16 8v16M19.5 11v10M23 14v4" />
          </svg>
          <span>Dictate</span>
        </div>
        <ul className="nav">
          {PAGES.map((p) => (
            <li key={p.id}>
              <button
                className="nav-item"
                aria-current={p.id === page ? "page" : undefined}
                onClick={() => goTo(p.id)}
              >
                <Icon name={p.icon} />
                <span>{p.title}</span>
                {p.id === "home" && setupIssues > 0 && (
                  <span className="nav-count" aria-label={`${setupIssues} setup issues`}>
                    {setupIssues}
                  </span>
                )}
              </button>
            </li>
          ))}
        </ul>
        <div className={`sidebar-status status-${state.status}`} aria-live="polite">
          <span className="dot" />
          {STATUS_TITLE[state.status]}
        </div>
      </nav>

      <main className="content" ref={content}>
        <header className="page-head">
          <h1>{current.title}</h1>
          <p className="muted">{current.description}</p>
        </header>
        {pages[page]}
      </main>

      {(error || notice) && (
        <div className={`toast ${error ? "error" : ""}`} role={error ? "alert" : "status"}>
          <Icon name={error ? "alert" : "check"} />
          <span className="toast-text">{error ?? notice}</span>
          <button className="icon-button" aria-label="Dismiss" onClick={() => (setError(null), setNotice(null))}>
            <Icon name="x" />
          </button>
        </div>
      )}
    </div>
  );
}

function Tile(props: { icon: IconName; title: string; onClick: () => void; children: ReactNode }) {
  return (
    <button className="tile" onClick={props.onClick}>
      <span className="tile-head">
        <Icon name={props.icon} size={14} />
        {props.title}
      </span>
      <span className="tile-body">{props.children}</span>
    </button>
  );
}

/** Big start/stop button with the live status and the last transcript. */
function Hero({ state, onError, onShowHistory }: { state: AppState; onError: (msg: string) => void; onShowHistory: () => void }) {
  const { status, settings, lastTranscript } = state;
  const [copied, setCopied] = useState(false);
  const shortcut = kbd(settings.hotkey);

  async function copy() {
    try {
      await navigator.clipboard.writeText(lastTranscript);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch (e) {
      onError(`Couldn't copy: ${e}`);
    }
  }

  return (
    <section className={`card hero hero-${status}`}>
      <div className="hero-main">
        <button
          className="mic"
          onClick={() => api.toggleDictation()}
          disabled={status === "transcribing"}
          aria-label={status === "recording" ? "Stop dictation" : "Start dictation"}
        >
          <span className={status === "transcribing" ? "spin" : undefined}>
            <Icon name={status === "recording" ? "stop" : status === "transcribing" ? "loader" : "mic"} size={22} />
          </span>
        </button>
        <div aria-live="polite">
          <div className="hero-title">{STATUS_TITLE[status]}</div>
          <p className="muted small">
            {settings.mode === "hold" ? (
              <>Hold {shortcut} anywhere, speak, then let go.</>
            ) : (
              <>Press {shortcut} to start, and again to stop.</>
            )}
          </p>
        </div>
      </div>
      {lastTranscript && (
        <div className="last">
          <div className="last-head">
            <span className="eyebrow">Last dictation</span>
            <span className="last-actions">
              <button className="text-button" onClick={copy}>
                <Icon name={copied ? "check" : "copy"} size={14} />
                {copied ? "Copied" : "Copy"}
              </button>
              <button className="text-button" onClick={onShowHistory}>
                <Icon name="history" size={14} />
                History
              </button>
            </span>
          </div>
          <p className="transcript">{lastTranscript}</p>
        </div>
      )}
    </section>
  );
}

function ModelList(props: {
  models: Model[];
  selectedId: string;
  recommendation: AppState["recommendation"];
  progress: Record<string, DownloadProgress>;
  onSelect: (m: Model) => void;
  onDownload: (id: string) => void;
  onDelete: (id: string) => void;
}) {
  const [showWhy, setShowWhy] = useState(false);
  // Deleting throws away a large download, so it takes a second click to confirm.
  const [confirming, setConfirming] = useState<string | null>(null);

  return (
    <div className="models" role="radiogroup" aria-label="Speech model">
      {props.models.map((m) => {
        const p = props.progress[m.id];
        const pct = p ? Math.round((p.downloaded / p.total) * 100) : 0;
        const isSelected = m.id === props.selectedId;
        const recommended = m.id === props.recommendation.model;
        return (
          <div key={m.id} className={`model ${isSelected ? "selected" : ""}`}>
            <label className="model-pick">
              <input type="radio" name="model" checked={isSelected} onChange={() => props.onSelect(m)} />
              <span className="model-text">
                <span className="model-name">
                  {m.name}
                  <span className="muted model-size">{m.sizeMb} MB</span>
                  {recommended && (
                    <span className="badge">
                      Recommended
                      <button
                        type="button"
                        className="badge-info"
                        aria-label="Why is this recommended?"
                        aria-expanded={showWhy}
                        onClick={(e) => {
                          // The badge sits inside the radio's label; don't select the model.
                          e.preventDefault();
                          e.stopPropagation();
                          setShowWhy((v) => !v);
                        }}
                      >
                        <Icon name="info" size={13} />
                      </button>
                    </span>
                  )}
                </span>
                <span className="help">{m.description}</span>
                {recommended && showWhy && <span className="recommendation">{props.recommendation.reason}</span>}
              </span>
            </label>
            <div className="model-action">
              {m.downloading ? (
                <div className="progress-wrap">
                  <div
                    className="progress"
                    role="progressbar"
                    aria-label={`Downloading ${m.name}`}
                    aria-valuenow={pct}
                    aria-valuemin={0}
                    aria-valuemax={100}
                  >
                    <div style={{ transform: `scaleX(${pct / 100})` }} />
                  </div>
                  <span className="small muted tabular">{pct}%</span>
                </div>
              ) : m.downloaded ? (
                confirming === m.id ? (
                  <button
                    className="danger"
                    autoFocus
                    onClick={() => (setConfirming(null), props.onDelete(m.id))}
                    onBlur={() => setConfirming(null)}
                  >
                    Delete {m.sizeMb} MB?
                  </button>
                ) : (
                  <button className="icon-button" aria-label={`Delete ${m.name}`} title="Delete" onClick={() => setConfirming(m.id)}>
                    <Icon name="trash" />
                  </button>
                )
              ) : (
                <button onClick={() => props.onDownload(m.id)}>
                  <Icon name="download" size={14} />
                  Download
                </button>
              )}
            </div>
          </div>
        );
      })}
    </div>
  );
}

/** Online transcription: provider, model, server URL (custom) and API key. */
function CloudSettings(props: {
  state: AppState;
  update: (patch: Partial<Settings>) => Promise<void>;
  refresh: () => Promise<void>;
  onError: (msg: string) => void;
}) {
  const { state, update, refresh, onError } = props;
  const { settings } = state;
  const provider: Provider | undefined = state.providers.find((p) => p.id === settings.cloudProvider);
  const hasKey = state.cloudKeys.includes(settings.cloudProvider);
  const [key, setKey] = useState("");
  const [test, setTest] = useState<{ ok: boolean; text: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const ids = useId();

  async function saveKey() {
    try {
      await api.saveCloudKey(settings.cloudProvider, key);
      setKey("");
      setTest(null);
      await refresh(); // re-read which providers have a saved key
    } catch (e) {
      onError(String(e));
    }
  }

  async function runTest() {
    setBusy(true);
    setTest(null);
    try {
      setTest({ ok: true, text: await api.testCloud() });
    } catch (e) {
      setTest({ ok: false, text: String(e) });
    }
    setBusy(false);
  }

  return (
    <div className="cloud">
      <div className="field">
        <label htmlFor={`${ids}-provider`}>Provider</label>
        <Select
          id={`${ids}-provider`}
          value={settings.cloudProvider}
          options={state.providers.map((p) => [p.id, p.name] as [string, string])}
          onChange={(cloudProvider) => {
            const next = state.providers.find((p) => p.id === cloudProvider);
            setTest(null);
            update({ cloudProvider, cloudModel: next?.models[0] ?? settings.cloudModel });
          }}
        />
      </div>

      {settings.cloudProvider === "custom" && (
        <div className="field">
          <label htmlFor={`${ids}-url`}>Server URL</label>
          <input
            id={`${ids}-url`}
            type="url"
            placeholder="https://example.com/v1"
            defaultValue={settings.cloudBaseUrl}
            onBlur={(e) => update({ cloudBaseUrl: e.target.value.trim() })}
          />
          <span className="help">Any server with an OpenAI-style /audio/transcriptions endpoint.</span>
        </div>
      )}

      <div className="field">
        <label htmlFor={`${ids}-model`}>Model</label>
        <input
          id={`${ids}-model`}
          list="cloud-models"
          defaultValue={settings.cloudModel}
          key={settings.cloudProvider}
          onBlur={(e) => update({ cloudModel: e.target.value.trim() })}
        />
        <datalist id="cloud-models">
          {provider?.models.map((m) => <option key={m} value={m} />)}
        </datalist>
      </div>

      <div className="field">
        <label htmlFor={`${ids}-key`}>
          API key
          {hasKey && (
            <span className="saved">
              <Icon name="check" size={12} /> Saved in Keychain
            </span>
          )}
        </label>
        <div className="input-row">
          <input
            id={`${ids}-key`}
            type="password"
            placeholder={hasKey ? "Enter a new key to replace it" : "Paste your API key"}
            value={key}
            onChange={(e) => setKey(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && key.trim() && saveKey()}
            autoComplete="off"
          />
          <button className="primary" onClick={saveKey} disabled={!key.trim()}>
            Save
          </button>
          {hasKey && (
            <button className="ghost" onClick={() => api.saveCloudKey(settings.cloudProvider, "").then(refresh)}>
              Remove
            </button>
          )}
        </div>
        {provider?.keyUrl && (
          <button className="link" onClick={() => openUrl(provider.keyUrl)}>
            Get {/^[AEIOU]/i.test(provider.name) ? "an" : "a"} {provider.name} API key <Icon name="external" size={12} />
          </button>
        )}
      </div>

      <div className="test-row">
        <button onClick={runTest} disabled={!hasKey || busy}>
          {busy && (
            <span className="spin">
              <Icon name="loader" size={14} />
            </span>
          )}
          {busy ? "Testing…" : "Test connection"}
        </button>
        {test && (
          <span className={`result ${test.ok ? "ok" : "bad"}`} role="status">
            <Icon name={test.ok ? "check" : "alert"} size={14} />
            {test.text}
          </span>
        )}
      </div>

      <p className="privacy">
        <Icon name="shield" size={14} />
        <span>
          Recordings are sent to {provider?.name ?? "the provider"}. If a request fails, Dictate uses your downloaded
          local model instead.
        </span>
      </p>
    </div>
  );
}

/**
 * Click to record a new shortcut. While recording, the current global shortcut is
 * paused so it can't swallow the keys, and held modifiers are shown live.
 * Escape, clicking elsewhere, or leaving the window cancels.
 */
function HotkeyInput(props: {
  value: string;
  onChange: (accel: string) => Promise<void>;
  onError: (msg: string) => void;
}) {
  const ref = useRef<HTMLButtonElement>(null);
  const [capturing, setCapturing] = useState(false);
  const [held, setHeld] = useState("");
  const [hint, setHint] = useState<string | null>(null);
  // Keep the latest callbacks without re-running the capture effect on every render.
  const latest = useRef(props);
  latest.current = props;

  useEffect(() => {
    if (!capturing) return;
    let done = false;
    // Most modifiers held at once, so releasing Ctrl+Shift can save a modifier-only shortcut.
    let peak = "";
    const finish = async (accel: string | null) => {
      if (done) return;
      done = true;
      setCapturing(false);
      setHeld("");
      setHint(null);
      if (accel) await latest.current.onChange(accel);
      await api.resumeHotkey().catch((e) => latest.current.onError(String(e)));
    };

    const onKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.code === "Escape") return void finish(null);
      const accel = acceleratorFromEvent(e);
      if (accel) return void finish(accel);
      const mods = heldModifiers(e);
      if (mods.split("+").length > peak.split("+").length) peak = mods;
      setHeld(mods);
      // A plain key was pressed without a modifier: explain why nothing happened.
      if (!e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey && e.key.length === 1) {
        setHint("Add Ctrl, Alt, Shift or ⌘, or use an F-key.");
      }
    };
    const onKeyUp = (e: KeyboardEvent) => {
      const mods = heldModifiers(e);
      setHeld(mods);
      // All keys released after holding two or more modifiers: use them alone.
      if (!mods && peak.includes("+")) void finish(peak);
    };
    const onPointerDown = (e: PointerEvent) => {
      if (!ref.current?.contains(e.target as Node)) void finish(null);
    };
    const onBlur = () => void finish(null);

    api.pauseHotkey().catch(() => {});
    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    window.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
      window.removeEventListener("pointerdown", onPointerDown, true);
      window.removeEventListener("blur", onBlur);
      void finish(null); // unmounted mid-capture: make sure the shortcut comes back
    };
  }, [capturing]);

  let label = formatAccelerator(props.value);
  if (capturing) label = held ? `${formatAccelerator(held)} …` : "Press keys…";

  return (
    <div className="hotkey-field">
      <button
        ref={ref}
        className={`hotkey ${capturing ? "capturing" : ""}`}
        aria-label={capturing ? "Recording a shortcut. Press Escape to cancel." : `Shortcut ${label}. Click to change.`}
        // Always start (never toggle) so a stray second click can't silently stop recording.
        onClick={() => {
          setCapturing(true);
          ref.current?.focus();
        }}
      >
        {label}
      </button>
      {capturing && (
        <span className="help" role="status">
          {hint ?? "Esc to cancel"}
        </span>
      )}
    </div>
  );
}

function WaylandHelp({ tools }: { tools: string[] }) {
  return (
    <Callout>
      <strong>You’re on Wayland.</strong> Global shortcuts set here may not work in every app. Instead, add a shortcut
      in your desktop’s keyboard settings that runs <code>dictate --toggle</code>.
      {tools.length === 0 ? (
        <>
          {" "}
          To type text into apps automatically, install <code>wtype</code> (Sway, Hyprland, KDE) or{" "}
          <code>ydotool</code> (GNOME, e.g. <code>sudo dnf install ydotool</code>). Until then, text is copied to your
          clipboard.
        </>
      ) : (
        <> Typing with {tools.join(" / ")}.</>
      )}
    </Callout>
  );
}
