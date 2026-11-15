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
let repairFailure: string | null;
let holdRepair: Promise<void> | null;
let statusFailure: string | null;
/** The preference on disk when the installer or CLI changed it behind Settings. */
let diskSaved: boolean | null;
/** Every OS startup write a save made, as the enabled state it asked for. */
let osWrites: boolean[];

beforeEach(() => {
  vi.stubGlobal("__TAURI_INTERNALS__", {});
  persisted = structuredClone(mockSettings());
  saveFailure = null;
  holdSave = null;
  repairFailure = null;
  holdRepair = null;
  statusFailure = null;
  diskSaved = null;
  osWrites = [];
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    switch (command) {
      case "get_settings":
        return structuredClone(persisted);
      case "get_startup_status":
        if (statusFailure) throw new Error(statusFailure);
        return { ...structuredClone(persisted.startup), saved: diskSaved ?? persisted.config.general.launch_at_login };
      case "repair_startup":
        if (holdRepair) await holdRepair;
        if (repairFailure) throw new Error(repairFailure);
        persisted.startup = { registered: true, enabled: true, other_copy: false, error: null };
        return structuredClone(persisted.startup);
      case "save_settings": {
        if (holdSave) await holdSave;
        if (saveFailure) throw new Error(saveFailure);
        // Mirrors `settings::save`: `startup::settings_preference` compares
        // the draft with the running config and what is on disk, and
        // `startup::save_settings` touches the OS only when the preference
        // changes (and then explicitly, re-enabling a disabled entry).
        const config = structuredClone((args as { config: Config }).config);
        const running = persisted.config.general.launch_at_login;
        const saved = diskSaved ?? running;
        const draft = config.general.launch_at_login;
        const wanted = draft === running ? saved : draft;
        config.general.launch_at_login = wanted;
        if (saved !== wanted) {
          osWrites.push(wanted);
          persisted.startup = { registered: wanted, enabled: wanted, other_copy: false, error: null };
        }
        persisted.config = config;
        diskSaved = null;
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
const repairCalls = () => vi.mocked(invoke).mock.calls.filter(([command]) => command === "repair_startup");

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
    expect(screen.getByText(registered ? /Startup status: Disabled outside Sevak/ : /Startup status: Entry missing/)).toBeTruthy();
    expect(screen.getByRole("button", { name: registered ? "Enable again" : "Repair startup" })).toBeTruthy();
    expect(saveCalls()).toHaveLength(0);
    expect(repairCalls()).toHaveLength(0);
  });

  it("shows a status read error without pretending startup is active", async () => {
    persisted.startup = { registered: false, enabled: false, error: "Cannot read startup permissions" };
    await show();
    expect(screen.getByRole("alert").textContent).toContain("Could not check startup: Cannot read startup permissions");
    expect(screen.getByText("Startup status: Unavailable")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Repair startup" })).toBeNull();
  });

  it.each([true, false])("preserves an inactive OS entry on unrelated saves (registered %s)", async (registered) => {
    persisted.config.general.launch_at_login = true;
    persisted.startup = { registered, enabled: false, error: null };
    await show();
    await fireEvent.click(screen.getByRole("switch", { name: "Hide when focus is lost" }));
    await fireEvent.click(saveButton());
    await waitFor(() => expect(saveButton().disabled).toBe(true));
    const sent = saveCalls()[0][1] as { config: Config };
    expect(sent.config.general.launch_at_login).toBe(true);
    // The save left the inactive entry alone: no OS write (which would
    // re-enable a disabled entry or create a missing one) and no repair.
    expect(osWrites).toEqual([]);
    expect(persisted.startup).toEqual({ registered, enabled: false, error: null });
    expect(repairCalls()).toHaveLength(0);
    // The form was reloaded from the backend and still reports the inactive entry.
    expect(vi.mocked(invoke).mock.calls.filter(([command]) => command === "get_settings")).toHaveLength(2);
    expect(startupSwitch().getAttribute("aria-checked")).toBe("true");
    expect(screen.getByText(registered ? /Startup status: Disabled outside Sevak/ : /Startup status: Entry missing/)).toBeTruthy();
    expect(screen.getByRole("button", { name: registered ? "Enable again" : "Repair startup" })).toBeTruthy();
  });

  it.each([true, false])("explicitly repairs an inactive saved opt-in in one click (registered %s)", async (registered) => {
    persisted.config.general.launch_at_login = true;
    persisted.startup = { registered, enabled: false, error: null };
    await show();
    await fireEvent.click(screen.getByRole("button", { name: registered ? "Enable again" : "Repair startup" }));
    await screen.findByText("Startup status: Enabled for your account");
    expect(screen.getByText(/Startup enabled\. Sevak will start/)).toBeTruthy();
    expect(repairCalls()).toEqual([["repair_startup"]]);
    expect(saveCalls()).toHaveLength(0);
    expect(persisted.config.general.launch_at_login).toBe(true);
    expect(screen.queryByText("Saved")).toBeNull();
  });

  it("keeps unrelated unsaved edits through repair and a later status check", async () => {
    persisted.config.general.launch_at_login = true;
    persisted.startup = { registered: false, enabled: false, error: null };
    await show();
    const hide = () => screen.getByRole("switch", { name: "Hide when focus is lost" });
    const original = persisted.config.general.hide_on_blur;
    await fireEvent.click(hide());
    await fireEvent.click(screen.getByRole("button", { name: "Repair startup" }));
    await screen.findByText("Startup status: Enabled for your account");
    await fireEvent.click(screen.getByRole("button", { name: "Check status" }));
    await waitFor(() => expect(saveButton().disabled).toBe(false));
    expect(hide().getAttribute("aria-checked")).toBe(String(!original));
    expect(persisted.config.general.hide_on_blur).toBe(original);
    expect(saveCalls()).toHaveLength(0);
    await fireEvent.click(saveButton());
    await screen.findByText("Saved");
    expect(persisted.config.general.hide_on_blur).toBe(!original);
  });

  it("blocks repair while the startup preference itself has an unsaved change", async () => {
    persisted.config.general.launch_at_login = true;
    persisted.startup = { registered: true, enabled: false, error: null };
    await show();
    await fireEvent.click(startupSwitch());
    const button = screen.getByRole<HTMLButtonElement>("button", { name: "Enable again" });
    expect(button.disabled).toBe(true);
    expect(screen.getByText(/Save or undo your startup preference change/)).toBeTruthy();
    await fireEvent.click(button);
    expect(repairCalls()).toHaveLength(0);
    await fireEvent.click(startupSwitch());
    expect(button.disabled).toBe(false);
  });

  it("coordinates repair with Save, Close and duplicate clicks while preserving later draft edits", async () => {
    persisted.config.general.launch_at_login = true;
    persisted.startup = { registered: true, enabled: false, error: null };
    let finish!: () => void;
    holdRepair = new Promise<void>((resolve) => { finish = resolve; });
    await show();
    await fireEvent.click(screen.getByRole("button", { name: "Enable again" }));
    const busy = screen.getByRole<HTMLButtonElement>("button", { name: "Enabling…" });
    expect(busy.disabled).toBe(true);
    expect(startupSwitch().disabled).toBe(true);
    expect(screen.getByRole<HTMLButtonElement>("button", { name: "Close" }).disabled).toBe(true);
    await fireEvent.click(busy);
    await fireEvent.click(screen.getByRole("switch", { name: "Hide when focus is lost" }));
    expect(saveButton().disabled).toBe(true);
    await fireEvent.keyDown(window, { key: "s", ctrlKey: true });
    expect(saveCalls()).toHaveLength(0);
    expect(repairCalls()).toHaveLength(1);
    expect(persisted.startup.enabled).toBe(false);
    finish();
    await screen.findByText("Startup status: Enabled for your account");
    expect(saveButton().disabled).toBe(false);
    expect(startupSwitch().disabled).toBe(false);
  });

  it("shows a repair error without reporting success and allows an explicit retry", async () => {
    persisted.config.general.launch_at_login = true;
    persisted.startup = { registered: false, enabled: false, error: "Read permission denied" };
    repairFailure = "Startup registration is blocked by policy";
    await show();
    await fireEvent.click(screen.getByRole("button", { name: "Repair startup" }));
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain(repairFailure));
    expect(screen.getByText("Startup status: Unavailable")).toBeTruthy();
    expect(persisted.startup.enabled).toBe(false);
    expect(screen.queryByText(/Startup enabled\./)).toBeNull();
    repairFailure = null;
    await fireEvent.click(screen.getByRole("button", { name: "Repair startup" }));
    await screen.findByText("Startup status: Enabled for your account");
    expect(screen.queryByRole("alert")).toBeNull();
    expect(saveCalls()).toHaveLength(0);
  });

  it("refreshes OS status without writes and recovers from a read error", async () => {
    persisted.config.general.launch_at_login = true;
    persisted.startup = { registered: true, enabled: true, error: null };
    await show();
    expect(screen.getByText("Startup status: Enabled for your account")).toBeTruthy();
    statusFailure = "OS status unavailable";
    await fireEvent.click(screen.getByRole("button", { name: "Check status" }));
    await screen.findByText("Startup status: Unavailable");
    statusFailure = null;
    persisted.startup = { registered: true, enabled: false, error: null };
    await fireEvent.click(screen.getByRole("button", { name: "Check status" }));
    await screen.findByText("Startup status: Disabled outside Sevak");
    expect(repairCalls()).toHaveLength(0);
    expect(saveCalls()).toHaveLength(0);
  });

  it("follows a preference turned off outside Settings after a refused repair, keeping other edits", async () => {
    persisted.config.general.launch_at_login = true;
    persisted.startup = { registered: false, enabled: false, error: null };
    await show();
    const hide = () => screen.getByRole("switch", { name: "Hide when focus is lost" });
    const original = persisted.config.general.hide_on_blur;
    await fireEvent.click(hide());
    // `--set-startup off` ran while this window was open.
    diskSaved = false;
    repairFailure = "Turn on Start Sevak when I sign in and save it before repairing startup.";
    await fireEvent.click(screen.getByRole("button", { name: "Repair startup" }));
    await waitFor(() => expect(startupSwitch().getAttribute("aria-checked")).toBe("false"));
    expect(screen.getByText(/changed outside Settings and is now off/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Repair startup" })).toBeNull();
    expect(hide().getAttribute("aria-checked")).toBe(String(!original));
    // Saving the other edit must not write the stale "on" back.
    await fireEvent.click(saveButton());
    await screen.findByText("Saved");
    const sent = (saveCalls().at(-1)![1] as { config: Config }).config;
    expect(sent.general.launch_at_login).toBe(false);
    expect(sent.general.hide_on_blur).toBe(!original);
  });

  it("re-reads the OS status after a failed repair", async () => {
    persisted.config.general.launch_at_login = true;
    persisted.startup = { registered: true, enabled: false, error: null };
    repairFailure = "Restoring the previous startup entry also failed";
    await show();
    persisted.startup = { registered: false, enabled: false, error: null };
    await fireEvent.click(screen.getByRole("button", { name: "Enable again" }));
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain(repairFailure));
    expect(screen.getByText("Startup status: Entry missing")).toBeTruthy();
    expect(saveCalls()).toHaveLength(0);
  });

  it("locks the form while saving so an edit cannot be lost when the saved settings reload", async () => {
    let finish!: () => void;
    holdSave = new Promise<void>((resolve) => { finish = resolve; });
    await show();
    const hide = () => screen.getByRole<HTMLButtonElement>("switch", { name: "Hide when focus is lost" });
    const panel = screen.getByRole("tabpanel");
    const original = persisted.config.general.hide_on_blur;
    await fireEvent.click(hide());
    expect(panel.getAttribute("aria-busy")).toBe("false");
    expect(hide().matches(":disabled")).toBe(false);
    await fireEvent.click(saveButton());
    // Every control in the page is disabled and the page is marked busy, so
    // nothing typed now would be silently replaced by the reloaded settings.
    expect(panel.getAttribute("aria-busy")).toBe("true");
    expect(hide().matches(":disabled")).toBe(true);
    expect(screen.getByLabelText("Shortcut").matches(":disabled")).toBe(true);
    expect(screen.getByRole("tab", { name: "General" }).matches(":disabled")).toBe(false);
    finish();
    await screen.findByText("Saved");
    expect(panel.getAttribute("aria-busy")).toBe("false");
    expect(hide().matches(":disabled")).toBe(false);
    expect(hide().getAttribute("aria-checked")).toBe(String(!original));
    expect(persisted.config.general.hide_on_blur).toBe(!original);
  });

  it("labels an active entry that the saved preference does not ask for", async () => {
    persisted.config.general.launch_at_login = false;
    persisted.startup = { registered: true, enabled: true, error: null };
    await show();
    expect(screen.getByText("Startup status: Enabled outside Sevak's saved setting")).toBeTruthy();
    expect(screen.queryByText("Startup status: Enabled for your account")).toBeNull();
    expect(screen.queryByRole("button", { name: /Enable again|Repair startup/ })).toBeNull();
  });

  it("offers repair when the entry starts another copy of Sevak", async () => {
    persisted.config.general.launch_at_login = true;
    persisted.startup = { registered: true, enabled: true, other_copy: true, error: null };
    await show();
    expect(screen.getByText("Startup status: Registered for another copy of Sevak")).toBeTruthy();
    expect(screen.getByText(/Repair startup registers this installed copy again/)).toBeTruthy();
    await fireEvent.click(screen.getByRole("button", { name: "Repair startup" }));
    await screen.findByText("Startup status: Enabled for your account");
    expect(repairCalls()).toHaveLength(1);
    expect(saveCalls()).toHaveLength(0);
  });
});
