import { beforeEach, describe, expect, it, vi } from "vitest";
import { mockSettings } from "./mock";
import type { Appearance, Config, WebSearchEngine } from "./settings-ipc";
import {
  MAX_DEPTH,
  appearanceErrors,
  engineErrors,
  hotkeyErrors,
  isColor,
  totalProblems,
  validate,
} from "./validate";

// mock.ts reads `location` when it builds the browser-preview settings.
beforeEach(() => {
  vi.stubGlobal("location", { search: "" });
});

const config = (): Config => structuredClone(mockSettings().config);
const engine = (keyword: string, extra: Partial<WebSearchEngine> = {}): WebSearchEngine => ({
  keyword,
  name: keyword.toUpperCase(),
  url: `https://example.org/?q={query}`,
  ...extra,
});

describe("isColor", () => {
  it("accepts #rgb, #rrggbb and rgb()", () => {
    for (const ok of ["#fff", "#FFF", "#1c1b2e", " #abc ", "rgb(0, 0, 0)", "rgb(255 255 255)", "RGB(1,2,3)"]) {
      expect(isColor(ok), ok).toBe(true);
    }
  });

  it("rejects everything else, matching what Rust accepts", () => {
    for (const bad of ["", "#ff", "#ffff", "#fffff", "#1c1b2e00", "#ggg", "red", "rgb(256, 0, 0)", "rgb(1, 2)", "rgba(1, 2, 3, 0.5)"]) {
      expect(isColor(bad), bad).toBe(false);
    }
  });
});

describe("hotkeyErrors", () => {
  const main = "Super+Space";

  it("requires a key", () => {
    expect(hotkeyErrors({ key: "  ", query: "x" }, [], main).key).toBe("Required");
  });

  it("flags the main and Universal Actions shortcuts, whatever the spelling", () => {
    expect(hotkeyErrors({ key: "win+space" }, [], main).key).toBe("Same as the main shortcut");
    expect(hotkeyErrors({ key: "alt+ctrl+SPACE" }, [], main, "Ctrl+Alt+Space").key).toBe(
      "Same as the Universal Actions shortcut",
    );
  });

  it("does not compare with an empty (turned off) Universal Actions key", () => {
    const empty = hotkeyErrors({ key: "Ctrl+T", query: "x" }, [{ key: "Ctrl+T", query: "x" }], main, "");
    expect(empty).toEqual({});
  });

  it("flags a key used twice, in either position", () => {
    const all = [
      { key: "Ctrl+Alt+T", query: "a" },
      { key: "alt + control + t", query: "b" },
    ];
    expect(hotkeyErrors(all[0], all, main).key).toBe("Already used");
    expect(hotkeyErrors(all[1], all, main).key).toBe("Already used");
  });

  it("wants a result id when `run` is present but blank", () => {
    expect(hotkeyErrors({ key: "Ctrl+Alt+R", run: "  " }, [], main).value).toBe("Enter a result id");
    expect(hotkeyErrors({ key: "Ctrl+Alt+R", run: "apps:x" }, [], main).value).toBeUndefined();
    expect(hotkeyErrors({ key: "Ctrl+Alt+R", query: "" }, [], main).value).toBeUndefined();
  });
});

describe("appearanceErrors", () => {
  const base = (): Appearance => config().appearance;

  it("passes the defaults", () => {
    expect(appearanceErrors(base())).toEqual({});
  });

  it("checks the accent color", () => {
    expect(appearanceErrors({ ...base(), accent: "#f59e0b" })).toEqual({});
    expect(appearanceErrors({ ...base(), accent: "orange" }).accent).toBeDefined();
  });

  it("limits font names to letters, digits, spaces and a few separators", () => {
    expect(appearanceErrors({ ...base(), font_family: "Segoe UI, sans-serif" })).toEqual({});
    expect(appearanceErrors({ ...base(), font_family: "Noto Sans CJK \u65e5\u672c" })).toEqual({});
    expect(appearanceErrors({ ...base(), font_family: "a; color: red" }).fontFamily).toBeDefined();
    expect(appearanceErrors({ ...base(), font_family: "x{}" }).fontFamily).toBeDefined();
  });

  it("keeps theme and stylesheet paths inside the config folder", () => {
    for (const path of ["/etc/passwd", "\\\\server\\share\\a.toml", "C:\\a.css", "../a.css", "themes/../../a.css"]) {
      expect(appearanceErrors({ ...base(), custom_css: path }).customCss, path).toBeDefined();
      expect(appearanceErrors({ ...base(), theme_file: path }).themeFile, path).toBeDefined();
    }
    for (const path of ["custom.css", "themes/Nord.toml", "themes\\Nord.toml", "a..b.css"]) {
      expect(appearanceErrors({ ...base(), custom_css: path }).customCss, path).toBeUndefined();
      expect(appearanceErrors({ ...base(), theme_file: path }).themeFile, path).toBeUndefined();
    }
  });
});

describe("engineErrors", () => {
  it("accepts a complete engine", () => {
    const e = engine("ddg");
    expect(engineErrors(e, [e])).toEqual({});
  });

  it("requires a one-word keyword", () => {
    expect(engineErrors(engine(" "), [engine(" ")]).keyword).toBe("Required");
    expect(engineErrors(engine("a b"), [engine("a b")]).keyword).toBe("No spaces");
  });

  it("flags duplicate keywords ignoring case and surrounding spaces", () => {
    const all = [engine("g"), engine(" G ")];
    expect(engineErrors(all[0], all).keyword).toBe("Already used");
    expect(engineErrors(all[1], all).keyword).toBe("Already used");
  });

  it("requires a name and an http(s) URL with {query}", () => {
    expect(engineErrors(engine("x", { name: " " }), []).name).toBe("Required");
    expect(engineErrors(engine("x", { url: "ftp://a/{query}" }), []).url).toMatch(/http/);
    expect(engineErrors(engine("x", { url: "https://a/" }), []).url).toBe("Must contain {query}");
    expect(engineErrors(engine("x", { url: "HTTP://a/?q={query}" }), []).url).toBeUndefined();
  });
});

describe("validate", () => {
  it("passes the stock settings with no problems at all", () => {
    const problems = validate(config());
    expect(totalProblems(problems)).toBe(0);
    expect(problems.engines).toHaveLength(3);
    expect(problems.hotkeys).toHaveLength(2);
  });

  it("counts problems per section", () => {
    const c = config();
    c.web_search.push(engine("g")); // duplicates the stock `g`
    c.hotkey.push({ key: "Super+Space", query: "x" });
    c.appearance.accent = "nope";
    const problems = validate(c);
    expect(problems.count.web).toBe(2); // both `g` rows
    expect(problems.count.hotkeys).toBe(1);
    expect(problems.count.appearance).toBe(1);
    expect(totalProblems(problems)).toBe(4);
  });

  describe("keyword clash rules mirrored from Rust", () => {
    it("rejects a fallback engine that is not defined", () => {
      const c = config();
      c.search.fallback_web_search = ["g", "nope"];
      expect(validate(c).fallback).toContain("nope");
      expect(validate(c).count.search).toBe(1);
    });

    it("accepts a fallback in either config.toml form, case-insensitively", () => {
      const c = config();
      c.search.fallback_web_search = "G";
      expect(validate(c).fallback).toBeUndefined();
      c.search.fallback_web_search = ["g", "YT"];
      expect(validate(c).fallback).toBeUndefined();
      c.search.fallback_web_search = [];
      expect(validate(c).fallback).toBeUndefined();
    });

    it("rejects a files keyword that a web search already uses", () => {
      const c = config();
      c.files.keyword = "GH";
      const problems = validate(c);
      expect(problems.keywords["files.keyword"]).toBe("Already a web search keyword");
      expect(problems.count.files).toBe(1);
    });

    it("lets the files keyword be empty (turned off) but not contain spaces", () => {
      const c = config();
      c.files.keyword = "";
      expect(validate(c).keywords["files.keyword"]).toBeUndefined();
      c.files.keyword = "a b";
      expect(validate(c).keywords["files.keyword"]).toBe("No spaces");
    });

    it("checks the files depth bounds", () => {
      const c = config();
      for (const ok of [0, 1, MAX_DEPTH]) {
        c.files.max_depth = ok;
        expect(validate(c).filesDepth, String(ok)).toBeUndefined();
      }
      for (const bad of [-1, MAX_DEPTH + 1, 2.5, Number.NaN]) {
        c.files.max_depth = bad;
        expect(validate(c).filesDepth, String(bad)).toBeDefined();
      }
    });

    it("compares hotkey entries against the main and Universal Actions keys", () => {
      const c = config();
      c.hotkey = [{ key: "Ctrl+Alt+Space", query: "x" }];
      expect(validate(c).hotkeys[0].key).toBe("Same as the Universal Actions shortcut");
      c.general.actions_hotkey = "";
      expect(validate(c).hotkeys[0].key).toBeUndefined();
    });

    // The form mirrors Rust's validate_builtin_keywords: fixed keywords and the
    // configurable built-in ones (files, bookmarks, tasks, media, contacts,
    // 1Password, dictionary) all clash with web searches and with each other.
    it("flags a web search keyword that a fixed built-in keyword owns", () => {
      const c = config();
      c.web_search = [engine("emoji")];
      c.search.fallback_web_search = [];
      expect(validate(c).engines[0].keyword).toBe("Used by the emoji picker");
    });

    it("flags both sides of a clash between configurable built-in keywords", () => {
      const c = config();
      c.tasks.keyword = "ctt";
      c.media.keyword = "ctt";
      const problems = validate(c);
      expect(problems.keywords["tasks.keyword"]).toBe("Already used by media controls");
      expect(problems.keywords["media.keyword"]).toBe("Already used by automation tasks");
    });

    it("will not let the contacts, 1Password and dictionary keywords be empty", () => {
      const c = config();
      c.contacts.keyword = "";
      c.onepassword.keyword = "";
      c.dictionary.define_keyword = "";
      c.dictionary.spell_keyword = "";
      const problems = validate(c);
      expect(problems.keywords["contacts.keyword"]).toBe("Required");
      expect(problems.keywords["onepassword.keyword"]).toBe("Required");
      expect(problems.keywords["dictionary.define_keyword"]).toBe("Required");
      expect(problems.keywords["dictionary.spell_keyword"]).toBe("Required");
      expect(problems.count.integrations).toBe(4);
    });
  });
});
