// IPC of the Sevak UI. Only the app's own commands: no plugin permissions.

import { invoke } from "@tauri-apps/api/core";
import type { UiEventMessage, UiPayload, UiUpdate } from "./types";

/** Tells the shell what happened in the UI `id`; a `run` answers with the script's update. */
export async function sendUiEvent(id: string, event: UiEventMessage): Promise<UiUpdate | null> {
  return (await invoke<UiUpdate | null>("sevak_ui_event", { id, event })) ?? null;
}

/** What the window `id` shows (a UI window asks on load). */
export async function uiSession(id: string): Promise<UiPayload | null> {
  try {
    return (await invoke<UiPayload | null>("sevak_ui_session", { id })) ?? null;
  } catch (err) {
    console.warn("[ipc] sevak_ui_session failed:", err);
    return null;
  }
}

export interface UiCheck {
  ports: string[];
  components: number;
  warnings: string[];
}

/** The builder's Validate button: parses a definition as a run would. */
export async function validateUiDefinition(args: {
  definition: string | null;
  file: string | null;
  scripts: string[];
  folder: string | null;
}): Promise<{ ok: true; value: UiCheck } | { ok: false; error: string }> {
  try {
    return { ok: true, value: await invoke<UiCheck>("validate_ui_definition", args) };
  } catch (err) {
    return { ok: false, error: typeof err === "string" ? err : String(err) };
  }
}
