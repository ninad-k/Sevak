// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mockSettings } from "../mock";
import type { Config } from "../settings-ipc";
import { MAX_WINDOW_GAP, validate } from "../validate";
import WindowsPage from "./WindowsPage.svelte";
import {
  WINDOW_COMMANDS,
  commandGroups,
  hotkeyRun,
  isValidGap,
  typedCommand,
} from "./windows-commands";

// mock.ts reads `location` when it builds the browser-preview settings.
beforeEach(() => {
  vi.stubGlobal("location", { search: "" });
});
afterEach(cleanup);

const config = (): Config => structuredClone(mockSettings().config);

function show(c: Config, platform: "windows" | "macos" | "linux" = "windows", off = false) {
  return render(WindowsPage, {
    config: c,
    problems: validate(c),
    platform,
    pluginOff: () => off,
  });
}

describe("the cheat sheet data", () => {
  it("has unique keys and typed spellings", () => {
    const keys = WINDOW_COMMANDS.map((c) => c.key);
    expect(new Set(keys).size).toBe(keys.length);
    const typed = WINDOW_COMMANDS.flatMap((c) => c.typed.map((t) => t.toLowerCase()));
    expect(new Set(typed).size).toBe(typed.length);
    for (const command of WINDOW_COMMANDS) {
      expect(command.typed.length, command.key).toBeGreaterThan(0);
      expect(command.key, command.key).toMatch(/^[a-z_]+$/);
    }
  });

  it("lists the commands the task names", () => {
    const typed = new Set(WINDOW_COMMANDS.map((c) => c.typed[0]));
    for (const wanted of [
      "left",
      "right",
      "top",
      "bottom",
      "max",
      "center",
      "restore",
      "next display",
      "previous display",
      "almost-max",
    ]) {
      expect(typed.has(wanted), wanted).toBe(true);
    }
  });

  it("groups every command exactly once, in order", () => {
    const groups = commandGroups();
    expect(groups.map((g) => g.name)).toEqual([
      "Halves",
      "Quarters",
      "Thirds",
      "Size and place",
      "Displays",
    ]);
    expect(groups.flatMap((g) => g.commands)).toEqual(WINDOW_COMMANDS);
  });

  it("builds what to type and the hotkey target", () => {
    const left = WINDOW_COMMANDS[0];
    expect(typedCommand("win", left)).toBe("win left");
    expect(typedCommand("  snap ", left)).toBe("snap left");
    expect(typedCommand("", left)).toBe("win left");
    expect(hotkeyRun(left)).toBe("windows:left");
  });

  it("accepts a gap only as a whole number in range", () => {
    expect(isValidGap(0, MAX_WINDOW_GAP)).toBe(true);
    expect(isValidGap(12, MAX_WINDOW_GAP)).toBe(true);
    expect(isValidGap(MAX_WINDOW_GAP, MAX_WINDOW_GAP)).toBe(true);
    for (const bad of [-1, MAX_WINDOW_GAP + 1, 1.5, Number.NaN, null, undefined, "5"]) {
      expect(isValidGap(bad, MAX_WINDOW_GAP), String(bad)).toBe(false);
    }
  });
});

describe("validation of the window settings", () => {
  it("accepts the defaults", () => {
    const problems = validate(config());
    expect(problems.keywords["window_management.keyword"]).toBeUndefined();
    expect(problems.keywords["window_management.switcher_keyword"]).toBeUndefined();
    expect(problems.windowGap).toBeUndefined();
    expect(problems.count.windows).toBe(0);
  });

  it("flags a gap out of range and counts it for the section", () => {
    for (const gap of [-1, MAX_WINDOW_GAP + 1, 2.5, Number.NaN]) {
      const c = config();
      c.window_management.gap = gap;
      const problems = validate(c);
      expect(problems.windowGap, String(gap)).toMatch(/whole number from 0 to 200/);
      expect(problems.count.windows).toBe(1);
    }
  });

  it("flags keyword clashes on both sides, with spaces and with web searches", () => {
    const c = config();
    c.window_management.keyword = "t";
    let problems = validate(c);
    expect(problems.keywords["window_management.keyword"]).toBe("Already used by automation tasks");
    expect(problems.keywords["tasks.keyword"]).toBe("Already used by window layouts");

    c.window_management.keyword = "win";
    c.window_management.switcher_keyword = "g";
    problems = validate(c);
    expect(problems.keywords["window_management.switcher_keyword"]).toBe(
      "Already a web search keyword",
    );

    c.window_management.switcher_keyword = "win";
    problems = validate(c);
    expect(problems.keywords["window_management.keyword"]).toBe("Already used by the window switcher");

    c.window_management.switcher_keyword = "two words";
    expect(validate(c).keywords["window_management.switcher_keyword"]).toBe("No spaces");

    // Empty turns a search off and never clashes.
    c.window_management.switcher_keyword = "";
    c.window_management.keyword = "";
    problems = validate(c);
    expect(problems.keywords["window_management.keyword"]).toBeUndefined();
    expect(problems.count.windows).toBe(0);
  });
});

describe("WindowsPage", () => {
  it("shows the settings and the cheat sheet with the configured keyword", () => {
    const c = config();
    c.window_management.keyword = "snap";
    show(c);
    expect(screen.getByRole("heading", { name: "Windows" })).toBeTruthy();
    expect(screen.getByRole("switch", { name: "Window management" }).getAttribute("aria-checked")).toBe(
      "true",
    );
    expect((screen.getByLabelText("Layouts keyword") as HTMLInputElement).value).toBe("snap");
    expect((screen.getByLabelText("Switcher keyword") as HTMLInputElement).value).toBe("w");
    expect((screen.getByLabelText("Gap") as HTMLInputElement).value).toBe("0");
    expect(screen.getByText("snap left")).toBeTruthy();
    expect(screen.getByText("snap next display")).toBeTruthy();
    expect(screen.getByText("snap almost-max")).toBeTruthy();
    expect(screen.getAllByRole("group", { name: /Halves|Quarters|Thirds|Size and place|Displays/ })).toHaveLength(5);
  });

  it("writes what is typed and toggled back to the config", async () => {
    // A plain object here (the settings window passes reactive state), so the
    // controls are checked through the values they write.
    const c = config();
    show(c);
    const gap = screen.getByLabelText("Gap") as HTMLInputElement;
    await fireEvent.input(gap, { target: { value: "16" } });
    expect(c.window_management.gap).toBe(16);
    await fireEvent.input(screen.getByLabelText("Layouts keyword"), { target: { value: "snap" } });
    expect(c.window_management.keyword).toBe("snap");
    await fireEvent.input(screen.getByLabelText("Switcher keyword"), { target: { value: "sw" } });
    expect(c.window_management.switcher_keyword).toBe("sw");
    await fireEvent.click(screen.getByRole("switch", { name: "Show layouts in ordinary searches" }));
    expect(c.window_management.global).toBe(true);
    await fireEvent.click(screen.getByRole("switch", { name: "Window management" }));
    expect(c.window_management.enabled).toBe(false);
  });

  it("disables the dependent controls while window management is off", () => {
    const c = config();
    c.window_management.enabled = false;
    show(c);
    expect((screen.getByLabelText("Gap") as HTMLInputElement).disabled).toBe(true);
    expect(
      (screen.getByRole("switch", { name: "Show layouts in ordinary searches" }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
  });

  it("explains each platform's requirement", () => {
    show(config(), "macos");
    expect(screen.getByText(/System Events/, { selector: "strong" })).toBeTruthy();
    cleanup();
    show(config(), "linux");
    expect(screen.getByText(/X11/, { selector: "strong" })).toBeTruthy();
    expect(screen.getByText(/Wayland does not let applications/)).toBeTruthy();
    cleanup();
    show(config(), "windows");
    expect(screen.getByText(/run as administrator/)).toBeTruthy();
  });

  it("warns when the plugin is switched off on the Plugins page", () => {
    show(config(), "windows", true);
    expect(screen.getByRole("status").textContent).toContain("Window management");
    cleanup();
    show(config(), "windows", false);
    expect(screen.queryByRole("status")).toBeNull();
  });
});
