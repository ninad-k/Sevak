// @vitest-environment jsdom
import { invoke } from "@tauri-apps/api/core";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import Settings from "./Settings.svelte";
import { mockSettings } from "./lib/mock";
import type { Config, SettingsDto } from "./lib/settings-ipc";

// Exercise the complete Settings form and its real IPC wrappers. Only the
// native bridge is replaced, so these tests never change the host's startup.
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

const startupName = "Start Sevak when I sign in";
let persisted: SettingsDto;
let saveFailure: string | null;
let holdSave: Promise<void> | null;

beforeEach(() => {
  vi.stubGlobal("__TAURI_INTERNALS__", {});
  persisted = structuredClone(mockSettings());
  saveFailure = null;
  holdSave = null;
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    switch (command) {
      case "get_settings":
        return structuredClone(persisted);
      case "save_settings": {
        if (holdSave) await holdSave;
        if (saveFailure) throw new Error(saveFailure);
        const config = (args as { config: Config }).config;
        const changed = config.general.launch_at_login !== persisted.config.general.launch_at_login;
        persisted.config = structuredClone(config);
        const enabled = config.general.launch_at_login;
        if (changed) persisted.startup = { registered: enabled, enabled, error: null };
        return null;
      }
      case "get_status":
      case "validate_hotkey":
      case "extensions_take_page":
      case "close_settings":
        return null;
      default:
        throw new Error(`Unexpected Settings IPC: ${command}`);
    }
  });
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const saveButton = () => screen.getByRole<HTMLButtonElement>("button", { name: "Save" });
const startupSwitch = () => screen.getByRole<HTMLButtonElement>("switch", { name: startupName });
const saveCalls = () => vi.mocked(invoke).mock.calls.filter(([command]) => command === "save_settings");

async function show() {
  const view = render(Settings);
  await screen.findByRole("switch", { name: startupName });
  return view;
}

describe("start-at-sign-in settings integration", () => {
  it.each([
    ["windows", "windows", /Windows Settings → Apps → Startup/],
    ["macos", "macos", /System Settings → General → Login Items/],
    ["linux", "x11", /desktop's autostart settings/],
  ] as const)("saves, reloads and disables the preference on %s", async (platform, display, hint) => {
    persisted.platform = platform;
    persisted.display = display;
    const view = await show();
    expect(screen.getByText(hint)).toBeTruthy();
    expect(screen.getByText(/Click Save to apply/)).toBeTruthy();
    expect(startupSwitch().getAttribute("aria-checked")).toBe("false");
    expect(saveButton().disabled).toBe(true);

    await fireEvent.click(startupSwitch());
    expect(startupSwitch().getAttribute("aria-checked")).toBe("true");
    expect(saveCalls()).toHaveLength(0);
    expect(persisted.config.general.launch_at_login).toBe(false);

    await fireEvent.click(saveButton());
    await waitFor(() => expect(saveButton().disabled).toBe(true));
    expect(saveCalls()).toHaveLength(1);
    expect(persisted.config.general.launch_at_login).toBe(true);
    expect(vi.mocked(invoke).mock.calls.filter(([command]) => command === "get_settings")).toHaveLength(2);

    view.unmount();
    await show();
    expect(startupSwitch().getAttribute("aria-checked")).toBe("true");
    await fireEvent.click(startupSwitch());
    await fireEvent.click(saveButton());
    await waitFor(() => expect(saveButton().disabled).toBe(true));
    expect(persisted.config.general.launch_at_login).toBe(false);
    expect(startupSwitch().getAttribute("aria-checked")).toBe("false");
    expect(saveCalls()).toHaveLength(2);
  });

  it("cancels a draft without changing the startup registration", async () => {
    await show();
    await fireEvent.click(startupSwitch());
    await fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(invoke).toHaveBeenCalledWith("close_settings");
    expect(saveCalls()).toHaveLength(0);
    expect(persisted.startup.enabled).toBe(false);
  });

  it.each([false, true])("keeps the previous preference after an OS failure (previously %s)", async (initial) => {
    persisted.config.general.launch_at_login = initial;
    persisted.startup = { registered: initial, enabled: initial, error: null };
    saveFailure = "Could not update startup: access denied by your operating system";
    vi.spyOn(console, "warn").mockImplementation(() => {});
    await show();
    await fireEvent.click(startupSwitch());
    await fireEvent.click(saveButton());
    expect((await screen.findByRole("alert")).textContent).toContain(saveFailure);
    expect(screen.queryByText("Saved")).toBeNull();
    expect(persisted.config.general.launch_at_login).toBe(initial);
    expect(persisted.startup.enabled).toBe(initial);
    expect(saveButton().disabled).toBe(false);
    expect(startupSwitch().getAttribute("aria-checked")).toBe(String(!initial));

    saveFailure = null;
    await fireEvent.click(saveButton());
    await waitFor(() => expect(saveButton().disabled).toBe(true));
    expect(persisted.startup.enabled).toBe(!initial);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("waits for the startup operation before reporting success", async () => {
    let finish!: () => void;
    holdSave = new Promise<void>((resolve) => { finish = resolve; });
    await show();
    await fireEvent.click(startupSwitch());
    await fireEvent.click(saveButton());
    expect(screen.getByRole<HTMLButtonElement>("button", { name: "Saving…" }).disabled).toBe(true);
    expect(startupSwitch().disabled).toBe(true);
    expect(screen.queryByText("Saved")).toBeNull();
    expect(persisted.startup.enabled).toBe(false);
    finish();
    await screen.findByText("Saved");
    expect(startupSwitch().disabled).toBe(false);
    expect(persisted.startup.enabled).toBe(true);
  });

  it.each([true, false])("warns when the saved opt-in is not active (registered %s)", async (registered) => {
    persisted.config.general.launch_at_login = true;
    persisted.startup = { registered, enabled: false, error: null };
    await show();
    expect(startupSwitch().getAttribute("aria-checked")).toBe("true");
    expect(screen.getByText(registered ? /Startup is disabled outside Sevak/ : /startup entry is missing/)).toBeTruthy();
    expect(screen.getByText(/off and Save, then on and Save again/)).toBeTruthy();
    expect(saveCalls()).toHaveLength(0);
  });

  it("shows a status read error without pretending startup is active", async () => {
    persisted.startup = { registered: false, enabled: false, error: "Cannot read startup permissions" };
    await show();
    expect(screen.getByRole("alert").textContent).toContain("Could not check startup: Cannot read startup permissions");
    expect(screen.queryByText(/startup entry is missing/)).toBeNull();
  });

  it("keeps showing an external disable after saving an unrelated setting", async () => {
    persisted.config.general.launch_at_login = true;
    persisted.startup = { registered: true, enabled: false, error: null };
    await show();
    await fireEvent.click(screen.getByRole("switch", { name: "Hide when focus is lost" }));
    await fireEvent.click(saveButton());
    await waitFor(() => expect(saveButton().disabled).toBe(true));
    const sent = saveCalls()[0][1] as { config: Config };
    expect(sent.config.general.launch_at_login).toBe(true);
    expect(startupSwitch().getAttribute("aria-checked")).toBe("true");
    expect(screen.getByText(/Startup is disabled outside Sevak/)).toBeTruthy();
    expect(persisted.startup.enabled).toBe(false);
  });
});
