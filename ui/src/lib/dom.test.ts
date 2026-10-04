// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";
import { applyAppearance } from "./appearance";
import { applyTheme } from "./theme";
import { cssVars } from "./themes";

beforeEach(() => {
  document.head.innerHTML = "";
  document.documentElement.removeAttribute("data-theme");
  document.documentElement.removeAttribute("data-blur");
});

describe("applyTheme", () => {
  it("pins light and dark with data-theme", () => {
    applyTheme("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
    applyTheme("light");
    expect(document.documentElement.dataset.theme).toBe("light");
  });

  it("removes the pin for system, null and undefined so the media query decides", () => {
    for (const value of ["system", null, undefined] as const) {
      applyTheme("dark");
      applyTheme(value);
      expect(document.documentElement.dataset.theme, String(value)).toBeUndefined();
    }
  });
});

describe("applyAppearance", () => {
  const sheet = (id: string) => document.getElementById(id) as HTMLStyleElement | null;

  it("injects the appearance sheet before the custom one, both last in <head>", () => {
    applyAppearance({ css: ":root{--accent:red}", custom_css: ".x{}", warnings: [], blur: false });
    const ids = [...document.head.querySelectorAll("style")].map((s) => s.id);
    expect(ids).toEqual(["sevak-appearance", "sevak-custom-css"]);
    expect(sheet("sevak-appearance")!.textContent).toBe(":root{--accent:red}");
  });

  it("updates in place instead of adding sheets", () => {
    applyAppearance({ css: "a{}", custom_css: "", warnings: [], blur: false });
    const first = sheet("sevak-appearance");
    applyAppearance({ css: "b{}", custom_css: "", warnings: [], blur: false });
    expect(document.head.querySelectorAll("#sevak-appearance")).toHaveLength(1);
    expect(sheet("sevak-appearance")).toBe(first);
    expect(first!.textContent).toBe("b{}");
  });

  it("removes a sheet whose css became empty", () => {
    applyAppearance({ css: "a{}", custom_css: "b{}", warnings: [], blur: false });
    applyAppearance({ css: "a{}", custom_css: "", warnings: [], blur: false });
    expect(sheet("sevak-custom-css")).toBeNull();
    expect(sheet("sevak-appearance")).not.toBeNull();
  });

  it("toggles the blur attribute", () => {
    applyAppearance({ css: "", custom_css: "", warnings: [], blur: true });
    expect(document.documentElement.hasAttribute("data-blur")).toBe(true);
    applyAppearance({ css: "", custom_css: "", warnings: [], blur: false });
    expect(document.documentElement.hasAttribute("data-blur")).toBe(false);
  });

  it("ignores a missing payload", () => {
    applyAppearance({ css: "a{}", custom_css: "", warnings: [], blur: true });
    applyAppearance(null);
    applyAppearance(undefined);
    expect(sheet("sevak-appearance")!.textContent).toBe("a{}");
    expect(document.documentElement.hasAttribute("data-blur")).toBe(true);
  });
});

describe("cssVars action", () => {
  it("sets custom properties inline and drops the ones an update no longer has", () => {
    const node = document.createElement("div");
    const action = cssVars(node, { "--bg": "#fff", "--fg": "#000" });
    expect(node.style.getPropertyValue("--bg")).toBe("#fff");
    action.update({ "--bg": "#111" });
    expect(node.style.getPropertyValue("--bg")).toBe("#111");
    expect(node.style.getPropertyValue("--fg")).toBe("");
  });
});
