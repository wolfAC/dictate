import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { Icon } from "./icons";

/**
 * A styled replacement for <select>, which can't be themed in the system webviews.
 * Follows the ARIA listbox pattern: arrows/Home/End move, Enter/Space picks,
 * Escape/Tab/click outside closes, and typing jumps to a matching option.
 */
export function Select<T extends string>(props: {
  value: T;
  options: [T, string][];
  onChange: (value: T) => void;
  label?: string;
  id?: string;
  disabled?: boolean;
}) {
  const { value, options, onChange, disabled } = props;
  const autoId = useId();
  const id = props.id ?? autoId;
  const listId = `${id}-list`;
  const root = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLUListElement>(null);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const [upward, setUpward] = useState(false);
  const typed = useRef({ text: "", at: 0 });

  const selectedIndex = Math.max(
    0,
    options.findIndex(([v]) => v === value),
  );
  const current = options[selectedIndex]?.[1] ?? "";

  function show() {
    if (disabled) return;
    setActive(selectedIndex);
    setOpen(true);
  }

  function close(refocus = true) {
    setOpen(false);
    if (refocus) button.current?.focus();
  }

  function pick(i: number) {
    const option = options[i];
    if (option && option[0] !== value) onChange(option[0]);
    close();
  }

  // Open upward when there isn't room below (e.g. the last setting in the window).
  useLayoutEffect(() => {
    if (!open || !button.current) return;
    const rect = button.current.getBoundingClientRect();
    const below = window.innerHeight - rect.bottom;
    setUpward(below < 240 && rect.top > below);
  }, [open]);

  // Keep the highlighted option visible while moving with the keyboard.
  useEffect(() => {
    // Scroll only the list; scrollIntoView would also scroll the settings page behind it.
    const ul = list.current;
    const item = ul?.children[active] as HTMLElement | undefined;
    if (!open || !ul || !item) return;
    if (item.offsetTop < ul.scrollTop) ul.scrollTop = item.offsetTop - 4;
    else if (item.offsetTop + item.offsetHeight > ul.scrollTop + ul.clientHeight)
      ul.scrollTop = item.offsetTop + item.offsetHeight - ul.clientHeight + 4;
  }, [open, active]);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (e: PointerEvent) => {
      if (!root.current?.contains(e.target as Node)) close(false);
    };
    const onBlur = () => close(false);
    window.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("pointerdown", onPointerDown, true);
      window.removeEventListener("blur", onBlur);
    };
  }, [open]);

  function typeAhead(key: string) {
    const now = Date.now();
    const t = typed.current;
    t.text = now - t.at > 700 ? key : t.text + key;
    t.at = now;
    const start = open ? active : selectedIndex;
    const order = [...options.keys()].map((k) => (start + 1 + k) % options.length);
    const hit = order.find((i) => options[i][1].toLowerCase().startsWith(t.text.toLowerCase()));
    if (hit === undefined) return;
    if (open) setActive(hit);
    else onChange(options[hit][0]);
  }

  function onKeyDown(e: React.KeyboardEvent) {
    const last = options.length - 1;
    if (!open) {
      if (["ArrowDown", "ArrowUp", "Enter", " "].includes(e.key)) {
        e.preventDefault();
        show();
      } else if (e.key.length === 1) {
        typeAhead(e.key);
      }
      return;
    }
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        setActive((i) => Math.min(last, i + 1));
        break;
      case "ArrowUp":
        e.preventDefault();
        setActive((i) => Math.max(0, i - 1));
        break;
      case "Home":
        e.preventDefault();
        setActive(0);
        break;
      case "End":
        e.preventDefault();
        setActive(last);
        break;
      case "Enter":
      case " ":
        e.preventDefault();
        pick(active);
        break;
      case "Escape":
        e.preventDefault();
        close();
        break;
      case "Tab":
        close(false);
        break;
      default:
        if (e.key.length === 1) typeAhead(e.key);
    }
  }

  return (
    <div className={`select ${open ? "open" : ""}`} ref={root}>
      <button
        ref={button}
        id={id}
        type="button"
        className="select-button"
        role="combobox"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={listId}
        aria-activedescendant={open ? `${id}-opt-${active}` : undefined}
        aria-label={props.label}
        disabled={disabled}
        onClick={() => (open ? close() : show())}
        onKeyDown={onKeyDown}
      >
        <span className="select-value">{current}</span>
        <Icon name="chevrons" size={14} />
      </button>
      {open && (
        <ul
          ref={list}
          id={listId}
          role="listbox"
          aria-label={props.label}
          className={`select-list ${upward ? "upward" : ""}`}
        >
          {options.map(([v, text], i) => (
            <li
              key={v}
              id={`${id}-opt-${i}`}
              role="option"
              aria-selected={v === value}
              className={i === active ? "active" : undefined}
              onPointerMove={() => setActive(i)}
              // Keep focus on the button so keyboard handling keeps working.
              onPointerDown={(e) => e.preventDefault()}
              onClick={() => pick(i)}
            >
              <span>{text}</span>
              {v === value && <Icon name="check" size={14} />}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
