import { useState } from "react";
import type { Replacement, Settings } from "./api";
import { Icon } from "./icons";
import { Group } from "./ui";

/** Custom words the speech model should spell your way, and spoken shortcuts. */
export function DictionaryPage(props: { settings: Settings; update: (patch: Partial<Settings>) => Promise<void> }) {
  const { settings, update } = props;
  return (
    <>
      <Group title="Words">
        <Words words={settings.dictionary} onChange={(dictionary) => update({ dictionary })} />
      </Group>
      <Group title="Replacements">
        <Replacements items={settings.replacements} onChange={(replacements) => update({ replacements })} />
      </Group>
    </>
  );
}

function Words({ words, onChange }: { words: string[]; onChange: (words: string[]) => void }) {
  const [draft, setDraft] = useState("");
  const [note, setNote] = useState<string | null>(null);

  function add() {
    // Pasting "Anbu, Byepo, Tauri" adds all three.
    const fresh = draft
      .split(/[,\n]/)
      .map((w) => w.trim())
      .filter(Boolean);
    const known = new Set(words.map((w) => w.toLowerCase()));
    const added = fresh.filter((w) => !known.has(w.toLowerCase()) && known.add(w.toLowerCase()));
    if (added.length) onChange([...words, ...added]);
    setNote(added.length < fresh.length ? "Already in your dictionary." : null);
    setDraft("");
  }

  return (
    <div className="pad">
      <p className="help">
        Names, brands and jargon the speech model should recognise and spell exactly as written here, like{" "}
        <em>GitHub</em> or <em>Byepo</em>.
      </p>
      <form
        className="input-row"
        onSubmit={(e) => {
          e.preventDefault();
          add();
        }}
      >
        <input
          aria-label="New word"
          placeholder="Add a word or name"
          value={draft}
          onChange={(e) => (setDraft(e.target.value), setNote(null))}
        />
        <button className="primary" type="submit" disabled={!draft.trim()}>
          <Icon name="plus" size={14} />
          Add
        </button>
      </form>
      {note && (
        <span className="help" role="status">
          {note}
        </span>
      )}
      {words.length > 0 ? (
        <ul className="chips" aria-label="Dictionary words">
          {words.map((w) => (
            <li key={w} className="chip">
              {w}
              <button
                className="chip-remove"
                aria-label={`Remove ${w}`}
                onClick={() => onChange(words.filter((x) => x !== w))}
              >
                <Icon name="x" size={12} />
              </button>
            </li>
          ))}
        </ul>
      ) : (
        <p className="empty">No words yet.</p>
      )}
    </div>
  );
}

function Replacements({ items, onChange }: { items: Replacement[]; onChange: (items: Replacement[]) => void }) {
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const duplicate = items.some((r) => r.from.trim().toLowerCase() === from.trim().toLowerCase());

  function edit(i: number, patch: Partial<Replacement>) {
    const next = items.map((r, j) => (j === i ? { ...r, ...patch } : r));
    if (next[i].from.trim() && next[i].to.trim()) onChange(next);
  }

  return (
    <div className="pad">
      <p className="help">
        Say a phrase to type something longer. Saying just the phrase types only the replacement, for example
        “my email” → your address.
      </p>

      {items.length > 0 && (
        <div className="replacements">
          <div className="replacement head" aria-hidden="true">
            <span>When I say</span>
            <span />
            <span>Type</span>
            <span />
          </div>
          {items.map((r, i) => (
            // Keyed by content so the inputs reset if the list changes elsewhere.
            <div className="replacement" key={`${r.from}\u0000${r.to}`}>
              <input
                aria-label="When I say"
                defaultValue={r.from}
                onBlur={(e) => e.target.value !== r.from && edit(i, { from: e.target.value.trim() })}
              />
              <Icon name="arrow" size={14} />
              <input
                aria-label="Type"
                defaultValue={r.to}
                onBlur={(e) => e.target.value !== r.to && edit(i, { to: e.target.value })}
              />
              <button
                className="icon-button"
                aria-label={`Delete replacement for ${r.from}`}
                onClick={() => onChange(items.filter((_, j) => j !== i))}
              >
                <Icon name="trash" size={14} />
              </button>
            </div>
          ))}
        </div>
      )}

      <form
        className="replacement add"
        onSubmit={(e) => {
          e.preventDefault();
          onChange([...items, { from: from.trim(), to }]);
          setFrom("");
          setTo("");
        }}
      >
        <input aria-label="New phrase to say" placeholder="my email" value={from} onChange={(e) => setFrom(e.target.value)} />
        <Icon name="arrow" size={14} />
        <input aria-label="Text to type" placeholder="name@example.com" value={to} onChange={(e) => setTo(e.target.value)} />
        <button className="primary" type="submit" disabled={!from.trim() || !to.trim() || duplicate}>
          <Icon name="plus" size={14} />
          Add
        </button>
      </form>
      {duplicate && from.trim() && (
        <span className="help" role="status">
          You already have a replacement for “{from.trim()}”.
        </span>
      )}
    </div>
  );
}
