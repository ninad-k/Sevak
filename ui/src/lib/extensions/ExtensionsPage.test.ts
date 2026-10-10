// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Catalog, CatalogItem, InstalledItem, Overview } from "./extensions-ipc";

vi.mock("./extensions-ipc", () => ({
  getOverview: vi.fn(),
  refreshCatalog: vi.fn(),
  installItem: vi.fn(),
  updateItem: vi.fn(),
  uninstallItem: vi.fn(),
  setItemEnabled: vi.fn(),
  reviewItem: vi.fn(),
  openPluginsFolder: vi.fn(),
  onChanged: vi.fn(() => Promise.resolve(() => {})),
}));

import * as ipc from "./extensions-ipc";
import ExtensionsPage from "./ExtensionsPage.svelte";

const HASH = "ab".repeat(32);

function item(over: Partial<CatalogItem> & Pick<CatalogItem, "id" | "name">): CatalogItem {
  return {
    kind: "workflow",
    description: `${over.name} does a thing.`,
    author: "Sevak",
    version: "1.0",
    tags: [],
    homepage: null,
    repository: null,
    license: null,
    permissions: [],
    min_sevak: null,
    source: "https://raw.githubusercontent.com/ninad-k/Sevak/v1/gallery/packages/x.zip",
    sha256: HASH,
    platforms: [],
    theme_mode: null,
    state: "available",
    installed_version: null,
    unavailable: null,
    removable: false,
    ...over,
  };
}

function mine(over: Partial<InstalledItem> & Pick<InstalledItem, "id" | "name">): InstalledItem {
  return {
    kind: "workflow",
    folder: over.id,
    version: "1.0",
    author: "Sevak",
    installed_at: 1,
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

function catalog(items: CatalogItem[], over: Partial<Catalog> = {}): Catalog {
  return {
    source: "https://raw.githubusercontent.com/ninad-k/Sevak/v1/gallery/index.json",
    note: null,
    themes_error: null,
    skipped: 0,
    fetched_at: Math.floor(Date.now() / 1000),
    from_cache: false,
    items,
    ...over,
  };
}

function overview(over: Partial<Overview> = {}): Overview {
  return {
    installed: [],
    catalog: null,
    platform: "linux-x86_64",
    plugins_folder: "/home/u/.config/sevak/plugins",
    ...over,
  };
}

const ok = <T,>(value: T) => ({ ok: true as const, value });

const DOCS = item({ id: "docs", name: "Docs search", description: "Search the docs." });
const NATIVE = item({
  id: "weather",
  name: "Weather now",
  kind: "native",
  author: "Ada",
  version: "0.4.2",
  license: "MIT",
  permissions: ["network", "gpu"],
  min_sevak: "0.1.0",
  platforms: ["linux-x86_64"],
  repository: "https://github.com/example/weather",
});
const NORD = item({ id: "nord", name: "Nord", kind: "theme", description: "Arctic colors." });

beforeEach(() => {
  vi.mocked(ipc.onChanged).mockReturnValue(Promise.resolve(() => {}));
});
afterEach(cleanup);

async function show(value: Overview) {
  vi.mocked(ipc.getOverview).mockResolvedValue(ok(value));
  render(ExtensionsPage);
  await screen.findByRole("tablist");
  await waitFor(() => expect(screen.queryByText("Loading…")).toBeNull());
}

describe("marketplace links", () => {
  it("offers the marketplace and the submission form without requesting anything", async () => {
    await show(overview());
    expect(screen.getByRole("button", { name: "Browse the marketplace" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Submit your extension" })).toBeTruthy();
    expect(ipc.refreshCatalog).not.toHaveBeenCalled();
  });
});

describe("before a list is loaded", () => {
  it("offers to load it, and requests nothing until asked", async () => {
    await show(overview());
    expect(screen.getByRole("button", { name: "Load the list" })).toBeTruthy();
    expect(ipc.refreshCatalog).not.toHaveBeenCalled();
    expect(ipc.installItem).not.toHaveBeenCalled();
    expect(screen.getByText(/has not been loaded on this computer yet/)).toBeTruthy();
  });

  it("loads the list when the button is pressed and shows it", async () => {
    await show(overview());
    vi.mocked(ipc.refreshCatalog).mockResolvedValue(ok(overview({ catalog: catalog([DOCS, NORD]) })));
    await fireEvent.click(screen.getByRole("button", { name: "Load the list" }));
    expect(await screen.findByText("Docs search")).toBeTruthy();
    expect(screen.getByText("Nord")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Refresh the list" })).toBeTruthy();
    expect(screen.getByText("The list is loaded: 2 items.")).toBeTruthy();
  });

  it("says the gallery is unreachable and still lets the user manage what is installed", async () => {
    await show(overview({ installed: [mine({ id: "docs", name: "Docs search" })] }));
    vi.mocked(ipc.refreshCatalog).mockResolvedValue({ ok: false, error: "could not download it: offline" });
    await fireEvent.click(screen.getByRole("button", { name: "Load the list" }));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("Could not reach the gallery");
    expect(alert.textContent).toContain("offline");
    expect(alert.textContent).toContain("what is installed");
    await fireEvent.click(screen.getByRole("tab", { name: /Installed/ }));
    expect(screen.getByText("Docs search")).toBeTruthy();
  });

  it("keeps showing the saved list when a refresh fails, and says how old it is", async () => {
    const saved = catalog([DOCS], { from_cache: true, fetched_at: Math.floor(Date.now() / 1000) - 3 * 86400 });
    await show(overview({ catalog: saved }));
    expect(screen.getByText("Docs search")).toBeTruthy();
    expect(screen.getByText(/saved 3 days ago/)).toBeTruthy();
    vi.mocked(ipc.refreshCatalog).mockResolvedValue({ ok: false, error: "offline" });
    await fireEvent.click(screen.getByRole("button", { name: "Refresh the list" }));
    expect((await screen.findByRole("alert")).textContent).toContain("Showing the list saved 3 days ago");
    expect(screen.getByText("Docs search")).toBeTruthy();
  });
});

describe("browsing", () => {
  const loaded = () => overview({ catalog: catalog([DOCS, NATIVE, NORD]) });

  it("searches by name, author and description, and filters by kind", async () => {
    await show(loaded());
    const search = screen.getByRole("searchbox", { name: "Search extensions" });
    await fireEvent.input(search, { target: { value: "arctic" } });
    expect(screen.getByText("Nord")).toBeTruthy();
    expect(screen.queryByText("Docs search")).toBeNull();
    await fireEvent.input(search, { target: { value: "" } });
    await fireEvent.input(search, { target: { value: "ada" } });
    expect(screen.getByText("Weather now")).toBeTruthy();
    expect(screen.queryByText("Nord")).toBeNull();
    await fireEvent.input(search, { target: { value: "zzz" } });
    expect(screen.getByText("Nothing matches.")).toBeTruthy();
    await fireEvent.input(search, { target: { value: "" } });
    await fireEvent.change(screen.getByLabelText("Show"), { target: { value: "theme" } });
    expect(screen.getByText("Nord")).toBeTruthy();
    expect(screen.queryByText("Docs search")).toBeNull();
  });

  it("Escape clears the search box", async () => {
    await show(loaded());
    const search = screen.getByRole("searchbox", { name: "Search extensions" }) as HTMLInputElement;
    await fireEvent.input(search, { target: { value: "nord" } });
    await fireEvent.keyDown(search, { key: "Escape" });
    expect(search.value).toBe("");
  });

  it("installs a workflow with one press", async () => {
    await show(loaded());
    vi.mocked(ipc.installItem).mockResolvedValue(
      ok(overview({ catalog: catalog([{ ...DOCS, state: "installed", installed_version: "1.0" }, NATIVE, NORD]) })),
    );
    await fireEvent.click(screen.getByRole("button", { name: "Install Docs search" }));
    await waitFor(() => expect(ipc.installItem).toHaveBeenCalledWith("docs"));
    expect((await screen.findByRole("status", {}, { timeout: 2000 })).textContent).toContain(
      "Installed Docs search",
    );
    expect(screen.getByRole("button", { name: "Docs search: Installed" }).hasAttribute("disabled")).toBe(true);
  });

  it("shows an install failure and changes nothing", async () => {
    await show(loaded());
    vi.mocked(ipc.installItem).mockResolvedValue({
      ok: false,
      error: "The download does not match the checksum in the gallery index, so it was discarded.",
    });
    await fireEvent.click(screen.getByRole("button", { name: "Install Docs search" }));
    expect((await screen.findByRole("alert")).textContent).toContain("checksum");
    expect(screen.getByRole("button", { name: "Install Docs search" })).toBeTruthy();
  });

  it("makes a native extension a deliberate two-step: review the details, then install", async () => {
    await show(loaded());
    // No one-press install for a native extension.
    expect(screen.queryByRole("button", { name: "Install Weather now" })).toBeNull();
    await fireEvent.click(screen.getByRole("button", { name: "Review Weather now before installing" }));
    const detail = document.getElementById("ext-detail-weather") as HTMLElement;
    expect(detail).toBeTruthy();
    const text = detail.textContent ?? "";
    expect(text).toContain("Ada");
    expect(text).toContain("MIT");
    expect(text).toContain("network: connects to the internet");
    expect(text).toContain("gpu: (the author's own label)");
    expect(text).toContain("Sevak does not enforce it");
    expect(text).toContain(HASH);
    expect(text).toContain("compiled program");
    expect(text).toContain("does not sandbox");
    expect(text).toContain("https://github.com/example/weather");
    expect(ipc.installItem).not.toHaveBeenCalled();

    vi.mocked(ipc.installItem).mockResolvedValue(ok(overview({ catalog: catalog([DOCS, NATIVE, NORD]) })));
    await fireEvent.click(within(detail).getByRole("button", { name: "Install native extension" }));
    await waitFor(() => expect(ipc.installItem).toHaveBeenCalledWith("weather"));
  });

  it("labels a native extension and shows what it declares in the row", async () => {
    await show(loaded());
    const row = document.getElementById("ext-head-weather") as HTMLElement;
    expect(row.textContent).toContain("Native extension");
    expect(row.textContent).toContain("declares: network, gpu");
  });

  it("explains why something is not available and offers no install", async () => {
    const future = item({
      id: "future",
      name: "Future tool",
      kind: "native",
      state: "unavailable",
      unavailable: "Needs Sevak 9.0.0 or newer; this is Sevak 0.1.0.",
    });
    await show(overview({ catalog: catalog([future]) }));
    expect(screen.getByText(/Needs Sevak 9.0.0 or newer/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Future tool: Not available" }).hasAttribute("disabled")).toBe(true);
  });

  it("opens and closes details from the keyboard", async () => {
    await show(loaded());
    const head = document.getElementById("ext-head-docs") as HTMLElement;
    expect(head.getAttribute("aria-expanded")).toBe("false");
    await fireEvent.click(head);
    expect(head.getAttribute("aria-expanded")).toBe("true");
    const inside = within(document.getElementById("ext-detail-docs") as HTMLElement).getByRole("button");
    inside.focus();
    await fireEvent.keyDown(inside, { key: "Escape" });
    expect(head.getAttribute("aria-expanded")).toBe("false");
    expect(document.activeElement).toBe(head);
  });
});

describe("what is installed", () => {
  const withInstalled = () =>
    overview({
      catalog: catalog([DOCS]),
      installed: [
        mine({ id: "docs", name: "Docs search", update_to: "1.2", keywords: ["docs"] }),
        mine({ id: "weather", name: "Weather now", kind: "native", status: "waiting", can_review: true, program_sha256: HASH }),
        mine({ id: "old", name: "Broken one", status: "broken", problem: "plugin.toml is not valid" }),
        mine({ id: "nord", name: "Nord", kind: "theme", status: "theme" }),
      ],
    });

  it("lists installs with their status, update indicator and native label", async () => {
    await show(withInstalled());
    const tabText = (screen.getByRole("tab", { name: /Installed/ }).textContent ?? "").replace(/\s+/g, " ");
    expect(tabText).toContain("Installed (4)");
    expect(tabText).toContain("1 update");
    await fireEvent.click(screen.getByRole("tab", { name: /Installed/ }));
    expect(screen.getByText("Update to v1.2")).toBeTruthy();
    expect(screen.getByText("Waiting for your permission")).toBeTruthy();
    expect(screen.getByText("plugin.toml is not valid")).toBeTruthy();
    expect(screen.getByText("Native extension")).toBeTruthy();
    expect(screen.getByText(HASH)).toBeTruthy();
    // A broken item and a theme have no switch.
    expect(screen.queryByRole("switch", { name: "Enable Broken one" })).toBeNull();
    expect(screen.queryByRole("switch", { name: "Enable Nord" })).toBeNull();
    expect(screen.getByRole("switch", { name: "Enable Docs search" })).toBeTruthy();
  });

  async function installedTab() {
    await show(withInstalled());
    await fireEvent.click(screen.getByRole("tab", { name: /Installed/ }));
  }

  it("updates, reviews, switches and uninstalls through the backend", async () => {
    await installedTab();
    vi.mocked(ipc.updateItem).mockResolvedValue(ok(withInstalled()));
    await fireEvent.click(screen.getByRole("button", { name: "Update Docs search to 1.2" }));
    await waitFor(() => expect(ipc.updateItem).toHaveBeenCalledWith("docs"));

    vi.mocked(ipc.reviewItem).mockResolvedValue(ok(true));
    await fireEvent.click(screen.getByRole("button", { name: "Review Weather now" }));
    await waitFor(() => expect(ipc.reviewItem).toHaveBeenCalledWith("weather"));

    vi.mocked(ipc.setItemEnabled).mockResolvedValue(ok(withInstalled()));
    await fireEvent.click(screen.getByRole("switch", { name: "Enable Docs search" }));
    await waitFor(() => expect(ipc.setItemEnabled).toHaveBeenCalledWith("docs", false));

    vi.mocked(ipc.uninstallItem).mockResolvedValue(
      ok(overview({ installed: withInstalled().installed.filter((i) => i.id !== "docs") })),
    );
    await fireEvent.click(screen.getByRole("button", { name: "Uninstall Docs search" }));
    await waitFor(() => expect(ipc.uninstallItem).toHaveBeenCalledWith("docs"));
    await waitFor(() => expect(screen.queryByText("Docs search")).toBeNull());
  });

  it("keeps the item when the confirmation is declined", async () => {
    await installedTab();
    vi.mocked(ipc.uninstallItem).mockResolvedValue(ok(null));
    await fireEvent.click(screen.getByRole("button", { name: "Uninstall Docs search" }));
    await waitFor(() => expect(ipc.uninstallItem).toHaveBeenCalled());
    expect(screen.getByText("Docs search")).toBeTruthy();
    expect(screen.queryByText(/^Removed/)).toBeNull();
  });

  it("shows an empty state", async () => {
    await show(overview());
    await fireEvent.click(screen.getByRole("tab", { name: /Installed/ }));
    expect(screen.getByText(/Nothing installed from the gallery yet/)).toBeTruthy();
  });

  it("arrow keys move between the tabs", async () => {
    await show(overview());
    const discover = screen.getByRole("tab", { name: "Discover" });
    expect(discover.getAttribute("aria-selected")).toBe("true");
    await fireEvent.keyDown(discover, { key: "ArrowRight" });
    expect(screen.getByRole("tab", { name: /Installed/ }).getAttribute("aria-selected")).toBe("true");
    await fireEvent.keyDown(screen.getByRole("tab", { name: /Installed/ }), { key: "ArrowLeft" });
    expect(screen.getByRole("tab", { name: "Discover" }).getAttribute("aria-selected")).toBe("true");
  });

  it("reloads when something changed from outside the page", async () => {
    let notify: () => void = () => {};
    vi.mocked(ipc.onChanged).mockImplementation((handler) => {
      notify = handler;
      return Promise.resolve(() => {});
    });
    await show(overview());
    expect(ipc.getOverview).toHaveBeenCalledTimes(1);
    vi.mocked(ipc.getOverview).mockResolvedValue(ok(overview({ installed: [mine({ id: "z", name: "Zed" })] })));
    notify();
    await waitFor(() => expect(ipc.getOverview).toHaveBeenCalledTimes(2));
    await fireEvent.click(screen.getByRole("tab", { name: /Installed/ }));
    expect(await screen.findByText("Zed")).toBeTruthy();
  });
});
