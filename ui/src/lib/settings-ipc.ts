// IPC contract of the settings window. Like ipc.ts, every wrapper tolerates
// running in a plain browser (`npm run dev`) where there is no Rust backend.

import { invoke } from "@tauri-apps/api/core";
import { hasTauri, type Status, type ThemeSetting } from "./ipc";

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
  };
  window: { width: number };
  linux: { wayland_use_xwayland: boolean };
  actions: { use_primary_selection: boolean; use_clipboard_fallback: boolean };
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
  };
  bookmarks: { browsers: string[]; keyword: string; global: boolean };
  /** Edited in the config file; kept here so saving the form round-trips it. */
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

/** Native folder picker; `null` when cancelled. Paths come back `~`-relative where possible. */
export async function pickDirectory(): Promise<string | null> {
  if (preview()) return "~/Projects";
  try {
    return await invoke<string | null>("pick_directory");
  } catch (err) {
    console.warn("[ipc] pick_directory failed:", err);
    return null;
  }
}

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
