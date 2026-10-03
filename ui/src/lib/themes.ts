// Theme files in the frontend: the shapes the backend sends, color math for
// the editor (parsing, alpha, WCAG contrast) and the CSS variables a preview
// sets. Rust (`sevak_core::theme_file`) validates everything again on save and
// stays the authority; the checks here only give instant feedback.

export type Mode = "light" | "dark";

/** Palette entry key -> normalized value (`#rrggbb`, `rgba(r, g, b, a)` or a shadow). */
export type Palette = Record<string, string>;

export interface ThemeSpec {
  name: string;
  author: string;
  description: string;
  font: { family: string; size: number | null };
  layout: {
    radius: number | null;
    opacity: number | null;
    row_height: number | null;
    search_size: number | null;
    icon_size: number | null;
    window_width: number | null;
  };
  light: Palette | null;
  dark: Palette | null;
}

/** A theme file on disk, or a built-in with the file it would be written to. */
export interface StoredTheme {
  file: string;
  spec: ThemeSpec;
  warnings: string[];
}

export interface ThemesDto {
  builtin: StoredTheme[];
  installed: StoredTheme[];
  themes_dir: string;
}

export interface GalleryItem {
  id: string;
  name: string;
  author: string;
  description: string;
  mode: string;
  url: string;
  sha256: string;
  installed: boolean;
}

export interface ColorField {
  key: string;
  /** The CSS variable the key sets. */
  cssVar: string;
  label: string;
  hint: string;
  kind: "color" | "shadow";
  /** Shown under "More colors" rather than up front. */
  advanced: boolean;
  /** Leaving it out is fine (`selection_text` follows `text`). */
  optional?: boolean;
}

/** Every palette entry, in the order of theme files. Mirrors `COLOR_KEYS` in Rust. */
export const COLOR_FIELDS: ColorField[] = [
  { key: "background", cssVar: "--bg", label: "Background", hint: "The search bar", kind: "color", advanced: false },
  { key: "text", cssVar: "--fg", label: "Text", hint: "Result titles and what you type", kind: "color", advanced: false },
  { key: "subtext", cssVar: "--muted", label: "Subtext", hint: "Subtitles, hints, placeholder", kind: "color", advanced: false },
  { key: "selection", cssVar: "--selected", label: "Selection", hint: "Background of the selected row", kind: "color", advanced: false },
  { key: "selection_text", cssVar: "--selected-fg", label: "Selected text", hint: "Title of the selected row; empty follows Text", kind: "color", advanced: false, optional: true },
  { key: "accent", cssVar: "--accent", label: "Accent", hint: "Caret, buttons, switches", kind: "color", advanced: false },
  { key: "border", cssVar: "--border", label: "Border", hint: "Hairlines and the bar's outline", kind: "color", advanced: false },
  { key: "shadow", cssVar: "--shadow", label: "Shadow", hint: "A box-shadow, e.g. 0 8px 28px rgba(0, 0, 0, 0.5), or none", kind: "shadow", advanced: false },
  { key: "accent_strong", cssVar: "--accent-strong", label: "Accent (hover)", hint: "Hover and focus accent", kind: "color", advanced: true },
  { key: "on_accent", cssVar: "--on-accent", label: "Text on accent", hint: "Text on an accent background", kind: "color", advanced: true },
  { key: "tile", cssVar: "--tile", label: "Icon tile", hint: "Behind result icons", kind: "color", advanced: true },
  { key: "kbd_background", cssVar: "--kbd-bg", label: "Key cap", hint: "Key hints such as Ctrl+1", kind: "color", advanced: true },
  { key: "kbd_border", cssVar: "--kbd-border", label: "Key cap border", hint: "", kind: "color", advanced: true },
  { key: "surface", cssVar: "--surface", label: "Panel", hint: "Panels in Settings", kind: "color", advanced: true },
  { key: "input_background", cssVar: "--input-bg", label: "Field", hint: "Form controls in Settings", kind: "color", advanced: true },
  { key: "input_border", cssVar: "--input-border", label: "Field border", hint: "", kind: "color", advanced: true },
  { key: "switch_off", cssVar: "--switch-off", label: "Switch (off)", hint: "Toggle and slider tracks", kind: "color", advanced: true },
  { key: "warn", cssVar: "--warn", label: "Warning", hint: "Notices", kind: "color", advanced: true },
  { key: "error", cssVar: "--error", label: "Error", hint: "Error messages", kind: "color", advanced: true },
  { key: "ok", cssVar: "--ok", label: "Success", hint: "Success marks", kind: "color", advanced: true },
];

/** Limits of the sliders. Mirror `theme.rs` / `theme_file.rs`. */
export const LIMITS = {
  font_size: [12, 22],
  radius: [0, 32],
  opacity: [30, 100],
  row_height: [32, 96],
  search_size: [14, 40],
  icon_size: [16, 64],
  window_width: [400, 1600],
} as const;

/** What an unset size looks like (the launcher's own defaults). */
export const SIZE_DEFAULTS = {
  font_size: 15,
  radius: 14,
  opacity: 100,
  row_height: 48,
  search_size: 22,
  icon_size: 32,
  window_width: 720,
} as const;

export const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T;

export const emptySpec = (): ThemeSpec => ({
  name: "",
  author: "",
  description: "",
  font: { family: "", size: null },
  layout: {
    radius: null,
    opacity: null,
    row_height: null,
    search_size: null,
    icon_size: null,
    window_width: null,
  },
  light: null,
  dark: null,
});

// ---------------------------------------------------------------------------
// Colors
// ---------------------------------------------------------------------------

export interface Rgba {
  r: number;
  g: number;
  b: number;
  a: number;
}

const channel = (text: string): number | null => {
  if (!/^\d{1,3}$/.test(text)) return null;
  const n = Number(text);
  return n <= 255 ? n : null;
};

/** `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()` and `rgba()`; `null` if it is none of these. */
export function parseColor(input: string): Rgba | null {
  const text = input.trim();
  if (text.startsWith("#")) {
    const hex = text.slice(1);
    if (!/^[0-9a-f]+$/i.test(hex)) return null;
    if (hex.length === 3 || hex.length === 4) {
      const d = [...hex].map((c) => parseInt(c + c, 16));
      return { r: d[0], g: d[1], b: d[2], a: hex.length === 4 ? d[3] / 255 : 1 };
    }
    if (hex.length === 6 || hex.length === 8) {
      const d = [0, 2, 4, 6].map((i) => parseInt(hex.slice(i, i + 2), 16));
      return { r: d[0], g: d[1], b: d[2], a: hex.length === 8 ? d[3] / 255 : 1 };
    }
    return null;
  }
  const match = /^rgba?\((.*)\)$/i.exec(text);
  if (!match) return null;
  const parts = match[1].split(/[\s,/]+/).filter((p) => p !== "");
  if (parts.length !== 3 && parts.length !== 4) return null;
  const rgb = parts.slice(0, 3).map(channel);
  if (rgb.some((c) => c === null)) return null;
  let a = 1;
  if (parts.length === 4) {
    const raw = parts[3];
    a = raw.endsWith("%") ? Number(raw.slice(0, -1)) / 100 : Number(raw);
    if (!Number.isFinite(a) || a < 0 || a > 1 || raw === "") return null;
  }
  return { r: rgb[0]!, g: rgb[1]!, b: rgb[2]!, a };
}

const hex2 = (n: number) => Math.round(n).toString(16).padStart(2, "0");

/** `#rrggbb` when opaque, else `rgba(r, g, b, a)`, like Rust's `format_color`. */
export function formatColor({ r, g, b, a }: Rgba): string {
  if (a >= 0.9995) return `#${hex2(r)}${hex2(g)}${hex2(b)}`;
  const alpha = String(Number(a.toFixed(3)));
  return `rgba(${Math.round(r)}, ${Math.round(g)}, ${Math.round(b)}, ${alpha})`;
}

/** `#rrggbb` for `<input type="color">`, ignoring alpha. */
export function toHex6(text: string, fallback = "#000000"): string {
  const c = parseColor(text);
  return c ? `#${hex2(c.r)}${hex2(c.g)}${hex2(c.b)}` : fallback;
}

/** `text` with its opacity replaced. */
export function withAlpha(text: string, alpha: number): string {
  const c = parseColor(text);
  return c ? formatColor({ ...c, a: alpha }) : text;
}

/** `text` with its color replaced by `#rrggbb` and its alpha kept. */
export function withRgb(text: string, hex: string): string {
  const alpha = parseColor(text)?.a ?? 1;
  const c = parseColor(hex);
  return c ? formatColor({ ...c, a: alpha }) : text;
}

export const isColor = (text: string) => parseColor(text) !== null;

/** Splits at `separator` outside parentheses; `null` when they do not balance. */
function splitTop(text: string, separator: RegExp): string[] | null {
  const parts: string[] = [];
  let depth = 0;
  let current = "";
  for (const ch of text) {
    if (ch === "(") depth++;
    if (ch === ")" && --depth < 0) return null;
    if (depth === 0 && separator.test(ch)) {
      parts.push(current);
      current = "";
    } else {
      current += ch;
    }
  }
  if (depth !== 0) return null;
  parts.push(current);
  return parts;
}

/** The same grammar as Rust's `normalize_shadow`: `none`, or up to 6 `[inset] x y [blur [spread]] color` layers. */
export function isShadow(input: string): boolean {
  const text = input.trim();
  if (text.toLowerCase() === "none") return true;
  const layers = splitTop(text, /,/);
  if (!layers || layers.length > 6) return false;
  return layers.every((layer) => {
    const tokens = splitTop(layer.trim(), /\s/)?.filter((t) => t !== "");
    if (!tokens) return false;
    let inset = false;
    let lengths = 0;
    let color = false;
    for (const token of tokens) {
      if (token.toLowerCase() === "inset" && !inset) inset = true;
      else if (/^-?(\d+\.?\d*|\.\d+)(px)?$/.test(token) && Math.abs(parseFloat(token)) <= 200) lengths++;
      else if (!color && isColor(token)) color = true;
      else return false;
    }
    return color && lengths >= 2 && lengths <= 4;
  });
}

// ---------------------------------------------------------------------------
// Contrast (WCAG 2.x), the same math as `contrast_checks` in Rust
// ---------------------------------------------------------------------------

export const AA_NORMAL_TEXT = 4.5;

const linear = (v: number) => {
  const c = v / 255;
  return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
};

const luminance = ({ r, g, b }: Rgba) => 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);

export function contrastRatio(a: Rgba, b: Rgba): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

/** `top` painted over the opaque `bottom`. */
export function blend(top: Rgba, bottom: Rgba): Rgba {
  const mix = (t: number, b: number) => Math.round(top.a * t + (1 - top.a) * b);
  return { r: mix(top.r, bottom.r), g: mix(top.g, bottom.g), b: mix(top.b, bottom.b), a: 1 };
}

export interface ContrastCheck {
  id: string;
  label: string;
  ratio: number;
  aa: boolean;
}

/** Text/background pairs of a palette; missing colors come from `defaults`. */
export function contrastChecks(palette: Palette, defaults: Palette): ContrastCheck[] {
  const get = (key: string): Rgba =>
    parseColor(palette[key] ?? "") ?? parseColor(defaults[key] ?? "") ?? { r: 0, g: 0, b: 0, a: 1 };
  const background = get("background");
  const text = get("text");
  const selection = blend(get("selection"), background);
  const selectionText = palette.selection_text ? get("selection_text") : text;
  const pair = (id: string, label: string, fg: Rgba, behind: Rgba): ContrastCheck => {
    const ratio = contrastRatio(blend(fg, behind), behind);
    return { id, label, ratio, aa: ratio >= AA_NORMAL_TEXT };
  };
  return [
    pair("text", "Text on background", text, background),
    pair("selection", "Selected row text on selection", selectionText, selection),
    pair("subtext", "Subtext on background", get("subtext"), background),
    pair("on_accent", "Text on accent buttons", get("on_accent"), get("accent")),
  ];
}

// ---------------------------------------------------------------------------
// Deriving the secondary colors
// ---------------------------------------------------------------------------

/**
 * The colors that follow from the main ones: borders, tiles, key caps, panels,
 * fields, the accent's hover shade and the selection. The same recipe the
 * built-in themes were made with.
 */
export function deriveSecondary(palette: Palette, mode: Mode): Palette {
  const dark = mode === "dark";
  const text = parseColor(palette.text ?? "") ?? { r: 0, g: 0, b: 0, a: 1 };
  const accent = parseColor(palette.accent ?? "") ?? { r: 245, g: 158, b: 11, a: 1 };
  const background = parseColor(palette.background ?? "") ?? { r: 255, g: 255, b: 255, a: 1 };
  const ofText = (a: number) => formatColor({ ...text, a });
  const shade = (c: number) => Math.round(c * 0.82);
  const onAccent =
    contrastRatio(text, accent) > contrastRatio(background, accent) ? { ...text, a: 1 } : { ...background, a: 1 };
  return {
    ...palette,
    border: ofText(0.12),
    accent_strong: formatColor({ r: shade(accent.r), g: shade(accent.g), b: shade(accent.b), a: 1 }),
    on_accent: formatColor(onAccent),
    selection: formatColor({ ...accent, a: dark ? 0.2 : 0.17 }),
    tile: ofText(dark ? 0.08 : 0.06),
    kbd_background: ofText(dark ? 0.08 : 0.06),
    kbd_border: ofText(dark ? 0.16 : 0.14),
    surface: ofText(dark ? 0.05 : 0.035),
    input_background: dark ? ofText(0.06) : "#ffffff",
    input_border: ofText(dark ? 0.24 : 0.22),
    switch_off: ofText(dark ? 0.28 : 0.26),
  };
}

// ---------------------------------------------------------------------------
// Preview
// ---------------------------------------------------------------------------

/** The palette to show: `mode`'s if the theme has it, else the only one, else `fallback`. */
export function paletteFor(spec: ThemeSpec, mode: Mode, fallback: Palette): { mode: Mode; palette: Palette } {
  if (spec[mode]) return { mode, palette: spec[mode] };
  const other: Mode = mode === "light" ? "dark" : "light";
  if (spec[other]) return { mode: other, palette: spec[other] };
  return { mode, palette: fallback };
}

/**
 * The CSS custom properties that make the launcher components look like the
 * theme: its palette (missing colors from `defaults`) and sizes. The preview
 * sets them on its own container, so the Settings window keeps its own look.
 */
export function previewVariables(
  spec: ThemeSpec,
  palette: Palette,
  defaults: Palette,
  mode: Mode,
): Record<string, string> {
  const vars: Record<string, string> = { "color-scheme": mode };
  for (const field of COLOR_FIELDS) {
    const value = palette[field.key] ?? defaults[field.key];
    if (value) vars[field.cssVar] = value;
  }
  // An unset selection_text follows the text color.
  if (!palette.selection_text) delete vars["--selected-fg"];
  const fontSize = spec.font.size ?? SIZE_DEFAULTS.font_size;
  vars["--font-size"] = `${fontSize}px`;
  vars["--font-scale"] = String(fontSize / SIZE_DEFAULTS.font_size);
  vars["--radius"] = `${spec.layout.radius ?? SIZE_DEFAULTS.radius}px`;
  vars["--card-opacity"] = String((spec.layout.opacity ?? SIZE_DEFAULTS.opacity) / 100);
  const family = fontFamilyCss(spec.font.family);
  if (family) vars["font-family"] = family;
  if (spec.layout.row_height != null) vars["--row-h"] = `${spec.layout.row_height}px`;
  if (spec.layout.search_size != null) vars["--search-size"] = `${spec.layout.search_size}px`;
  if (spec.layout.icon_size != null) vars["--icon-size"] = `${spec.layout.icon_size}px`;
  return vars;
}

/** Sets `vars` on `element` (and removes the ones it set before that are gone). */
export function applyVariables(element: HTMLElement, vars: Record<string, string>, previous: string[]): string[] {
  for (const name of previous) if (!(name in vars)) element.style.removeProperty(name);
  for (const [name, value] of Object.entries(vars)) element.style.setProperty(name, value);
  return Object.keys(vars);
}

/** Svelte action: keeps `vars` set on the element as inline custom properties. */
export function cssVars(node: HTMLElement, vars: Record<string, string>) {
  let applied = applyVariables(node, vars, []);
  return {
    update(next: Record<string, string>) {
      applied = applyVariables(node, next, applied);
    },
  };
}

/** The few variables a theme card's miniature needs. */
export function miniVars(spec: ThemeSpec, mode: Mode, defaults: Record<Mode, Palette>): Record<string, string> {
  const { mode: shown, palette } = paletteFor(spec, mode, defaults[mode]);
  const vars = previewVariables(spec, palette, defaults[shown], shown);
  const keep = ["--bg", "--fg", "--muted", "--accent", "--selected", "--selected-fg", "--tile", "--border", "--shadow", "color-scheme"];
  return Object.fromEntries(Object.entries(vars).filter(([name]) => keep.includes(name)));
}

/** Where the online gallery's index lives; fetched only when the user asks. */
export const GALLERY_INDEX_URL = "https://raw.githubusercontent.com/ninad-k/Sevak/main/gallery/themes.json";

/** Valid for `font-family` in a theme: names of letters, digits, spaces and `- _ .`. */
export function fontFamilyError(family: string): string | null {
  const text = family.trim();
  if (text === "") return null;
  if (text.length > 200) return "At most 200 characters";
  return /^[\p{L}\p{N} ,._'"-]+$/u.test(text)
    ? null
    : "Only letters, digits, spaces and , . _ - are allowed";
}

const GENERIC_FAMILIES = new Set([
  "serif", "sans-serif", "monospace", "cursive", "fantasy", "system-ui", "ui-serif",
  "ui-sans-serif", "ui-monospace", "ui-rounded", "math", "emoji",
]);

/** A `font-family` value from a comma-separated list, quoting names like the backend does; `""` if invalid or empty. */
export function fontFamilyCss(list: string): string {
  if (fontFamilyError(list) !== null || list.trim() === "") return "";
  const families: string[] = [];
  for (const raw of list.split(",")) {
    const name = raw.trim().replace(/^(["'])(.*)\1$/, "$2").trim();
    if (name === "") return "";
    families.push(GENERIC_FAMILIES.has(name.toLowerCase()) ? name.toLowerCase() : `"${name.replace(/["']/g, "")}"`);
  }
  return families.join(", ");
}

/** Common font stacks to suggest when the installed fonts cannot be listed. */
export const FONT_SUGGESTIONS = [
  "system-ui, sans-serif",
  "Segoe UI, sans-serif",
  "Inter, sans-serif",
  "Roboto, sans-serif",
  "Noto Sans, sans-serif",
  "Ubuntu, sans-serif",
  "Cantarell, sans-serif",
  "Helvetica Neue, sans-serif",
  "Arial, sans-serif",
  "Georgia, serif",
  "Cascadia Mono, monospace",
  "JetBrains Mono, monospace",
  "Fira Code, monospace",
  "Menlo, monospace",
  "Consolas, monospace",
];

// ---------------------------------------------------------------------------
// Undo / redo
// ---------------------------------------------------------------------------

/**
 * Snapshots for undo and redo. Edits to the same field within `window`
 * milliseconds (a slider being dragged, a hex field being typed in) become one
 * step.
 */
export class History {
  private past: string[] = [];
  private future: string[] = [];
  private lastKey = "";
  private lastTime = 0;

  constructor(private readonly window = 700) {}

  /** Call before changing `current`. */
  record(current: string, key: string, now = Date.now()): void {
    const merge = key === this.lastKey && now - this.lastTime < this.window && this.past.length > 0;
    this.lastKey = key;
    this.lastTime = now;
    if (merge) return;
    this.past.push(current);
    if (this.past.length > 100) this.past.shift();
    this.future = [];
  }

  get canUndo(): boolean {
    return this.past.length > 0;
  }

  get canRedo(): boolean {
    return this.future.length > 0;
  }

  undo(current: string): string | null {
    const previous = this.past.pop();
    if (previous === undefined) return null;
    this.future.push(current);
    this.lastKey = "";
    return previous;
  }

  redo(current: string): string | null {
    const next = this.future.pop();
    if (next === undefined) return null;
    this.past.push(current);
    this.lastKey = "";
    return next;
  }

  clear(): void {
    this.past = [];
    this.future = [];
    this.lastKey = "";
  }
}
