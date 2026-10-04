// IPC of Settings > AI assistant. Like settings-ipc.ts, every wrapper tolerates
// running in a plain browser (`npm run dev`) where there is no Rust backend.
//
// The API key only ever travels one way: `aiSetKey` hands what the user typed to
// Rust, and nothing here (or in Rust) returns it.

import { invoke } from "@tauri-apps/api/core";
import { hasTauri } from "./ipc";
import type { AiConfig, AiProviderId, KeyStatus } from "./ai";

const preview = () => import.meta.env.DEV && !hasTauri();

function errorText(err: unknown): string {
  return typeof err === "string" ? err : err instanceof Error ? err.message : String(err);
}

/** A result that is either a value or an error message. */
export type Reply<T> = { ok: true; value: T } | { ok: false; text: string };

const previewStatus = (): KeyStatus => ({
  openai: { state: "none", env_var: "OPENAI_API_KEY" },
  anthropic: { state: "env", env_var: "ANTHROPIC_API_KEY" },
  protection: "encrypted with Windows DPAPI for your user account",
});

export async function aiKeyStatus(): Promise<KeyStatus | null> {
  if (preview()) return previewStatus();
  try {
    return await invoke<KeyStatus>("ai_key_status");
  } catch (err) {
    console.warn("[ipc] ai_key_status failed:", err);
    return null;
  }
}

/** Stores the key at once (it is not part of the form's Save). */
export async function aiSetKey(provider: AiProviderId, key: string): Promise<Reply<KeyStatus>> {
  if (preview()) return { ok: true, value: { ...previewStatus(), [provider]: { state: "stored", env_var: null } } };
  try {
    return { ok: true, value: await invoke<KeyStatus>("ai_set_key", { provider, key }) };
  } catch (err) {
    return { ok: false, text: errorText(err) };
  }
}

export async function aiClearKey(provider: AiProviderId): Promise<Reply<KeyStatus>> {
  if (preview()) return { ok: true, value: previewStatus() };
  try {
    return { ok: true, value: await invoke<KeyStatus>("ai_clear_key", { provider }) };
  } catch (err) {
    return { ok: false, text: errorText(err) };
  }
}

/** Checks the address, key and model as in the form; sends no question. */
export async function aiTestConnection(config: AiConfig): Promise<Reply<string>> {
  if (preview()) return { ok: true, value: "Connected to localhost:11434; the model “llama3.2” is installed." };
  try {
    return { ok: true, value: await invoke<string>("ai_test_connection", { config }) };
  } catch (err) {
    return { ok: false, text: errorText(err) };
  }
}
