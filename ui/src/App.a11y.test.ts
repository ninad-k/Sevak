// @vitest-environment jsdom
// The launcher as a screen reader meets it: the search box is an ARIA combobox
// that drives the result listbox (or the action panel) through
// aria-activedescendant, a polite live region says how many results a search
// found, and axe finds no serious or critical problem. No Tauri: the searches
// come from the browser-preview fixtures in lib/mock.ts.
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App.svelte";
import { describe as explain, seriousViolations, stubLayout } from "./test/a11y";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => null) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

beforeEach(stubLayout);
afterEach(cleanup);

const combobox = () => screen.getByRole("combobox", { name: "Search" });
const liveRegion = () => document.querySelector<HTMLElement>('[role="status"][aria-live="polite"]')!;
const key = (k: string, init: KeyboardEventInit = {}) => fireEvent.keyDown(window, { key: k, ...init });

async function type(text: string) {
  const box = combobox() as HTMLInputElement;
  box.value = text;
  await fireEvent.input(box);
}

/** The element the search box's aria-activedescendant names. */
function active(): HTMLElement | null {
  const id = combobox().getAttribute("aria-activedescendant");
  return id ? document.getElementById(id) : null;
}

describe("launcher combobox", () => {
  it("starts collapsed, with a live region already in the page", () => {
    render(App);
    const box = combobox();
    expect(box.getAttribute("aria-expanded")).toBe("false");
    expect(box.getAttribute("aria-autocomplete")).toBe("list");
    expect(box.hasAttribute("aria-activedescendant")).toBe(false);
    expect(liveRegion()).toBeTruthy();
    expect(liveRegion().textContent).toBe("");
  });

  it("points at the selected option of the listbox it controls", async () => {
    render(App);
    await type("report");
    await waitFor(() => expect(screen.getAllByRole("option").length).toBeGreaterThan(1));

    const box = combobox();
    expect(box.getAttribute("aria-expanded")).toBe("true");
    const listbox = document.getElementById(box.getAttribute("aria-controls")!)!;
    expect(listbox.getAttribute("role")).toBe("listbox");
    expect(listbox.getAttribute("aria-label")).toBe("Results");

    const first = active()!;
    expect(first.getAttribute("role")).toBe("option");
    expect(first.getAttribute("aria-selected")).toBe("true");
    expect(listbox.contains(first)).toBe(true);
    expect(listbox.querySelectorAll('[aria-selected="true"]')).toHaveLength(1);

    await key("ArrowDown");
    const second = active()!;
    expect(second).not.toBe(first);
    expect(second.getAttribute("aria-selected")).toBe("true");
    expect(first.getAttribute("aria-selected")).toBe("false");
  });

  it("names a new element for row 0 of every new search, so it is read again", async () => {
    render(App);
    await type("rep");
    await waitFor(() => expect(active()).not.toBeNull());
    const before = combobox().getAttribute("aria-activedescendant");
    await type("repo");
    await waitFor(() => expect(combobox().getAttribute("aria-activedescendant")).not.toBe(before));
    expect(active()!.getAttribute("aria-selected")).toBe("true");
  });

  it("announces the result count, and that nothing matched", async () => {
    render(App);
    await type("report");
    await waitFor(() => expect(liveRegion().textContent).toMatch(/^\d+ results$/), { timeout: 2000 });
    const count = screen.getAllByRole("option").length;
    expect(liveRegion().textContent).toBe(`${count} results`);

    await type("none");
    await waitFor(() => expect(liveRegion().textContent).toBe("No results."), { timeout: 2000 });
    expect(combobox().getAttribute("aria-expanded")).toBe("false");
    expect(combobox().hasAttribute("aria-activedescendant")).toBe(false);
  });

  it("moves to the action panel's listbox and back", async () => {
    render(App);
    await type("report");
    await waitFor(() => expect(active()).not.toBeNull());
    const title = active()!.querySelector(".title")!.textContent;

    await key("k", { ctrlKey: true });
    const box = combobox();
    await waitFor(() => expect(box.getAttribute("aria-controls")).toBe("actions"));
    const panel = document.getElementById("actions")!;
    expect(panel.getAttribute("role")).toBe("listbox");
    expect(panel.getAttribute("aria-label")).toBe(`Actions for ${title}`);
    expect(active()!.id).toBe("action-0");
    expect(active()!.getAttribute("aria-selected")).toBe("true");

    await key("ArrowDown");
    expect(active()!.id).toBe("action-1");
    await waitFor(() => expect(liveRegion().textContent).toMatch(/^Actions for .+: \d+ actions\. Escape closes\.$/), {
      timeout: 2000,
    });
    expect(await seriousViolations(), explain(await seriousViolations())).toEqual([]);

    await key("Escape");
    await waitFor(() => expect(box.getAttribute("aria-controls")).toBe("results"));
    expect(active()!.getAttribute("role")).toBe("option");
  });

  it("has no serious or critical axe violations with results and the grid", async () => {
    render(App);
    expect(await seriousViolations(), explain(await seriousViolations())).toEqual([]);
    await type("report");
    await waitFor(() => expect(active()).not.toBeNull());
    expect(await seriousViolations(), explain(await seriousViolations())).toEqual([]);
    await type(":smile");
    await waitFor(() => expect(document.querySelector(".grid [role='option']")).not.toBeNull());
    expect(active()!.closest("[role='listbox']")!.id).toBe("results");
    expect(await seriousViolations(), explain(await seriousViolations())).toEqual([]);
  });
});
