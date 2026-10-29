const MODIFIER_CODES = new Set([
  "ShiftLeft", "ShiftRight", "ControlLeft", "ControlRight",
  "AltLeft", "AltRight", "MetaLeft", "MetaRight",
]);

const isMac = navigator.userAgent.includes("Mac");

/**
 * Turns a keydown into an accelerator string the backend understands,
 * e.g. "Ctrl+Shift+Space". Returns null while only modifiers are held.
 */
export function acceleratorFromEvent(e: KeyboardEvent): string | null {
  if (MODIFIER_CODES.has(e.code)) return null;
  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  if (e.metaKey) parts.push("Super");
  const isFunctionKey = /^F\d{1,2}$/.test(e.code);
  if (parts.length === 0 && !isFunctionKey) return null; // bare keys would hijack typing
  parts.push(e.code);
  return parts.join("+");
}

/** The modifiers currently held, e.g. "Ctrl+Shift", for live feedback while recording. */
export function heldModifiers(e: KeyboardEvent): string {
  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  if (e.metaKey) parts.push("Super");
  return parts.join("+");
}

/** Friendly label: "Ctrl+Shift+KeyD" → "⌃ ⇧ D" on macOS, "Ctrl + Shift + D" elsewhere. */
export function formatAccelerator(accel: string): string {
  const names: Record<string, [string, string]> = {
    Ctrl: ["⌃", "Ctrl"],
    Control: ["⌃", "Ctrl"],
    Alt: ["⌥", "Alt"],
    Option: ["⌥", "Alt"],
    Shift: ["⇧", "Shift"],
    Super: ["⌘", "Win"],
    Cmd: ["⌘", "Win"],
    Command: ["⌘", "Win"],
  };
  return accel
    .split("+")
    .map((part) => {
      const named = names[part];
      if (named) return isMac ? named[0] : named[1];
      return part.replace(/^Key/, "").replace(/^Digit/, "");
    })
    .join(isMac ? " " : " + ");
}
