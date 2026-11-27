// IPC for Settings > Usage: a read-only digest of the usage statistics.
// In a plain browser (`npm run dev`) it shows made-up numbers.

import { invoke } from "@tauri-apps/api/core";
import { hasTauri } from "./ipc";

/** Mirrors `sevak_core::UsageTopItem`. */
export interface UsageTopItem {
  /** The item's own name, never a folder path. */
  title: string;
  plugin: string;
  count: number;
  /** Unix seconds. */
  last_used: number;
}

/** Mirrors `UsageStats` in `src-tauri/src/settings.rs`. */
export interface UsageStats {
  total_launches: number;
  tracked_items: number;
  history_entries: number;
  top: UsageTopItem[];
  /** `[plugin, launches]`, most first. */
  plugins: [string, number][];
  /** What "Copy stats" copies. */
  text: string;
}

const PREVIEW: UsageStats = {
  total_launches: 42,
  tracked_items: 3,
  history_entries: 5,
  top: [
    { title: "firefox", plugin: "app", count: 30, last_used: 1_700_000_000 },
    { title: "report.pdf", plugin: "files", count: 8, last_used: 1_699_000_000 },
    { title: "rust", plugin: "web", count: 4, last_used: 1_698_000_000 },
  ],
  plugins: [
    ["app", 30],
    ["files", 8],
    ["web", 4],
  ],
  text: "Sevak usage statistics\nLaunches tracked: 42\n",
};

export async function getUsageStats(): Promise<UsageStats> {
  if (import.meta.env.DEV && !hasTauri()) return PREVIEW;
  return await invoke<UsageStats>("usage_stats");
}
