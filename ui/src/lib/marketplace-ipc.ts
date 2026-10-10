// Opens the marketplace pages in the default browser. The webview names a page
// with one of three words; the addresses live in the backend (`open_marketplace`
// in src-tauri/src/settings.rs), so no link text from the page is ever opened.

import { invoke } from "@tauri-apps/api/core";
import { hasTauri } from "./ipc";

export type MarketplacePage = "browse" | "submit" | "guide";

/** Resolves to an error message, or `null` on success. */
export async function openMarketplace(page: MarketplacePage): Promise<string | null> {
  // Plain browser (`npm run dev`): there is no backend to open anything.
  if (import.meta.env.DEV && !hasTauri()) return null;
  try {
    await invoke("open_marketplace", { target: page });
    return null;
  } catch (err) {
    return typeof err === "string" ? err : err instanceof Error ? err.message : String(err);
  }
}
