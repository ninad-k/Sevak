// IPC contract of the settings window. Like ipc.ts, every wrapper tolerates
// running in a plain browser (`npm run dev`) where there is no Rust backend.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { hasTauri, type Status, type ThemeSetting } from "./ipc";

import type { AiConfig } from "./ai";

export interface WebSearchEngine {
  keyword: string;
  name: string;
  url: string;
}

/** One `[[hotkey]]` entry: set `query` (open with text) or `run` (a result id), not both. */
export interface HotkeyBinding {
  key: string;
  query?: string | null;
  run?: string | null;
}

export interface Appearance {
  theme: ThemeSetting;
  accent: string;
  font_size: number;
  font_family: string;
  opacity: number;
  /** Frosted-glass blur behind the search bar (Windows and macOS). */
  blur: boolean;
  radius: number;
  /** A theme file in the config folder, such as `themes/Nord.toml`; empty uses none. */
  theme_file: string;
  custom_css: string;
}

/** Mirrors `sevak_core::Config` (serde defaults make every field present). */
export interface Config {
  general: {
    hotkey: string;
    /** Shortcut for Universal Actions; empty turns it off. */
    actions_hotkey: string;
    hide_on_blur: boolean;
    launch_at_login: boolean;
    check_for_updates: boolean;
    /** Release channel: "stable" (default) or "beta" (earlier, less tested builds). */
    update_channel: "stable" | "beta";
  };
  window: { width: number };
  linux: { wayland_use_xwayland: boolean };
  /** Universal Actions: how the selection in another app is captured. */
  actions: { use_primary_selection: boolean; use_clipboard_fallback: boolean };
  /** How text is pasted into the app you came from. */
  paste: { restore_clipboard: boolean };
  search: {
    max_results: number;
    /** One engine keyword, or several (config.toml accepts a string or a list). */
    fallback_web_search: string | string[];
    query_history: boolean;
  };
  appearance: Appearance;
  plugins: { disabled: string[] };
  calculator: { currency: boolean };
  files: {
    directories: string[];
    max_depth: number;
    include_hidden: boolean;
    keyword: string;
    global: boolean;
    use_os_index: boolean;
    index_keyword: string;
    content_keyword: string;
  };
  bookmarks: { browsers: string[]; keyword: string; global: boolean };
  /** The file buffer: files collected from the results to act on together. */
  file_buffer: { keep_between_shows: boolean };
  /** Clipboard history (`cb`). Opt-in; `*_bytes` are in bytes. */
  clipboard: {
    enabled: boolean;
    max_items: number;
    max_item_bytes: number;
    ignore_apps: string[];
    images: boolean;
    files: boolean;
    max_image_bytes: number;
  };
  contacts: { enabled: boolean; keyword: string; use_system: boolean; vcard_files: string[] };
  onepassword: {
    enabled: boolean;
    keyword: string;
    op_path: string;
    account: string;
    cache_minutes: number;
  };
  dictionary: { define_keyword: string; spell_keyword: string; use_system: boolean };
  /** The optional AI assistant (`ai <question>`). Never holds an API key. */
  ai: AiConfig;
  /** System commands (lock, restart, ...) and settings pages. */
  system: { confirm: boolean; disabled: string[] };
  /** Automation tasks (dark mode, volume, kill, ...). */
  tasks: { confirm: boolean; disabled: string[]; keyword: string; global: boolean };
  media: { keyword: string; global: boolean; now_playing: boolean };
  /** Expanding `[[snippet]]` keywords as you type in other apps (off by default). */
  snippets: {
    auto_expand: boolean;
    prefix: string;
    expand_on: "immediate" | "delimiter";
    case_sensitive: boolean;
    ignore_apps: string[];
    expand_in_terminals: boolean;
  };
  /** The `>` command: which terminal and shell run it. */
  shell: { terminal: string; shell: string; keep_open: boolean };
  web_search: WebSearchEngine[];
  hotkey: HotkeyBinding[];
}

export interface PluginInfo {
  id: string;
  name: string;
  description: string;
  keyword: string | null;
  enabled: boolean;
}

export interface SettingsDto {
  config: Config;
  catalog: PluginInfo[];
  display: Status["display"];
  is_gnome: boolean;
  config_path: string;
  log_dir: string;
  platform: "windows" | "macos" | "linux";
}

function errorText(err: unknown): string {
  return typeof err === "string" ? err : err instanceof Error ? err.message : String(err);
}

const preview = () => import.meta.env.DEV && !hasTauri();

/** Loads everything the settings window shows, or `null` on failure. */
export async function getSettings(): Promise<SettingsDto | null> {
  if (preview()) {
    const { mockSettings } = await import("./mock");
    return mockSettings();
  }
  try {
    return await invoke<SettingsDto>("get_settings");
  } catch (err) {
    console.warn("[ipc] get_settings failed:", err);
    return null;
  }
}

/** Writes and applies the configuration. Resolves to an error message, or `null` on success. */
export async function saveSettings(config: Config): Promise<string | null> {
  if (preview()) return null;
  try {
    await invoke("save_settings", { config });
    return null;
  } catch (err) {
    console.warn("[ipc] save_settings failed:", err);
    return errorText(err);
  }
}

/** Asks Rust to parse an accelerator. Resolves to an error message, or `null` if valid. */
export async function validateHotkey(hotkey: string): Promise<string | null> {
  if (preview()) return hotkey.includes("+") ? null : "preview: not a valid shortcut";
  try {
    await invoke("validate_hotkey", { hotkey });
    return null;
  } catch (err) {
    return errorText(err);
  }
}

/** Unregisters the global hotkey while the recorder listens for keys. */
export async function suspendHotkey(): Promise<void> {
  if (preview()) return;
  try {
    await invoke("suspend_hotkey");
  } catch (err) {
    console.warn("[ipc] suspend_hotkey failed:", err);
  }
}

/** Registers the configured hotkey again. */
export async function resumeHotkey(): Promise<void> {
  if (preview()) return;
  try {
    await invoke("resume_hotkey");
  } catch (err) {
    console.warn("[ipc] resume_hotkey failed:", err);
  }
}

/**
 * Asks the keyboard hook (Windows) for the next shortcut pressed, even one the OS
 * keeps from the page (Win+Space). Resolves to false where there is no hook; the
 * recorder then uses the keys the page sees. The shortcut arrives through
 * {@link onHotkeyRecorded}.
 */
export async function startHotkeyRecording(): Promise<boolean> {
  if (preview()) return false;
  try {
    await invoke("start_hotkey_recording");
    return true;
  } catch {
    return false;
  }
}

export async function stopHotkeyRecording(): Promise<void> {
  if (preview()) return;
  try {
    await invoke("stop_hotkey_recording");
  } catch (err) {
    console.warn("[ipc] stop_hotkey_recording failed:", err);
  }
}

/** The hook recorded a shortcut (`null` when Escape cancelled the recording). */
export async function onHotkeyRecorded(
  cb: (accelerator: string | null) => void,
): Promise<UnlistenFn> {
  if (!hasTauri()) return () => {};
  return listen<{ accelerator: string | null }>("sevak:hotkey-recorded", (event) =>
    cb(event.payload.accelerator),
  );
}

/** "Let Sevak use Cmd+Space / Super+Space": asks first, then changes the OS shortcut. */
export async function takeOverHotkey(): Promise<Outcome> {
  if (preview()) return { ok: true, text: "Done. Sevak now uses the shortcut." };
  try {
    return { ok: true, text: await invoke<string>("takeover_hotkey") };
  } catch (err) {
    return { ok: false, text: errorText(err) };
  }
}

/** Puts back the OS shortcut Sevak switched off or moved. */
export async function restoreTakeover(): Promise<Outcome> {
  if (preview()) return { ok: true, text: "Spotlight's shortcut is on again." };
  try {
    return { ok: true, text: await invoke<string>("restore_takeover") };
  } catch (err) {
    return { ok: false, text: errorText(err) };
  }
}

/** Native folder picker; `null` when cancelled. Paths come back `~`-relative where possible. */
export async function pickDirectory(title?: string): Promise<string | null> {
  if (preview()) return "~/Projects";
  try {
    return await invoke<string | null>("pick_directory", { title });
  } catch (err) {
    console.warn("[ipc] pick_directory failed:", err);
    return null;
  }
}

/**
 * Native file picker; `null` when cancelled. `extensions` limits it to those
 * file types (such as `vcf`); empty shows every file.
 */
export async function pickFile(title: string, extensions: string[] = []): Promise<string | null> {
  if (preview()) return extensions.includes("vcf") ? "~/contacts.vcf" : "C:/Tools/chosen.exe";
  try {
    return await invoke<string | null>("pick_file", { title, extensions });
  } catch (err) {
    console.warn("[ipc] pick_file failed:", err);
    return null;
  }
}

/** Deletes the saved clipboard history and its image files. Resolves to an error message, or `null`. */
export const clearClipboardHistory = () => simple("clear_clipboard_history");

export interface Outcome {
  ok: boolean;
  text: string;
}

/** Wayland: bind the shortcut to `sevak --toggle` in GNOME (or explain how). */
export async function setupWaylandHotkey(hotkey: string): Promise<Outcome> {
  if (preview()) {
    return { ok: true, text: "GNOME shortcut created: <Alt>space -> sevak --toggle" };
  }
  try {
    return { ok: true, text: await invoke<string>("setup_wayland_hotkey", { hotkey }) };
  } catch (err) {
    return { ok: false, text: errorText(err) };
  }
}

async function simple(command: string): Promise<string | null> {
  if (preview()) return null;
  try {
    await invoke(command);
    return null;
  } catch (err) {
    return errorText(err);
  }
}

/** Each resolves to an error message, or `null` on success. */
export const openConfigFile = () => simple("open_config_file");
export const openLogDir = () => simple("open_log_dir");
export const closeSettings = () => simple("close_settings");

/** The fallback engine keywords in order, whichever form the config uses. */
export function fallbackList(value: string | string[]): string[] {
  return (Array.isArray(value) ? value : [value]).map((k) => k.trim()).filter((k) => k !== "");
}
