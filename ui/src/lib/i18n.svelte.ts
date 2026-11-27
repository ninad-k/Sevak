// The UI's message catalogue. English (`locales/en.json`) is the source; the
// other languages are bundled the same way, so every build and installer has
// them and nothing is read from disk at run time. `t("settings.save")` looks a
// message up in the current language and falls back to English, then to the
// key itself. docs/localisation.md explains how to add or fix a language.
//
// A message is a string with `{name}` placeholders, or an object of plural
// forms (`one`, `few`, `many`, `other`... as `Intl.PluralRules` names them)
// chosen by the `count` parameter. `other` is required. A few messages carry
// inline markup (`<code>`, `<kbd>`, `<strong>`, `<em>`); render those with
// `{@html tHtml(...)}`, which escapes every parameter.

import en from "../locales/en.json";
import es from "../locales/es.json";
import de from "../locales/de.json";
import fr from "../locales/fr.json";
import hi from "../locales/hi.json";

/** The languages Sevak ships, each named in itself (for the language picker). */
export const LANGUAGES = [
  { code: "en", name: "English" },
  { code: "es", name: "Español" },
  { code: "de", name: "Deutsch" },
  { code: "fr", name: "Français" },
  { code: "hi", name: "हिन्दी" },
] as const;

export type Lang = (typeof LANGUAGES)[number]["code"];
/** The `[general] language` setting: a language code, or `auto` for the OS language. */
export type LanguageSetting = Lang | "auto";

type Plural = { other: string } & Partial<Record<Intl.LDMLPluralRule, string>>;
type Catalogue = { [key: string]: string | Plural | Catalogue };

/** Dotted paths to every message of `T`; plural objects are messages too. */
type Paths<T> = {
  [K in keyof T & string]: K extends "_meta"
    ? never
    : T[K] extends string
      ? K
      : T[K] extends { other: string }
        ? K
        : `${K}.${Paths<T[K]>}`;
}[keyof T & string];

/** Every message key, checked against the English catalogue at compile time. */
export type MessageKey = Paths<typeof en>;
export type Params = Record<string, string | number>;

export const CATALOGUES: Record<Lang, Catalogue> = { en, es, de, fr, hi } as Record<Lang, Catalogue>;

const isLang = (code: string): code is Lang => LANGUAGES.some((l) => l.code === code);

/**
 * The language to show for a setting: the configured one when Sevak ships it,
 * else (for `auto` or an unknown code) the first of the OS's preferred
 * languages Sevak has, matched on the primary subtag (`de-AT` is German), else
 * English.
 */
export function resolveLanguage(setting: string | null | undefined, preferred: readonly string[] = []): Lang {
  const wanted = (setting ?? "").trim().toLowerCase();
  if (isLang(wanted)) return wanted;
  for (const tag of preferred) {
    const primary = tag.trim().toLowerCase().split(/[-_]/)[0];
    if (isLang(primary)) return primary;
  }
  return "en";
}

const state = $state<{ lang: Lang }>({ lang: "en" });

/** The language the UI shows now. */
export function currentLanguage(): Lang {
  return state.lang;
}

/** The OS's preferred languages as the webview reports them. */
function osLanguages(): readonly string[] {
  if (typeof navigator === "undefined") return [];
  return navigator.languages?.length ? navigator.languages : navigator.language ? [navigator.language] : [];
}

/**
 * Switches the UI to `setting` (`auto` follows the OS) at once: every `t()` in a
 * template re-renders, and `<html lang>` follows so screen readers and
 * hyphenation use the right language. Returns the language chosen.
 */
export function setLanguage(setting: string | null | undefined): Lang {
  const lang = resolveLanguage(setting, osLanguages());
  state.lang = lang;
  if (typeof document !== "undefined") document.documentElement.lang = lang;
  return lang;
}

function lookup(catalogue: Catalogue, key: string): string | Plural | undefined {
  let node: string | Plural | Catalogue | undefined = catalogue;
  for (const part of key.split(".")) {
    if (node === undefined || typeof node === "string") return undefined;
    node = (node as Catalogue)[part];
  }
  if (typeof node === "string") return node;
  if (node && typeof node === "object" && typeof (node as Plural).other === "string") return node as Plural;
  return undefined;
}

const pluralRules = new Map<Lang, Intl.PluralRules>();
function pluralCategory(lang: Lang, count: number): Intl.LDMLPluralRule {
  let rules = pluralRules.get(lang);
  if (!rules) {
    rules = new Intl.PluralRules(lang);
    pluralRules.set(lang, rules);
  }
  return rules.select(count);
}

function interpolate(text: string, params: Params | undefined, escape: (s: string) => string): string {
  if (!params) return text;
  return text.replace(/\{(\w+)\}/g, (whole, name: string) => {
    if (!(name in params)) return whole;
    const value = params[name];
    return escape(typeof value === "number" ? formatNumber(value) : value);
  });
}

function message(key: MessageKey, params: Params | undefined, escape: (s: string) => string): string {
  const lang = state.lang;
  let found = lookup(CATALOGUES[lang], key);
  let from: Lang = lang;
  if (found === undefined) {
    found = lookup(CATALOGUES.en, key);
    from = "en";
  }
  if (found === undefined) return key;
  let text: string;
  if (typeof found === "string") {
    text = found;
  } else {
    const count = Number(params?.count ?? 0);
    text = found[pluralCategory(from, count)] ?? found.other;
  }
  return interpolate(text, params, escape);
}

/** The message `key` in the current language, with `{name}` placeholders filled. */
export function t(key: MessageKey, params?: Params): string {
  return message(key, params, (s) => s);
}

const escapeHtml = (s: string) =>
  s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);

/**
 * Like `t`, for a message with inline markup, to render with `{@html}`.
 * Parameters are escaped; the markup comes only from the bundled catalogue,
 * whose tags the locale test limits to `RICH_TAGS`.
 */
export function tHtml(key: MessageKey, params?: Params): string {
  return message(key, params, escapeHtml);
}

/** The inline tags a message may contain. */
export const RICH_TAGS = ["code", "kbd", "strong", "em"] as const;

/**
 * A number in the current language's format (`1,234.5`, `1.234,5`, `12,34,567`...).
 * `t` formats number parameters this way; pass a string for a year, port or id.
 */
export function formatNumber(value: number, options?: Intl.NumberFormatOptions): string {
  return new Intl.NumberFormat(state.lang, options).format(value);
}

/** A date and/or time in the current language's format. */
export function formatDate(value: Date | number, options?: Intl.DateTimeFormatOptions): string {
  return new Intl.DateTimeFormat(state.lang, options).format(value);
}

/** A list such as "a, b and c" in the current language. */
export function formatList(items: readonly string[], type: "conjunction" | "disjunction" = "conjunction"): string {
  return new Intl.ListFormat(state.lang, { style: "long", type }).format(items);
}
