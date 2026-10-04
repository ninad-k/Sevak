// IPC for Settings > Backup & restore. Like the other wrappers it also runs in
// a plain browser (`npm run dev`), against made-up data, so the page can be
// built and looked at without the app.

import { invoke } from "@tauri-apps/api/core";
import { hasTauri } from "../ipc";
import type {
  AutoSettings,
  BackupReport,
  CategoryId,
  CategoryInfo,
  CategoryPreview,
  Contents,
  Inspected,
  Mode,
  Overview,
  RestorePreview,
  RestoreResult,
} from "./backup-model";

export type Result<T> = { ok: true; value: T } | { ok: false; error: string };

const preview = () => import.meta.env.DEV && !hasTauri();

function errorText(err: unknown): string {
  return typeof err === "string" ? err : err instanceof Error ? err.message : String(err);
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<Result<T>> {
  try {
    return { ok: true, value: await invoke<T>(command, args) };
  } catch (err) {
    return { ok: false, error: errorText(err) };
  }
}

// ---------------------------------------------------------------------------
// What the app would send, for the browser preview
// ---------------------------------------------------------------------------

const CATEGORIES: CategoryInfo[] = [
  {
    id: "settings",
    label: "Settings",
    description:
      "Your shortcuts, appearance, search and plugin options, from config.toml. The 1Password section is left out.",
    default_on: true,
    runs_code: false,
  },
  {
    id: "snippets",
    label: "Snippets",
    description: "Your snippets and the snippet expansion options.",
    default_on: true,
    runs_code: false,
  },
  {
    id: "web_search",
    label: "Web search engines",
    description: "The web search engines and their keywords.",
    default_on: true,
    runs_code: false,
  },
  {
    id: "themes",
    label: "Themes",
    description: "Theme files and your custom stylesheet.",
    default_on: true,
    runs_code: false,
  },
  {
    id: "plugins",
    label: "Script plugins",
    description:
      "Script plugins and the ones installed from the gallery, with their scripts. They run code, so Sevak asks you to allow each one again after a restore.",
    default_on: true,
    runs_code: true,
  },
  {
    id: "workflows",
    label: "Workflows",
    description:
      "Workflows and the ones installed from the gallery, with their scripts. Sevak asks you to allow each one that runs code again after a restore.",
    default_on: true,
    runs_code: true,
  },
];

const NEVER = [
  "AI assistant and other API keys, tokens and passwords",
  "Clipboard history and the images you copied",
  "Search and usage history",
  "Anything from a password manager or the system credential store, including 1Password",
  "The encrypted history database, if you use one",
  "Logs and diagnostic reports",
  "Which scripts, plugins and workflows you allowed to run",
  "Per-plugin data folders and cached files",
];

let mockAuto: AutoSettings = {
  schedule: "off",
  on_update: false,
  keep: 5,
  folder: "",
  resolved_folder: "~/Documents/Sevak backups",
};
let mockLast: Overview["last_backup"] = null;
let mockUndo: Overview["undo"] = null;

const NOW = "2026-10-04T15:30:45+05:30";

function mockContents(ids: string[]): Contents {
  const all: Record<string, { entries: string[]; files: number; bytes: number }> = {
    settings: { entries: ["[general]", "[appearance]", "[search]", "[plugins]", "[[hotkey]] (1)"], files: 1, bytes: 1180 },
    snippets: { entries: ["[snippets]", "[[snippet]] (4)"], files: 1, bytes: 420 },
    web_search: { entries: ["[[web_search]] (3)"], files: 1, bytes: 380 },
    themes: { entries: ["Nord.toml", "Paper.toml"], files: 2, bytes: 2210 },
    plugins: { entries: ["hello (3 files)", "timers (5 files)"], files: 8, bytes: 9120 },
    workflows: { entries: ["clean-downloads (2 files)"], files: 2, bytes: 1640 },
  };
  const categories = CATEGORIES.filter((c) => ids.includes(c.id)).map((c) => ({
    category: c.id,
    label: c.label,
    ...all[c.id],
  }));
  return {
    categories,
    warnings: ids.includes("plugins")
      ? ['The script plugin "broken" was not included: plugin.toml is not valid: missing `protocol`.']
      : [],
    left_out: ids.includes("settings") ? ["[onepassword]"] : [],
    bytes: categories.reduce((sum, c) => sum + c.bytes, 0),
  };
}

const mockInspected = (): Inspected => ({
  display: "~/Documents/Sevak backups/sevak-backup-20261004-153045.sevakbackup",
  archive: {
    app_version: "0.1.0",
    created: NOW,
    created_unix: 1790000000,
    platform: "windows",
    kind: "manual",
    categories: CATEGORIES.map((c) => c.id),
    files: 15,
    size: 6840,
    sha256: "0f".repeat(32),
  },
  categories: CATEGORIES,
  warnings: [],
});

function mockPreview(ids: string[], mode: Mode): RestorePreview {
  const base = (id: CategoryId): CategoryPreview => {
    const info = CATEGORIES.find((c) => c.id === id)!;
    return {
      category: id,
      label: info.label,
      description: info.description,
      in_backup: true,
      selected: ids.includes(id),
      added: 0,
      changed: 0,
      unchanged: 0,
      removed: 0,
      items: [],
      problem: null,
    };
  };
  const settings = base("settings");
  settings.changed = 2;
  settings.unchanged = 3;
  settings.items = [
    { name: "General (shortcuts, startup, updates)", change: "changed", files: ["[general]"], needs_approval: false },
    { name: "Appearance", change: "changed", files: ["[appearance]"], needs_approval: false },
    { name: "Search", change: "unchanged", files: ["[search]"], needs_approval: false },
  ];
  const snippets = base("snippets");
  snippets.added = 2;
  snippets.changed = 1;
  snippets.removed = mode === "replace" ? 1 : 0;
  snippets.items = [
    { name: "Signature", change: "changed", files: [], needs_approval: false },
    { name: "Address", change: "added", files: [], needs_approval: false },
    { name: "Meeting link", change: "added", files: [], needs_approval: false },
  ];
  if (mode === "replace") {
    snippets.items.push({ name: "Local only", change: "removed", files: [], needs_approval: false });
  }
  const plugins = base("plugins");
  plugins.added = 1;
  plugins.changed = 1;
  plugins.items = [
    { name: "hello", change: "changed", files: ["~ main.py"], needs_approval: true },
    { name: "timers", change: "added", files: ["plugin.toml", "main.py", "lib/util.py"], needs_approval: true },
  ];
  const categories = (["settings", "snippets", "web_search", "themes", "plugins", "workflows"] as CategoryId[]).map(
    (id) => (id === "settings" ? settings : id === "snippets" ? snippets : id === "plugins" ? plugins : base(id)),
  );
  return {
    archive: mockInspected().archive,
    mode,
    categories,
    warnings: [],
    needs_approval: ['Plugin "hello"', 'Plugin "timers"'],
    problem: null,
    nothing_to_change: false,
  };
}

// ---------------------------------------------------------------------------
// The calls
// ---------------------------------------------------------------------------

export async function getOverview(): Promise<Result<Overview>> {
  if (preview()) {
    return {
      ok: true,
      value: {
        categories: CATEGORIES,
        never_included: NEVER,
        default_file_name: "sevak-backup-20261004-153045.sevakbackup",
        folder: mockAuto.resolved_folder,
        last_backup: mockLast,
        auto: mockAuto,
        auto_problem: null,
        undo: mockUndo,
        version: "0.1.0",
      },
    };
  }
  return call<Overview>("backup_overview");
}

/** What a backup of these categories would hold, without writing anything. */
export async function getContents(categories: string[]): Promise<Result<Contents>> {
  if (preview()) return { ok: true, value: mockContents(categories) };
  return call<Contents>("backup_contents", { categories });
}

/** Asks where to save, then makes the backup. `null` when cancelled. */
export async function saveBackupAs(categories: string[]): Promise<Result<BackupReport | null>> {
  if (preview()) return finishMockBackup(categories, "~/Documents/my-backup.sevakbackup");
  return call<BackupReport | null>("backup_save_as", { categories });
}

/** A backup in the default folder, without asking. */
export async function backupNow(categories: string[]): Promise<Result<BackupReport>> {
  if (preview()) {
    const done = await finishMockBackup(categories, `${mockAuto.resolved_folder}/sevak-backup-20261004-153045.sevakbackup`);
    return done.ok && done.value ? { ok: true, value: done.value } : { ok: false, error: "failed" };
  }
  return call<BackupReport>("backup_now", { categories });
}

async function finishMockBackup(categories: string[], display: string): Promise<Result<BackupReport | null>> {
  const contents = mockContents(categories);
  mockLast = { path: display, created: NOW, kind: "manual", exists: true };
  return {
    ok: true,
    value: { path: display, display, bytes: 6840, created: NOW, contents },
  };
}

/** Opens a file picker and checks the file. `null` when cancelled. */
export async function pickBackup(): Promise<Result<Inspected | null>> {
  if (preview()) return { ok: true, value: mockInspected() };
  return call<Inspected | null>("restore_pick");
}

export async function previewRestore(categories: string[], mode: Mode): Promise<Result<RestorePreview>> {
  if (preview()) return { ok: true, value: mockPreview(categories, mode) };
  return call<RestorePreview>("restore_preview", { categories, mode });
}

export async function applyRestore(
  categories: string[],
  mode: Mode,
  sha256: string,
): Promise<Result<RestoreResult>> {
  if (preview()) {
    const shown = mockPreview(categories, mode);
    mockUndo = { created: NOW, categories: categories as CategoryId[] };
    return {
      ok: true,
      value: {
        mode,
        categories: shown.categories.filter((c) => c.selected),
        snapshot: "~/AppData/Roaming/sevak/backup-snapshots/pre-restore-20261004-153045.sevakbackup",
        needs_approval: shown.needs_approval,
        warnings: [],
        changed: true,
      },
    };
  }
  return call<RestoreResult>("restore_apply", { categories, mode, sha256 });
}

export async function undoRestore(): Promise<Result<RestoreResult>> {
  if (preview()) {
    mockUndo = null;
    return {
      ok: true,
      value: { mode: "replace", categories: [], snapshot: null, needs_approval: [], warnings: [], changed: true },
    };
  }
  return call<RestoreResult>("restore_undo");
}

export async function setAuto(auto: AutoSettings): Promise<Result<AutoSettings>> {
  if (preview()) {
    mockAuto = { ...auto, resolved_folder: auto.folder || "~/Documents/Sevak backups" };
    return { ok: true, value: mockAuto };
  }
  return call<AutoSettings>("backup_set_auto", { auto });
}

export async function openBackupFolder(): Promise<Result<null>> {
  if (preview()) return { ok: true, value: null };
  const done = await call<null>("backup_open_folder");
  return done;
}
