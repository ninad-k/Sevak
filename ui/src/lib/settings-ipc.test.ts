import { afterEach, describe, expect, it, vi } from "vitest";
import { hasTauri } from "./ipc";
import { fallbackList } from "./settings-ipc";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("fallbackList", () => {
  it("accepts the single-string and the list form of config.toml", () => {
    expect(fallbackList("g")).toEqual(["g"]);
    expect(fallbackList(["g", "yt"])).toEqual(["g", "yt"]);
  });

  it("trims names and drops blanks, keeping the order", () => {
    expect(fallbackList("  g ")).toEqual(["g"]);
    expect(fallbackList(["yt", "", "  ", " g"])).toEqual(["yt", "g"]);
    expect(fallbackList("")).toEqual([]);
    expect(fallbackList([])).toEqual([]);
  });
});

describe("hasTauri", () => {
  it("is false in a plain browser or node", () => {
    expect(hasTauri()).toBe(false);
  });

  it("is true once the Tauri runtime has injected its internals", () => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    expect(hasTauri()).toBe(true);
  });
});
