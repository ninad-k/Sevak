import type { Config } from "../settings-ipc";

const trimmed = (list: string[]) => list.map((entry) => entry.trim()).filter((entry) => entry !== "");

/** Ranges of `[passwords]` (as Rust's `sevak_core::config` has them). */
export const MIN_PASSWORD_LENGTH = 4;
export const MAX_PASSWORD_LENGTH = 128;
export const MIN_PASSPHRASE_WORDS = 2;
export const MAX_PASSPHRASE_WORDS = 20;

/** A whole number in `min..=max`; `fallback` when the field was left empty or is not a number. */
export function clampWhole(value: unknown, min: number, max: number, fallback: number): number {
  const number = typeof value === "number" ? value : Number.NaN;
  if (!Number.isFinite(number)) return fallback;
  return Math.min(max, Math.max(min, Math.round(number)));
}

/**
 * Trims the text fields of the plugin pages before a save, the way Rust's
 * `Config::normalized` would on the next load (so the form shows what is written).
 * Works on a copy of the draft.
 */
export function tidyPluginSettings(config: Config): void {
  config.bookmarks.keyword = config.bookmarks.keyword.trim();
  config.bookmarks.browsers = trimmed(config.bookmarks.browsers);
  config.tasks.keyword = config.tasks.keyword.trim();
  config.tasks.disabled = trimmed(config.tasks.disabled);
  config.media.keyword = config.media.keyword.trim();
  config.window_management.keyword = config.window_management.keyword.trim();
  config.window_management.switcher_keyword = config.window_management.switcher_keyword.trim();
  config.system.disabled = trimmed(config.system.disabled);
  config.contacts.keyword = config.contacts.keyword.trim();
  config.contacts.vcard_files = trimmed(config.contacts.vcard_files);
  config.onepassword.keyword = config.onepassword.keyword.trim();
  config.onepassword.op_path = config.onepassword.op_path.trim();
  config.onepassword.account = config.onepassword.account.trim();
  config.dictionary.define_keyword = config.dictionary.define_keyword.trim();
  config.dictionary.spell_keyword = config.dictionary.spell_keyword.trim();
  config.ai.keyword = config.ai.keyword.trim();
  config.ai.model = config.ai.model.trim();
  config.ai.base_url = config.ai.base_url.trim().replace(/\/+$/, "");
  config.ai.system_prompt = config.ai.system_prompt.trim();
  config.clipboard.ignore_apps = trimmed(config.clipboard.ignore_apps);
  config.snippets.ignore_apps = trimmed(config.snippets.ignore_apps);
  config.snippets.prefix = config.snippets.prefix.trim();
  config.shell.terminal = config.shell.terminal.trim();
  config.shell.shell = config.shell.shell.trim();
  config.clock.keyword = config.clock.keyword.trim();
  config.clock.cities = trimmed(config.clock.cities);
  config.passwords.keyword = config.passwords.keyword.trim();
  config.passwords.length = clampWhole(config.passwords.length, MIN_PASSWORD_LENGTH, MAX_PASSWORD_LENGTH, 20);
  config.passwords.words = clampWhole(config.passwords.words, MIN_PASSPHRASE_WORDS, MAX_PASSPHRASE_WORDS, 6);
}
