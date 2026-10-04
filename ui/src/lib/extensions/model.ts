// Pure helpers of the Extensions page: labels, filtering and wording. No DOM and
// no IPC, so they are tested in plain node (model.test.ts).

import type { CatalogItem, InstalledItem, ItemKind } from "./extensions-ipc";

export const KIND_LABEL: Record<ItemKind, string> = {
  workflow: "Workflow",
  plugin: "Script plugin",
  native: "Native extension",
  theme: "Theme",
};

/** Kinds in the order the filter lists them. */
export const KIND_ORDER: ItemKind[] = ["workflow", "plugin", "native", "theme"];

export type KindFilter = "all" | ItemKind;

/** What a permission a native extension declares means, in plain words. */
const PERMISSION_MEANING: Record<string, string> = {
  network: "connects to the internet or your network",
  filesystem: "reads or writes files outside its own folders",
  processes: "starts other programs",
  clipboard: "reads or changes the clipboard itself",
  system: "changes system settings",
};

export function permissionLabel(permission: string): string {
  const meaning = PERMISSION_MEANING[permission];
  return meaning ? `${permission}: ${meaning}` : `${permission}: (the author's own label)`;
}

/** Lower-case words of a search box. */
export function terms(query: string): string[] {
  return query
    .toLowerCase()
    .split(/\s+/)
    .filter((word) => word.length > 0);
}

/** Whether `item` matches every word: name, id, tags, author, description, kind. */
export function matches(item: CatalogItem, words: string[]): boolean {
  if (words.length === 0) return true;
  const haystack = [
    item.name,
    item.id,
    item.author,
    item.description,
    KIND_LABEL[item.kind],
    ...item.tags,
  ]
    .join("\n")
    .toLowerCase();
  return words.every((word) => haystack.includes(word));
}

/**
 * The catalog as listed: filtered by kind and search, updates first, then what can
 * be installed, then what is installed, then what is not offered here; each group
 * alphabetical.
 */
export function visibleItems(
  items: CatalogItem[],
  filter: KindFilter,
  query: string,
): CatalogItem[] {
  const words = terms(query);
  const rank = { update_available: 0, available: 1, installed: 2, unavailable: 3 } as const;
  return items
    .filter((item) => filter === "all" || item.kind === filter)
    .filter((item) => matches(item, words))
    .sort(
      (a, b) =>
        rank[a.state] - rank[b.state] ||
        a.name.localeCompare(b.name, undefined, { sensitivity: "base" }),
    );
}

/** How many installed items have a newer version in the loaded list. */
export function updateCount(installed: InstalledItem[]): number {
  return installed.filter((item) => item.update_to !== null).length;
}

/** The status chip's words for an installed item. */
export function statusLabel(item: InstalledItem): string {
  switch (item.status) {
    case "ready":
      return "Ready";
    case "waiting":
      return "Waiting for your permission";
    case "disabled":
      return "Switched off";
    case "broken":
      return "Not loaded";
    case "theme":
      return "Installed";
  }
}

/** The state button's text for a catalog item. */
export function actionLabel(item: CatalogItem): string {
  switch (item.state) {
    case "available":
      return "Install";
    case "update_available":
      return "Update";
    case "installed":
      return "Installed";
    case "unavailable":
      return "Not available";
  }
}

/** A readable age: "just now", "3 hours ago", "5 days ago". */
export function ago(epochSeconds: number, nowMs: number = Date.now()): string {
  const seconds = Math.max(0, Math.floor(nowMs / 1000 - epochSeconds));
  if (seconds < 60) return "just now";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} minute${minutes === 1 ? "" : "s"} ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} hour${hours === 1 ? "" : "s"} ago`;
  const days = Math.floor(hours / 24);
  return `${days} day${days === 1 ? "" : "s"} ago`;
}

/** The host of an address, for showing where something is downloaded from. */
export function hostOf(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

/** The first 12 characters of a checksum, for rows that cannot show all 64. */
export function shortHash(hash: string): string {
  return hash.slice(0, 12);
}

/** What the page says before an install, per kind. */
export function installWarning(item: CatalogItem): string {
  switch (item.kind) {
    case "native":
      return (
        "A native extension is a compiled program, not a script you can read. Sevak does not " +
        "sandbox it: once you allow it, it runs with your account's permissions. It never runs " +
        "before you allow it in the dialog that follows, which shows its publisher, the " +
        "permissions it declares and its checksum."
      );
    case "theme":
      return "A theme only changes colors and sizes. It is checked and rewritten before it is saved.";
    case "plugin":
      return "A script plugin runs code. It does not run until you allow it in the dialog that follows.";
    case "workflow":
      return "A workflow that runs a program or pastes into another app waits for your permission; one that only opens links does not run any code.";
  }
}
