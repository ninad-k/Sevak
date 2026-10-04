// Types and small pure helpers for Settings > Backup & restore. The shapes
// mirror the Rust side (`src-tauri/src/backup.rs` and the `sevak-backup` crate).

export type CategoryId =
  | "settings"
  | "snippets"
  | "web_search"
  | "themes"
  | "plugins"
  | "workflows";

export type Mode = "merge" | "replace";
export type Change = "added" | "changed" | "unchanged" | "removed";

export interface CategoryInfo {
  id: CategoryId;
  label: string;
  description: string;
  /** Ticked when the page opens. */
  default_on: boolean;
  /** Restored scripts ask for approval again. */
  runs_code: boolean;
}

export interface LastBackup {
  path: string;
  created: string;
  kind: "manual" | "auto";
  /** The file is still where it was saved. */
  exists: boolean;
}

export type Schedule = "off" | "daily" | "weekly";

export interface AutoSettings {
  schedule: Schedule;
  on_update: boolean;
  keep: number;
  /** As typed; empty means the default folder. */
  folder: string;
  resolved_folder: string;
}

export interface Overview {
  categories: CategoryInfo[];
  never_included: string[];
  default_file_name: string;
  /** Where "Back up now" and automatic backups write. */
  folder: string;
  last_backup: LastBackup | null;
  auto: AutoSettings;
  /** backup.toml is damaged: the reason. */
  auto_problem: string | null;
  undo: { created: string; categories: CategoryId[] } | null;
  version: string;
}

export interface CategoryContents {
  category: CategoryId;
  label: string;
  entries: string[];
  files: number;
  bytes: number;
}

export interface Contents {
  categories: CategoryContents[];
  warnings: string[];
  /** config.toml sections that stay out on purpose. */
  left_out: string[];
  bytes: number;
}

export interface BackupReport {
  path: string;
  display: string;
  bytes: number;
  created: string;
  contents: Contents;
}

export interface ArchiveInfo {
  app_version: string;
  created: string;
  created_unix: number;
  platform: string;
  kind: "manual" | "auto" | "snapshot";
  categories: CategoryId[];
  files: number;
  size: number;
  /** Identifies the exact file that was checked. */
  sha256: string;
}

export interface Inspected {
  display: string;
  archive: ArchiveInfo;
  categories: CategoryInfo[];
  warnings: string[];
}

export interface ItemPreview {
  name: string;
  change: Change;
  files: string[];
  needs_approval: boolean;
}

export interface CategoryPreview {
  category: CategoryId;
  label: string;
  description: string;
  in_backup: boolean;
  selected: boolean;
  added: number;
  changed: number;
  unchanged: number;
  removed: number;
  items: ItemPreview[];
  problem: string | null;
}

export interface RestorePreview {
  archive: ArchiveInfo;
  mode: Mode;
  categories: CategoryPreview[];
  warnings: string[];
  /** Scripts, plugins and workflows that will ask for approval again. */
  needs_approval: string[];
  problem: string | null;
  nothing_to_change: boolean;
}

export interface RestoreResult {
  mode: Mode;
  categories: CategoryPreview[];
  /** The safety copy "Undo restore" uses. */
  snapshot: string | null;
  needs_approval: string[];
  warnings: string[];
  changed: boolean;
}

/** The ids of the ticked categories, in the order of `categories`. */
export function chosen(categories: { id: string }[], ticks: Record<string, boolean>): string[] {
  return categories.filter((c) => ticks[c.id]).map((c) => c.id);
}

/** Every category ticked or not, from its default. */
export function defaultTicks(categories: CategoryInfo[]): Record<string, boolean> {
  return Object.fromEntries(categories.map((c) => [c.id, c.default_on]));
}

/** "2 new, 1 changed" for one category; "nothing changes" when it all matches. */
export function summarize(category: {
  added: number;
  changed: number;
  removed: number;
  unchanged: number;
}): string {
  const parts: string[] = [];
  if (category.added > 0) parts.push(`${category.added} new`);
  if (category.changed > 0) parts.push(`${category.changed} changed`);
  if (category.removed > 0) parts.push(`${category.removed} removed`);
  if (parts.length === 0) {
    return category.unchanged > 0 ? `${category.unchanged} already the same` : "nothing in it";
  }
  if (category.unchanged > 0) parts.push(`${category.unchanged} the same`);
  return parts.join(", ");
}

/** What a change is called in the list (the word, not the code). */
export function changeLabel(change: Change, mode: Mode): string {
  switch (change) {
    case "added":
      return "New";
    case "changed":
      return "Changed";
    case "unchanged":
      return "Same";
    case "removed":
      return mode === "replace" ? "Removed" : "Kept";
  }
}

/** Whether a restore in this state would do anything, among the ticked categories. */
export function changesSomething(preview: RestorePreview | null): boolean {
  if (!preview) return false;
  return preview.categories.some(
    (c) => c.selected && c.added + c.changed + c.removed > 0,
  );
}

/** How many things Replace mode would remove or reset, among the ticked categories. */
export function removals(preview: RestorePreview | null): number {
  if (!preview) return 0;
  return preview.categories.filter((c) => c.selected).reduce((sum, c) => sum + c.removed, 0);
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(bytes < 10 * 1024 ? 1 : 0)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/** An ISO time as the person's own date and time; the text itself if it does not parse. */
export function formatWhen(iso: string): string {
  const time = Date.parse(iso);
  if (Number.isNaN(time)) return iso;
  return new Date(time).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

export function platformName(platform: string): string {
  switch (platform) {
    case "windows":
      return "Windows";
    case "macos":
      return "macOS";
    case "linux":
      return "Linux";
    default:
      return platform;
  }
}

export const SCHEDULES: { value: Schedule; label: string }[] = [
  { value: "off", label: "Off" },
  { value: "daily", label: "Every day" },
  { value: "weekly", label: "Every week" },
];

export const MAX_KEEP = 50;

/** `keep` as the backend will see it: a whole number from 1 to 50. */
export function clampKeep(value: number): number {
  if (!Number.isFinite(value)) return 5;
  return Math.min(MAX_KEEP, Math.max(1, Math.round(value)));
}
