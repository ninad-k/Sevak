# Settings window

Configure Sevak through the Settings window.

## Open Settings

- Click the tray or menu-bar icon and select **Settings**
- Run `sevak --settings` from the command line

## Using Settings

Each tab covers a different aspect of Sevak. Changes are **not saved** until you click **Save**. Click **Close** or **Cancel** to discard them.

| Control | Result |
|---|---|
| ++ctrl+s++ | Save settings |
| ++arrow-up++/++arrow-down++ on a tab button | Move between tabs |
| ++home++/++end++ on a tab button | Jump to first or last tab |

Validation errors appear under each field and prevent saving until they are fixed.

## General

### Shortcut

The global hotkey to show and hide Sevak. Tap it to toggle the launcher from anywhere.

Examples: ++alt+space++, ++ctrl+space++, ++ctrl+shift+k++. The key combination must not be in use by another app or your desktop.

On **Linux Wayland**, the desktop manages global hotkeys. If the field won't register, click **Set up GNOME shortcut** to create a desktop binding, or run `sevak --setup-hotkey "Your+Combo"`.

Links to: `[general] hotkey` in configuration.

### Universal Actions shortcut

Global hotkey to capture and act on what you have selected in another app: text, a link, or files.

Leave **empty** to turn off Universal Actions.

On **Wayland**, use the same **Set up GNOME shortcut** button.

Links to: `[general] actions_hotkey` in configuration.

### Use the clipboard if the selection can't be read

**Wayland**, terminal windows and some apps prevent Sevak from reading the selection. Turn this on to act on the clipboard instead when the direct method fails.

Links to: `[actions] use_clipboard_fallback` in configuration.

### Hide when focus is lost

Automatically close the launcher when you click outside it or switch to another window.

Links to: `[general] hide_on_blur` in configuration.

### Launch at login

Start Sevak in the background when you sign in.

Links to: `[general] launch_at_login` in configuration.

### Check for updates

Look for a new version at startup and once per day. If an update is available, you are asked before it is installed.

Links to: `[general] check_for_updates` in configuration.

## Hotkeys

Define custom global hotkeys that open Sevak with text already typed, or run a result without showing the launcher.

### Adding a hotkey

Click **Add hotkey** and fill in:

- **Key**: the shortcut (e.g., ++ctrl+alt+t++)
- **Query** or **Run**: either type text to pre-fill the search bar, or give a result ID to run immediately

### Examples

| Hotkey | Action | Result |
|---|---|---|
| ++ctrl+alt+t++ | Query `> ` | Opens Sevak with `> ` typed (ready for a shell command) |
| ++ctrl+alt+l++ | Run `system:lock` | Locks the screen without showing the launcher |

### Finding result IDs

Hover over any result for a moment to see its ID. Examples:
- `apps:firefox.desktop` (Linux app)
- `apps:Mozilla Firefox` (Windows app)
- `system:lock`, `system:settings:bluetooth`
- `files:/Users/me/Documents`
- `shell:git status`

Calculator, web search and clipboard history results have no stable ID and cannot be bound.

**Wayland**: custom hotkeys are registered as GNOME shortcuts. Delete them in GNOME's keyboard settings when you remove an entry.

Links to: `[[hotkey]]` in [configuration](configuration.md#hotkey).

## Appearance

### Theme

- **System**: follow your operating system's light/dark setting
- **Light**: always light
- **Dark**: always dark

Links to: `[appearance] theme` in configuration.

### Window width

The width of the search bar in pixels. Range: 400–1600 px.

Links to: `[window] width` in configuration.

### Accent color

Highlight color for buttons, toggles and the caret. Format: `#rgb`, `#rrggbb`, or `rgb(r, g, b)`. Leave empty to use the theme's default.

Examples: `#7c3aed`, `#fa0`, `rgb(124, 58, 237)`.

Links to: `[appearance] accent` in configuration.

### Font size

Size of the result titles in pixels. Range: 12–22 px. The rest of the search bar scales with it.

Links to: `[appearance] font_size` in configuration.

### Font family

Font for the launcher. Leave empty to use your system's default. Examples: `Fira Sans, sans-serif`, `Monospace`.

Links to: `[appearance] font_family` in configuration.

### Opacity

Background opacity of the search bar as a percentage. Range: 30–100%. Text stays fully opaque.

Links to: `[appearance] opacity` in configuration.

### Frosted-glass blur

Blurs the desktop behind the search bar (Windows and macOS; hidden on Linux). Lower **Background opacity** to see it. Links to: `[appearance] blur` in configuration.

### Corner radius

Rounding of the search bar's corners in pixels. Range: 0–32 px (0 = sharp corners).

Links to: `[appearance] radius` in configuration.

### Theme editor and gallery

**Theme editor** creates and edits theme files with a live preview: eight built-in themes (Sevak Light, Sevak Dark, Nord, Dracula, Solarized Light, Solarized Dark, Gruvbox and High Contrast), color pickers, sliders for sizes, a WCAG contrast check, undo and redo, **Save as…**, **Apply**, **Import…** and **Export…**. **Open themes folder** shows the `themes` folder next to `config.toml`.

**Browse online themes** loads the community theme gallery. Nothing is requested until you click it, and **Install** saves a theme only if its checksum matches.

Links to: `[appearance] theme_file` in configuration and [Theme files and the editor](themes.md#theme-files-and-the-editor).

### Custom stylesheet

A CSS file in your config folder to override theme variables. Example: `theme.css`. Leave empty to use none.

Sevak's CSS variables are listed in [Themes and appearance](themes.md#css-variables). Your stylesheet is loaded after the built-in theme, so it overrides everything.

Links to: `[appearance] custom_css` in configuration and [Themes guide](themes.md).

## Search

### Maximum results

How many rows the result list shows. Range: 1–20.

Links to: `[search] max_results` in configuration.

### Fallback web search

Offered when a plain query (not a web keyword) produces no results. Choose from your defined web search engines, or **None** to turn it off.

Examples: If set to `g`, a query like `rust traits` that has no app or file results shows a Google search row.

config.toml can list several fallbacks; the dropdown shows them. If you pick a different one here, it overwrites the list.

Links to: `[search] fallback_web_search` in configuration.

### Remember searches

On an empty search bar, press ++arrow-up++ and ++arrow-down++ to step through your last 50 searches, newest first.

Disabling this also deletes the stored search history.

Links to: `[search] query_history` in configuration.

## Plugins

Enable or disable built-in result sources:

- **Applications** (Windows Start Menu shortcuts and packages, macOS app bundles, Linux desktop entries)
- **Calculator** (arithmetic, unit conversion, currency)
- **Files** (indexed folders; also see Files tab)
- **Bookmarks** (browser bookmarks; also see Bookmarks section in configuration)
- **Clipboard history** (if enabled; search recent copied text, images and files)
- **Snippets** (saved text templates)
- **Emoji picker** (`:heart`, `emoji heart`)
- **System commands** (lock, sleep, settings, etc.)
- **Automation tasks** (dark mode, volume, quit, kill, eject, keep awake…)
- **Media controls** (play, pause, next, now playing)
- **Shell** (type `> command` to run in a terminal)
- **Selection** (Universal Actions)
- **Contacts** and **1Password** (also need `enabled = true` in `config.toml`)
- **Dictionary** (`define`, `spell`)
- **Script plugins** and **workflows** you installed
- **Web search engines** (see Web search tab)

Contacts, 1Password, the dictionary, automation tasks and media controls have no fields of their own here; edit their sections in `config.toml` ([configuration](configuration.md)).

### Currency conversion

Optional: download the European Central Bank's daily exchange rates to convert currencies.

When enabled, `100 usd in eur` works in the calculator. Rates are cached locally for 24 hours. Disable to keep Sevak offline.

Links to: `[calculator] currency` in configuration and [Privacy](privacy.md#currency-conversion).

### Expand snippets as you type

Off by default. When on, typing a snippet's keyword in any app replaces it with the snippet's text. While it is on, Sevak watches your keystrokes for the keyword (the last few characters, in memory only). When it is on, two more fields appear:

- **Keyword prefix**: typed before every keyword, such as `;`, so that `;sig` expands and a plain `sig` does not.
- **Expand**: as soon as the keyword is typed, or after a space or punctuation mark.

A warning under the switch says what is missing (macOS Input Monitoring permission, Wayland). The other options (`case_sensitive`, `ignore_apps`, `expand_in_terminals`) are in `config.toml`.

Links to: `[snippets]` in [configuration](configuration.md#snippets) and [Expand snippets as you type](features/snippets.md#expand-snippets-as-you-type).

## Workflows

Lists every [workflow](workflows.md) with a switch to turn it on or off, **Edit**, **Delete**, and **Review…** for one that waits for your permission. **New workflow** starts a blank one in the visual builder; **New from template…** offers three starting points. See [The builder](workflows.md#the-builder).

Workflows are saved as files in the `workflows` folder next to `config.toml`, not in `config.toml`, so they are saved by the builder's own **Save**, not the Settings **Save** button.

## Gallery

Lists ready-made workflows and script plugins. Opening the page requests nothing: **Load gallery** downloads the list, **Install** downloads one package and checks its checksum, and the installed folder asks for permission before anything in it runs. See [The gallery](workflows.md#the-gallery).

## Web search

Define keywords and URL templates for web search.

### Adding an engine

1. Click **Add engine**
2. Fill in:
   - **Keyword**: what you type (e.g., `g`, `yt`, `gh`)
   - **Name**: displayed name (e.g., `Google`, `YouTube`)
   - **URL**: search URL with `{query}` placeholder (e.g., `https://www.google.com/search?q={query}`)

Type the keyword and your search terms to use it: `g rust traits` opens Google with those terms.

### Example engines

| Keyword | Name | URL |
|---|---|---|
| `g` | Google | `https://www.google.com/search?q={query}` |
| `yt` | YouTube | `https://www.youtube.com/results?search_query={query}` |
| `gh` | GitHub | `https://github.com/search?q={query}` |

### Default engines

Sevak starts with Google, YouTube and GitHub. You can remove them and add your own.

Links to: `[[web_search]]` in [configuration](configuration.md#web_search).

## Files

### Folders to search

Directories and their subdirectories that are indexed and searchable.

Click **Add folder…** to browse and select. Click the **×** to remove.

The index is capped at 100,000 entries. Keep roots focused on your active projects.

Links to: `[files] directories` in configuration.

### Folder depth

How many levels below each folder are indexed. Range: 0 (just the folder itself) to unlimited.

Default: 4.

Links to: `[files] max_depth` in configuration.

### Include hidden files

Index dot-files and dot-folders (e.g., `.git`, `.env`).

Default: off.

Links to: `[files] include_hidden` in configuration.

### Keyword

Type "keyword filename" to search only files. Leave empty to disable.

Default: `f`.

Links to: `[files] keyword` in configuration and [configuration guide](configuration.md#files).

### Show in global results

Also include matching files in plain searches, ranked lower than apps and bookmarks.

Default: on.

Links to: `[files] global` in configuration.

### Search the whole disk

Turns `ff` (file names anywhere) and `in` (words inside files) on or off. Both ask your computer's own file index (Windows Search, Spotlight, `locate`, Tracker or Baloo); nothing leaves your computer. When on, two fields set the keywords:

- **Whole-disk keyword**: default `ff`. Empty turns off name search.
- **Contents keyword**: default `in`. Empty turns off content search.

Every keyword must be one word and differ from all other keywords; Settings refuses to save a clash.

Links to: `[files] use_os_index`, `index_keyword` and `content_keyword` in configuration, and [Whole-disk and content search](features/files.md#whole-disk-and-content-search).

## Linux

Wayland-specific options (appears only on Wayland sessions).

### Use XWayland on Wayland

Native Wayland windows cannot position themselves or reliably receive focus, so Sevak draws through XWayland to stay centered and typeable.

Turn this off to use the native Wayland backend (experimental; you may need to position the window manually).

**Restart Sevak to apply.**

Links to: `[linux] wayland_use_xwayland` in configuration.

## Footer buttons

| Button | Action |
|---|---|
| **Open config file** | Opens `config.toml` in your default editor |
| **Reveal logs folder** | Shows the folder containing Sevak's debug logs |
| **Save** | Save and apply all changes (only enabled when valid) |
| **Cancel** / **Close** | Close Settings without saving (or discard changes if you made any) |

Validation errors (shown in red) must be fixed before you can save.
