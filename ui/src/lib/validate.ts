// Client-side checks for the settings form. They mirror `settings::validate` in
// the Rust shell, which re-checks everything on save; the shortcut itself is
// parsed by Rust only (see `validateHotkey`).

import { acceleratorId } from "./accelerator";
import {
  fallbackList,
  type Appearance,
  type Config,
  type HotkeyBinding,
  type WebSearchEngine,
} from "./settings-ipc";

export type SectionId =
  | "general"
  | "hotkeys"
  | "appearance"
  | "search"
  | "plugins"
  | "web"
  | "files"
  | "linux";

export interface EngineErrors {
  keyword?: string;
  name?: string;
  url?: string;
}

export interface HotkeyErrors {
  key?: string;
  value?: string;
}

export interface AppearanceErrors {
  accent?: string;
  fontFamily?: string;
  themeFile?: string;
  customCss?: string;
}

export interface Problems {
  engines: EngineErrors[];
  hotkeys: HotkeyErrors[];
  appearance: AppearanceErrors;
  fallback?: string;
  filesKeyword?: string;
  filesDepth?: string;
  /** Number of problems per section (hotkey problems are added by the form). */
  count: Record<SectionId, number>;
}

export const MIN_WIDTH = 400;
export const MAX_WIDTH = 1600;
export const MAX_DEPTH = 32;
export const MIN_FONT_SIZE = 12;
export const MAX_FONT_SIZE = 22;
export const MIN_OPACITY = 30;
export const MAX_OPACITY = 100;
export const MAX_RADIUS = 32;

/** A color Rust accepts: `#rgb`, `#rrggbb` or `rgb(r, g, b)`. */
export function isColor(text: string): boolean {
  const value = text.trim();
  if (/^#([0-9a-f]{3}|[0-9a-f]{6})$/i.test(value)) return true;
  const match = /^rgb\(\s*(\d{1,3})[\s,]+(\d{1,3})[\s,]+(\d{1,3})\s*\)$/i.exec(value);
  return match !== null && match.slice(1).every((channel) => Number(channel) <= 255);
}

/** Spellings of one key that Sevak treats as the same shortcut (Win and Super, order, case, spaces). */
const keyId = (key: string) => (key.trim() === "" ? "" : acceleratorId(key));

export function hotkeyErrors(
  binding: HotkeyBinding,
  all: HotkeyBinding[],
  mainKey: string,
  actionsKey = "",
): HotkeyErrors {
  const errors: HotkeyErrors = {};
  const key = keyId(binding.key);
  if (key === "") errors.key = "Required";
  else if (key === keyId(mainKey)) errors.key = "Same as the main shortcut";
  else if (key === keyId(actionsKey)) errors.key = "Same as the Universal Actions shortcut";
  else if (all.filter((other) => keyId(other.key) === key).length > 1) errors.key = "Already used";
  if (binding.run != null && binding.run.trim() === "") errors.value = "Enter a result id";
  return errors;
}

export function appearanceErrors(appearance: Appearance): AppearanceErrors {
  const errors: AppearanceErrors = {};
  const accent = appearance.accent.trim();
  if (accent !== "" && !isColor(accent)) errors.accent = "Use #rrggbb, #rgb or rgb(r, g, b)";
  const family = appearance.font_family.trim();
  if (family !== "" && !/^[\p{L}\p{N} ,._'"-]+$/u.test(family)) {
    errors.fontFamily = "Only letters, digits, spaces and , . _ - are allowed";
  }
  const css = appearance.custom_css.trim();
  if (css !== "" && (/^[\\/]|^[A-Za-z]:|(^|[\\/])\.\.([\\/]|$)/.test(css))) {
    errors.customCss = "Must be a path inside the config folder";
  }
  const themeFile = appearance.theme_file.trim();
  if (themeFile !== "" && (/^[\\/]|^[A-Za-z]:|(^|[\\/])\.\.([\\/]|$)/.test(themeFile))) {
    errors.themeFile = "Must be a path inside the config folder";
  }
  return errors;
}

export function engineErrors(engine: WebSearchEngine, all: WebSearchEngine[]): EngineErrors {
  const errors: EngineErrors = {};
  const keyword = engine.keyword.trim();
  if (keyword === "") errors.keyword = "Required";
  else if (/\s/.test(keyword)) errors.keyword = "No spaces";
  else if (all.filter((other) => other.keyword.trim().toLowerCase() === keyword.toLowerCase()).length > 1) {
    errors.keyword = "Already used";
  }
  if (engine.name.trim() === "") errors.name = "Required";
  const url = engine.url.trim();
  if (!/^https?:\/\//i.test(url)) errors.url = "Must start with http:// or https://";
  else if (!engine.url.includes("{query}")) errors.url = "Must contain {query}";
  return errors;
}

export function validate(config: Config): Problems {
  const count: Record<SectionId, number> = {
    general: 0,
    hotkeys: 0,
    appearance: 0,
    search: 0,
    plugins: 0,
    web: 0,
    files: 0,
    linux: 0,
  };

  const engines = config.web_search.map((engine) => engineErrors(engine, config.web_search));
  for (const errors of engines) count.web += Object.keys(errors).length;

  const hotkeys = config.hotkey.map((binding) =>
    hotkeyErrors(binding, config.hotkey, config.general.hotkey, config.general.actions_hotkey),
  );
  for (const errors of hotkeys) count.hotkeys += Object.keys(errors).length;

  const appearance = appearanceErrors(config.appearance);
  count.appearance += Object.keys(appearance).length;

  const keywords = new Set(config.web_search.map((engine) => engine.keyword.trim().toLowerCase()));
  const problems: Problems = { engines, hotkeys, appearance, count };

  const missing = fallbackList(config.search.fallback_web_search).find(
    (keyword) => !keywords.has(keyword.toLowerCase()),
  );
  if (missing !== undefined) {
    problems.fallback = `The engine "${missing}" no longer exists.`;
    count.search++;
  }

  const filesKeyword = config.files.keyword.trim();
  if (/\s/.test(filesKeyword)) problems.filesKeyword = "No spaces";
  else if (filesKeyword !== "" && keywords.has(filesKeyword.toLowerCase())) {
    problems.filesKeyword = "Already a web search keyword";
  }
  if (problems.filesKeyword) count.files++;

  const depth = config.files.max_depth;
  if (!Number.isInteger(depth) || depth < 0 || depth > MAX_DEPTH) {
    problems.filesDepth = `Enter a whole number from 0 to ${MAX_DEPTH}`;
    count.files++;
  }
  return problems;
}

export function totalProblems(problems: Problems): number {
  return Object.values(problems.count).reduce((sum, n) => sum + n, 0);
}
