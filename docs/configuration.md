# Configuring Sevak

[← Help center](README.md) · [User guide](usage.md) · [Troubleshooting](troubleshooting.md)

## Start in Settings

Open **Settings** from the tray/menu bar, or run `sevak --settings`.
Make your changes and click **Save**. Settings validates the form before saving;
the running app then reloads the saved configuration.

![The actual Sevak Settings interface with example configuration](media/sevak-settings.png)

| Section | Controls |
|---|---|
| General | Launcher shortcut, hide-on-blur, launch at login, update checks |
| Hotkeys | Extra global keys that type a query or run a result |
| Appearance | Theme, launcher width, accent color, font, size, opacity, radius, custom stylesheet |
| Search | Result limit, fallback web engines, search history |
| Plugins | Available result sources (including script plugins) and currency conversion |
| Web search | Keywords, provider names and URL templates |
| Files | Indexed folders, depth, dot-files and global file results |
| Linux | Wayland-related options when applicable |

## Editing TOML

Sevak creates `config.toml` on first launch. Use **Open config file** in Settings
or find it in the table below. After manual edits, choose **Reload index**
from the tray or restart Sevak.

You can keep a small config: missing fields use defaults. Settings preserves
existing comments when saving. Back up the file before a large manual edit.

## Common recipes

### Change the shortcut

```toml
[general]
hotkey = "Ctrl+Space"
```

Choose a combination not already used by your desktop or another app. On
macOS, Option is the Alt key. On Wayland, also update the desktop binding;
changing this field alone cannot register a global shortcut.

[Wayland instructions](install.md#setting-up-the-hotkey-on-linux).

### Search a project folder

```toml
[files]
directories = ["~/Documents", "~/Projects"]
max_depth = 4
include_hidden = false
keyword = "f"
global = true
```

Setting `directories` replaces the root list, so keep any default folders you
still want. `~` expands to your home directory on each platform. Forward
slashes work in the portable examples; avoid unescaped backslashes inside
TOML double-quoted Windows paths.

Search matches filenames and folder names, not document contents. Some cache
and generated directories are always pruned; see [file search](usage.md#files-and-folders).
Set `global = false` to show file results only through `f <name>`.

### Choose dark mode and fewer results

```toml
[appearance]
theme = "dark"

[search]
max_results = 6
fallback_web_search = "g"
```

Themes are `system`, `light` and `dark`. The result limit is 1–20.
Set `fallback_web_search = ""` to turn off the no-match web suggestion, or
list several engines to offer them in order: `["g", "yt", "gh"]`.

### Restyle the launcher

```toml
[appearance]
accent = "#7c3aed"     # "#fa0", "#7c3aed" or "rgb(124, 58, 237)"; "" = the theme's
font_size = 15         # 12–22 px; the bar scales with it
font_family = ""       # "Fira Sans, sans-serif"; "" = the system font
opacity = 100          # 30–100, percent opacity of the bar's background
radius = 14            # 0–32 px
custom_css = ""        # "theme.css", a stylesheet in the config folder
```

Invalid values fall back to their defaults with a warning in the log and in
Settings → Appearance. `opacity` and `radius` are whole numbers. The
[themes guide](themes.md) lists the CSS variables a stylesheet can override.

### Custom hotkeys

Each `[[hotkey]]` has a `key` and exactly one of `query` or `run`:

```toml
[[hotkey]]
key = "Ctrl+Alt+T"
query = "> "                  # open Sevak with this text typed in

[[hotkey]]
key = "Ctrl+Alt+L"
run = "system:lock"           # run this result; Sevak stays hidden
```

`run` takes a result id; hover over a result for a moment to see it. Examples:
`apps:firefox.desktop` (Linux desktop-file id), `apps:<Start Menu path or
AppUserModelID>` (Windows), `files:<full path>`, `system:lock`,
`system:settings:bluetooth`, `snippets:<name>`, `shell:<command>`, or a
bookmark's id. Restart, shut down, log out and empty trash still ask first.
Calculator, web search and clipboard history results have no stable id and
cannot be bound. Keys that cannot be registered are skipped and listed with the
reason under Settings → Hotkeys. On Wayland, `sevak --setup-hotkey` creates a
GNOME shortcut per entry; delete it in GNOME's keyboard settings after removing
an entry.

### System commands

```toml
[system]
confirm = true         # ask before restart, shut down, log out, empty trash
disabled = []          # "lock", "sleep", "hibernate", "restart", "shutdown",
                       # "logout", "empty_trash", "settings", "settings:<page>"
```

### Terminal commands

```toml
[shell]
terminal = ""          # "" = auto-detect; e.g. "wt", "conhost", "iterm", "kitty", "alacritty --class sevak"
shell = ""             # "" = pwsh/powershell/cmd on Windows, $SHELL elsewhere
keep_open = true       # leave the terminal at a prompt after the command exits
```

### Bookmarks

```toml
[bookmarks]
browsers = []          # [] = every browser found, or e.g. ["chrome", "firefox"]
keyword = "b"
global = true          # also show bookmarks in ordinary searches
```

Browser ids: `chrome`, `edge`, `brave`, `vivaldi`, `chromium`, `opera`,
`opera-gx`, `firefox`, `librewolf`, `zen`.

### Clipboard history and pasting

```toml
[clipboard]
enabled = false        # clipboard history is opt-in
max_items = 200
max_item_bytes = 65536 # longer text is not recorded
ignore_apps = []       # e.g. ["KeePassXC", "1Password"]

[paste]
restore_clipboard = false   # put the previous clipboard text back after pasting
```

### Snippets

```toml
[[snippet]]
name = "Email signature"
keyword = "sig"        # optional
text = "Best regards,\nNinad\n{date}"
```

Settings never reformats hand-written `[[snippet]]` entries.
[Placeholders](usage.md#paste-clipboard-history-and-snippets).

### Currency conversion

```toml
[calculator]
currency = false       # true: convert currencies with the ECB's daily rates (network)
```

### Universal Actions

```toml
[general]
actions_hotkey = "Ctrl+Alt+Space"   # "" turns Universal Actions off

[actions]
use_primary_selection = true        # Linux X11: read highlighted text without Ctrl+C
use_clipboard_fallback = false      # act on the clipboard when the selection can't be read
```

`Ctrl+Alt+Space` can type a no-break space on some AltGr keyboard layouts;
pick another key if it does. On Wayland, `sevak --setup-hotkey` binds the key
to `sevak --actions`. [How it works](usage.md#universal-actions).

### Keep the config in a synced folder

Point Sevak at a config folder in Dropbox, iCloud Drive, OneDrive or a git
repository with `SEVAK_CONFIG_DIR=~/Dropbox/sevak` or
`sevak --config ~/Dropbox/sevak` (the flag wins). A path ending in `.toml`
names the file itself. The folder gets a default `config.toml` if it has none.
Usage statistics and logs stay in the data folder (`usage.json` changes on
every launch, so leave it out of synced folders); `SEVAK_DATA_DIR` moves that
folder too. `--config` only matters for the instance that starts Sevak: quit
with `sevak --quit` and start again to switch configs.

### Add a web search keyword

Each engine needs a unique keyword without spaces, a display name, and an
HTTP(S) URL containing `{query}`.

**Important:** defining any `[[web_search]]` entries replaces the default
engine list. This example keeps Google, YouTube and GitHub, then adds
DuckDuckGo:

```toml
[[web_search]]
keyword = "g"
name = "Google"
url = "https://www.google.com/search?q={query}"

[[web_search]]
keyword = "yt"
name = "YouTube"
url = "https://www.youtube.com/results?search_query={query}"

[[web_search]]
keyword = "gh"
name = "GitHub"
url = "https://github.com/search?q={query}"

[[web_search]]
keyword = "ddg"
name = "DuckDuckGo"
url = "https://duckduckgo.com/?q={query}"
```

Now `ddg rust traits` offers a DuckDuckGo search. Sevak URL-encodes the
terms before substituting them. Keep the selected fallback engine defined
and enabled, or clear the fallback field.

### Disable a plugin

```toml
[plugins]
disabled = ["uuid", "web:yt"]
```

An instance ID such as `web:yt` disables that engine. A family ID such as
`web` disables every engine in that family. Available built-in families are
`apps`, `calculator`, `web`, `files`, `bookmarks`, `system`, `shell`,
`clipboard`, `snippets`, `emoji` (the emoji picker; `emoji:word` and `emoji:colon`
are its two keywords), `selection` (Universal Actions) and `uuid`. Script plugins use `script:<name>`, or
`script` for all of them.
[Plugin details](plugins.md#registry-and-enablingdisabling).

### Control startup and updates

```toml
[general]
launch_at_login = true
check_for_updates = false
```

Turning off automatic update checks still leaves **Check for updates**
available in the tray menu. Update installation requires your agreement.

## Default settings at a glance

| Setting | Default | Notes |
|---|---|---|
| `general.hotkey` | `"Alt+Space"` | Configurable launcher shortcut |
| `general.actions_hotkey` | `"Ctrl+Alt+Space"` | Universal Actions; empty turns it off |
| `actions.use_primary_selection` / `use_clipboard_fallback` | `true` / `false` | Linux X11 highlight; clipboard when capture fails |
| `general.hide_on_blur` | `true` | Hide when another window takes focus |
| `general.launch_at_login` | `false` | Start with the desktop session |
| `general.check_for_updates` | `true` | Automatic GitHub update checks |
| `window.width` | `720` | 400–1600 logical pixels |
| `appearance.theme` | `"system"` | Also `"light"` or `"dark"` |
| `appearance.accent` / `font_size` / `font_family` | `""` / `15` / `""` | Empty uses the theme's / system's |
| `appearance.opacity` / `radius` / `custom_css` | `100` / `14` / `""` | Percent, pixels, stylesheet path |
| `search.max_results` | `8` | 1–20 results |
| `search.fallback_web_search` | `"g"` | A keyword or a list; empty string disables fallback |
| `search.query_history` | `true` | `↑` on an empty bar recalls searches |
| `plugins.disabled` | `[]` | Disabled plugin IDs |
| `calculator.currency` | `false` | Currency conversion (downloads ECB rates) |
| `bookmarks.browsers` / `keyword` / `global` | `[]` / `"b"` / `true` | `[]` reads every browser found |
| `system.confirm` / `disabled` | `true` / `[]` | Confirmation for destructive commands |
| `shell.terminal` / `shell` / `keep_open` | `""` / `""` / `true` | Empty auto-detects |
| `clipboard.enabled` | `false` | Clipboard history is opt-in |
| `paste.restore_clipboard` | `false` | Restore the clipboard after pasting |
| `files.directories` | Desktop, Documents, Downloads under `~` | Replaced when explicitly set |
| `files.max_depth` | `4` | Settings accepts 0–32 |
| `files.include_hidden` | `false` | Include dot-files and dot-folders |
| `files.keyword` | `"f"` | Dedicated file-search prefix |
| `files.global` | `true` | Include files in ordinary searches |
| `linux.wayland_use_xwayland` | `true` | Restart after changing this option |

The commented default template lives in
[`config.rs`](../crates/sevak-core/src/config.rs). Use it as the source of truth
when introducing or documenting a new setting.

## Data and file locations

| OS | Configuration | Usage and logs |
|---|---|---|
| Windows | `%APPDATA%\sevak\config.toml` | `%APPDATA%\sevak\usage.json` and `logs\` |
| macOS | `~/Library/Application Support/sevak/config.toml` | `usage.json` and `logs/` in that same directory |
| Linux | `~/.config/sevak/config.toml` | `~/.local/share/sevak/usage.json` and `logs/` |

Linux locations follow the OS/XDG directory configuration when overridden;
`SEVAK_CONFIG_DIR` / `--config` and `SEVAK_DATA_DIR` override them on every OS.
Script plugins live in `plugins/` next to `config.toml`.

The data folder holds:

| File | Contents |
|---|---|
| `usage.json` | Launch counts for ranking, your last 50 searches (if `query_history` is on) and the `>` commands you ran |
| `clipboard-history.json` | Clipboard history, unencrypted, only if enabled |
| `currency-rates.json` | Cached ECB exchange rates, only if currency conversion is on |
| `script-plugin-approvals.json` | Script plugins you allowed |
| `logs/` | Log files |

Usage history affects ranking; logs help diagnose failures. These files stay
local. **Reveal logs folder** in Settings takes you to the correct directory.

To reset preferences safely, quit Sevak and **rename** `config.toml` to a backup
name. The next launch creates defaults. Keep the backup until you are happy
with the new settings.
