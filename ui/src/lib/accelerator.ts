// Turns keyboard events into accelerator strings such as "Ctrl+Shift+K".
//
// The names are the ones Tauri's global-shortcut parser (global-hotkey)
// accepts: Ctrl, Alt, Shift and Super as modifiers, then exactly one key.
// Rust parses the result again on save, so this only has to be close.

const NAMED_KEYS: Record<string, string> = {
  Space: "Space",
  Enter: "Enter",
  Tab: "Tab",
  Backspace: "Backspace",
  Delete: "Delete",
  Insert: "Insert",
  Home: "Home",
  End: "End",
  PageUp: "PageUp",
  PageDown: "PageDown",
  ArrowUp: "Up",
  ArrowDown: "Down",
  ArrowLeft: "Left",
  ArrowRight: "Right",
  Minus: "-",
  Equal: "=",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  BracketLeft: "[",
  BracketRight: "]",
  Backquote: "`",
  PrintScreen: "PrintScreen",
  ScrollLock: "ScrollLock",
  Pause: "Pause",
  NumLock: "NumLock",
  CapsLock: "CapsLock",
  NumpadAdd: "NumpadAdd",
  NumpadSubtract: "NumpadSubtract",
  NumpadMultiply: "NumpadMultiply",
  NumpadDivide: "NumpadDivide",
  NumpadDecimal: "NumpadDecimal",
  NumpadEnter: "NumpadEnter",
  NumpadEqual: "NumpadEqual",
};

const MODIFIER_CODES = /^(Control|Alt|Shift|Meta|OS)(Left|Right)?$/;

/** The accelerator name of a physical key (`KeyboardEvent.code`), or null if unsupported. */
export function keyName(code: string): string | null {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (/^Numpad[0-9]$/.test(code)) return code;
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code;
  return NAMED_KEYS[code] ?? null;
}

export function isModifierCode(code: string): boolean {
  return MODIFIER_CODES.test(code);
}

export function modifierNames(e: KeyboardEvent): string[] {
  const mods: string[] = [];
  if (e.ctrlKey) mods.push("Ctrl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  if (e.metaKey) mods.push("Super");
  return mods;
}

export type Captured =
  /** Only modifiers are held so far; `text` previews them ("Ctrl+Alt+"). */
  | { kind: "partial"; text: string }
  | { kind: "complete"; text: string }
  | { kind: "rejected"; text: string; reason: string };

/** Interprets one keydown while recording. */
export function capture(e: KeyboardEvent): Captured {
  const mods = modifierNames(e);
  if (isModifierCode(e.code)) {
    return { kind: "partial", text: mods.length ? mods.join("+") + "+" : "" };
  }
  const key = keyName(e.code);
  if (key === null) {
    return { kind: "rejected", text: mods.join("+"), reason: "That key cannot be used." };
  }
  const isFunctionKey = /^F\d+$/.test(key);
  if (mods.length === 0 && !isFunctionKey) {
    return {
      kind: "rejected",
      text: "",
      reason: "Hold Ctrl, Alt, Shift or Super together with the key.",
    };
  }
  return { kind: "complete", text: [...mods, key].join("+") };
}
