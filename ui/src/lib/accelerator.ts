// Turns keyboard events into accelerator strings such as "Ctrl+Shift+K", and
// shows an accelerator the way the current OS names its keys.
//
// The names are the ones Tauri's global-shortcut parser (global-hotkey)
// accepts: Ctrl, Alt, Shift and Super as modifiers, then exactly one key.
// Rust parses the result again on save, so this only has to be close. (Rust
// also reads Win, Windows and Meta as Super, so typed text may use them.)

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

export type Platform = "windows" | "macos" | "linux";

/** The shortcuts offered in the dropdown beside the recorder. */
export const PRESETS = ["Super+Space", "Alt+Space", "Ctrl+Space", "Ctrl+Alt+Space"] as const;

/** The key a modifier name stands for on this OS (`Super` is Win on Windows, Cmd on macOS). */
function modifierLabel(token: string, platform: Platform): string | null {
  switch (token.toLowerCase()) {
    case "super":
    case "win":
    case "windows":
    case "meta":
    case "cmd":
    case "command":
      return platform === "windows" ? "Win" : platform === "macos" ? "Cmd" : "Super";
    case "alt":
    case "option":
      return platform === "macos" ? "Option" : "Alt";
    case "ctrl":
    case "control":
      return "Ctrl";
    case "shift":
      return "Shift";
    default:
      return null;
  }
}

/** An accelerator as this OS names its keys: `Super+Space` shows as `Win+Space`, `Cmd+Space`, `Super+Space`. */
export function displayAccelerator(text: string, platform: Platform): string {
  return text
    .split("+")
    .map((raw) => {
      const token = raw.trim();
      return modifierLabel(token, platform) ?? token;
    })
    .join("+");
}

/**
 * A shortcut's identity: spellings of one shortcut (`Win+Space`, `super + SPACE`,
 * with the modifiers in any order) give the same text.
 */
export function acceleratorId(text: string): string {
  const tokens = text
    .split("+")
    .map((raw) => raw.trim())
    .map((token) => (modifierLabel(token, "linux") ?? token).toLowerCase());
  const key = tokens.pop() ?? "";
  return [...new Set(tokens)].sort().concat(key).join("+");
}

/** Whether two spellings are the same shortcut once names, order and spacing are ignored. */
export function sameAccelerator(a: string, b: string): boolean {
  return acceleratorId(a) === acceleratorId(b);
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
