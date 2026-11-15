// Pure helpers of the Extensions page: labels, filtering and wording. No DOM and
// no IPC, so they are tested in plain node (model.test.ts).

import { currentLanguage, t, type MessageKey } from "../i18n.svelte";
import type { CatalogItem, InstalledItem, ItemKind } from "./extensions-ipc";

/** A message under `extensions.`, for keys built from a kind, state or status. */
const x = (key: string) => t(`extensions.${key}` as MessageKey);

/** Words per kind, looked up on every read so they follow the language. */
function byKind(group: string): Record<ItemKind, string> {
  return {
    get workflow() {
      return x(`${group}.workflow`);
    },
    get plugin() {
      return x(`${group}.plugin`);
    },
    get native() {
      return x(`${group}.native`);
    },
    get theme() {
      return x(`${group}.theme`);
    },
  };
}

/** A kind's name: "Script plugin". */
export const KIND_LABEL = byKind("kind");
/** Several of a kind, for the filter: "Script plugins". */
export const KIND_PLURAL = byKind("kindPlural");
/** A kind inside a sentence: "this script plugin". */
export const KIND_NOUN = byKind("kindNoun");

/** Kinds in the order the filter lists them. */
export const KIND_ORDER: ItemKind[] = ["workflow", "plugin", "native", "theme"];

export type KindFilter = "all" | ItemKind;

/** The permissions Sevak can explain in plain words (`extensions.permission.*`). */
const KNOWN_PERMISSIONS = ["network", "filesystem", "processes", "clipboard", "system"];

export function permissionLabel(permission: string): string {
  const meaning = KNOWN_PERMISSIONS.includes(permission)
    ? x(`permission.${permission}`)
    : t("extensions.permission.own");
  return `${permission}: ${meaning}`;
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
  return x(`status.${item.status}`);
}

/** The state button's text for a catalog item. */
export function actionLabel(item: CatalogItem): string {
  return x(`action.${item.state}`);
}

/** A readable age in the current language: "just now", "3 hours ago", "5 days ago". */
export function ago(epochSeconds: number, nowMs: number = Date.now()): string {
  const seconds = Math.max(0, Math.floor(nowMs / 1000 - epochSeconds));
  if (seconds < 60) return t("extensions.justNow");
  const relative = new Intl.RelativeTimeFormat(currentLanguage(), { numeric: "always" });
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return relative.format(-minutes, "minute");
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return relative.format(-hours, "hour");
  return relative.format(-Math.floor(hours / 24), "day");
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
  return x(`warning.${item.kind}`);
}
