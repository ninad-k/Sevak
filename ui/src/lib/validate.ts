// Client-side checks for the settings form. They mirror `settings::validate` in
// the Rust shell, which re-checks everything on save; the shortcut itself is
// parsed by Rust only (see `validateHotkey`).

import { acceleratorId } from "./accelerator";
import { formatNumber, t, type MessageKey } from "./i18n.svelte";
import { aiProblems, type AiProblems } from "./ai";
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
  | "clipboard"
  | "tasks"
  | "windows"
  | "integrations"
  | "ai"
  | "system"
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

/** The configurable keywords of the built-in searches, by `section.key`. */
export type KeywordField =
  | "files.keyword"
  | "files.index_keyword"
  | "files.content_keyword"
  | "bookmarks.keyword"
  | "tasks.keyword"
  | "media.keyword"
  | "window_management.keyword"
  | "window_management.switcher_keyword"
  | "contacts.keyword"
  | "onepassword.keyword"
  | "dictionary.define_keyword"
  | "dictionary.spell_keyword"
  | "ai.keyword"
  | "ssh.keyword"
  | "projects.keyword"
  | "calendar.keyword"
  | "clock.keyword"
  | "passwords.keyword";

export interface ClipboardErrors {
  maxItems?: string;
  maxItemBytes?: string;
  maxImageBytes?: string;
  viewerRows?: string;
  previewChars?: string;
}

export interface Problems {
  engines: EngineErrors[];
  hotkeys: HotkeyErrors[];
  appearance: AppearanceErrors;
  /** What is wrong with each keyword field (only the fields with a problem). */
  keywords: Partial<Record<KeywordField, string>>;
  clipboard: ClipboardErrors;
  cacheMinutes?: string;
  /** What is wrong with each field of the AI assistant page (the keyword is under `keywords`). */
  ai: AiProblems;
  windowGap?: string;
  /** `[calendar] days` out of range. */
  calendarDays?: string;
  fallback?: string;
  filesDepth?: string;
  filesResults?: string;
  /** Number of problems per section (hotkey problems are added by the form). */
  count: Record<SectionId, number>;
}

export const MIN_WIDTH = 400;
export const MAX_WIDTH = 1600;
export const MAX_DEPTH = 32;
/** `[files] max_results`: 0 uses the search limit, otherwise 1 to this. */
export const MAX_FILE_RESULTS = 50;
export const MIN_FONT_SIZE = 12;
export const MAX_FONT_SIZE = 22;
export const MIN_OPACITY = 30;
export const MAX_OPACITY = 100;
export const MAX_RADIUS = 32;
export const MAX_ANIMATION_MS = 400;
export const MAX_CLIPBOARD_ITEMS = 5_000;
export const MAX_CLIPBOARD_ITEM_BYTES = 4 * 1024 * 1024;
export const MAX_CLIPBOARD_IMAGE_BYTES = 64 * 1024 * 1024;
export const MAX_CACHE_MINUTES = 24 * 60;
/** Largest gap between snapped windows, in pixels (as in Rust's `window_layout::MAX_GAP`). */
export const MAX_WINDOW_GAP = 200;
/** Largest `[calendar] days` (as in Rust's `config::MAX_CALENDAR_DAYS`). */
export const MAX_CALENDAR_DAYS = 60;

/** What answers a keyword, as the message key of its name ("the emoji picker"). */
type Owner = MessageKey & `validate.owner.${string}`;

/** Keywords of built-in searches that cannot be changed, and what they open (as in Rust). */
const FIXED_KEYWORDS: [string, Owner][] = [
  [">", "validate.owner.terminal"],
  ["cb", "validate.owner.clipboard"],
  ["s", "validate.owner.snippets"],
  ["emoji", "validate.owner.emoji"],
  [":", "validate.owner.emoji"],
  ["@", "validate.owner.contacts"],
  ["uuid", "validate.owner.uuid"],
];

interface KeywordSpec {
  field: KeywordField;
  owner: Owner;
  /** An empty keyword means the default (never "off"). */
  required: boolean;
  section: SectionId;
  read: (config: Config) => string;
}

/** In Rust's order (`sevak_plugins::keywords::configurable_keywords`). */
const KEYWORDS: KeywordSpec[] = [
  { field: "files.keyword", owner: "validate.owner.files", required: false, section: "files", read: (c) => c.files.keyword },
  { field: "files.index_keyword", owner: "validate.owner.indexFiles", required: false, section: "files", read: (c) => c.files.index_keyword },
  { field: "files.content_keyword", owner: "validate.owner.contentFiles", required: false, section: "files", read: (c) => c.files.content_keyword },
  { field: "bookmarks.keyword", owner: "validate.owner.bookmarks", required: false, section: "files", read: (c) => c.bookmarks.keyword },
  { field: "tasks.keyword", owner: "validate.owner.tasks", required: false, section: "tasks", read: (c) => c.tasks.keyword },
  { field: "media.keyword", owner: "validate.owner.media", required: false, section: "tasks", read: (c) => c.media.keyword },
  { field: "window_management.keyword", owner: "validate.owner.windowLayouts", required: false, section: "windows", read: (c) => c.window_management.keyword },
  { field: "window_management.switcher_keyword", owner: "validate.owner.windowSwitcher", required: false, section: "windows", read: (c) => c.window_management.switcher_keyword },
  { field: "contacts.keyword", owner: "validate.owner.contacts", required: true, section: "integrations", read: (c) => c.contacts.keyword },
  { field: "onepassword.keyword", owner: "validate.owner.onepassword", required: true, section: "integrations", read: (c) => c.onepassword.keyword },
  { field: "dictionary.define_keyword", owner: "validate.owner.dictionary", required: true, section: "integrations", read: (c) => c.dictionary.define_keyword },
  { field: "dictionary.spell_keyword", owner: "validate.owner.spelling", required: true, section: "integrations", read: (c) => c.dictionary.spell_keyword },
  { field: "ai.keyword", owner: "validate.owner.ai", required: true, section: "ai", read: (c) => c.ai.keyword },
  { field: "ssh.keyword", owner: "validate.owner.ssh", required: true, section: "integrations", read: (c) => c.ssh.keyword },
  { field: "projects.keyword", owner: "validate.owner.projects", required: true, section: "integrations", read: (c) => c.projects.keyword },
  { field: "calendar.keyword", owner: "validate.owner.calendar", required: true, section: "integrations", read: (c) => c.calendar.keyword },
  { field: "clock.keyword", owner: "validate.owner.clock", required: true, section: "integrations", read: (c) => c.clock.keyword },
  { field: "passwords.keyword", owner: "validate.owner.passwords", required: true, section: "integrations", read: (c) => c.passwords.keyword },
];

/** Every keyword of a built-in search (lowercased) and what answers it, named in the UI language. */
function reservedKeywords(config: Config): Map<string, string> {
  const reserved = new Map(FIXED_KEYWORDS.map(([keyword, owner]) => [keyword, t(owner)]));
  for (const spec of KEYWORDS) {
    const keyword = spec.read(config).trim().toLowerCase();
    if (keyword !== "" && !reserved.has(keyword)) reserved.set(keyword, t(spec.owner));
  }
  return reserved;
}

/**
 * Problems with the keywords of the built-in searches, like `validate_builtin_keywords`
 * in the Rust shell: one word each, no clash with a web search keyword, a fixed
 * keyword or another built-in one (both sides of a clash are flagged, so fixing
 * either clears it). An empty keyword turns a search off and never clashes,
 * except for the contacts, 1Password and dictionary ones, which cannot be off.
 */
export function keywordProblems(config: Config): Partial<Record<KeywordField, string>> {
  const web = new Set(config.web_search.map((engine) => engine.keyword.trim().toLowerCase()));
  const problems: Partial<Record<KeywordField, string>> = {};
  for (const spec of KEYWORDS) {
    const keyword = spec.read(config).trim();
    if (keyword === "") {
      if (spec.required) problems[spec.field] = t("validate.required");
      continue;
    }
    const lower = keyword.toLowerCase();
    const fixed = FIXED_KEYWORDS.find(([used]) => used === lower);
    const other = KEYWORDS.find(
      (candidate) => candidate !== spec && candidate.read(config).trim().toLowerCase() === lower,
    );
    if (/\s/.test(keyword)) problems[spec.field] = t("validate.noSpaces");
    else if (web.has(lower)) problems[spec.field] = t("validate.webKeyword");
    else if (fixed) problems[spec.field] = t("validate.usedBy", { owner: t(fixed[1]) });
    else if (other) problems[spec.field] = t("validate.usedBy", { owner: t(other.owner) });
  }
  return problems;
}

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
  if (key === "") errors.key = t("validate.required");
  else if (key === keyId(mainKey)) errors.key = t("validate.sameAsMain");
  else if (key === keyId(actionsKey)) errors.key = t("validate.sameAsActions");
  else if (all.filter((other) => keyId(other.key) === key).length > 1) errors.key = t("validate.alreadyUsed");
  if (binding.run != null && binding.run.trim() === "") errors.value = t("validate.resultId");
  return errors;
}

export function appearanceErrors(appearance: Appearance): AppearanceErrors {
  const errors: AppearanceErrors = {};
  const accent = appearance.accent.trim();
  if (accent !== "" && !isColor(accent)) errors.accent = t("validate.color");
  const family = appearance.font_family.trim();
  if (family !== "" && !/^[\p{L}\p{N} ,._'"-]+$/u.test(family)) {
    errors.fontFamily = t("validate.fontFamily");
  }
  const css = appearance.custom_css.trim();
  if (css !== "" && (/^[\\/]|^[A-Za-z]:|(^|[\\/])\.\.([\\/]|$)/.test(css))) {
    errors.customCss = t("validate.insideConfig");
  }
  const themeFile = appearance.theme_file.trim();
  if (themeFile !== "" && (/^[\\/]|^[A-Za-z]:|(^|[\\/])\.\.([\\/]|$)/.test(themeFile))) {
    errors.themeFile = t("validate.insideConfig");
  }
  return errors;
}

export function engineErrors(
  engine: WebSearchEngine,
  all: WebSearchEngine[],
  /** Keywords of the built-in searches (lowercased) and what answers them. */
  reserved: Map<string, string> = new Map(),
): EngineErrors {
  const errors: EngineErrors = {};
  const keyword = engine.keyword.trim();
  if (keyword === "") errors.keyword = t("validate.required");
  else if (/\s/.test(keyword)) errors.keyword = t("validate.noSpaces");
  else if (reserved.has(keyword.toLowerCase())) {
    errors.keyword = t("validate.engineUsedBy", { owner: reserved.get(keyword.toLowerCase())! });
  }
  else if (all.filter((other) => other.keyword.trim().toLowerCase() === keyword.toLowerCase()).length > 1) {
    errors.keyword = t("validate.alreadyUsed");
  }
  if (engine.name.trim() === "") errors.name = t("validate.required");
  const url = engine.url.trim();
  if (!/^https?:\/\//i.test(url)) errors.url = t("validate.urlScheme");
  else if (!engine.url.includes("{query}")) errors.url = t("validate.urlQuery");
  return errors;
}

const wholeNumber = (min: number, max: number) => t("validate.wholeNumber", { min, max });

/** Sizes are edited in KB and MB but kept in bytes, as in the file. */
function sizeError(bytes: number, max: number, unit: string): string | undefined {
  if (!Number.isFinite(bytes) || bytes < 1 || bytes > max) {
    return t("validate.size", { max: unit });
  }
  return undefined;
}

export function clipboardErrors(config: Config): ClipboardErrors {
  const errors: ClipboardErrors = {};
  const { max_items, max_item_bytes, max_image_bytes } = config.clipboard;
  if (!Number.isInteger(max_items) || max_items < 1 || max_items > MAX_CLIPBOARD_ITEMS) {
    errors.maxItems = wholeNumber(1, MAX_CLIPBOARD_ITEMS);
  }
  const text = sizeError(max_item_bytes, MAX_CLIPBOARD_ITEM_BYTES, `${formatNumber(4096)} KB`);
  if (text) errors.maxItemBytes = text;
  const image = sizeError(max_image_bytes, MAX_CLIPBOARD_IMAGE_BYTES, `${formatNumber(64)} MB`);
  if (image) errors.maxImageBytes = image;
  const { viewer_rows, preview_chars } = config.clipboard;
  if (!Number.isInteger(viewer_rows) || viewer_rows < 5 || viewer_rows > 100) {
    errors.viewerRows = wholeNumber(5, 100);
  }
  if (!Number.isInteger(preview_chars) || preview_chars < 40 || preview_chars > 500) {
    errors.previewChars = wholeNumber(40, 500);
  }
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
    clipboard: 0,
    tasks: 0,
    windows: 0,
    integrations: 0,
    ai: 0,
    system: 0,
    linux: 0,
  };

  const reserved = reservedKeywords(config);
  const engines = config.web_search.map((engine) =>
    engineErrors(engine, config.web_search, reserved),
  );
  for (const errors of engines) count.web += Object.keys(errors).length;

  const hotkeys = config.hotkey.map((binding) =>
    hotkeyErrors(binding, config.hotkey, config.general.hotkey, config.general.actions_hotkey),
  );
  for (const errors of hotkeys) count.hotkeys += Object.keys(errors).length;

  const appearance = appearanceErrors(config.appearance);
  count.appearance += Object.keys(appearance).length;

  const keywords = new Set(config.web_search.map((engine) => engine.keyword.trim().toLowerCase()));
  const keywordErrors = keywordProblems(config);
  for (const spec of KEYWORDS) {
    if (keywordErrors[spec.field]) count[spec.section]++;
  }
  const clipboard = clipboardErrors(config);
  count.clipboard += Object.keys(clipboard).length;
  const ai = aiProblems(config.ai);
  count.ai += Object.keys(ai).length;
  const problems: Problems = {
    engines,
    hotkeys,
    appearance,
    keywords: keywordErrors,
    clipboard,
    ai,
    count,
  };

  const minutes = config.onepassword.cache_minutes;
  if (!Number.isInteger(minutes) || minutes < 1 || minutes > MAX_CACHE_MINUTES) {
    problems.cacheMinutes = wholeNumber(1, MAX_CACHE_MINUTES);
    count.integrations++;
  }

  const gap = config.window_management.gap;
  if (!Number.isInteger(gap) || gap < 0 || gap > MAX_WINDOW_GAP) {
    problems.windowGap = wholeNumber(0, MAX_WINDOW_GAP);
    count.windows++;
  }

  const days = config.calendar.days;
  if (!Number.isInteger(days) || days < 1 || days > MAX_CALENDAR_DAYS) {
    problems.calendarDays = wholeNumber(1, MAX_CALENDAR_DAYS);
    count.integrations++;
  }

  const missing = fallbackList(config.search.fallback_web_search).find(
    (keyword) => !keywords.has(keyword.toLowerCase()),
  );
  if (missing !== undefined) {
    problems.fallback = t("validate.fallbackGone", { keyword: missing });
    count.search++;
  }

  const depth = config.files.max_depth;
  if (!Number.isInteger(depth) || depth < 0 || depth > MAX_DEPTH) {
    problems.filesDepth = wholeNumber(0, MAX_DEPTH);
    count.files++;
  }
  const results = config.files.max_results;
  if (!Number.isInteger(results) || results < 0 || results > MAX_FILE_RESULTS) {
    problems.filesResults = wholeNumber(0, MAX_FILE_RESULTS);
    count.files++;
  }
  return problems;
}

export function totalProblems(problems: Problems): number {
  return Object.values(problems.count).reduce((sum, n) => sum + n, 0);
}
