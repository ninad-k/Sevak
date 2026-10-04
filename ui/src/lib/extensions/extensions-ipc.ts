// IPC for Settings > Extensions. Like the other wrappers, these run in a plain
// browser (`npm run dev`) against the in-memory data of `mock.ts`.
//
// The page requests nothing by itself: `getOverview` reads local files only,
// `refreshCatalog` is the one network request (the "Load" button), and an
// install, update or removal happens only when a button for it is pressed.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { hasTauri } from "../ipc";

export type Result<T> = { ok: true; value: T } | { ok: false; error: string };

/** What an item is: a theme, a workflow, a script plugin or a native extension. */
export type ItemKind = "workflow" | "plugin" | "native" | "theme";

/** `available` can be installed, `update_available` has a newer version. */
export type CatalogState = "available" | "installed" | "update_available" | "unavailable";

/** Mirrors `sevak_plugins::extensions::CatalogItem`. */
export interface CatalogItem {
  id: string;
  kind: ItemKind;
  name: string;
  description: string;
  author: string;
  version: string;
  tags: string[];
  homepage: string | null;
  /** Native: where the source code is. */
  repository: string | null;
  license: string | null;
  /** Native: what the author says the program does beyond answering queries. Not enforced. */
  permissions: string[];
  min_sevak: string | null;
  /** The file for this computer, and its SHA-256. */
  source: string | null;
  sha256: string | null;
  /** Native: the platforms with a build. */
  platforms: string[];
  theme_mode: string | null;
  state: CatalogState;
  installed_version: string | null;
  /** Why it cannot be installed on this computer. */
  unavailable: string | null;
  /** Sevak installed it, so Sevak can remove it. */
  removable: boolean;
}

/** Mirrors `sevak_plugins::extensions::CatalogView`. */
export interface Catalog {
  source: string;
  note: string | null;
  themes_error: string | null;
  skipped: number;
  /** Seconds since the Unix epoch. */
  fetched_at: number;
  /** Read from the file saved by an earlier load, not just now. */
  from_cache: boolean;
  items: CatalogItem[];
}

/** `waiting` = needs the user's Allow before it can run. */
export type InstalledStatus = "ready" | "waiting" | "disabled" | "broken" | "theme";

/** Mirrors `InstalledRow` in `src-tauri/src/extensions.rs`. */
export interface InstalledItem {
  id: string;
  kind: ItemKind;
  name: string;
  folder: string;
  version: string;
  author: string;
  installed_at: number;
  update_to: string | null;
  program_sha256: string | null;
  enabled: boolean;
  status: InstalledStatus;
  problem: string | null;
  keywords: string[];
  can_review: boolean;
}

export interface Overview {
  installed: InstalledItem[];
  catalog: Catalog | null;
  /** `<os>-<arch>`, the build a native extension needs. */
  platform: string;
  plugins_folder: string;
}

const preview = () => import.meta.env.DEV && !hasTauri();

function errorText(err: unknown): string {
  return typeof err === "string" ? err : err instanceof Error ? err.message : String(err);
}

async function call<T>(command: string, args: Record<string, unknown> = {}): Promise<Result<T>> {
  try {
    return { ok: true, value: await invoke<T>(command, args) };
  } catch (err) {
    console.warn(`[ipc] ${command} failed:`, err);
    return { ok: false, error: errorText(err) };
  }
}

/** The preview data. A production build drops this import, so none of it ships. */
async function mock() {
  if (import.meta.env.DEV) return import("./mock");
  throw new Error("preview data is only available in development");
}

/** What is installed and the list saved earlier. Local files only; no network. */
export async function getOverview(): Promise<Result<Overview>> {
  if (preview()) return (await mock()).overview();
  return call("extensions_overview");
}

/** Loads the gallery list from GitHub. Only the Load / Refresh button calls this. */
export async function refreshCatalog(): Promise<Result<Overview>> {
  if (preview()) return (await mock()).refresh();
  return call("extensions_refresh");
}

export async function installItem(id: string): Promise<Result<Overview>> {
  if (preview()) return (await mock()).install(id);
  return call("extensions_install", { id });
}

export async function updateItem(id: string): Promise<Result<Overview>> {
  if (preview()) return (await mock()).update(id);
  return call("extensions_update", { id });
}

/** Resolves to `null` when the user declined the confirmation. */
export async function uninstallItem(id: string): Promise<Result<Overview | null>> {
  if (preview()) return (await mock()).uninstall(id);
  return call("extensions_uninstall", { id });
}

export async function setItemEnabled(id: string, enabled: boolean): Promise<Result<Overview>> {
  if (preview()) return (await mock()).setEnabled(id, enabled);
  return call("extensions_set_enabled", { id, enabled });
}

/** Shows the Allow dialog again; resolves to whether it was allowed. */
export async function reviewItem(id: string): Promise<Result<boolean>> {
  if (preview()) return (await mock()).review(id);
  return call("extensions_review", { id });
}

export async function openPluginsFolder(): Promise<Result<null>> {
  if (preview()) return { ok: true, value: null };
  return call("extensions_open_folder");
}

/** Called when something changed from outside the page (an install from the launcher). */
export function onChanged(handler: () => void): Promise<UnlistenFn> {
  if (!hasTauri()) return Promise.resolve(() => {});
  return listen("sevak:extensions", () => handler());
}

/**
 * Called with a page id when the launcher asks Settings to open on it, both for
 * a window that was already open (an event) and for one that was just created
 * (a pending request, read once). Returns a function that stops listening.
 */
export function onSettingsPage(handler: (page: string) => void): () => void {
  if (!hasTauri()) return () => {};
  let stopped = false;
  let unlisten: UnlistenFn | null = null;
  void invoke<string | null>("extensions_take_page")
    .then((page) => {
      if (page && !stopped) handler(page);
    })
    .catch(() => {});
  void listen<string>("sevak:settings-page", (event) => handler(event.payload)).then((fn) => {
    if (stopped) fn();
    else unlisten = fn;
  });
  return () => {
    stopped = true;
    unlisten?.();
  };
}
