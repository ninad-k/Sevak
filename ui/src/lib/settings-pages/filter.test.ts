import { describe, expect, it } from "vitest";
import { filterOptions } from "./filter";
import type { KeyOption } from "./types";

const options: KeyOption[] = [
  { key: "dark_mode", label: "Toggle dark mode", group: "Desktop" },
  { key: "force_quit_app", label: "Force quit an app", group: "Apps & power" },
  { key: "wifi", label: "Turn Wi-Fi on or off", group: "Network" },
];

describe("filterOptions", () => {
  it("keeps all options for an empty query", () => {
    expect(filterOptions(options, "")).toBe(options);
  });

  it("matches the label, the group and the key as words", () => {
    expect(filterOptions(options, "dark").map((o) => o.key)).toEqual(["dark_mode"]);
    expect(filterOptions(options, "network").map((o) => o.key)).toEqual(["wifi"]);
    expect(filterOptions(options, "force quit").map((o) => o.key)).toEqual(["force_quit_app"]);
  });

  it("needs every word and ignores case", () => {
    expect(filterOptions(options, "QUIT app").map((o) => o.key)).toEqual(["force_quit_app"]);
    expect(filterOptions(options, "quit wifi")).toEqual([]);
  });
});
