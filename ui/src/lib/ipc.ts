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

export interface Status {
  version: string;
  display: "windows" | "x11" | "wayland" | "unknown";
  hotkey: HotkeyStatus;
}

export const EVENT_SHOW = "sevak:show";
export const EVENT_HIDDEN = "sevak:hidden";
export const EVENT_STATUS = "sevak:status";

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
