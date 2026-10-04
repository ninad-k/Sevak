import { describe, expect, it } from "vitest";
import builtinThemes from "./builtin-themes.json";
import {
  AA_NORMAL_TEXT,
  COLOR_FIELDS,
  History,
  LIMITS,
  SIZE_DEFAULTS,
  applyVariables,
  blend,
  clone,
  contrastChecks,
  contrastRatio,
  deriveSecondary,
  emptySpec,
  fontFamilyCss,
  fontFamilyError,
  formatColor,
  isColor,
  isShadow,
  miniVars,
  paletteFor,
  parseColor,
  previewVariables,
  toHex6,
  withAlpha,
  withRgb,
  type Palette,
  type StoredTheme,
} from "./themes";

const themes = builtinThemes as StoredTheme[];
const BLACK = { r: 0, g: 0, b: 0, a: 1 };
const WHITE = { r: 255, g: 255, b: 255, a: 1 };

describe("parseColor", () => {
  it("reads hex in all four lengths", () => {
    expect(parseColor("#f00")).toEqual({ r: 255, g: 0, b: 0, a: 1 });
    expect(parseColor("#1c1b2e")).toEqual({ r: 28, g: 27, b: 46, a: 1 });
    expect(parseColor("#f008")).toMatchObject({ r: 255, g: 0, b: 0 });
    expect(parseColor("#f008")!.a).toBeCloseTo(0x88 / 255);
    expect(parseColor("#00000080")!.a).toBeCloseTo(128 / 255);
  });

  it("reads rgb() and rgba() with commas, spaces, slash and percent alpha", () => {
    expect(parseColor("rgb(1, 2, 3)")).toEqual({ r: 1, g: 2, b: 3, a: 1 });
    expect(parseColor("rgb(1 2 3)")).toEqual({ r: 1, g: 2, b: 3, a: 1 });
    expect(parseColor("rgba(1, 2, 3, 0.5)")).toEqual({ r: 1, g: 2, b: 3, a: 0.5 });
    expect(parseColor("rgb(1 2 3 / 25%)")).toEqual({ r: 1, g: 2, b: 3, a: 0.25 });
    expect(parseColor("  RGBA(0,0,0,1)  ")).toEqual({ r: 0, g: 0, b: 0, a: 1 });
  });

  it("rejects malformed input", () => {
    for (const bad of [
      "",
      "red",
      "#",
      "#ff",
      "#fffff",
      "#fffffff",
      "#gggggg",
      "rgb(256, 0, 0)",
      "rgb(-1, 0, 0)",
      "rgb(1.5, 0, 0)",
      "rgb(1, 2)",
      "rgb(1, 2, 3, 4, 5)",
      "rgba(1, 2, 3, 1.5)",
      "rgba(1, 2, 3, -0.1)",
      "rgba(1, 2, 3, x)",
      "rgb(1, 2, 3",
    ]) {
      expect(parseColor(bad), bad).toBeNull();
      expect(isColor(bad), bad).toBe(false);
    }
  });
});

describe("formatColor and color editing", () => {
  it("writes #rrggbb when opaque and rgba() otherwise", () => {
    expect(formatColor({ r: 255, g: 158, b: 11, a: 1 })).toBe("#ff9e0b");
    expect(formatColor({ r: 0, g: 0, b: 0, a: 0.9996 })).toBe("#000000");
    expect(formatColor({ r: 28, g: 27, b: 46, a: 0.12 })).toBe("rgba(28, 27, 46, 0.12)");
    expect(formatColor({ r: 1, g: 2, b: 3, a: 0.123456 })).toBe("rgba(1, 2, 3, 0.123)");
  });

  it("round-trips what it writes", () => {
    for (const text of ["#fcfcfd", "rgba(28, 27, 46, 0.12)", "rgba(245, 158, 11, 0.17)"]) {
      expect(formatColor(parseColor(text)!)).toBe(text);
    }
  });

  it("toHex6 drops alpha and falls back on garbage", () => {
    expect(toHex6("rgba(255, 0, 0, 0.2)")).toBe("#ff0000");
    expect(toHex6("#abc")).toBe("#aabbcc");
    expect(toHex6("nope")).toBe("#000000");
    expect(toHex6("nope", "#123456")).toBe("#123456");
  });

  it("withAlpha and withRgb change one part and keep the other", () => {
    expect(withAlpha("#ff0000", 0.5)).toBe("rgba(255, 0, 0, 0.5)");
    expect(withAlpha("rgba(255, 0, 0, 0.5)", 1)).toBe("#ff0000");
    expect(withAlpha("garbage", 0.5)).toBe("garbage");
    expect(withRgb("rgba(255, 0, 0, 0.5)", "#00ff00")).toBe("rgba(0, 255, 0, 0.5)");
    expect(withRgb("garbage", "#00ff00")).toBe("#00ff00");
    expect(withRgb("#ff0000", "not-a-color")).toBe("#ff0000");
  });
});

describe("isShadow (the grammar of Rust's normalize_shadow)", () => {
  it("accepts none and layered shadows", () => {
    expect(isShadow("none")).toBe(true);
    expect(isShadow("NONE")).toBe(true);
    expect(isShadow("0 8px 28px rgba(0, 0, 0, 0.5)")).toBe(true);
    expect(isShadow("0 8px 28px rgba(28, 27, 46, 0.22), 0 1px 3px rgba(28, 27, 46, 0.12)")).toBe(true);
    expect(isShadow("inset 0 1px 0 #fff")).toBe(true);
    expect(isShadow("2px 2px 4px 1px #000")).toBe(true);
    expect(isShadow("-2px -2px 4px #000")).toBe(true);
  });

  it("rejects bad shapes", () => {
    expect(isShadow("")).toBe(false);
    expect(isShadow("0 8px")).toBe(false); // no color
    expect(isShadow("#000")).toBe(false); // no lengths
    expect(isShadow("0 1px 2px 3px 4px #000")).toBe(false); // five lengths
    expect(isShadow("0 0 0 red")).toBe(false); // named colors are not accepted
    expect(isShadow("0 0 999px #000")).toBe(false); // beyond 200
    expect(isShadow("0 0 4px #000, 0 0 4px #000, 0 0 4px #000, 0 0 4px #000, 0 0 4px #000, 0 0 4px #000, 0 0 4px #000")).toBe(false);
    expect(isShadow("0 0 4px rgba(0, 0, 0, 0.5")).toBe(false); // unbalanced
    expect(isShadow("0 0 4px #000) ")).toBe(false);
    expect(isShadow("inset inset 0 0 #000")).toBe(false);
  });
});

describe("contrast", () => {
  it("follows WCAG 2 for the extremes", () => {
    expect(contrastRatio(BLACK, WHITE)).toBeCloseTo(21, 5);
    expect(contrastRatio(WHITE, WHITE)).toBeCloseTo(1, 5);
    expect(contrastRatio(WHITE, BLACK)).toBeCloseTo(contrastRatio(BLACK, WHITE), 10);
  });

  it("matches a known mid-grey value (#767676 on white is the 4.54:1 AA threshold)", () => {
    const grey = parseColor("#767676")!;
    expect(contrastRatio(grey, WHITE)).toBeCloseTo(4.54, 2);
    expect(contrastRatio(grey, WHITE)).toBeGreaterThanOrEqual(AA_NORMAL_TEXT);
    expect(contrastRatio(parseColor("#777777")!, WHITE)).toBeLessThan(AA_NORMAL_TEXT);
  });

  it("blend paints a translucent color over an opaque one", () => {
    expect(blend({ r: 0, g: 0, b: 0, a: 0.5 }, WHITE)).toEqual({ r: 128, g: 128, b: 128, a: 1 });
    expect(blend({ r: 10, g: 20, b: 30, a: 1 }, WHITE)).toEqual({ r: 10, g: 20, b: 30, a: 1 });
    expect(blend({ r: 10, g: 20, b: 30, a: 0 }, WHITE)).toEqual(WHITE);
  });

  it("contrastChecks reports the four pairs and flags a failing one", () => {
    const good: Palette = {
      background: "#ffffff",
      text: "#000000",
      subtext: "#444444",
      selection: "rgba(0, 0, 0, 0.1)",
      accent: "#000000",
      on_accent: "#ffffff",
    };
    const checks = contrastChecks(good, {});
    expect(checks.map((c) => c.id)).toEqual(["text", "selection", "subtext", "on_accent"]);
    expect(checks.every((c) => c.aa)).toBe(true);

    const bad = contrastChecks({ ...good, subtext: "#cccccc" }, {});
    expect(bad.find((c) => c.id === "subtext")!.aa).toBe(false);
    expect(bad.find((c) => c.id === "text")!.aa).toBe(true);
  });

  it("contrastChecks uses the selected-text color only when it is set", () => {
    const palette: Palette = { background: "#ffffff", text: "#000000", selection: "#ffffff" };
    const follows = contrastChecks(palette, {}).find((c) => c.id === "selection")!;
    const own = contrastChecks({ ...palette, selection_text: "#fefefe" }, {}).find((c) => c.id === "selection")!;
    expect(follows.ratio).toBeCloseTo(21, 5);
    expect(own.aa).toBe(false);
  });

  it("contrastChecks takes missing colors from the defaults", () => {
    const defaults: Palette = { background: "#000000", text: "#ffffff", subtext: "#bbbbbb", selection: "#222222", accent: "#fbbf24", on_accent: "#000000" };
    const checks = contrastChecks({}, defaults);
    expect(checks.every((c) => c.aa)).toBe(true);
  });
});

describe("deriveSecondary", () => {
  const base: Palette = { background: "#ffffff", text: "#1c1b2e", accent: "#f59e0b" };

  it("keeps the main colors and fills every secondary color a theme file lists", () => {
    const light = deriveSecondary(base, "light");
    expect(light.background).toBe("#ffffff");
    expect(light.text).toBe("#1c1b2e");
    expect(light.accent).toBe("#f59e0b");
    for (const key of ["border", "accent_strong", "on_accent", "selection", "tile", "kbd_background", "kbd_border", "surface", "input_background", "input_border", "switch_off"]) {
      expect(light[key], key).toBeTruthy();
      expect(isColor(light[key]), key).toBe(true);
    }
  });

  it("makes the hover accent darker and picks readable text on the accent", () => {
    const derived = deriveSecondary(base, "light");
    const accent = parseColor("#f59e0b")!;
    const strong = parseColor(derived.accent_strong)!;
    expect(strong.r).toBeLessThan(accent.r);
    expect(contrastRatio(parseColor(derived.on_accent)!, accent)).toBeGreaterThan(4.5);
    // a dark accent gets light text
    const dark = deriveSecondary({ ...base, accent: "#1d4ed8" }, "light");
    expect(parseColor(dark.on_accent)).toMatchObject({ r: 255, g: 255, b: 255 });
  });

  it("differs between light and dark modes", () => {
    expect(deriveSecondary(base, "light").input_background).toBe("#ffffff");
    expect(deriveSecondary(base, "dark").input_background).not.toBe("#ffffff");
    expect(deriveSecondary(base, "dark").selection).toBe("rgba(245, 158, 11, 0.2)");
    expect(deriveSecondary(base, "light").selection).toBe("rgba(245, 158, 11, 0.17)");
  });

  it("does not throw on an empty palette", () => {
    const derived = deriveSecondary({}, "dark");
    expect(isColor(derived.border)).toBe(true);
  });
});

describe("font family helpers", () => {
  it("accepts names and rejects punctuation that could escape a declaration", () => {
    expect(fontFamilyError("")).toBeNull();
    expect(fontFamilyError("Segoe UI, sans-serif")).toBeNull();
    expect(fontFamilyError("Fira_Code-2.0")).toBeNull();
    expect(fontFamilyError("a;b")).not.toBeNull();
    expect(fontFamilyError("a{b}")).not.toBeNull();
    expect(fontFamilyError("a".repeat(201))).toBe("At most 200 characters");
  });

  it("quotes names and keeps generic families bare", () => {
    expect(fontFamilyCss("Segoe UI, sans-serif")).toBe('"Segoe UI", sans-serif');
    expect(fontFamilyCss("'Fira Code', MONOSPACE")).toBe('"Fira Code", monospace');
    expect(fontFamilyCss("system-ui")).toBe("system-ui");
  });

  it("gives an empty string for empty, invalid or holey lists", () => {
    expect(fontFamilyCss("")).toBe("");
    expect(fontFamilyCss("  ")).toBe("");
    expect(fontFamilyCss("a;b")).toBe("");
    expect(fontFamilyCss("Arial,,serif")).toBe("");
    expect(fontFamilyCss("Arial, ")).toBe("");
  });
});

describe("paletteFor and the preview", () => {
  const light: Palette = { background: "#ffffff", text: "#000000", accent: "#f59e0b" };
  const dark: Palette = { background: "#000000", text: "#ffffff", accent: "#fbbf24" };
  const fallback: Palette = { background: "#eeeeee" };

  it("prefers the requested mode, then the only one a theme has, then the fallback", () => {
    const both = { ...emptySpec(), light, dark };
    expect(paletteFor(both, "dark", fallback)).toEqual({ mode: "dark", palette: dark });
    const lightOnly = { ...emptySpec(), light };
    expect(paletteFor(lightOnly, "dark", fallback)).toEqual({ mode: "light", palette: light });
    expect(paletteFor(emptySpec(), "dark", fallback)).toEqual({ mode: "dark", palette: fallback });
  });

  it("previewVariables sets the palette's CSS variables and the sizes", () => {
    const spec = {
      ...emptySpec(),
      font: { family: "Segoe UI, sans-serif", size: 18 },
      layout: { ...emptySpec().layout, radius: 8, opacity: 90, row_height: 56 },
    };
    const vars = previewVariables(spec, { ...light, selection_text: "#111111" }, dark, "light");
    expect(vars["color-scheme"]).toBe("light");
    expect(vars["--bg"]).toBe("#ffffff");
    expect(vars["--fg"]).toBe("#000000");
    expect(vars["--selected-fg"]).toBe("#111111");
    expect(vars["--font-size"]).toBe("18px");
    expect(vars["--font-scale"]).toBe(String(18 / SIZE_DEFAULTS.font_size));
    expect(vars["--radius"]).toBe("8px");
    expect(vars["--card-opacity"]).toBe("0.9");
    expect(vars["--row-h"]).toBe("56px");
    expect(vars["font-family"]).toBe('"Segoe UI", sans-serif');
    expect(vars["--search-size"]).toBeUndefined();
  });

  it("falls back to default colors and sizes, and drops --selected-fg when unset", () => {
    const vars = previewVariables(emptySpec(), { background: "#ffffff" }, { ...dark, selection_text: "#123456" }, "light");
    expect(vars["--bg"]).toBe("#ffffff");
    expect(vars["--fg"]).toBe("#ffffff"); // from the defaults
    expect(vars["--selected-fg"]).toBeUndefined();
    expect(vars["--radius"]).toBe(`${SIZE_DEFAULTS.radius}px`);
    expect(vars["--card-opacity"]).toBe("1");
    expect(vars["font-family"]).toBeUndefined();
  });

  it("miniVars keeps only the variables a theme card needs", () => {
    const spec = { ...emptySpec(), light };
    const vars = miniVars(spec, "light", { light, dark });
    expect(Object.keys(vars).sort()).toEqual(["--accent", "--bg", "--fg", "color-scheme"]);
    expect(vars["color-scheme"]).toBe("light");
  });

  it("applyVariables sets new properties, removes stale ones and returns the applied names", () => {
    const store = new Map<string, string>();
    const element = {
      style: {
        setProperty: (name: string, value: string) => store.set(name, value),
        removeProperty: (name: string) => store.delete(name),
      },
    } as unknown as HTMLElement;
    const first = applyVariables(element, { "--a": "1", "--b": "2" }, []);
    expect(first).toEqual(["--a", "--b"]);
    const second = applyVariables(element, { "--a": "3" }, first);
    expect(second).toEqual(["--a"]);
    expect([...store]).toEqual([["--a", "3"]]);
  });
});

describe("History (undo and redo)", () => {
  it("undoes and redoes in order", () => {
    const history = new History(0);
    history.record("a", "x", 1);
    history.record("b", "x", 2);
    expect(history.canUndo).toBe(true);
    expect(history.undo("c")).toBe("b");
    expect(history.undo("b")).toBe("a");
    expect(history.undo("a")).toBeNull();
    expect(history.canRedo).toBe(true);
    expect(history.redo("a")).toBe("b");
    expect(history.redo("b")).toBe("c");
    expect(history.redo("c")).toBeNull();
  });

  it("merges edits to the same field made within the window into one step", () => {
    const history = new History(700);
    history.record("v0", "slider", 1000);
    history.record("v1", "slider", 1200);
    history.record("v2", "slider", 1500);
    expect(history.undo("v3")).toBe("v0");
    expect(history.undo("v0")).toBeNull();
  });

  it("starts a new step for another field, or after the window", () => {
    const history = new History(700);
    history.record("v0", "slider", 1000);
    history.record("v1", "color", 1100);
    history.record("v2", "color", 5000);
    expect(history.undo("v3")).toBe("v2");
    expect(history.undo("v2")).toBe("v1");
    expect(history.undo("v1")).toBe("v0");
  });

  it("clears the redo stack on a new edit and on clear()", () => {
    const history = new History(0);
    history.record("a", "x", 1);
    history.undo("b");
    expect(history.canRedo).toBe(true);
    history.record("a", "y", 2);
    expect(history.canRedo).toBe(false);
    history.clear();
    expect(history.canUndo).toBe(false);
  });

  it("keeps at most 100 steps", () => {
    const history = new History(0);
    for (let i = 0; i < 150; i++) history.record(String(i), `k${i}`, i);
    let steps = 0;
    let current = "now";
    for (let next = history.undo(current); next !== null; next = history.undo(current)) {
      current = next;
      steps++;
    }
    expect(steps).toBe(100);
    expect(current).toBe("50");
  });
});

describe("theme JSON as the backend sends it (builtin-themes.json)", () => {
  it("has a light and a dark built-in with unique names and warning-free files", () => {
    expect(themes.length).toBeGreaterThanOrEqual(2);
    const names = themes.map((t) => t.spec.name);
    expect(new Set(names).size).toBe(names.length);
    for (const theme of themes) {
      expect(theme.file).toMatch(/^themes\/.+\.toml$/);
      expect(theme.warnings, theme.file).toEqual([]);
      expect(theme.spec.light || theme.spec.dark, theme.file).toBeTruthy();
    }
  });

  it("gives every palette every color key, in a form the editor and Rust both accept", () => {
    for (const theme of themes) {
      for (const mode of ["light", "dark"] as const) {
        const palette = theme.spec[mode];
        if (!palette) continue;
        for (const field of COLOR_FIELDS) {
          const value = palette[field.key];
          if (value === undefined) {
            expect(field.optional, `${theme.spec.name}/${mode} lacks ${field.key}`).toBe(true);
            continue;
          }
          if (field.kind === "shadow") expect(isShadow(value), `${theme.spec.name}/${mode}/${field.key}`).toBe(true);
          else expect(isColor(value), `${theme.spec.name}/${mode}/${field.key}: ${value}`).toBe(true);
        }
      }
    }
  });

  it("only uses keys the editor knows", () => {
    const known = new Set(COLOR_FIELDS.map((f) => f.key));
    for (const theme of themes) {
      for (const mode of ["light", "dark"] as const) {
        for (const key of Object.keys(theme.spec[mode] ?? {})) {
          expect(known.has(key), `${theme.spec.name}/${mode}/${key}`).toBe(true);
        }
      }
    }
  });

  it("keeps the built-in text readable (AA) in every palette", () => {
    for (const theme of themes) {
      for (const mode of ["light", "dark"] as const) {
        const palette = theme.spec[mode];
        if (!palette) continue;
        for (const check of contrastChecks(palette, {})) {
          expect(check.aa, `${theme.spec.name}/${mode}: ${check.label} is ${check.ratio.toFixed(2)}`).toBe(true);
        }
      }
    }
  });

  it("keeps sizes inside the slider limits", () => {
    const within = (value: number | null, [min, max]: readonly [number, number]) =>
      value === null || (value >= min && value <= max);
    for (const { spec } of themes) {
      expect(within(spec.font.size, LIMITS.font_size), spec.name).toBe(true);
      expect(within(spec.layout.radius, LIMITS.radius), spec.name).toBe(true);
      expect(within(spec.layout.opacity, LIMITS.opacity), spec.name).toBe(true);
      expect(within(spec.layout.row_height, LIMITS.row_height), spec.name).toBe(true);
      expect(within(spec.layout.search_size, LIMITS.search_size), spec.name).toBe(true);
      expect(within(spec.layout.icon_size, LIMITS.icon_size), spec.name).toBe(true);
      expect(within(spec.layout.window_width, LIMITS.window_width), spec.name).toBe(true);
    }
  });

  it("survives a clone and a JSON round trip unchanged", () => {
    for (const theme of themes) {
      expect(clone(theme)).toEqual(theme);
      expect(JSON.parse(JSON.stringify(theme.spec))).toEqual(theme.spec);
    }
  });

  it("has an empty spec that no palette or size overrides", () => {
    const spec = emptySpec();
    expect(spec.light).toBeNull();
    expect(spec.dark).toBeNull();
    expect(Object.values(spec.layout).every((v) => v === null)).toBe(true);
    expect(clone(spec)).not.toBe(spec);
  });
});
