// Dev-only stand-in for the theme backend, so the editor can be tried in a plain
// browser (`npm run dev`) where there is no Rust. Never imported in production
// builds. `builtin-themes.json` is generated from the Rust built-ins (a test in
// sevak-core keeps it current).

import builtinThemes from "./builtin-themes.json";
import type { Result } from "./theme-ipc";
import { clone, type GalleryItem, type StoredTheme, type ThemeGallery, type ThemeSpec, type ThemesDto } from "./themes";

const builtin = builtinThemes as StoredTheme[];

/** Themes "saved" in the browser preview (lost on reload). */
const savedThemes: StoredTheme[] = [];
const installedGallery = new Set<string>();

/** Community themes the preview's gallery offers beyond the built-ins. */
const communityThemes: Record<string, ThemeSpec> = {
  "catppuccin-mocha": {
    "name": "Catppuccin Mocha",
    "author": "Sevak; palette by Catppuccin",
    "description": "Soft mauve accents and deep blue-gray surfaces from the Catppuccin Mocha palette.",
    "font": {
      "size": 15,
      "family": ""
    },
    "layout": {
      "radius": 14,
      "opacity": 100,
      "row_height": 48,
      "search_size": 22,
      "icon_size": 32,
      "window_width": 720
    },
    "dark": {
      "background": "#1e1e2e",
      "text": "#cdd6f4",
      "subtext": "#bac2de",
      "border": "#45475a",
      "accent": "#cba6f7",
      "accent_strong": "#b4befe",
      "on_accent": "#1e1e2e",
      "selection": "rgba(203, 166, 247, 0.18)",
      "tile": "#313244",
      "kbd_background": "#313244",
      "kbd_border": "#585b70",
      "shadow": "0 8px 28px rgba(17, 17, 27, 0.55), 0 1px 3px rgba(17, 17, 27, 0.4)",
      "surface": "#181825",
      "input_background": "#313244",
      "input_border": "#585b70",
      "switch_off": "#585b70",
      "warn": "#f9e2af",
      "error": "#f38ba8",
      "ok": "#a6e3a1"
    },
    "light": null
  },
  "midnight-ocean": {
    name: "Midnight Ocean",
    author: "someone",
    description: "Deep blues with a coral accent.",
    font: { family: "", size: null },
    layout: { radius: 20, opacity: null, row_height: null, search_size: null, icon_size: null, window_width: null },
    light: null,
    dark: {
      ...(builtin[2].spec.dark ?? {}),
      background: "#0b1d2e",
      text: "#e6f1ff",
      subtext: "#9db6d1",
      accent: "#ff7a6b",
      accent_strong: "#d16356",
      on_accent: "#0b1d2e",
      selection: "rgba(255, 122, 107, 0.2)",
    },
  },
};

const themeFile = (name: string) => `themes/${name}.toml`;
const slug = (name: string) => name.toLowerCase().replace(/ /g, "-");

function remember(stored: StoredTheme): void {
  const at = savedThemes.findIndex((t) => t.file === stored.file);
  if (at >= 0) savedThemes[at] = stored;
  else savedThemes.push(stored);
}

export function mockThemes(): ThemesDto {
  return {
    builtin: clone(builtin),
    installed: clone(savedThemes),
    themes_dir: "C:/Users/someone/AppData/Roaming/sevak/themes",
  };
}

export function mockSaveTheme(theme: ThemeSpec): Result<StoredTheme> {
  const name = theme.name.trim();
  if (name === "") return { ok: false, error: "Give the theme a name first." };
  if (builtin.some((b) => b.spec.name.toLowerCase() === name.toLowerCase())) {
    return { ok: false, error: `"${name}" is the name of a built-in theme. Choose another name.` };
  }
  const stored: StoredTheme = { file: themeFile(name), spec: { ...clone(theme), name }, warnings: [] };
  remember(stored);
  return { ok: true, value: clone(stored) };
}

export function mockUseBuiltin(name: string): Result<StoredTheme> {
  const found = builtin.find((b) => b.spec.name.toLowerCase() === name.toLowerCase());
  if (!found) return { ok: false, error: `There is no built-in theme "${name}".` };
  return { ok: true, value: clone(found) };
}

const fakeHash = (id: string) => (id.length.toString(16).padStart(2, "0") + "ab12cd34ef56").repeat(6).slice(0, 64);

export function mockGallery(): Result<ThemeGallery> {
  const base = "https://raw.githubusercontent.com/ninad-k/Sevak/v0.1.0/gallery/themes/";
  const entry = (id: string, spec: ThemeSpec): GalleryItem => ({
    id,
    name: spec.name,
    author: spec.author,
    description: spec.description,
    mode: spec.light && !spec.dark ? "light" : "dark",
    url: base + spec.name.replace(/ /g, "-") + ".toml",
    sha256: fakeHash(id),
    installed: installedGallery.has(id),
  });
  return {
    ok: true,
    value: {
      source: "https://raw.githubusercontent.com/ninad-k/Sevak/v0.1.0/gallery/themes.json",
      note: null,
      items: [
        ...builtin.map((b) => entry(slug(b.spec.name), b.spec)),
        ...Object.entries(communityThemes).map(([id, spec]) => entry(id, spec)),
      ],
    },
  };
}

export function mockInstallGallery(id: string, replace = false): Result<StoredTheme> {
  const found = builtin.find((b) => slug(b.spec.name) === id);
  const spec = found ? clone(found.spec) : communityThemes[id] ? clone(communityThemes[id]) : null;
  if (!spec) return { ok: false, error: "Open the gallery again; that theme is no longer in the list." };
  if (installedGallery.has(id) && !replace) {
    return {
      ok: false,
      error: `A theme named "${spec.name}" is already in your themes folder, so it was not replaced. Remove it first, or use Reinstall to replace it.`,
    };
  }
  installedGallery.add(id);
  const stored: StoredTheme = { file: themeFile(spec.name), spec, warnings: [] };
  remember(stored);
  return { ok: true, value: clone(stored) };
}
