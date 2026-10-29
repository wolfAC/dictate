import { useId, type ReactNode } from "react";
import { Icon, type IconName } from "./icons";
import { formatAccelerator } from "./hotkey";

// ---------- Layout primitives ----------

export function Group({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="group">
      <h2>{title}</h2>
      <div className="card">{children}</div>
    </section>
  );
}

export function Row({ label, help, children }: { label: string; help?: ReactNode; children: ReactNode }) {
  return (
    <div className="setting">
      <div className="setting-text">
        <span className="setting-label">{label}</span>
        {help && <span className="help">{help}</span>}
      </div>
      <div className="setting-control">{children}</div>
    </div>
  );
}

export function SwitchRow(props: {
  label: string;
  help?: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (checked: boolean) => void;
}) {
  const id = useId();
  return (
    <div className={`setting ${props.disabled ? "disabled" : ""}`}>
      <div className="setting-text">
        <label className="setting-label" htmlFor={id}>
          {props.label}
        </label>
        {props.help && <span className="help">{props.help}</span>}
      </div>
      <button
        id={id}
        role="switch"
        className="switch"
        aria-checked={props.checked}
        disabled={props.disabled}
        onClick={() => props.onChange(!props.checked)}
      >
        <span className="thumb" />
      </button>
    </div>
  );
}

export function Callout(props: { tone?: "warn"; icon?: IconName; action?: ReactNode; children: ReactNode }) {
  return (
    <div className={`callout ${props.tone ?? ""}`} role={props.tone === "warn" ? "alert" : undefined}>
      <Icon name={props.icon ?? (props.tone === "warn" ? "alert" : "info")} size={18} />
      <div className="callout-text">{props.children}</div>
      {props.action}
    </div>
  );
}

export function kbd(accel: string) {
  return <kbd>{formatAccelerator(accel)}</kbd>;
}

export function Segmented<T extends string>(props: {
  label: string;
  value: T;
  options: [T, string][];
  onChange: (v: T) => void;
}) {
  return (
    <div className="segmented" role="radiogroup" aria-label={props.label}>
      {props.options.map(([value, label]) => (
        <button
          key={value}
          role="radio"
          aria-checked={props.value === value}
          className={props.value === value ? "active" : ""}
          onClick={() => props.onChange(value)}
        >
          {label}
        </button>
      ))}
    </div>
  );
}
