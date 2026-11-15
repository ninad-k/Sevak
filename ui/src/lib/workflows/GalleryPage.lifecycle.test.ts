// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Gallery, GalleryEntry } from "./ipc";
vi.mock("./ipc", () => ({ loadGallery: vi.fn(), installGalleryEntry: vi.fn() }));
import * as ipc from "./ipc";
import GalleryPage from "./GalleryPage.svelte";

const entry = (over: Partial<GalleryEntry> = {}): GalleryEntry => ({ id: "docs", kind: "workflow", name: "Docs", description: "Docs search", version: "1.0", author: "Sevak", source: "https://raw.githubusercontent.com/ninad-k/Sevak/v1/gallery/packages/docs.zip", sha256: "ab".repeat(32), installed: false, ...over });
const gallery = (entries: GalleryEntry[]): Gallery => ({ name: "Gallery", source: "https://raw.githubusercontent.com/ninad-k/Sevak/v1/gallery/index.json", note: null, entries, skipped: 0 });
beforeEach(() => vi.clearAllMocks());
afterEach(cleanup);
async function load(value: Gallery) {
  vi.mocked(ipc.loadGallery).mockResolvedValue({ ok: true, value });
  render(GalleryPage);
  expect(ipc.loadGallery).not.toHaveBeenCalled();
  await fireEvent.click(screen.getByRole("button", { name: "Load gallery" }));
  await screen.findByRole("heading", { name: "Docs" });
}
it("shows validated deprecation and replacement and blocks a fresh install", async () => {
  await load(gallery([entry({ deprecated: "Unmaintained.", replaced_by: "new-docs" })]));
  expect(screen.getByText(/Deprecated: Unmaintained.*Replacement: new-docs/)).toBeTruthy();
  expect(screen.getByRole("button", { name: "Install Docs" }).hasAttribute("disabled")).toBe(true);
  expect(ipc.installGalleryEntry).not.toHaveBeenCalled();
});
it("names a listed replacement instead of showing its id", async () => {
  await load(gallery([entry({ deprecated: "Unmaintained.", replaced_by: "new-docs" }), entry({ id: "new-docs", name: "New docs" })]));
  expect(screen.getByText(/Deprecated: Unmaintained\. Replacement: New docs\./)).toBeTruthy();
  expect(screen.queryByText(/Replacement: new-docs/)).toBeNull();
});
it("warns about an unrecorded copy when another version than the listed one is revoked", async () => {
  const installed_revocation = { id: "docs", versions: ["1.0"], reason: "Old version unsafe.", advisory_url: null };
  await load(gallery([entry({ version: "1.1", installed: true, installed_version: null, installed_revocation, installed_path: "/home/u/.config/sevak/workflows/docs" })]));
  const alert = screen.getByRole("alert").textContent ?? "";
  expect(alert).toContain("installed version is unknown");
  expect(alert).toContain("/home/u/.config/sevak/workflows/docs");
  expect(alert).toContain("version 1.0: Old version unsafe.");
  expect(alert).not.toContain("Remove this copy from Extensions");
  expect(screen.queryByText(/Listed version revoked/)).toBeNull();
});
it("shows revocation for installed and not-installed versions", async () => {
  const revocation = { id: "docs", versions: ["1.0"], reason: "Unsafe test version.", advisory_url: null };
  await load(gallery([entry({ revocation })]));
  expect(screen.getByRole("alert").textContent).toContain("cannot be installed");
  expect(screen.getByRole("button", { name: "Install Docs" }).hasAttribute("disabled")).toBe(true);
  vi.mocked(ipc.loadGallery).mockResolvedValue({ ok: true, value: gallery([entry({ revocation, installed: true })]) });
  await fireEvent.click(screen.getByRole("button", { name: "Reload" }));
  await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("installed version is unknown"));
});
it("uses the managed installed version's notice even when the listed update is unaffected", async () => {
  await load(gallery([entry({ version: "1.1", installed: true, installed_version: "1.0", installed_revocation: { id: "docs", versions: ["1.0"], reason: "Old version unsafe.", advisory_url: null } })]));
  expect(screen.getByRole("alert").textContent).toContain("Installed version revoked");
  expect(screen.getByRole("alert").textContent).toContain("Remove this copy from Extensions");
  expect(screen.queryByText(/Listed version revoked/)).toBeNull();
});
it("an unknown check never claims a clean result and refresh failure keeps notices", async () => {
  await load(gallery([entry()]));
  expect(screen.getByText(/Current revocation status is unknown/)).toBeTruthy();
  vi.mocked(ipc.loadGallery).mockResolvedValue({ ok: false, error: "Offline" });
  await fireEvent.click(screen.getByRole("button", { name: "Reload" }));
  expect(await screen.findByRole("alert")).toHaveProperty("textContent", "Offline");
  expect(screen.getByText(/refresh failed; current revocation status is unknown/)).toBeTruthy();
});
it("shows newly authenticated notices even when the catalog reload failed", async () => {
  await load(gallery([entry()]));
  const value = gallery([entry({ revocation: { id: "docs", versions: ["1.0"], reason: "New security notice.", advisory_url: null } })]);
  value.load_error = "Catalog offline. Showing the previously loaded list.";
  vi.mocked(ipc.loadGallery).mockResolvedValue({ ok: true, value });
  await fireEvent.click(screen.getByRole("button", { name: "Reload" }));
  expect(await screen.findByText(/Catalog offline/)).toBeTruthy();
  expect(await screen.findByText(/New security notice/)).toBeTruthy();
  expect(screen.getByRole("button", { name: "Install Docs" }).hasAttribute("disabled")).toBe(true);
});
