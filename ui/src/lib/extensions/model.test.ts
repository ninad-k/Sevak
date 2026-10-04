import { describe, expect, it } from "vitest";
import type { CatalogItem, InstalledItem } from "./extensions-ipc";
import {
  actionLabel,
  ago,
  hostOf,
  installWarning,
  matches,
  permissionLabel,
  shortHash,
  statusLabel,
  terms,
  updateCount,
  visibleItems,
} from "./model";

function item(over: Partial<CatalogItem>): CatalogItem {
  return {
    id: "x",
    kind: "workflow",
    name: "X",
    description: "",
    author: "Sevak",
    version: "1.0",
    tags: [],
    homepage: null,
    repository: null,
    license: null,
    permissions: [],
    min_sevak: null,
    source: null,
    sha256: null,
    platforms: [],
    theme_mode: null,
    state: "available",
    installed_version: null,
    unavailable: null,
    removable: false,
    ...over,
  };
}

function installed(over: Partial<InstalledItem>): InstalledItem {
  return {
    id: "x",
    kind: "workflow",
    name: "X",
    folder: "x",
    version: "1.0",
    author: "Sevak",
    installed_at: 0,
    update_to: null,
    program_sha256: null,
    enabled: true,
    status: "ready",
    problem: null,
    keywords: [],
    can_review: false,
    ...over,
  };
}

describe("search", () => {
  it("splits a query into lower-case words", () => {
    expect(terms("  Case  CONVERTER ")).toEqual(["case", "converter"]);
    expect(terms("")).toEqual([]);
  });

  it("matches name, id, author, description, kind and tags, every word required", () => {
    const tool = item({
      id: "case-converter-node",
      name: "Case converter",
      author: "Ada",
      description: "camelCase and snake_case",
      kind: "plugin",
      tags: ["needs-node"],
    });
    for (const q of ["case", "converter", "ada", "snake", "script", "needs-node", "case ada"]) {
      expect(matches(tool, terms(q)), q).toBe(true);
    }
    expect(matches(tool, terms("case zzz"))).toBe(false);
    expect(matches(tool, [])).toBe(true);
  });
});

describe("the list", () => {
  const items = [
    item({ id: "b", name: "Beta", state: "installed" }),
    item({ id: "a", name: "alpha", state: "available" }),
    item({ id: "c", name: "Gamma", state: "update_available" }),
    item({ id: "d", name: "Delta", state: "unavailable", kind: "native" }),
    item({ id: "e", name: "Epsilon", state: "available", kind: "theme" }),
  ];

  it("lists updates, then installable, then installed, then unavailable; each by name", () => {
    expect(visibleItems(items, "all", "").map((i) => i.id)).toEqual(["c", "a", "e", "b", "d"]);
  });

  it("filters by kind and by search", () => {
    expect(visibleItems(items, "theme", "").map((i) => i.id)).toEqual(["e"]);
    expect(visibleItems(items, "all", "eps").map((i) => i.id)).toEqual(["e"]);
    expect(visibleItems(items, "native", "alpha")).toEqual([]);
  });

  it("does not change the list it is given", () => {
    const before = items.map((i) => i.id);
    visibleItems(items, "all", "");
    expect(items.map((i) => i.id)).toEqual(before);
  });
});

describe("wording", () => {
  it("says what each state does", () => {
    expect(actionLabel(item({ state: "available" }))).toBe("Install");
    expect(actionLabel(item({ state: "update_available" }))).toBe("Update");
    expect(actionLabel(item({ state: "installed" }))).toBe("Installed");
    expect(actionLabel(item({ state: "unavailable" }))).toBe("Not available");
  });

  it("names the installed statuses", () => {
    expect(statusLabel(installed({ status: "waiting" }))).toBe("Waiting for your permission");
    expect(statusLabel(installed({ status: "broken" }))).toBe("Not loaded");
    expect(statusLabel(installed({ status: "disabled" }))).toBe("Switched off");
  });

  it("counts updates", () => {
    expect(
      updateCount([installed({ update_to: "2.0" }), installed({}), installed({ update_to: "1.1" })]),
    ).toBe(2);
  });

  it("explains permissions, and shows unknown ones as the author's label", () => {
    expect(permissionLabel("network")).toContain("internet");
    expect(permissionLabel("gpu")).toContain("author's own label");
  });

  it("warns honestly about native extensions", () => {
    const text = installWarning(item({ kind: "native" }));
    expect(text).toContain("compiled program");
    expect(text).toContain("does not sandbox");
    expect(text).toContain("never runs before you allow it");
    expect(installWarning(item({ kind: "theme" }))).toContain("only changes colors");
  });

  it("formats ages", () => {
    const now = 1_000_000_000 * 1000;
    const at = (secondsAgo: number) => ago(1_000_000_000 - secondsAgo, now);
    expect(at(5)).toBe("just now");
    expect(at(60)).toBe("1 minute ago");
    expect(at(3 * 3600)).toBe("3 hours ago");
    expect(at(2 * 86400)).toBe("2 days ago");
    expect(ago(1_000_000_000 + 500, now)).toBe("just now");
  });

  it("shows hosts and short hashes", () => {
    expect(hostOf("https://raw.githubusercontent.com/x/y")).toBe("raw.githubusercontent.com");
    expect(hostOf("not a url")).toBe("not a url");
    expect(shortHash("abcdef0123456789")).toBe("abcdef012345");
  });
});
