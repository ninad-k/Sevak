import { afterEach, describe, expect, it } from "vitest";
import {
  CATALOGUES,
  LANGUAGES,
  RICH_TAGS,
  currentLanguage,
  formatNumber,
  resolveLanguage,
  setLanguage,
  t,
  tHtml,
  type Lang,
} from "./i18n.svelte";

type Node = string | { [key: string]: Node };

/** Every message in a catalogue: dotted key → its texts (one, or each plural form). */
function messages(node: Node, prefix = "", out = new Map<string, string[]>()): Map<string, string[]> {
  if (typeof node === "string") {
    out.set(prefix, [node]);
    return out;
  }
  if (typeof node.other === "string") {
    out.set(prefix, Object.values(node) as string[]);
    return out;
  }
  for (const [key, child] of Object.entries(node)) {
    if (prefix === "" && key === "_meta") continue;
    messages(child, prefix ? `${prefix}.${key}` : key, out);
  }
  return out;
}

const placeholders = (texts: string[]) =>
  [...new Set(texts.flatMap((text) => [...text.matchAll(/\{(\w+)\}/g)].map((m) => m[1])))].sort();
const tags = (texts: string[]) =>
  [...new Set(texts.flatMap((text) => [...text.matchAll(/<\/?([a-z]+)[^>]*>/g)].map((m) => m[0])))].sort();

const english = messages(CATALOGUES.en as Node);
const others = LANGUAGES.map((l) => l.code).filter((code) => code !== "en") as Lang[];

describe("locale catalogues", () => {
  it("English has messages", () => {
    expect(english.size).toBeGreaterThan(5);
  });

  it.each(others)("%s has exactly the English keys", (lang) => {
    const keys = [...messages(CATALOGUES[lang] as Node).keys()];
    const missing = [...english.keys()].filter((key) => !keys.includes(key));
    const unknown = keys.filter((key) => !english.has(key));
    expect({ missing, unknown }).toEqual({ missing: [], unknown: [] });
  });

  it.each(others)("%s keeps every {placeholder} and tag of English", (lang) => {
    const mismatched: string[] = [];
    for (const [key, texts] of messages(CATALOGUES[lang] as Node)) {
      const source = english.get(key);
      if (!source) continue;
      if (placeholders(texts).join() !== placeholders(source).join()) mismatched.push(`${key}: placeholders`);
      if (tags(texts).join() !== tags(source).join()) mismatched.push(`${key}: tags`);
    }
    expect(mismatched).toEqual([]);
  });

  it.each(others)("%s says whether a native speaker has reviewed it", (lang) => {
    // docs/localisation.md: a reviewer replaces the draft marker with "reviewed".
    expect(["machine-drafted, needs native review", "reviewed"]).toContain(
      (CATALOGUES[lang] as { _meta?: { status?: string } })._meta?.status,
    );
  });

  it.each(LANGUAGES.map((l) => l.code))("%s uses only the allowed inline tags", (lang) => {
    const allowed = new Set<string>(RICH_TAGS);
    const bad: string[] = [];
    for (const [key, texts] of messages(CATALOGUES[lang] as Node)) {
      for (const text of texts) {
        for (const m of text.matchAll(/<\/?([a-zA-Z]+)([^>]*)>/g)) {
          if (!allowed.has(m[1]) || m[2].trim() !== "") bad.push(`${key}: ${m[0]}`);
        }
      }
    }
    expect(bad).toEqual([]);
  });

  it.each(LANGUAGES.map((l) => l.code))("%s plural messages have an `other` form and only CLDR categories", (lang) => {
    const categories = new Set(["zero", "one", "two", "few", "many", "other"]);
    const bad: string[] = [];
    const walk = (node: Node, prefix: string) => {
      if (typeof node === "string") return;
      const keys = Object.keys(node);
      const plural = keys.some((k) => categories.has(k));
      if (plural) {
        if (!keys.every((k) => categories.has(k)) || typeof node.other !== "string") bad.push(prefix);
        return;
      }
      for (const [k, child] of Object.entries(node)) if (k !== "_meta") walk(child, prefix ? `${prefix}.${k}` : k);
    };
    walk(CATALOGUES[lang] as Node, "");
    expect(bad).toEqual([]);
  });
});

describe("t", () => {
  afterEach(() => {
    setLanguage("en");
  });

  it("looks a message up and fills placeholders", () => {
    expect(t("common.save")).toBe("Save");
    expect(t("language.auto", { language: "Deutsch" })).toBe("Automatic (Deutsch)");
  });

  it("follows the current language and falls back to English, then the key", () => {
    setLanguage("de");
    expect(currentLanguage()).toBe("de");
    expect(t("common.save")).toBe("Speichern");
    const de = CATALOGUES.de as { common: Record<string, string> };
    const saved = de.common.close;
    delete de.common.close;
    try {
      expect(t("common.close")).toBe("Close");
    } finally {
      de.common.close = saved;
    }
    expect(t("no.such.key" as never)).toBe("no.such.key");
  });

  it("picks the plural form with Intl.PluralRules", () => {
    const en = CATALOGUES.en as Record<string, unknown>;
    en.__test = { n: { one: "{count} result", other: "{count} results" } };
    try {
      expect(t("__test.n" as never, { count: 1 })).toBe("1 result");
      expect(t("__test.n" as never, { count: 1200 })).toBe("1,200 results");
      setLanguage("de");
      expect(t("__test.n" as never, { count: 1200 })).toBe("1.200 results");
    } finally {
      delete en.__test;
    }
  });

  it("escapes parameters of rich messages", () => {
    const en = CATALOGUES.en as Record<string, unknown>;
    en.__test = { rich: "Type <code>{word}</code>" };
    try {
      expect(tHtml("__test.rich" as never, { word: "<b>&" })).toBe("Type <code>&lt;b&gt;&amp;</code>");
    } finally {
      delete en.__test;
    }
  });

  it("formats numbers for the language", () => {
    setLanguage("fr");
    expect(formatNumber(1234.5).replace(/\s/g, " ")).toBe("1 234,5");
  });
});

describe("resolveLanguage", () => {
  it("uses a shipped language as set", () => {
    expect(resolveLanguage("fr", ["de-DE"])).toBe("fr");
    expect(resolveLanguage(" HI ", [])).toBe("hi");
  });

  it("follows the OS for auto or an unknown code, matching the primary subtag", () => {
    expect(resolveLanguage("auto", ["pt-BR", "de-AT", "en"])).toBe("de");
    expect(resolveLanguage("xx", ["es_MX"])).toBe("es");
  });

  it("falls back to English", () => {
    expect(resolveLanguage("auto", ["ja-JP"])).toBe("en");
    expect(resolveLanguage(undefined, [])).toBe("en");
  });

  it("sets <html lang>", () => {
    const doc = { documentElement: { lang: "" } };
    const g = globalThis as { document?: unknown };
    const saved = g.document;
    g.document = doc;
    try {
      setLanguage("hi");
      expect(doc.documentElement.lang).toBe("hi");
    } finally {
      g.document = saved;
      setLanguage("en");
    }
  });
});
