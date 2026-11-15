// @vitest-environment jsdom
// Every Settings section through axe-core: no serious or critical violation
// (unnamed controls, broken ARIA, duplicate ids, missing labels...). No Tauri:
// each page loads its browser-preview fixtures, as `npm run dev` does.
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import Settings from "./Settings.svelte";
import ListEditor from "./lib/settings-pages/ListEditor.svelte";
import Row from "./lib/settings-pages/Row.svelte";
import { describe as explain, seriousViolations, stubLayout } from "./test/a11y";
import { createRawSnippet } from "svelte";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => null) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

beforeEach(stubLayout);
afterEach(cleanup);

async function renderSettings() {
  render(Settings);
  await waitFor(() => expect(screen.getByRole("tablist", { name: "Settings sections" })).toBeTruthy());
  return screen.getAllByRole("tab");
}

describe("Settings accessibility", () => {
  it("the axe helper does catch a serious violation", async () => {
    document.body.innerHTML = '<button type="button"></button><input type="text">';
    const found = (await seriousViolations()).map((v) => v.id);
    expect(found).toContain("button-name");
    expect(found).toContain("label");
    document.body.innerHTML = "";
  });

  it("has a tab per section, arrow keys between them, and one tab stop", async () => {
    const tabs = await renderSettings();
    expect(tabs.length).toBeGreaterThan(10);
    expect(tabs.filter((t) => t.tabIndex === 0)).toHaveLength(1);
    expect(screen.getByRole("tabpanel").getAttribute("aria-labelledby")).toBe(tabs[0].id);
    await fireEvent.keyDown(tabs[0], { key: "ArrowDown" });
    await waitFor(() => expect(document.activeElement).toBe(tabs[1]));
    expect(tabs[1].getAttribute("aria-selected")).toBe("true");
    await fireEvent.keyDown(tabs[1], { key: "End" });
    await waitFor(() => expect(document.activeElement).toBe(screen.getAllByRole("tab").at(-1)));
  });

  it("has no serious or critical axe violations on any section", async () => {
    const tabs = await renderSettings();
    const failures: string[] = [];
    for (const tab of tabs) {
      await fireEvent.click(tab);
      await waitFor(() => expect(tab.getAttribute("aria-selected")).toBe("true"));
      // Pages that load data (gallery, extensions, backup) settle first.
      await new Promise((resolve) => setTimeout(resolve, 30));
      expect(document.getElementById("panel")!.querySelectorAll("input, button, select, a, p").length).toBeGreaterThan(0);
      const found = await seriousViolations();
      if (found.length > 0) failures.push(`[${tab.textContent?.trim()}]\n${explain(found)}`);
    }
    expect(failures.join("\n\n")).toBe("");
  }, 30_000);

  it("keeps the focus in a list when an entry is removed or added with Tab", async () => {
    render(ListEditor, { items: ["London", "Tokyo"], label: "Cities", id: "cities" });
    await fireEvent.click(screen.getByRole("button", { name: "Remove London" }));
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole("button", { name: "Remove Tokyo" })));
    await fireEvent.click(screen.getByRole("button", { name: "Remove Tokyo" }));
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole("textbox", { name: "Add to Cities" })));
    // The Add button stays focusable while there is nothing to add.
    const add = screen.getByRole("button", { name: "Add" });
    expect(add.hasAttribute("disabled")).toBe(false);
    expect(add.getAttribute("aria-disabled")).toBe("true");
  });

  it("links a row's problem to its control with aria-describedby", async () => {
    const control = createRawSnippet(() => ({ render: () => '<input id="days" aria-invalid="true">' }));
    const { rerender } = render(Row, { label: "Days ahead", forId: "days", error: "Use 1 to 31.", children: control });
    const input = screen.getByLabelText("Days ahead");
    const message = document.getElementById(input.getAttribute("aria-describedby") ?? "");
    expect(message?.textContent).toBe("Use 1 to 31.");
    expect(message?.getAttribute("role")).toBe("alert");
    await rerender({ error: "" });
    expect(input.hasAttribute("aria-describedby")).toBe(false);
  });
});
