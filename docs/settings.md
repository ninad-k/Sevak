# Settings window

Configure Sevak through the Settings window.

## Open Settings

- Click the tray icon (Windows) or menu-bar icon (macOS), or right-click it and
  select **Settings** (on Linux, click the icon and select **Settings**)
- Run `sevak --settings` from the command line

The tabs run from General to Files, with Linux where it applies, then **Backup & restore** and **Help** last.

## Using Settings

Each tab covers a different aspect of Sevak. Changes are **not saved** until you click **Save**. Click **Close** or **Cancel** to discard them.

| Control | Result |
|---|---|
| ++ctrl+s++ | Save settings |
| ++arrow-up++/++arrow-down++ on a tab button | Move between tabs |
| ++home++/++end++ on a tab button | Jump to first or last tab |

Validation errors appear under each field and prevent saving until they are fixed. A red dot on a tab says that the page has a problem. Keywords are checked as you type: Sevak tells you when one is already used by a web search engine, a fixed keyword (`>`, `cb`, `s`, `emoji`, `:`, `@`, `uuid`) or another search, and both fields of a clash are marked.

Saving keeps your comments, the order of your keys and every `[[snippet]]` entry exactly as they are in `config.toml`; only the lines you changed are rewritten.

Pages: [General](#general), [Hotkeys](#hotkeys), [Appearance](#appearance), [Search](#search), [Plugins](#plugins), [Workflows](#workflows), [Gallery](#gallery), [Web search](#web-search), [Files & bookmarks](#files-bookmarks), [Clipboard & paste](#clipboard-paste), [Tasks & media](#tasks-media), [Integrations](#integrations), [AI assistant](#ai-assistant), [System & terminal](#system-terminal), on Linux [Linux](#linux), [Backup & restore](#backup-restore) and [Help](#help).

## General

### Shortcut

The global hotkey to show and hide Sevak. Tap it to toggle the launcher from anywhere.

The default is ++super+space++ (shown as ++win+space++ on Windows and ++cmd+space++ on macOS). Examples: ++alt+space++, ++ctrl+space++, ++ctrl+shift+k++.

Three ways to set it:

- **Record** the keys. On Windows the recorder also sees ++win+space++, which Windows normally keeps from apps.
- Pick one from the **Presets** list: Super+Space, Alt+Space, Ctrl+Space or Ctrl+Alt+Space.
- **Type** it: ++win++, ++windows++, ++meta++, ++cmd++ and ++super++ all mean the Windows/Command key, and Sevak saves it as `Super`.

Under the field Sevak says how the key is delivered: registered the normal way, taken over with the Windows keyboard hook, Spotlight's shortcut turned off with your permission, GNOME's input-source shortcut moved, or Option+Space used because you kept Spotlight's. When Sevak changed a system shortcut with your permission, a **Restore** button puts it back; when the system shortcut is in the way, **Let Sevak use ...** asks. See [Win+Space, Cmd+Space and Super+Space](troubleshooting.md#super-space).

Another app's or your desktop's shortcut can stand in the way of a key; the status under the field says so.

On **Linux Wayland**, the desktop manages global hotkeys. If the field won't register, click **Set up GNOME shortcut** to create a desktop binding, or run `sevak --setup-hotkey "Your+Combo"`.

Links to: `[general] hotkey` in configuration.

### Universal Actions shortcut

Global hotkey to capture and act on what you have selected in another app: text, a link, or files.

Leave **empty** to turn off Universal Actions.

On **Wayland**, use the same **Set up GNOME shortcut** button.

Links to: `[general] actions_hotkey` in configuration.

### Accept shortcuts sent by other programs

Windows only. Off by default: Sevak's keyboard hook then ignores key presses that another program sends, so nothing else on your desktop can open Sevak or trigger Universal Actions by pressing the shortcut for you. Turn it on if AutoHotkey, PowerToys Keyboard Manager or a similar remapper is meant to type the shortcut. The change applies when you save.

Links to: `[general] accept_injected_hotkeys` in configuration.

### Use the clipboard if the selection can't be read

**Wayland**, terminal windows and some apps prevent Sevak from reading the selection. Turn this on to act on the clipboard instead when the direct method fails.

Links to: `[actions] use_clipboard_fallback` in configuration.

### Read highlighted text first (X11)

Linux X11 only. Universal Actions reads the PRIMARY selection (the text you have highlighted) before it presses ++ctrl+c++. Turn it off to always copy with the keyboard. Hidden on Wayland, Windows and macOS, where there is no PRIMARY selection.

Links to: `[actions] use_primary_selection` in configuration.

### Hide when focus is lost

Automatically close the launcher when you click outside it or switch to another window.

Links to: `[general] hide_on_blur` in configuration.

### Start Sevak when I sign in

Turn this on under **General**, then click **Save**. Sevak starts in the
background after you sign in to Windows, macOS or a Linux desktop, including
after a restart. Use your shortcut to open the launcher; signing in does not
open the search window. Turn the option off and **Save** to stop automatic starts.

This is off on a fresh install and applies only to your account, even when the
Windows app is installed for everyone. The Windows installer checkbox controls
the same preference; **Start Sevak now** is a separate, one-time action.

Sevak reports errors if it cannot register startup, rather than reporting a
successful save. It also shows a warning when your saved opt-in has been
disabled outside Sevak or the startup entry is missing. Windows exposes the
entry under **Settings → Apps → Startup** (also **Task Manager → Startup apps**).
macOS may restrict background items in **System Settings → General → Login
Items**; Linux desktops have their own startup application settings. Turning
startup off in your OS is respected when Sevak next starts or you save unrelated
settings. To re-enable it, use the OS control or turn this option off and **Save**,
then on and **Save** again.

Install Sevak in its permanent location first. For an AppImage, keep that file
in the same folder so the startup entry can find it. A macOS `.dmg` and Linux
packages have no Windows-style setup checkbox: launch the installed app and
use this setting. If an organization manages startup policy, it can prevent
automatic starts even when the preference is on.

Links to: `[general] launch_at_login` in [Configuration](configuration.md#general)
and [installation instructions](install.md#start-sevak-after-sign-in).

### Check for updates

Look for a new version at startup and once per day. If an update is available, you are asked before it is installed.

Links to: `[general] check_for_updates` in configuration.

### Update channel

**Stable** (the default) offers regular releases. **Beta** also offers pre-release builds (`1.3.0-beta.2`), which arrive
earlier and may be less tested. Both are downloaded from the same GitHub releases page and
checked with the same update signature.

Going back from Beta to Stable never downgrades: Sevak keeps the version you have and
offers the next stable version that is newer. If you want to leave a beta right away,
reinstall the stable version from the [Releases page](https://github.com/ninad-k/Sevak/releases).

Links to: `[general] update_channel` in configuration.

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
- **Files** (indexed folders; also see [Files & bookmarks](#files-bookmarks))
- **Bookmarks** (browser bookmarks; options under [Files & bookmarks](#files-bookmarks))
- **Clipboard history** (search recent copied text, images and files; also needs **Keep a clipboard history** on the [Clipboard & paste](#clipboard-paste) page)
- **Snippets** (saved text templates)
- **Emoji picker** (`:heart`, `emoji heart`)
- **System commands** (lock, sleep, settings, etc.; options under [System & terminal](#system-terminal))
- **Automation tasks** (dark mode, volume, quit, kill, eject, keep awake…; options under [Tasks & media](#tasks-media))
- **Media controls** (play, pause, next, now playing; options under [Tasks & media](#tasks-media))
- **Terminal commands** (type `> command` to run in a terminal; options under [System & terminal](#system-terminal))
- **Selection** (Universal Actions)
- **Contacts** and **1Password** (also need their own switch on the [Integrations](#integrations) page)
- **Dictionary** (`define`, `spell`; options under [Integrations](#integrations))
- **AI assistant** (`ai <question>`; also needs its own switch on the [AI assistant](#ai-assistant) page)
- **Script plugins** and **workflows** you installed
- **Web search engines** (see Web search tab)

Each plugin's own options are on the page named next to it. A plugin switched off here stays off, whatever its page says: its page shows a note when that is the case.

### Currency conversion

Optional: download the European Central Bank's daily exchange rates to convert currencies.

When enabled, `100 usd in eur` works in the calculator. Rates are cached locally for 24 hours. Disable to keep Sevak offline.

Links to: `[calculator] currency` in configuration and [Privacy](privacy.md#currency-conversion).

### Expand snippets as you type

Off by default. When on, typing a snippet's keyword in any app replaces it with the snippet's text. While it is on, Sevak watches your keystrokes for the keyword (the last few characters, in memory only). When it is on, two more fields appear:

- **Keyword prefix**: typed before every keyword, such as `;`, so that `;sig` expands and a plain `sig` does not.
- **Expand**: as soon as the keyword is typed, or after a space or punctuation mark.
- **Expand in web browsers**: off by default, because a password field in a web page cannot be told from other text boxes.

A warning under the switch says what is missing (macOS Input Monitoring permission, Wayland). Three more fields appear:

- **Match case**: off lets `SIG` and `sig` both expand.
- **Expand in terminals**: terminal windows are skipped unless this is on.
- **Never expand in these apps**: a list of program or app names (any case), such as `KeePassXC`. Type a name and press ++enter++ or **Add**; click the **×** to remove one.

Links to: `[snippets]` in [configuration](configuration.md#snippets) and [Expand snippets as you type](features/snippets.md#expand-snippets-as-you-type).

## Workflows

Lists every [workflow](workflows.md) with a switch to turn it on or off, **Edit**, **Delete**, and **Review…** for one that waits for your permission. **New workflow** starts a blank one in the visual builder; **New from template…** offers three starting points. See [The builder](workflows.md#the-builder).

Workflows are saved as files in the `workflows` folder next to `config.toml`, not in `config.toml`, so they are saved by the builder's own **Save**, not the Settings **Save** button.

## Gallery

Lists ready-made workflows and script plugins. Opening the page requests nothing: **Load gallery** downloads the list, **Install** downloads one package and checks its checksum, and the installed folder asks for permission before anything in it runs. See [The gallery](workflows.md#the-gallery).

## Extensions

Browse the gallery (workflows, script plugins, native extensions written in Rust, and themes), install, update, switch off and remove, with an indicator on **Installed** when an update is available. Opening the page requests nothing: **Load the list** downloads the two lists once, **Install** downloads one package and checks its checksum, and a new workflow, plugin or native extension asks for permission before anything in it runs. Works from the launcher too: `ext <name>` or `store <name>`. See [Extensions](features/extensions.md).

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

## Files & bookmarks

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

### Allow network paths

Windows only. Use paths on other computers (`\\server\share`) and mapped network drives. Off, Sevak shows "Network paths are turned off" instead of looking at them, because Windows signs in to a computer as soon as anything looks at its path. Folders in the list above that are on a share are skipped while this is off.

Default: off.

Links to: `[files] allow_network_paths` in configuration.

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

### File buffer

**Keep the buffer when Sevak hides**: the [file buffer](features/files.md#file-buffer) (files you collect with ++alt+arrow-down++ to act on together) is emptied whenever the launcher hides. Turn this on to keep it.

Links to: `[file_buffer] keep_between_shows` in configuration.

### Bookmarks

- **Keyword**: type "keyword term" to search only bookmarks. Default `b`; empty removes the keyword.
- **Show in ordinary searches**: also list matching bookmarks for plain searches.
- **Browsers**: a checklist of Chrome, Edge, Brave, Vivaldi, Chromium, Opera, Opera GX, Firefox, LibreWolf and Zen. **None ticked reads every browser Sevak finds.** All profiles of a ticked browser are read.

Bookmarks are read from disk when you search; nothing is sent anywhere. An id in `config.toml` that the checklist does not know is kept and shown as a chip you can remove.

Links to: `[bookmarks]` in [configuration](configuration.md#bookmarks) and [Bookmarks](features/bookmarks.md).

## Clipboard & paste

### Keep a clipboard history

Off by default. When on, Sevak watches the clipboard and keeps what you copy so that `cb` can paste it back. Only what you copy after you turn it on is remembered.

!!! warning "Privacy"
    The history is stored on this computer, in Sevak's local data folder: text and the paths of copied files in `clipboard-history.json`, images as PNG files in the `clipboard` folder. On Windows the files are encrypted for your account (see **Encrypt the history** below); on macOS and Linux they are plain and readable only by you. Nothing is sent anywhere. Content that a password manager marks as secret is never recorded, and neither is anything copied in a well-known password manager (see **Skip password managers**); apps that set no marker belong under **Ignore apps**.

If **Clipboard history** is switched off under **Plugins**, the page says so: `cb` then shows nothing whatever this switch says.

Links to: `[clipboard] enabled` in configuration and [Clipboard history](features/clipboard.md).

### What to keep

These apply while the history is on.

| Field | Range | Default | Notes |
|---|---|---|---|
| **Number of items** | 1–5000 | 200 | Older entries are dropped with their image files. |
| **Longest text** | up to 4096 KB | 64 KB | Longer text is not recorded. |
| **Encrypt the history** (Windows) | on/off | on | Encrypts the history file and the images for your Windows account (DPAPI). |
| **Record images** | on/off | on | Copied pictures, saved as PNG files. |
| **Largest image** | up to 64 MB | 10 MB | A picture whose PNG is larger is not recorded. |
| **Record files** | on/off | on | Only the paths of copied files and folders are kept. |
| **Skip password managers** | on/off | on | Never record copies made in KeePass, 1Password, Bitwarden and similar apps, or in system credential and passphrase prompts. |
| **Ignore apps** | list | empty | Copies made in these apps are never recorded (program or app names, any case). |

The sizes are shown in KB and MB and written to `config.toml` in bytes.

Links to: `[clipboard]` in [configuration](configuration.md#clipboard).

### Clear clipboard history

**Clear history…** asks once more (**Delete everything**) and then deletes every saved entry and the image files. It works at once and is not part of **Save**; it also deletes what an earlier run left behind when the history is off now. It cannot be undone.

### Put the clipboard back after pasting

After pasting a clipboard entry or a snippet, restore what was on the clipboard before. Off leaves the pasted text there. A note under it says what pasting needs on your system (macOS Accessibility permission; administrator windows on Windows; Wayland).

Links to: `[paste] restore_clipboard` in configuration.

## Tasks & media

### Automation tasks

[Automation tasks](features/tasks.md) are ready-made actions: dark mode, volume, screenshots, quit or kill an app, eject a drive and more.

- **Ask before risky tasks**: confirm before force quitting an app, ending a process and restarting Explorer or Finder.
- **Keyword**: type the keyword and a space to list the tasks. Default `t`; empty removes it.
- **Show in ordinary searches**: also match task names in plain searches, such as `dark mode` or `kill chrome`.
- **Tasks to offer**: a checklist of every task. Untick one to hide it. Only the tasks that work on your computer are offered at all.

A note says what your system needs (Windows: allow desktop apps under *Privacy & security, Radios* for Wi-Fi and Bluetooth; macOS: System Events and Accessibility permission; Linux: helper programs such as `wmctrl` and `nmcli`).

Links to: `[tasks]` in [configuration](configuration.md#tasks).

### Media controls

[Media controls](features/media.md) press play, pause, next and previous for whatever is playing.

- **Keyword**: type the keyword and a space to list the buttons and the track. Default `play`; empty removes it.
- **Show in ordinary searches**: also match `pause`, `next track` and so on in plain searches.
- **Show what is playing**: a row with the title, artist and app. It is read from your media player when you search and never stored or sent anywhere.

On **Linux** the buttons are not offered until `playerctl` is installed, and the page says so. On macOS only Music and Spotify report the track.

Links to: `[media]` in [configuration](configuration.md#media).

## Integrations

### Contacts

Off by default. Search your address book with `c` or `@`.

- **Search contacts**: the on switch.
- **Keyword**: default `c`; the `@` keyword always works too. It cannot be empty.
- **Read the system address book**: also search the Contacts app (macOS: the first search asks for permission), the Windows People store, or Evolution's local address books (Linux).
- **vCard files and folders**: `.vcf` files and folders of them, which work everywhere without a permission. **Add file…** and **Add folder…** open a picker; you can also type a path such as `~/contacts.vcf` and press ++enter++.

Contacts are read into memory only and are never written to disk, logged or sent anywhere. Searches in it stay out of the search history.

Links to: `[contacts]` in [configuration](configuration.md#contacts) and [Contacts](features/contacts.md).

### 1Password

Off by default. Find a login by title or website with `1p`. Sevak uses the official `op` command-line tool and never reads a password, one-time code or note.

- **Search 1Password logins**: the on switch.
- **Keyword**: default `1p`. It cannot be empty.
- **Path to op**: empty looks on `PATH` and in the usual install folders. **Browse…** picks the program.
- **Account**: which account to use when several are signed in (its address, short name or ID). Empty uses `op`'s default.
- **Keep the list of logins**: 1–1440 minutes (default 10) in memory before it is refreshed. A refresh may ask you to unlock 1Password.

**Needs the 1Password CLI**: install `op` and turn on *Settings, Developer, Integrate with 1Password CLI* in the 1Password app. Sevak asks `op` only for titles, vault names, websites and usernames, and keeps that list in memory.

Links to: `[onepassword]` in [configuration](configuration.md#onepassword) and [1Password](features/1password.md).

### Dictionary and spelling

On by default (switch it off under **Plugins**). Everything is offline.

- **Definitions keyword**: default `define`. It cannot be empty.
- **Spelling keyword**: default `spell`. It cannot be empty.
- **Prefer the system dictionary**: use the macOS Dictionary and the Windows spell checker where there is one. Linux has none Sevak can ask, so it always uses the bundled English dictionary. Off always uses the bundled one.

Links to: `[dictionary]` in [configuration](configuration.md#dictionary) and [Dictionary](features/dictionary.md).

## AI assistant

Off by default. Type `ai ` and a question; Enter sends it to the service you chose and shows the answer. The page begins with a plain statement of **what is sent and to which host**, which follows the settings as you edit them.

- **Use the AI assistant**: the on switch. Nothing is sent while it is off.
- **Keyword**: default `ai`. It cannot be empty.
- **Provider**: Ollama (a model on this computer, the default), OpenAI-compatible (ChatGPT / OpenAI, or any server with the same API) or Anthropic.
- **Model** and **Base URL**: empty uses the defaults shown in grey. Changing the provider carries over only values you typed yourself.
- **API key**: write-only. Paste it and press **Save key**; it is stored at once (it is not part of **Save**) and never shown again. **Remove** deletes it. The line under the field says where the key in use comes from. Keys are never written to `config.toml`.
- **System prompt**, **Longest answer** (tokens) and **Timeout** (seconds).
- **Test connection**: checks the address, the key and the model with the values on the page, saved or not. It lists the provider's models and sends no question.

Links to: `[ai]` in [configuration](configuration.md#ai) and [AI assistant](ai.md).

## System & terminal

### System commands

[System commands](features/system.md) lock, sleep, restart and shut down, and open settings pages.

- **Ask before destructive commands**: confirm restart, shut down, log out and emptying the trash. This also applies to a hotkey bound to one of them.
- **Commands to offer**: a checklist of the commands and of every settings page. Untick **All settings pages** to hide them all. Only the commands that exist on your computer are offered at all.

Links to: `[system]` in [configuration](configuration.md#system).

### Terminal commands

[Shell commands](features/shell.md): type `> command` and press ++enter++ to run it in a terminal. Nothing runs until you press ++enter++.

- **Terminal**: a program name or full path, optionally with arguments (`wt`, `iterm`, `kitty --class sevak`). Empty detects one. **Browse…** picks the program.
- **Shell**: the program that runs the command. Empty detects one. Not used on macOS, where the terminal starts your login shell.
- **Keep the terminal open**: leave it at a shell prompt after the command exits; off closes it.

Links to: `[shell]` in [configuration](configuration.md#shell).

## Linux

Wayland-specific options (appears only on Wayland sessions).

### Use XWayland on Wayland

Native Wayland windows cannot position themselves or reliably receive focus, so Sevak draws through XWayland to stay centered and typeable.

Turn this off to use the native Wayland backend (experimental; you may need to position the window manually).

**Restart Sevak to apply.**

Links to: `[linux] wayland_use_xwayland` in configuration.

## Backup & restore

Save your settings, snippets, web searches, themes, script plugins and workflows to one file, and restore them later or on another computer. This page is not part of the Save button: everything on it acts when you click its own button, and a restore reloads the other pages. The full guide is [Backup and restore](backup-and-restore.md).

| Part | What it does |
|---|---|
| **Back up** | Tick the categories (all by default), see exactly what is included, then **Save backup as…** (a save dialog), **Back up now** (the backup folder, no dialog) or **Open backup folder**. Shows when and where the last backup was made. Says what is never included (keys, history, the list of allowed scripts) and that the file is not encrypted. |
| **Restore** | **Choose a backup…**, tick the categories, pick **Merge** or **Replace**, read what would change (new, changed, same and removed counts with names and files) and click **Restore**. A safety copy is taken first and the restore is all or nothing. |
| **Undo restore** | Puts back what the last restore replaced. |
| **Automatic backups** | Off by default. Daily or weekly, and optionally when Sevak is updated; keeps the newest 1-50; the folder is yours to choose. Saved to `backup.toml`. |

Scripts and workflows that a restore adds or changes ask for your approval again.

## Help

Make a **diagnostics report** for a bug report. Sevak has no telemetry, so this is how you tell the maintainers what your installation looks like.

The page makes the report as soon as you open it and shows it in a read-only box, exactly as it will be copied or saved. Read it first: the top says what is included and what is not, and the log lines at the end are the part most worth a glance.

| Button | Action |
|---|---|
| **Copy diagnostics** | Puts the text in the box on the clipboard; paste it into the **Diagnostics report** box of the [bug report form](https://github.com/ninad-k/Sevak/issues/new/choose) |
| **Save as file…** | Asks where to save the text (`sevak-diagnostics.md` by default) |
| **Open logs folder** | Shows the folder with the raw log files, for when more than the last 100 lines is needed |
| **Refresh** | Makes the report again |

Nothing is sent anywhere; copying and saving are the only things that leave the box. The report is the fuller twin of [`sevak --diagnostics`](cli.md#-diagnostics): it also says whether the shortcut is registered, whether the tray icon exists and which plugins loaded. See [Privacy](privacy.md#diagnostics-report) for exactly what it contains.

## Footer buttons

| Button | Action |
|---|---|
| **Open config file** | Opens `config.toml` in your default editor |
| **Reveal logs folder** | Shows the folder containing Sevak's debug logs (also on the Help page as **Open logs folder**) |
| **Save** | Save and apply all changes (only enabled when valid) |
| **Cancel** / **Close** | Close Settings without saving (or discard changes if you made any) |

Validation errors (shown in red) must be fixed before you can save.
