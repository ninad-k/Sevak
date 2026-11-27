import { describe, expect, it } from "vitest";
import { SECTION_IDS, SETTINGS_INDEX, filterSections, isSectionId } from "./settings-search";

import rustIndex from "../../../crates/sevak-plugins/src/settings_index.json";
import settingsSource from "../Settings.svelte?raw";

const sections = [
  { id: "general", label: "General" },
  { id: "hotkeys", label: "Hotkeys" },
  { id: "appearance", label: "Appearance" },
  { id: "clipboard", label: "Clipboard & paste" },
  { id: "backup", label: "Backup & restore" },
];

describe("filterSections", () => {
  it("keeps everything for an empty query", () => {
    expect(filterSections(sections, "")).toEqual(sections);
    expect(filterSections(sections, "   ")).toEqual(sections);
  });

  it("matches the title, ignoring case", () => {
    expect(filterSections(sections, "HOTK").map((s) => s.id)).toEqual(["hotkeys"]);
  });

  it("matches synonyms from the index", () => {
    expect(filterSections(sections, "dark").map((s) => s.id)).toEqual(["appearance"]);
    expect(filterSections(sections, "snippets").map((s) => s.id)).toEqual(["clipboard"]);
    expect(filterSections(sections, "shortcut").map((s) => s.id)).toEqual(["hotkeys"]);
  });

  it("needs every word to match", () => {
    expect(filterSections(sections, "clipboard history").map((s) => s.id)).toEqual(["clipboard"]);
    expect(filterSections(sections, "clipboard zzz")).toEqual([]);
  });
});

describe("the settings index", () => {
  it("has the same sections as the sidebar in Settings.svelte", () => {
    const source = settingsSource;
    const block = source.slice(source.indexOf("const sections = $derived("));
    const ids = [...block.slice(0, block.indexOf(".map((section)")).matchAll(/\{ id: "(\w+)", label: t\("settings\.sections\./g)].map(
      (m) => m[1],
    );
    expect([...ids].sort()).toEqual([...SECTION_IDS].sort());
  });

  it("is identical to the file the launcher's `? ` search reads", () => {
    expect(SETTINGS_INDEX).toEqual(rustIndex);
  });

  it("points every option at a known section", () => {
    for (const entry of SETTINGS_INDEX) expect(isSectionId(entry.section)).toBe(true);
  });
});
