import { afterEach, describe, expect, it, vi } from "vitest";
import { debouncedAnnouncer, launcherAnnouncement, type LauncherState } from "./announce";
import { setLanguage } from "./i18n.svelte";

const base: LauncherState = {
  query: "fire",
  count: 3,
  grid: false,
  panelActions: null,
  textView: false,
  largeText: null,
  indexing: false,
};

describe("launcherAnnouncement", () => {
  it("counts results, with the singular for one", () => {
    expect(launcherAnnouncement(base)).toBe("3 results");
    expect(launcherAnnouncement({ ...base, count: 1 })).toBe("1 result");
    expect(launcherAnnouncement({ ...base, grid: true, count: 12 })).toBe("12 items");
  });

  it("says nothing for an empty search box, and says so when nothing matched", () => {
    expect(launcherAnnouncement({ ...base, query: "  ", count: 0 })).toBe("");
    expect(launcherAnnouncement({ ...base, count: 0 })).toBe("No results.");
    expect(launcherAnnouncement({ ...base, count: 0, indexing: true })).toBe(
      "No results yet, still indexing applications.",
    );
  });

  it("names the views that replace the list, most specific first", () => {
    expect(launcherAnnouncement({ ...base, panelActions: 4, panelTitle: "Actions for Firefox" })).toBe(
      "Actions for Firefox: 4 actions. Escape closes.",
    );
    expect(launcherAnnouncement({ ...base, panelActions: 1 })).toBe("1 action. Escape closes.");
    expect(launcherAnnouncement({ ...base, textView: true, panelActions: 2 })).toMatch(/^Text view\./);
    expect(launcherAnnouncement({ ...base, textView: true, largeText: "42" })).toBe(
      "Large Type: 42. Press Escape to close.",
    );
  });
});

describe("launcherAnnouncement in another language", () => {
  afterEach(() => setLanguage("en"));

  it("speaks the current language with its plural rules and number format", () => {
    setLanguage("de");
    expect(launcherAnnouncement({ ...base, count: 1 })).toBe("1 Ergebnis");
    expect(launcherAnnouncement({ ...base, count: 1200 })).toBe("1.200 Ergebnisse");
    expect(launcherAnnouncement({ ...base, count: 0 })).toBe("Keine Ergebnisse.");
    setLanguage("fr");
    expect(launcherAnnouncement({ ...base, panelActions: 2, panelTitle: "Actions pour Firefox" })).toBe(
      "Actions pour Firefox : 2 actions. Échap ferme.",
    );
  });
});

describe("debouncedAnnouncer", () => {
  afterEach(() => vi.useRealTimers());

  it("speaks only the text that settled, once", () => {
    vi.useFakeTimers();
    const say = vi.fn();
    const announcer = debouncedAnnouncer(say, 300);
    announcer.update("10 results");
    vi.advanceTimersByTime(100);
    announcer.update("4 results");
    vi.advanceTimersByTime(100);
    announcer.update("2 results");
    vi.advanceTimersByTime(300);
    expect(say.mock.calls).toEqual([["2 results"]]);
    // The same sentence again is not repeated; a new one is.
    announcer.update("2 results");
    vi.advanceTimersByTime(300);
    announcer.update("No results.");
    vi.advanceTimersByTime(300);
    expect(say.mock.calls).toEqual([["2 results"], ["No results."]]);
  });

  it("cancel drops a pending announcement", () => {
    vi.useFakeTimers();
    const say = vi.fn();
    const announcer = debouncedAnnouncer(say, 300);
    announcer.update("3 results");
    announcer.cancel();
    vi.advanceTimersByTime(1000);
    expect(say).not.toHaveBeenCalled();
  });
});
