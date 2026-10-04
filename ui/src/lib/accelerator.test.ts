import { describe, expect, it } from "vitest";
import {
  PRESETS,
  acceleratorId,
  capture,
  displayAccelerator,
  isModifierCode,
  keyName,
  modifierNames,
  sameAccelerator,
} from "./accelerator";

/** The parts of a KeyboardEvent the module reads. */
const key = (code: string, mods: Partial<Record<"ctrlKey" | "altKey" | "shiftKey" | "metaKey", boolean>> = {}) =>
  ({ code, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...mods }) as KeyboardEvent;

describe("keyName", () => {
  it("maps letters, digits, numpad digits and function keys", () => {
    expect(keyName("KeyA")).toBe("A");
    expect(keyName("KeyZ")).toBe("Z");
    expect(keyName("Digit0")).toBe("0");
    expect(keyName("Digit9")).toBe("9");
    expect(keyName("Numpad5")).toBe("Numpad5");
    expect(keyName("F1")).toBe("F1");
    expect(keyName("F12")).toBe("F12");
    expect(keyName("F24")).toBe("F24");
  });

  it("rejects function keys outside F1..F24 and lowercase or multi-letter key codes", () => {
    expect(keyName("F0")).toBeNull();
    expect(keyName("F25")).toBeNull();
    expect(keyName("Keya")).toBeNull();
    expect(keyName("KeyAB")).toBeNull();
    expect(keyName("Digit10")).toBeNull();
  });

  it("uses the names the shortcut parser accepts for named keys", () => {
    expect(keyName("Space")).toBe("Space");
    expect(keyName("ArrowUp")).toBe("Up");
    expect(keyName("ArrowRight")).toBe("Right");
    expect(keyName("Backquote")).toBe("`");
    expect(keyName("Backslash")).toBe("\\");
    expect(keyName("NumpadEnter")).toBe("NumpadEnter");
  });

  it("returns null for modifiers and unknown codes", () => {
    expect(keyName("ControlLeft")).toBeNull();
    expect(keyName("MediaPlayPause")).toBeNull();
    expect(keyName("")).toBeNull();
  });
});

describe("displayAccelerator", () => {
  it("names Super after the OS", () => {
    expect(displayAccelerator("Super+Space", "windows")).toBe("Win+Space");
    expect(displayAccelerator("Super+Space", "macos")).toBe("Cmd+Space");
    expect(displayAccelerator("Super+Space", "linux")).toBe("Super+Space");
  });

  it("names Alt as Option on macOS only", () => {
    expect(displayAccelerator("Alt+Space", "macos")).toBe("Option+Space");
    expect(displayAccelerator("Alt+Space", "windows")).toBe("Alt+Space");
  });

  it("accepts every spelling Rust reads and normalizes it", () => {
    expect(displayAccelerator("win+k", "windows")).toBe("Win+k");
    expect(displayAccelerator("Windows+K", "macos")).toBe("Cmd+K");
    expect(displayAccelerator("meta+K", "linux")).toBe("Super+K");
    expect(displayAccelerator("Command+K", "windows")).toBe("Win+K");
    expect(displayAccelerator("control+K", "linux")).toBe("Ctrl+K");
    expect(displayAccelerator("Option+K", "windows")).toBe("Alt+K");
  });

  it("trims spaces around tokens and leaves the key alone", () => {
    expect(displayAccelerator(" Ctrl + Shift + F5 ", "linux")).toBe("Ctrl+Shift+F5");
    expect(displayAccelerator("Ctrl+Up", "macos")).toBe("Ctrl+Up");
  });

  it("shows every preset on every platform without leaving a raw spelling", () => {
    for (const platform of ["windows", "macos", "linux"] as const) {
      for (const preset of PRESETS) {
        const shown = displayAccelerator(preset, platform);
        expect(shown.endsWith("Space")).toBe(true);
        expect(shown.split("+").length).toBe(preset.split("+").length);
      }
    }
  });
});

describe("acceleratorId", () => {
  it("is the same for spellings of one shortcut", () => {
    const id = acceleratorId("Super+Space");
    for (const spelling of ["Win+Space", "super + SPACE", "META+space", "Windows+Space", "cmd+space"]) {
      expect(acceleratorId(spelling)).toBe(id);
    }
  });

  it("ignores modifier order and repeated modifiers", () => {
    expect(acceleratorId("Ctrl+Alt+T")).toBe(acceleratorId("alt+ctrl+t"));
    expect(acceleratorId("Ctrl+Ctrl+T")).toBe(acceleratorId("Ctrl+T"));
    expect(acceleratorId("Control+Option+T")).toBe(acceleratorId("Ctrl+Alt+T"));
  });

  it("keeps the last token as the key, so different keys differ", () => {
    expect(acceleratorId("Ctrl+A")).not.toBe(acceleratorId("Ctrl+B"));
    expect(acceleratorId("Ctrl+Alt+T")).not.toBe(acceleratorId("Ctrl+Alt+Shift+T"));
    expect(acceleratorId("Ctrl+Space")).not.toBe(acceleratorId("Alt+Space"));
  });

  it("puts the modifiers first, sorted, then the key", () => {
    expect(acceleratorId("Shift+Ctrl+K")).toBe("ctrl+shift+k");
  });

  it("copes with empty and single-token text", () => {
    expect(acceleratorId("")).toBe("");
    expect(acceleratorId("F5")).toBe("f5");
  });
});

describe("sameAccelerator", () => {
  it("compares by identity", () => {
    expect(sameAccelerator("Win+Space", "super+space")).toBe(true);
    expect(sameAccelerator("Ctrl+Space", "Alt+Space")).toBe(false);
  });
});

describe("isModifierCode and modifierNames", () => {
  it("recognizes modifier key codes", () => {
    for (const code of ["ControlLeft", "ControlRight", "AltLeft", "ShiftRight", "MetaLeft", "OSRight", "Shift"]) {
      expect(isModifierCode(code)).toBe(true);
    }
    for (const code of ["KeyA", "Space", "ControlA", "AltGraph"]) {
      expect(isModifierCode(code)).toBe(false);
    }
  });

  it("lists held modifiers in a fixed order", () => {
    expect(modifierNames(key("KeyA", { metaKey: true, shiftKey: true, ctrlKey: true, altKey: true }))).toEqual([
      "Ctrl",
      "Alt",
      "Shift",
      "Super",
    ]);
    expect(modifierNames(key("KeyA"))).toEqual([]);
  });
});

describe("capture", () => {
  it("completes a modifier plus a key", () => {
    expect(capture(key("KeyK", { ctrlKey: true, shiftKey: true }))).toEqual({
      kind: "complete",
      text: "Ctrl+Shift+K",
    });
    expect(capture(key("Space", { metaKey: true }))).toEqual({ kind: "complete", text: "Super+Space" });
    expect(capture(key("ArrowLeft", { altKey: true }))).toEqual({ kind: "complete", text: "Alt+Left" });
  });

  it("previews held modifiers while only modifiers are down", () => {
    expect(capture(key("ControlLeft", { ctrlKey: true }))).toEqual({ kind: "partial", text: "Ctrl+" });
    expect(capture(key("ShiftLeft", { ctrlKey: true, shiftKey: true }))).toEqual({
      kind: "partial",
      text: "Ctrl+Shift+",
    });
    expect(capture(key("ControlLeft"))).toEqual({ kind: "partial", text: "" });
  });

  it("accepts a bare function key but no other bare key", () => {
    expect(capture(key("F9"))).toEqual({ kind: "complete", text: "F9" });
    const bare = capture(key("KeyA"));
    expect(bare.kind).toBe("rejected");
    expect(capture(key("Space")).kind).toBe("rejected");
  });

  it("rejects a key the parser would not know, keeping the modifiers", () => {
    const result = capture(key("MediaPlayPause", { ctrlKey: true }));
    expect(result).toMatchObject({ kind: "rejected", text: "Ctrl", reason: "That key cannot be used." });
  });

  it("produces text that acceleratorId treats as the same shortcut when re-ordered", () => {
    const captured = capture(key("KeyT", { altKey: true, ctrlKey: true }));
    expect(captured.kind).toBe("complete");
    expect(sameAccelerator(captured.text, "alt+control+t")).toBe(true);
  });
});
