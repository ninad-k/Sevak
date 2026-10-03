// Typed wrappers around the Tauri IPC contract with the Rust shell.
// Every wrapper tolerates running in a plain browser (no Tauri runtime):
// failures are logged with console.warn instead of thrown.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type HotkeyMode = "global" | "external";

export interface HotkeyStatus {
  accelerator: string;
  mode: HotkeyMode;
  error: string | null;
}

export type ThemeSetting = "system" | "light" | "dark";

export interface Status {
  version: string;
  display: "windows" | "macos" | "x11" | "wayland" | "unknown";
  hotkey: HotkeyStatus;
  /** The configured theme; `system` follows `prefers-color-scheme`. */
  theme: ThemeSetting;
  /** The search index is being (re)built. */
  indexing: boolean;
}

export type ActionKind =
  | "launch"
  | "open_path"
  | "open_url"
  | "copy_text"
  | "reveal_path"
  | "run_as_admin"
  | "paste_text"
  | "custom";

export type IconDto = { kind: "url"; url: string } | { kind: "builtin"; name: string };

/** A key held with Enter to run a secondary action. `ctrl` is Cmd on macOS. */
export type Modifier = "ctrl" | "shift" | "alt";

export interface SecondaryDto {
  label: string;
  modifier: Modifier | null;
  kind: ActionKind;
}

export interface ResultDto {
  id: string;
  title: string;
  subtitle: string;
  icon: IconDto | null;
  plugin_id: string;
  /** What Tab turns the input into, when the plugin offers a completion. */
  autocomplete?: string | null;
  action: ActionKind;
  /** Other actions; `execute`'s `action` index refers to this list. */
  secondary: SecondaryDto[];
  /** What Ctrl+C copies for this row, if anything. */
  copy_text: string | null;
}

export type IndexState = "indexing" | "ready";

export interface IndexEvent {
  state: IndexState;
}

export const EVENT_SHOW = "sevak:show";
export const EVENT_HIDDEN = "sevak:hidden";
export const EVENT_STATUS = "sevak:status";
export const EVENT_INDEX = "sevak:index";

/** Hide the launcher window. */
export async function hideWindow(): Promise<void> {
  try {
    await invoke("hide_window");
  } catch (err) {
    console.warn("[ipc] hide_window failed:", err);
  }
}

/** Fetch the current shell status, or `null` when unavailable. */
export async function getStatus(): Promise<Status | null> {
  try {
    return await invoke<Status>("get_status");
  } catch (err) {
    console.warn("[ipc] get_status failed:", err);
    return null;
  }
}

/** Tell Rust the logical pixel height of the rendered content. */
export async function setContentHeight(height: number): Promise<void> {
  try {
    await invoke("set_content_height", { height });
  } catch (err) {
    console.warn("[ipc] set_content_height failed:", err);
  }
}

// True inside the Tauri webview; plain-browser `npm run dev` previews use mock data.
export function hasTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** One search's results; `ticket` names this set when executing one of them. */
export interface SearchResponse {
  ticket: number;
  results: ResultDto[];
}

/** Query the engine. Returns `null` on failure so callers can tell it from "no results". */
export async function search(query: string): Promise<SearchResponse | null> {
  if (import.meta.env.DEV && !hasTauri()) {
    const { mockSearch } = await import("./mock");
    return { ticket: 0, results: mockSearch(query) };
  }
  try {
    return await invoke<SearchResponse>("search", { query });
  } catch (err) {
    console.warn("[ipc] search failed:", err);
    return null;
  }
}

/** Executed queries, most recent first (empty when history is off). */
export async function queryHistory(): Promise<string[]> {
  if (import.meta.env.DEV && !hasTauri()) {
    const { mockHistory } = await import("./mock");
    return mockHistory();
  }
  try {
    return await invoke<string[]>("query_history");
  } catch (err) {
    console.warn("[ipc] query_history failed:", err);
    return [];
  }
}

/**
 * Run result `id` of search `ticket` (the results on screen). `action` is the
 * index of one of its secondary actions; omit it for the primary action.
 * Resolves to an error message, or `null` on success.
 */
export async function execute(
  id: string,
  ticket: number,
  action?: number,
): Promise<string | null> {
  if (import.meta.env.DEV && !hasTauri()) {
    return id === "m:broken" ? "Could not start “Broken icon app” (preview error)" : null;
  }
  try {
    await invoke("execute", { id, ticket, action: action ?? null });
    return null;
  } catch (err) {
    console.warn("[ipc] execute failed:", err);
    return errorText(err);
  }
}

/** Copy result `id`'s most useful text (see `ResultDto.copy_text`) and hide. */
export async function copyResult(id: string, ticket: number): Promise<string | null> {
  if (import.meta.env.DEV && !hasTauri()) return null;
  try {
    await invoke("copy_result", { id, ticket });
    return null;
  } catch (err) {
    console.warn("[ipc] copy_result failed:", err);
    return errorText(err);
  }
}

/**
 * Stretch the window over the screen (`true`) for Large Type, or restore it.
 * Resolves to whether the window now covers the screen; `false` means the
 * caller should show the text inside the launcher instead.
 */
export async function setLargeType(on: boolean): Promise<boolean> {
  if (!hasTauri()) return on;
  try {
    await invoke("set_large_type", { on });
    return on;
  } catch (err) {
    console.warn("[ipc] set_large_type failed:", err);
    return false;
  }
}

function errorText(err: unknown): string {
  return typeof err === "string" ? err : err instanceof Error ? err.message : String(err);
}

async function safeListen<T>(event: string, cb: (payload: T) => void): Promise<UnlistenFn> {
  try {
    return await listen<T>(event, (e) => cb(e.payload));
  } catch (err) {
    console.warn(`[ipc] listen(${event}) failed:`, err);
    return () => {};
  }
}

/** Window is being shown: clear the query, focus and select the input. */
export function onShow(cb: () => void): Promise<UnlistenFn> {
  return safeListen<void>(EVENT_SHOW, () => cb());
}

/** Window was hidden: clear the query. */
export function onHidden(cb: () => void): Promise<UnlistenFn> {
  return safeListen<void>(EVENT_HIDDEN, () => cb());
}

/** Config reloaded / hotkey status changed. */
export function onStatus(cb: (status: Status) => void): Promise<UnlistenFn> {
  return safeListen<Status>(EVENT_STATUS, cb);
}

/** The search index started or finished (re)building. */
export function onIndex(cb: (state: IndexState) => void): Promise<UnlistenFn> {
  return safeListen<IndexEvent>(EVENT_INDEX, (e) => cb(e.state));
}
