// Dev-only stand-in for the theme backend, so the editor can be tried in a plain
// browser (`npm run dev`) where there is no Rust. Never imported in production
// builds. `builtin-themes.json` is generated from the Rust built-ins (a test in
// sevak-core keeps it current).

import builtinThemes from "./builtin-themes.json";
import type { Result } from "./theme-ipc";
import { clone, type GalleryItem, type StoredTheme, type ThemeSpec, type ThemesDto } from "./themes";

const builtin = builtinThemes as StoredTheme[];

/** Themes "saved" in the browser preview (lost on reload). */
const savedThemes: StoredTheme[] = [];
const installedGallery = new Set<string>();

/** Community themes the preview's gallery offers beyond the built-ins. */
const communityThemes: Record<string, ThemeSpec> = {
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

export function mockGallery(): Result<GalleryItem[]> {
  const base = "https://raw.githubusercontent.com/ninad-k/Sevak/main/gallery/themes/";
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
    value: [
      ...builtin.map((b) => entry(slug(b.spec.name), b.spec)),
      ...Object.entries(communityThemes).map(([id, spec]) => entry(id, spec)),
    ],
  };
}

export function mockInstallGallery(id: string): Result<StoredTheme> {
  const found = builtin.find((b) => slug(b.spec.name) === id);
  const spec = found ? clone(found.spec) : communityThemes[id] ? clone(communityThemes[id]) : null;
  if (!spec) return { ok: false, error: "Open the gallery again; that theme is no longer in the list." };
  installedGallery.add(id);
  const stored: StoredTheme = { file: themeFile(spec.name), spec, warnings: [] };
  remember(stored);
  return { ok: true, value: clone(stored) };
}
