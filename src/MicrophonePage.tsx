import { useCallback, useEffect, useState } from "react";
import { api, on, type AppState, type InputDevice, type Settings } from "./api";
import { Icon } from "./icons";
import { Select } from "./Select";
import { Group, Row } from "./ui";

const ACCESS_HELP = {
  macos: "System Settings → Privacy & Security → Microphone.",
  windows: "Settings → Privacy & security → Microphone, including “Let desktop apps access your microphone”.",
};

/** Microphone access, which input to record from, and a live level test. */
export function MicrophonePage(props: {
  state: AppState;
  update: (patch: Partial<Settings>) => Promise<void>;
  refresh: () => Promise<void>;
  onError: (msg: string) => void;
}) {
  const { state, update, refresh, onError } = props;
  const { settings, platform, micPermission } = state;
  const [devices, setDevices] = useState<InputDevice[] | null>(null);
  const [testing, setTesting] = useState(false);
  const [level, setLevel] = useState(0);

  // Microphones come and go, so re-list them whenever the window comes back.
  const loadDevices = useCallback(() => api.listMicrophones().then(setDevices), []);
  useEffect(() => {
    loadDevices();
    window.addEventListener("focus", loadDevices);
    return () => window.removeEventListener("focus", loadDevices);
  }, [loadDevices]);

  useEffect(() => {
    if (!testing) return;
    const sub = on.micLevel(setLevel);
    // Hiding the window or leaving the page stops the test, so the mic isn't left open.
    const stop = () => setTesting(false);
    window.addEventListener("blur", stop);
    return () => {
      sub.then((unlisten) => unlisten());
      window.removeEventListener("blur", stop);
      api.stopMicTest();
      setLevel(0);
    };
  }, [testing]);

  async function startTest() {
    try {
      await api.startMicTest();
      setTesting(true);
    } catch (e) {
      onError(String(e));
      refresh();
    }
  }

  async function choose(microphone: string) {
    await update({ microphone });
    // Keep testing with the newly chosen microphone.
    if (testing) await api.startMicTest().catch((e) => (onError(String(e)), setTesting(false)));
  }

  const fallback = devices?.find((d) => d.isDefault);
  const missing = !!settings.microphone && !!devices && !devices.some((d) => d.id === settings.microphone);
  const options: [string, string][] = [
    ["", fallback ? `System default (${fallback.name})` : "System default"],
    ...(missing ? [[settings.microphone, "Saved microphone (not connected)"] as [string, string]] : []),
    ...(devices ?? []).map((d) => [d.id, d.name] as [string, string]),
  ];
  const accessHelp = ACCESS_HELP[platform as keyof typeof ACCESS_HELP];

  return (
    <>
      {accessHelp && (
        <Group title="Access">
          <Row
            label="Microphone access"
            help={
              micPermission === "granted"
                ? "Dictate can use the microphone."
                : micPermission === "denied"
                  ? `Turned off, so Dictate only hears silence. Turn it on in ${accessHelp}`
                  : "Dictate hasn't asked yet."
            }
          >
            {micPermission === "granted" ? (
              <span className="result ok">
                <Icon name="check" size={14} />
                Allowed
              </span>
            ) : micPermission === "denied" ? (
              <button onClick={() => api.openMicSettings()}>Open Settings</button>
            ) : (
              <button className="primary" onClick={() => api.requestMicPermission()}>
                Allow
              </button>
            )}
          </Row>
        </Group>
      )}

      <Group title="Input">
        <Row
          label="Microphone"
          help={
            missing
              ? "Not connected right now, so Dictate uses the system default."
              : "System default follows whatever your computer is set to."
          }
        >
          <Select label="Microphone" value={settings.microphone} options={options} onChange={choose} />
        </Row>
        <Row label="Test" help={testing ? "Speak normally. The bar should reach the middle." : "Check the microphone hears you."}>
          <div className="mic-test">
            {testing && (
              <div
                className="meter"
                role="meter"
                aria-label="Microphone level"
                aria-valuenow={Math.round(level * 100)}
                aria-valuemin={0}
                aria-valuemax={100}
              >
                <div style={{ transform: `scaleX(${level})` }} />
              </div>
            )}
            <button onClick={() => (testing ? setTesting(false) : startTest())}>
              <Icon name={testing ? "stop" : "mic"} size={14} />
              {testing ? "Stop" : "Test microphone"}
            </button>
          </div>
        </Row>
        <Row label="Input volume" help="If the bar barely moves, raise the input volume.">
          <button onClick={() => api.openSoundSettings().catch((e) => onError(String(e)))}>
            Sound settings <Icon name="external" size={12} />
          </button>
        </Row>
      </Group>
    </>
  );
}
