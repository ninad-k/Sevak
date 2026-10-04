import { describe, expect, it } from "vitest";
import {
  applyRestore,
  backupNow,
  getContents,
  getOverview,
  pickBackup,
  previewRestore,
  setAuto,
  undoRestore,
} from "./backup-ipc";
import {
  changeLabel,
  changesSomething,
  chosen,
  clampKeep,
  defaultTicks,
  formatBytes,
  formatWhen,
  platformName,
  removals,
  summarize,
  type CategoryInfo,
  type RestorePreview,
} from "./backup-model";

const category = (id: string, default_on = true): CategoryInfo => ({
  id: id as CategoryInfo["id"],
  label: id,
  description: "",
  default_on,
  runs_code: false,
});

describe("the choices", () => {
  it("lists the ticked categories in the page's order", () => {
    const all = [category("settings"), category("themes"), category("plugins")];
    expect(chosen(all, { plugins: true, settings: true, themes: false })).toEqual([
      "settings",
      "plugins",
    ]);
    expect(chosen(all, {})).toEqual([]);
  });

  it("starts from each category's default", () => {
    expect(defaultTicks([category("a"), category("b", false)])).toEqual({ a: true, b: false });
  });
});

describe("summaries", () => {
  it("says what changes, and what stays the same", () => {
    expect(summarize({ added: 2, changed: 1, removed: 0, unchanged: 0 })).toBe("2 new, 1 changed");
    expect(summarize({ added: 0, changed: 0, removed: 3, unchanged: 4 })).toBe("3 removed, 4 the same");
    expect(summarize({ added: 0, changed: 0, removed: 0, unchanged: 5 })).toBe("5 already the same");
    expect(summarize({ added: 0, changed: 0, removed: 0, unchanged: 0 })).toBe("nothing in it");
  });

  it("names a kept item differently from a removed one", () => {
    expect(changeLabel("removed", "replace")).toBe("Removed");
    expect(changeLabel("removed", "merge")).toBe("Kept");
    expect(changeLabel("added", "merge")).toBe("New");
  });

  it("counts only the ticked categories", () => {
    const line = (selected: boolean, added: number, removed: number) =>
      ({
        category: "settings",
        label: "Settings",
        description: "",
        in_backup: true,
        selected,
        added,
        changed: 0,
        unchanged: 0,
        removed,
        items: [],
        problem: null,
      }) as RestorePreview["categories"][number];
    const preview = (categories: RestorePreview["categories"]): RestorePreview =>
      ({ categories, warnings: [], needs_approval: [], problem: null, nothing_to_change: false }) as unknown as RestorePreview;

    expect(changesSomething(null)).toBe(false);
    expect(changesSomething(preview([line(true, 0, 0)]))).toBe(false);
    expect(changesSomething(preview([line(false, 3, 0)]))).toBe(false);
    expect(changesSomething(preview([line(true, 1, 0)]))).toBe(true);
    expect(removals(preview([line(true, 0, 2), line(true, 0, 1), line(false, 0, 9)]))).toBe(3);
    expect(removals(null)).toBe(0);
  });
});

describe("formatting", () => {
  it("formats sizes", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(2048)).toBe("2.0 KB");
    expect(formatBytes(20 * 1024)).toBe("20 KB");
    expect(formatBytes(3 * 1024 * 1024)).toBe("3.0 MB");
  });

  it("shows a time the person's way, or the text itself", () => {
    expect(formatWhen("not a date")).toBe("not a date");
    expect(formatWhen("2026-10-04T15:30:45+05:30")).not.toBe("2026-10-04T15:30:45+05:30");
  });

  it("names platforms", () => {
    expect(platformName("windows")).toBe("Windows");
    expect(platformName("macos")).toBe("macOS");
    expect(platformName("linux")).toBe("Linux");
    expect(platformName("haiku")).toBe("haiku");
  });

  it("keeps the number of backups to keep in range", () => {
    expect(clampKeep(0)).toBe(1);
    expect(clampKeep(7.4)).toBe(7);
    expect(clampKeep(500)).toBe(50);
    expect(clampKeep(Number.NaN)).toBe(5);
  });
});

describe("the browser preview of the calls", () => {
  it("answers like the app, so the page can be built without it", async () => {
    const overview = await getOverview();
    expect(overview.ok).toBe(true);
    if (!overview.ok) return;
    expect(overview.value.categories.map((c) => c.id)).toEqual([
      "settings",
      "snippets",
      "web_search",
      "themes",
      "plugins",
      "workflows",
    ]);
    expect(overview.value.auto.schedule).toBe("off");
    expect(overview.value.never_included.length).toBeGreaterThan(3);

    const contents = await getContents(["themes"]);
    expect(contents.ok && contents.value.categories.map((c) => c.category)).toEqual(["themes"]);

    const made = await backupNow(["settings"]);
    expect(made.ok).toBe(true);
    const again = await getOverview();
    expect(again.ok && again.value.last_backup?.kind).toBe("manual");

    const picked = await pickBackup();
    expect(picked.ok && picked.value?.archive.sha256).toHaveLength(64);

    const merge = await previewRestore(["settings", "snippets"], "merge");
    const replace = await previewRestore(["settings", "snippets"], "replace");
    expect(merge.ok && removals(merge.value)).toBe(0);
    expect(replace.ok && removals(replace.value)).toBeGreaterThan(0);

    const applied = await applyRestore(["plugins"], "merge", "x");
    expect(applied.ok && applied.value.needs_approval.length).toBeGreaterThan(0);
    const withUndo = await getOverview();
    expect(withUndo.ok && withUndo.value.undo).not.toBeNull();
    await undoRestore();
    const withoutUndo = await getOverview();
    expect(withoutUndo.ok && withoutUndo.value.undo).toBeNull();

    const saved = await setAuto({
      schedule: "weekly",
      on_update: true,
      keep: 3,
      folder: "~/b",
      resolved_folder: "",
    });
    expect(saved.ok && saved.value.resolved_folder).toBe("~/b");
  });
});
