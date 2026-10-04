import type { Config } from "../settings-ipc";

const trimmed = (list: string[]) => list.map((entry) => entry.trim()).filter((entry) => entry !== "");

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
  config.system.disabled = trimmed(config.system.disabled);
  config.contacts.keyword = config.contacts.keyword.trim();
  config.contacts.vcard_files = trimmed(config.contacts.vcard_files);
  config.onepassword.keyword = config.onepassword.keyword.trim();
  config.onepassword.op_path = config.onepassword.op_path.trim();
  config.onepassword.account = config.onepassword.account.trim();
  config.dictionary.define_keyword = config.dictionary.define_keyword.trim();
  config.dictionary.spell_keyword = config.dictionary.spell_keyword.trim();
  config.clipboard.ignore_apps = trimmed(config.clipboard.ignore_apps);
  config.snippets.ignore_apps = trimmed(config.snippets.ignore_apps);
  config.snippets.prefix = config.snippets.prefix.trim();
  config.shell.terminal = config.shell.terminal.trim();
  config.shell.shell = config.shell.shell.trim();
}
