# Languages and localisation

Sevak's launcher, Settings window and tray menu are available in five
languages, and the Windows setup in four. This page says what is translated,
how the language is chosen, how far each translation has been checked, and how
to fix a translation or add a language.

## Choosing the language

**Settings → General → Language** lists **Automatic** and each language in its
own name. The change applies at once in Settings, and in the launcher and the
tray menu when you save. In `config.toml`:

```toml
[general]
language = "auto"   # or "en", "es", "de", "fr", "hi"
```

`auto` (the default) follows the operating system: the windows use the
languages the system webview reports (`navigator.languages`), the tray uses the
account's display language on Windows, `AppleLanguages` on macOS and
`LC_ALL`/`LC_MESSAGES`/`LANGUAGE`/`LANG` on Linux. The first of those Sevak
ships wins, matched on the language alone (`de-AT` and `de-CH` are German); if
none matches, Sevak uses English. An unknown value in `config.toml` is read as
`auto`. The page's `<html lang>` follows the language, so screen readers use the
right voice.

## Languages and review status

| Language | Code | App and tray | Windows setup (NSIS) | Status |
|---|---|---|---|---|
| English | `en` | Yes | Yes | Source text |
| Español | `es` | Yes | Yes | Machine-drafted, needs native review |
| Deutsch | `de` | Yes | Yes | Machine-drafted, needs native review |
| Français | `fr` | Yes | Yes | Machine-drafted, needs native review |
| हिन्दी (Hindi) | `hi` | Yes | No (see below) | Machine-drafted, needs native review |

The Spanish, German, French and Hindi text was drafted by machine translation and
checked only for completeness and consistency, not by native speakers. Expect
the occasional stiff or wrong phrase. Each of these catalogues says so in its
`_meta.status`. When a native speaker has reviewed a whole language, set it to
`"reviewed"` and update the table above.

Each language uses one form of address throughout, as the English text does:
Spanish *tú*, German *du*, French *vous*, Hindi *आप*.

## What is translated

- The launcher: the search box, the action panel, key hints, the preview pane,
  Text View, the file buffer, Large Type, and the screen-reader announcements
  (result counts and so on).
- Every Settings page, including Workflows, Gallery, Extensions, Backup &
  restore, Help, the startup status and repair messages, validation messages
  and native file-dialog titles.
- The tray menu and its tooltip.
- The Windows setup (NSIS) in English, German, French and Spanish, with a
  language selector at the start. It preselects your Windows display language
  and remembers your choice for the uninstaller.
- Numbers, dates, file sizes and lists that the UI formats itself follow the
  language (`1,234.5`, `1.234,5`, `1 234,5`, `12,34,567`).

## What stays in English

- **Search results and other text from Sevak's Rust core.** Plugin result
  titles and subtitles (application entries, calculator and converter answers,
  file results, system commands), error messages from the core, hotkey
  registration notes, the diagnostics report and command-line output stay in
  English for now. Translating them needs a catalogue on the Rust side and is
  not done yet.
- **What you type.** Keywords (`g`, `ff`, `t`), query syntax (`ext:pdf`,
  `modified:7d`, `3pm pst in ist`, `snap left`) and config keys are the same in
  every language, because the plugins only parse English.
- **Names.** Sevak, theme names, gallery and extension names and descriptions
  (they are published as their authors wrote them), and product names.
- **Key names.** Settings shows shortcuts as Sevak displays them (Ctrl, Alt,
  Shift, Super), so the prose uses the same names.
- **The MSI installer.** Sevak publishes exactly one MSI, which the release and
  installer validation expect, for managed deployment where the setup runs
  without a UI. WiX builds one MSI per language, so the MSI stays English. The
  NSIS setup (`Sevak_x.y.z_x64-setup.exe`) is the one with languages.
- **Hindi in the Windows setup.** Tauri's NSIS bundler ships its own installer
  messages only for a fixed list of languages, and Hindi is not on it
  (tauri-bundler 2.10.4, which Tauri CLI 2.12 uses; see `get_lang_data`). The setup offers English, German,
  French and Spanish; a Hindi system gets the English setup, and Sevak itself
  then follows the system language.
- macOS `.dmg` and Linux packages have no installer text of their own.
- This documentation site.

## How it works

- `ui/src/locales/<code>.json` holds one catalogue per language. They are
  imported at build time, so every build and installer carries all of them and
  nothing is read from disk at run time.
- `en.json` is the source. Keys are nested by area (`settings.general.…`,
  `pages.clipboard.…`), and `t("common.save")` in `ui/src/lib/i18n.svelte.ts`
  looks a message up in the current language, falling back to English and
  then to the key itself. Keys are type-checked against `en.json`.
- `{name}` is a placeholder. A message that depends on a number is an object of
  plural forms chosen by `Intl.PluralRules`, with `other` required:
  `{ "one": "{count} result", "other": "{count} results" }`. French and Spanish
  may add `many`, other languages their own CLDR categories.
- A few messages contain `<code>`, `<kbd>`, `<strong>` or `<em>` (and no other
  markup). They are shown with `tHtml`, which escapes every parameter.
- The tray's six strings are a small table in `src-tauri/src/tray.rs`.
  The setup's strings are `src-tauri/installer/<Language>.nsh`.
- `ui/src/lib/i18n.test.ts` fails when a catalogue misses an English key, has a
  key English does not, changes a `{placeholder}` or a tag, or uses markup that
  is not allowed. `scripts/installer.test.mjs` does the same for the setup's
  language files and their `$variables`.

## Fixing a translation

1. Find the English text in `ui/src/locales/en.json` and note its key.
2. Change the same key in your language's file. Keep every `{placeholder}` and
   tag; you may move them within the sentence.
3. Run `npm test` and `npm run check`.

For the tray, edit the `labels` table in `src-tauri/src/tray.rs`. For the
Windows setup, edit `src-tauri/installer/<Language>.nsh` and keep the
`$variables`, `${VERSION}` and `$\r$\n` line breaks.

## Adding a language

1. Copy `ui/src/locales/en.json` to `<code>.json` (an ISO 639-1 code), set
   `"_meta": { "status": "machine-drafted, needs native review" }` (or
   `"reviewed"`), and translate every message.
2. Import it in `ui/src/lib/i18n.svelte.ts` and add it to `LANGUAGES` with its
   name in that language.
3. Add the code to `LANGUAGES` in `crates/sevak-core/src/language.rs`, a row to
   the tray table in `src-tauri/src/tray.rs`, and the code to the `language`
   comment in `DEFAULT_CONFIG_TOML` (`crates/sevak-core/src/config.rs`) and in
   [Configuration](configuration.md#general).
4. If Tauri's NSIS bundler supports the language, add
   `src-tauri/installer/<Language>.nsh` (a translated copy of `English.nsh`)
   and list it in `bundle.windows.nsis.languages` and `customLanguageFiles` in
   `src-tauri/tauri.conf.json`, and in `scripts/installer.test.mjs`.
5. Update the table on this page and in [Settings](settings.md#language).
