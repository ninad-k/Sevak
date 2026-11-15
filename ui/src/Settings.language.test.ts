// @vitest-environment jsdom
// The language picker switches Settings at once, without saving, and every
// section stays accessible in a translated language (no control loses its name).
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import Settings from "./Settings.svelte";
import { CATALOGUES, setLanguage } from "./lib/i18n.svelte";
import { describe as explain, seriousViolations, stubLayout } from "./test/a11y";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => null) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

beforeEach(stubLayout);
afterEach(() => {
  cleanup();
  setLanguage("en");
});

const de = CATALOGUES.de as { settings: { sectionsLabel: string; title: string } };

async function renderInGerman() {
  render(Settings);
  const picker = await waitFor(() => screen.getByRole("combobox", { name: "Language" }) as HTMLSelectElement);
  await fireEvent.change(picker, { target: { value: "de" } });
  await waitFor(() => expect(screen.getByRole("tablist", { name: de.settings.sectionsLabel })).toBeTruthy());
}

describe("Settings language", () => {
  it("lists each language in its own name and switches live, with <html lang>", async () => {
    render(Settings);
    const picker = await waitFor(() => screen.getByRole("combobox", { name: "Language" }) as HTMLSelectElement);
    const names = [...picker.options].map((o) => o.textContent?.trim());
    expect(names).toEqual(["Automatic (English)", "English", "Español", "Deutsch", "Français", "हिन्दी"]);
    expect(picker.options[4].lang).toBe("fr");

    await fireEvent.change(picker, { target: { value: "de" } });
    await waitFor(() => expect(document.documentElement.lang).toBe("de"));
    expect(screen.getByRole("combobox", { name: "Sprache" })).toBe(picker);
    expect(screen.getByRole("button", { name: "Speichern" })).toBeTruthy();
    expect(document.title).toBe(de.settings.title);
  });

  it("has no serious or critical axe violations on any section in German", async () => {
    await renderInGerman();
    const failures: string[] = [];
    for (const tab of screen.getAllByRole("tab")) {
      await fireEvent.click(tab);
      await waitFor(() => expect(tab.getAttribute("aria-selected")).toBe("true"));
      await new Promise((resolve) => setTimeout(resolve, 30));
      const found = await seriousViolations();
      if (found.length > 0) failures.push(`[${tab.textContent?.trim()}]\n${explain(found)}`);
    }
    expect(failures.join("\n\n")).toBe("");
  }, 30_000);
});
