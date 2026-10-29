import { useEffect, useMemo, useState, type ReactElement } from "react";
import { api, on, type HistoryEntry, type Settings } from "./api";
import { Icon } from "./icons";
import { Select } from "./Select";
import { Group, Row, SwitchRow } from "./ui";

const KEEP_OPTIONS: [string, string][] = [
  ["1", "1 day"],
  ["7", "1 week"],
  ["30", "30 days"],
  ["90", "90 days"],
  ["365", "1 year"],
  ["0", "Forever"],
];

/** Past dictations: search, copy, delete, and how long to keep them. */
export function HistoryPage(props: {
  settings: Settings;
  update: (patch: Partial<Settings>) => Promise<void>;
  onError: (msg: string) => void;
}) {
  const { settings, update, onError } = props;
  const [entries, setEntries] = useState<HistoryEntry[] | null>(null);
  const [query, setQuery] = useState("");
  const [confirmClear, setConfirmClear] = useState(false);

  useEffect(() => {
    const load = () => api.getHistory().then(setEntries).catch((e) => onError(String(e)));
    load();
    const sub = on.history(load);
    return () => void sub.then((unlisten) => unlisten());
  }, [onError]);

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return (entries ?? []).filter((e) => !q || e.text.toLowerCase().includes(q));
  }, [entries, query]);

  async function remove(id: number) {
    setEntries((all) => all && all.filter((e) => e.id !== id));
    await api.deleteHistory(id).catch((e) => onError(String(e)));
  }

  async function clear() {
    setConfirmClear(false);
    await api.clearHistory().catch((e) => onError(String(e)));
  }

  if (entries === null) return null;

  return (
    <>
      <section className="group">
        <div className="history-bar">
          <label className="search">
            <Icon name="search" size={14} />
            <input
              type="search"
              aria-label="Search history"
              placeholder="Search dictations"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </label>
          {entries.length > 0 &&
            (confirmClear ? (
              <button className="danger" autoFocus onClick={clear} onBlur={() => setConfirmClear(false)}>
                Delete all {entries.length}?
              </button>
            ) : (
              <button className="ghost" onClick={() => setConfirmClear(true)}>
                <Icon name="trash" size={14} />
                Clear all
              </button>
            ))}
        </div>

        {shown.length === 0 ? (
          <div className="card empty-state">
            <Icon name={query ? "search" : "history"} size={22} />
            <p>
              {query
                ? `No dictations match “${query.trim()}”.`
                : settings.historyEnabled
                  ? "Your dictations will show up here."
                  : "History is off. Turn it on below to keep your dictations."}
            </p>
          </div>
        ) : (
          byDay(shown).map(([day, items]) => (
            <div key={day} className="day">
              <h2>{day}</h2>
              <ul className="card history-list">
                {items.map((e) => (
                  <HistoryItem key={e.id} entry={e} query={query.trim()} onDelete={() => remove(e.id)} onError={onError} />
                ))}
              </ul>
            </div>
          ))
        )}
      </section>

      <Group title="Settings">
        <SwitchRow
          label="Save history"
          help="Kept only on this computer."
          checked={settings.historyEnabled}
          onChange={(historyEnabled) => update({ historyEnabled })}
        />
        <Row label="Keep for" help="Older dictations are deleted automatically.">
          <Select
            label="Keep history for"
            value={String(settings.historyDays)}
            options={KEEP_OPTIONS}
            disabled={!settings.historyEnabled}
            onChange={(days) => update({ historyDays: Number(days) })}
          />
        </Row>
      </Group>
    </>
  );
}

function HistoryItem(props: { entry: HistoryEntry; query: string; onDelete: () => void; onError: (msg: string) => void }) {
  const { entry, query } = props;
  const [copied, setCopied] = useState(false);

  async function copy() {
    try {
      await navigator.clipboard.writeText(entry.text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch (e) {
      props.onError(`Couldn't copy: ${e}`);
    }
  }

  return (
    <li className="history-item">
      <div className="history-meta">
        <time dateTime={new Date(entry.at).toISOString()}>
          {new Date(entry.at).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })}
        </time>
        {entry.kind === "command" && <span className="tag">Command</span>}
        <span className="muted">
          {entry.model}
          {entry.durationMs > 0 && ` · ${formatDuration(entry.durationMs)}`}
        </span>
        <span className="history-actions">
          <button className="icon-button" aria-label={copied ? "Copied" : "Copy"} title="Copy" onClick={copy}>
            <Icon name={copied ? "check" : "copy"} size={14} />
          </button>
          <button className="icon-button" aria-label="Delete" title="Delete" onClick={props.onDelete}>
            <Icon name="trash" size={14} />
          </button>
        </span>
      </div>
      <p className="history-text">{highlight(entry.text, query)}</p>
    </li>
  );
}

/** Groups newest-first entries under "Today", "Yesterday" or a date. */
function byDay(entries: HistoryEntry[]): [string, HistoryEntry[]][] {
  const groups = new Map<string, HistoryEntry[]>();
  const today = new Date().toDateString();
  const yesterday = new Date(Date.now() - 86_400_000).toDateString();
  for (const e of entries) {
    const d = new Date(e.at);
    const key =
      d.toDateString() === today
        ? "Today"
        : d.toDateString() === yesterday
          ? "Yesterday"
          : d.toLocaleDateString([], { weekday: "long", month: "short", day: "numeric", year: d.getFullYear() === new Date().getFullYear() ? undefined : "numeric" });
    groups.set(key, [...(groups.get(key) ?? []), e]);
  }
  return [...groups];
}

function formatDuration(ms: number) {
  const s = Math.round(ms / 1000);
  return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${s % 60}s`;
}

function highlight(text: string, query: string) {
  if (!query) return text;
  const parts: (string | ReactElement)[] = [];
  const lower = text.toLowerCase();
  const q = query.toLowerCase();
  let i = 0;
  for (let hit = lower.indexOf(q); hit !== -1; hit = lower.indexOf(q, i)) {
    parts.push(text.slice(i, hit), <mark key={hit}>{text.slice(hit, hit + q.length)}</mark>);
    i = hit + q.length;
  }
  parts.push(text.slice(i));
  return parts;
}
