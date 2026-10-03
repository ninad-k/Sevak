# Sevak

[![CI](https://github.com/ninad-k/Sevak/actions/workflows/ci.yml/badge.svg)](https://github.com/ninad-k/Sevak/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/ninad-k/Sevak)](https://github.com/ninad-k/Sevak/releases/latest)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)

Sevak (Sanskrit/Hindi *sevak*, "one who serves") is a keyboard-first quick
launcher for Windows, macOS and Linux. Press a hotkey, type a few letters, press
Enter. Everything runs locally, there is no telemetry, and the whole thing is
configured from one TOML file.

Built with Rust, [Tauri](https://tauri.app) v2 and Svelte 5. Licensed under
[Apache-2.0](LICENSE).

## Features

- **App search**: fuzzy search over installed applications (Start Menu and
  packaged apps on Windows, `.app` bundles on macOS, `.desktop` entries on Linux).
- **Calculator**: type an expression such as `2^10 / 3` or `sqrt(2) * 5` and press Enter
  to copy the result.
- **Unit and currency conversion**: `10 km in mi`, `72°F to C`, `5 GB in MiB`,
  `2 h 30 min in min`, all offline. Currencies (`100 usd in eur`) are opt-in
  and use the European Central Bank's daily rates. See
  [Conversions](#conversions).
- **Web search keywords**: `g`, `yt` and `gh` search Google, YouTube and GitHub
  (`g rust traits`). Add your own engines in the config.
- **File search**: `f <name>` searches files and folders under the folders you
  choose (Desktop, Documents and Downloads by default). Optionally also shown
  for plain queries.
- **Bookmark search**: `b <text>` searches the bookmarks of Chrome, Edge,
  Brave, Vivaldi, Chromium, Opera, Firefox, LibreWolf and Zen, across all
  profiles, matching titles and URLs. Read locally and read-only. Optionally
  also shown for plain queries.
- **Result actions**: act on a result in more than one way: show a file or app
  in its folder, copy its path or URL, run an app as administrator (Windows).
  Press `Ctrl+Enter` / `Shift+Enter` / `Alt+Enter`, or open the action panel
  with `Right` or `Ctrl+K`. `Ctrl+C` copies the result's path, URL or value, and
  `Ctrl+L` shows it as Large Type.
- **Path browsing**: type a path (`~/Documents/`, `/etc/`, `C:\Users\`,
  `\\server\share\`) to list that folder, folders first, filtered by what you
  type after the last slash. `Tab` completes the selected entry (folders get a
  trailing slash so you can keep drilling down).
- **Search history**: `Up` and `Down` on an empty search bar recall the last
  searches you ran.
- **System commands**: type `lock`, `sleep`, `restart`, `shut down`, `log out`
  or `empty trash` (and `bluetooth`, `display`, `wifi` ... for OS settings
  pages). Restart, shut down, log out and emptying the trash ask first. See
  [System commands](#system-commands).
- **Terminal commands**: `> git status` (or `>git status`) shows "Run `git
  status` in terminal"; Enter opens your terminal and runs it. Recent commands
  are offered again. Nothing runs until you press Enter.
- **Clipboard history** (opt-in): `cb <text>` finds text you copied earlier and
  Enter pastes it into the app you were using. Text only; passwords from
  password managers are never recorded. Turn it on with `[clipboard] enabled`.
- **Snippets**: `s <name>` pastes text from your `[[snippet]]` entries into the
  app you were using, with `{date}`, `{time}`, `{clipboard}`, `{uuid}`
  placeholders filled in.
- **Frequency and recency ranking**: results you pick often and recently rise
  to the top, per query.
- **Plugin system**: every result source is a plugin; there is a worked example
  (`uuid`) and a guide to writing your own in [docs/plugins.md](docs/plugins.md).
- **Automatic updates**: Sevak checks for a new release daily and installs it
  after you agree (signed updates; can be turned off).
- **Per-command hotkeys**: bind extra global keys that open Sevak with text
  already typed (`> `, `g `) or run a result directly without showing the
  window (see [Custom hotkeys](#custom-hotkeys)).
- **Themes**: light, dark or system, plus your own accent color, font, corner
  radius, opacity and a custom stylesheet ([docs/themes.md](docs/themes.md)).
- **Settings window** (tray menu or `sevak --settings`), tray icon, hide-on-blur,
  launch at login, and a plain-text config file whose comments survive saves
  from the settings window. The config folder can live anywhere, for example a
  synced folder ([Config location](#config-location)).

## Install

With a package manager:

| Platform | Command |
|---|---|
| Windows (winget) | `winget install NinadKulkarni.Sevak` |
| Windows (Scoop) | `scoop bucket add ninad-k https://github.com/ninad-k/scoop-bucket` then `scoop install sevak` |
| macOS (Homebrew) | `brew install --cask ninad-k/tap/sevak` |
| Arch Linux (AUR) | `yay -S sevak-bin` (or any AUR helper) |

Or download the installer for your platform from the
[Releases page](https://github.com/ninad-k/Sevak/releases):

| Platform | Package |
|---|---|
| Windows 10/11 | `Sevak_<version>_x64-setup.exe` (per-user, no admin needed) or `.msi` |
| macOS 11+ (Apple silicon and Intel) | `Sevak_<version>_universal.dmg` |
| Ubuntu 22.04+ / Debian | `.deb` (`sudo apt install ./Sevak_<version>_amd64.deb`) |
| Fedora 39+ | `.rpm` (`sudo dnf install ./Sevak-<version>-1.x86_64.rpm`) |
| Other Linux | `.AppImage` |

RHEL 9 and its clones cannot run the prebuilt packages (no `webkit2gtk4.1`).
Details, Wayland hotkey setup and troubleshooting are in
[docs/install.md](docs/install.md).

### Linux hotkey notes

- **X11**: `Alt+Space` works out of the box.
- **Wayland** (GNOME on Ubuntu and Fedora by default): applications cannot grab
  global keys. Run once:

  ```sh
  sevak --setup-hotkey            # binds the hotkey from config.toml
  sevak --setup-hotkey Super+Space  # or pick a key
  ```

  This registers a GNOME custom shortcut that runs `sevak --toggle`, and one per
  `[[hotkey]]` entry in your config that runs `sevak --query ...` or
  `sevak --run ...`.
- **GNOME's `Alt+Space` conflict**: GNOME binds `Alt+Space` to the window menu.
  `--setup-hotkey` reports conflicts; to free the key, run
  `gsettings set org.gnome.desktop.wm.keybindings activate-window-menu "[]"`,
  or choose a different key such as `Super+Space` (note GNOME uses
  `Super+Space` to switch input sources if you have several) or `Ctrl+Space`.
- **Fedora tray**: stock GNOME has no tray. Install and enable the
  "AppIndicator and KStatusNotifierItem Support" GNOME extension to see Sevak's
  tray icon. Sevak works without a tray; the hotkey still works.

## Usage

| Key | Action |
|---|---|
| `Alt+Space` (configurable) | Show or hide Sevak |
| Type | Search |
| `Up` / `Down` (or `Ctrl+P` / `Ctrl+N`) | Move the selection |
| `PageUp` / `PageDown` | Move by a page |
| `Enter` | Run the selected result |
| `Tab` | Complete the input to the selected result (a keyword such as `g` becomes `g `, a folder gets a trailing slash) |
| `Shift+Tab` | Go up one folder while browsing a path |
| `Up` / `Down` on an empty search bar | Recall earlier searches (newest first); typing leaves history, `Ctrl+P` / `Ctrl+N` still move the selection |
| `Ctrl+1` ... `Ctrl+9` | Run the Nth visible result |
| `Ctrl+Enter`, `Shift+Enter`, `Alt+Enter` | Run the selected result's alternative action (shown under the list) |
| `Right` (caret at the end) or `Ctrl+K` | Open the action panel: every action for the selected result; `Up`/`Down` and `Enter` to pick, `Esc` or `Left` to close |
| `Ctrl+C` (nothing selected in the box) | Copy the selected result's path, URL or value |
| `Ctrl+L` | Show the selected result as Large Type; any key or click dismisses it |
| `Esc` | Hide Sevak |

On macOS, `Cmd` takes the place of `Ctrl`.

What the alternative actions are:

| Result | `Ctrl+Enter` | `Shift+Enter` | `Alt+Enter` |
|---|---|---|---|
| Application | Show in folder | Copy path | Run as administrator (Windows, not for Store apps) |
| File or folder | Show in folder | Copy path | |
| Web search | | Copy URL | |

Keywords (type the keyword, then a space):

| Keyword | Does |
|---|---|
| `g <terms>` | Google search |
| `yt <terms>` | YouTube search |
| `gh <terms>` | GitHub search |
| `f <name>` | Search files and folders only |
| `b <text>` | Search browser bookmarks only |
| `> <command>` or `><command>` | Run a command in a terminal (no space needed after `>`); `> ` alone lists recent commands |
| `uuid`, `uuid 5`, `uuid upper` | Generate random UUIDs and copy one (example plugin) |
| `cb <text>` | Clipboard history, newest first; Enter pastes (needs `[clipboard] enabled = true`) |
| `cb clear` | Shows a "Clear clipboard history" row at the bottom |
| `s <name>` | Snippets by name or keyword; Enter pastes the expanded text |

Enter on a clipboard or snippet result hides Sevak, returns to the app you were
using and presses Ctrl+V (Cmd+V on macOS). Where that is not possible (Wayland,
or macOS without Accessibility permission) the text is copied instead, and the
result says "Copies to clipboard". See [Paste, clipboard history and
snippets](#paste-clipboard-history-and-snippets).

Anything else searches apps and system commands (and files and bookmarks, if
`files.global` / `bookmarks.global` are on); an expression like `12*7` or a
conversion like `10 km in mi` shows the calculator. A path starting with `~/`,
`/`, a drive (`C:\`) or `\\server\share\` browses that folder (for plain
queries this needs `files.global`; after `f ` it always works). Typing just a
keyword (`g`) offers a row you can complete with `Tab`. When nothing matches,
the fallback web search (`g` by default; several can be listed) is offered.

The tray menu has Show, Settings (opens the settings window), Reload index
(re-reads the config and rescans apps, files and bookmarks) and Quit.

### System commands

System commands are matched by name in ordinary queries, like apps (two or more
letters). Only the commands that can work on your machine are listed.

| Command (aliases) | Windows | macOS | Linux |
|---|---|---|---|
| Lock screen (`lock`) | `LockWorkStation` | `pmset displaysleepnow` | `loginctl lock-session` |
| Sleep (`suspend`) | `SetSuspendState` | `pmset sleepnow` | `systemctl suspend` |
| Hibernate | `shutdown /h`, if hibernation is on | not offered | `systemctl hibernate`, if swap is set up |
| Restart (`reboot`) | `shutdown /r /t 0` | System Events | `systemctl reboot` |
| Shut down (`shutdown`, `power off`) | `shutdown /s /t 0` | System Events | `systemctl poweroff` |
| Log out (`logout`, `sign out`) | `shutdown /l` | System Events | `gnome-session-quit`, KDE `qdbus`, or `loginctl terminate-session` |
| Empty Recycle Bin / Trash | `SHEmptyRecycleBin` | Finder | `gio trash --empty` |
| Settings pages (`bluetooth`, `display`, `wifi`, `sound`, `network`, `apps`, `power`, ...) | `ms-settings:` | System Settings panes | `gnome-control-center <panel>`, if installed |

Restart, shut down, log out and empty trash show a confirmation dialog first.
On macOS the first use asks permission to control System Events or Finder, and
Lock needs "Require password after screen saver begins or display is turned off"
(the default) to actually lock. Turn confirmation off or hide commands in the
`[system]` section of the config (below).

### Conversions

The calculator converts `<amount> <unit> (in|to|as|=) <unit>`. The amount can
be any expression (`(2+3) km in m`), and the result is copied with its unit on
Enter. Separators are case-insensitive; `in` also works when you mean inches
(`12 in in cm`).

| Category | Examples |
|---|---|
| Length | `10 km in mi`, `5'11" to cm`, `3 ft 4 in to cm` (also m, cm, mm, yd, nmi, ly, au) |
| Mass | `1 kg in lb`, `8 oz to g`, `1 ton in kg` |
| Temperature | `100 f to c`, `72°F in C`, `0 c to k` |
| Volume | `3.5 cups to ml`, `1 gal in L`, `1 tbsp in tsp`, `1 imp gal in L`, `2 m3 in L` |
| Area | `1 acre in m2`, `500 sq ft to m²`, `1 ha in acres` |
| Speed | `60 mph in km/h`, `10 m/s to knots`, `miles per hour` |
| Data | `5 GB in MiB`, `100 Mbit to MB`, `1 TiB in GB` |
| Time | `2 h 30 min in min`, `90 min to h`, `1 yr in days` |
| Pressure | `1 atm in psi`, `1 bar in kPa`, `760 torr in atm` |
| Energy | `1 kcal in kJ`, `1 kWh in MJ` |
| Angle | `180 deg in rad`, `1 turn in deg` |

Spelling rules, where case matters or a word is ambiguous:

- Names, plurals and abbreviations are case-insensitive (`km`, `Km`,
  `kilometers`), except data units: `MB` is a megabyte and `Mb` a megabit,
  `B` a byte and `b` a bit. All-lowercase `kb`, `mb`, `gb`, ... mean bytes.
  `KB`/`kB`/`MB`/`GB` are SI (powers of 1000); `KiB`/`MiB`/`GiB` are binary
  (powers of 1024).
- `oz` is a mass ounce, `fl oz` a fluid ounce. `pt`, `qt`, `gal` and `cup` are
  US customary; use `imp pt` / `imp gal` for imperial. `ton` is the US short
  ton, `tonne` or `t` the metric one.
- `m` is meters and `min` minutes. A month is a twelfth of a Julian year
  (365.25 days).
- `cal` is the small calorie, `Cal` the food Calorie (1 kcal).
- `°` and `deg` alone are angles; before a letter they are temperatures
  (`°F`, `deg C`).
- Results keep up to 10 significant digits (`10 km in mi` is `6.213711922 mi`).

**Currency** conversion is off by default. Turn it on in Settings (Plugins) or
set `[calculator] currency = true`, then reload. Sevak downloads the ECB's
daily euro reference rates (the 30 currencies the ECB publishes; no cryptocurrencies) on a
background thread, never while you type, at most once a day, and keeps them in
`currency-rates.json` in the data folder. Until the first download finishes the
result row says "Fetching exchange rates…". The result's subtitle shows the
rate and the ECB's publication date. Use ISO codes (`usd`, `eur`, `gbp`,
`jpy`, `cad`, ...), signs (`€`, `$`, `£`, `¥`; `$` is the US dollar and `¥` the
yen) or words (`euros`): `50 € to $`, `$100 in eur`.

## Configuration

Sevak creates a commented config file on first run:

| OS | Path |
|---|---|
| Windows | `%APPDATA%\sevak\config.toml` |
| Linux | `~/.config/sevak/config.toml` |

Usage statistics (including your last 50 searches, see `query_history`, and
the `>` commands you ran) are in `usage.json`, the clipboard history (if
enabled) in `clipboard-history.json`, the exchange rates (if currency
conversion is on) in `currency-rates.json`, and logs in `logs/`, all under
`%APPDATA%\sevak\` (Windows) or `~/.local/share/sevak/` (Linux).

### Config location

To keep the config in a folder you sync (Dropbox, iCloud Drive, OneDrive, a git
repository), point Sevak at it:

| How | Example |
|---|---|
| `SEVAK_CONFIG_DIR` environment variable | `SEVAK_CONFIG_DIR=~/Dropbox/sevak` |
| `--config <path>` command-line option (wins over the variable) | `sevak --config ~/Dropbox/sevak` |

The path is the config *folder*: `config.toml` and anything else Sevak reads
from the config folder (such as a theme's `custom_css` file) live there.
A path ending in `.toml` names the config file itself instead, and its folder is
the config folder. The folder is created with a default `config.toml` when it
does not exist. Relative paths are relative to the current directory and `~` is
expanded.

Usage statistics and logs stay in the data folder (`usage.json` changes on
every launch, so it is better left out of a synced folder). `SEVAK_DATA_DIR`
moves that folder too.

Only one Sevak runs at a time, and `--config` only matters for the process that
starts it. Running `sevak --config <other> --toggle` while Sevak is already
running forwards `--toggle` to the running instance, which keeps the config it
started with (a warning is written to the log). To switch configs, quit with
`sevak --quit` and start Sevak again with the new path.

Key options (all optional; defaults shown):

```toml
[general]
hotkey = "Alt+Space"
hide_on_blur = true
launch_at_login = false
check_for_updates = true

[window]
width = 720            # 400-1600

[search]
max_results = 8        # 1-20
fallback_web_search = "g"   # or a list, shown in order: ["g", "yt", "gh"]
query_history = true       # Up/Down on an empty search bar recalls past searches

[appearance]
theme = "system"       # "system", "light" or "dark"
accent = ""            # "#7c3aed", "#fa0" or "rgb(124, 58, 237)"; "" = the theme's
font_size = 15         # 12-22 px (result titles; the bar scales with it)
font_family = ""       # "Fira Sans, sans-serif"; "" = the system font
opacity = 100          # 30-100 (% opacity of the bar's background)
radius = 14            # 0-32 px
custom_css = ""        # "theme.css", a stylesheet in the config folder

[plugins]
disabled = []          # "apps", "calculator", "files", "bookmarks", "system", "shell",
                       # "clipboard", "snippets", "web:<keyword>"

[calculator]
currency = false       # true: convert currencies with the ECB's daily rates (network)

[files]
directories = ["~/Desktop", "~/Documents", "~/Downloads"]
max_depth = 4
include_hidden = false
keyword = "f"
global = true

[bookmarks]
browsers = []          # [] = every browser found, or e.g. ["chrome", "firefox"]
keyword = "b"
global = true

[system]
confirm = true         # ask before restart, shut down, log out, empty trash
disabled = []          # "lock", "sleep", "hibernate", "restart", "shutdown",
                       # "logout", "empty_trash", "settings", "settings:<page>"

[shell]
terminal = ""          # "" = auto-detect; e.g. "wt", "iterm", "kitty", "alacritty --class sevak"
shell = ""             # "" = pwsh/powershell/cmd on Windows, $SHELL on Linux
keep_open = true       # leave the terminal open at a prompt after the command exits

[paste]
restore_clipboard = false   # put the clipboard's previous text back after pasting

[clipboard]
enabled = false        # clipboard history is opt-in
max_items = 200
max_item_bytes = 65536 # longer text is not recorded
ignore_apps = []       # e.g. ["KeePassXC", "1Password"]

[linux]
wayland_use_xwayland = true

[[web_search]]
keyword = "g"
name = "Google"
url = "https://www.google.com/search?q={query}"

[[snippet]]
name = "Email signature"
keyword = "sig"        # optional
text = "Best regards,\nNinad\n{date}"
```

Defining any `[[web_search]]` entry replaces the default list. After editing,
choose "Reload index" from the tray menu or restart Sevak.

Invalid appearance values fall back to their defaults and a warning is written
to the log (and shown in Settings, Appearance). The themes guide lists the CSS
variables a custom stylesheet can override: [docs/themes.md](docs/themes.md).

### Custom hotkeys

Besides `general.hotkey` you can bind more global keys. Each `[[hotkey]]` has a
`key` and exactly one of `query` or `run`:

```toml
[[hotkey]]
key = "Ctrl+Alt+T"
query = "> "                      # open Sevak with this text typed in

[[hotkey]]
key = "Ctrl+Alt+F"
run = "apps:firefox.desktop"      # run this result, Sevak stays hidden
```

- `query` opens the search bar with the text already in the field and the caret
  after it, so `g ` is "web search" and `f ` is "find a file". An empty `query`
  just opens Sevak.
- `run` takes a result id (hover over a result for a moment to see its id):
  `<plugin id>:<key>`, as in `apps:firefox.desktop`
  (the desktop-file id on Linux), `apps:<Start Menu path or AppUserModelID>` on
  Windows, or `files:<full path>` for a file or folder. Plugins whose results
  are not named by a stable id cannot be run this way. If the id cannot be
  found, Sevak opens and says so instead of doing nothing.
- Keys that cannot be registered (invalid, already taken by another program,
  repeated) are skipped, logged, and listed with the reason under Settings,
  Hotkeys; the other keys keep working.
- Settings has a Hotkeys page to edit the list. Changes apply on save and on
  "Reload index".
- On Linux Wayland the desktop owns global keys: `sevak --setup-hotkey` creates a
  GNOME shortcut for each entry that runs `sevak --query '<text>'` or
  `sevak --run <id>` (other desktops: bind those commands yourself; the command
  is printed). After you remove an entry, delete its shortcut in GNOME's
  keyboard settings.

### Terminal commands

`> <command>` opens a terminal and runs the command there; `[shell]` picks the
terminal and shell. With `terminal = ""` Sevak detects one:

| OS | Terminal | Shell |
|---|---|---|
| Windows | Windows Terminal (`wt`), else a plain console window | `pwsh`, else `powershell`, else `cmd` |
| macOS | Terminal.app (`terminal = "iterm"` for iTerm2) | your login shell |
| Linux | `$TERMINAL`, then `x-terminal-emulator`, `gnome-terminal`, `konsole`, `kitty`, `alacritty`, `wezterm`, `foot`, `xterm` | `$SHELL`, else `sh` |

`terminal` may include extra arguments (`"alacritty --class sevak"`), and on
Windows `"conhost"` forces a plain console window. With `keep_open = true` the
terminal stays open at a shell prompt after the command exits. The command runs
in a non-interactive shell (`-c`), so shell aliases defined in `.bashrc` and
similar are not available.

### Paste, clipboard history and snippets

**Pasting.** Results from `cb` and `s` paste into the app that had focus when
you opened Sevak. Sevak hides, brings that window back, and sends Ctrl+V
(Cmd+V on macOS). With `[paste] restore_clipboard = true` the clipboard's
previous text is put back about 300 ms later (text only: if the clipboard held an
image or files, it is left as Sevak set it).

- **Windows**: works everywhere except in windows running as administrator
  (Windows blocks key events sent to them from a normal program).
- **macOS**: synthetic key presses need the permission *System Settings >
  Privacy & Security > Accessibility > Sevak*. Without it Sevak copies instead
  and the result says so. The shortcut sends the `V` key, so keyboard layouts
  that move `V` (such as Dvorak) do not paste.
- **Linux X11**: works (XTest). **Wayland**: applications cannot read the focused
  window or send keys, so Sevak only copies.

**Clipboard history** is off by default. Set `[clipboard] enabled = true` and
reload; Sevak then checks the clipboard a few times a second and keeps the most
recent `max_items` pieces of text in `clipboard-history.json` in its data folder
(`%APPDATA%\sevak\` on Windows, `~/.local/share/sevak/` on Linux). It never
records:

- content its source app marks as secret (Windows: the
  `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory`
  and `CanUploadToCloudClipboard` formats; macOS: `org.nspasteboard.ConcealedType`,
  `TransientType` and `AutoGeneratedType`), which is what password managers use;
- copies made while an app in `ignore_apps` had focus (matched, ignoring case,
  against the program name such as `KeePassXC`/`keepassxc.exe`, or the app name
  on macOS);
- text over `max_item_bytes`, blank text, images and files;
- anything Sevak itself copied or pasted.

On Linux there is no secret marker to read, so use `ignore_apps`. On Wayland the
history only sees copies made in XWayland apps and is best effort. Images and
files are not supported yet. To delete the history type `cb clear` and pick
"Clear clipboard history", or delete the file; turning the option off stops
recording but keeps the file.

**Snippets** live in `[[snippet]]` entries (`name`, optional `keyword`, `text`).
Search by name or keyword. Placeholders in `text`, filled in when you press
Enter:

| Placeholder | Result |
|---|---|
| `{date}`, `{time}`, `{datetime}` | `2026-10-03`, `14:05`, `2026-10-03 14:05` |
| `{date:FORMAT}` | Custom [strftime](https://docs.rs/chrono/latest/chrono/format/strftime/index.html) format, e.g. `{date:%d %B %Y}` (`{time:..}` and `{datetime:..}` too) |
| `{clipboard}` | The clipboard's text |
| `{uuid}` | A new random UUID |
| `{{` and `}}` | A literal `{` and `}` |

Other text in braces is kept as written. Typing a snippet's keyword in any app
and having it expand in place (text expansion) needs a global keyboard hook and
is not implemented.

## Command line

```
sevak                     Start Sevak and show the search bar
sevak --toggle            Show the search bar, or hide it if visible
sevak --query TEXT        Show the search bar with TEXT already typed in
sevak --run ID            Run the result with this id, without showing the window
sevak --background        Start without showing the window
sevak --settings          Open the settings window
sevak --quit              Quit the running instance
sevak --setup-hotkey [KEY]  Bind KEY (default: config hotkey) to `sevak --toggle` in GNOME,
                          and the [[hotkey]] entries to --query / --run
sevak --config PATH       Use PATH as the config folder (combine with any option above)
sevak -h, --help          Print help
sevak -V, --version       Print the version
```

Only one instance runs at a time; running `sevak` again forwards the request
to the running instance.

## Privacy

Sevak has no telemetry or analytics. Config, usage statistics and logs stay on
your machine. The usage statistics (`usage.json`) also hold your recent searches
(turn them off with `search.query_history = false`, which also deletes them) and
the commands you ran with `>`, so they can be offered again; delete the file to
forget them. Those commands are only ever handed to your terminal.

Apart from the optional currency rates (below), Sevak makes one kind of request
on its own: at startup and once a day it downloads `latest.json` from this repository's GitHub Releases to see if
there is a new version (GitHub sees your IP address, nothing else is sent).
Turn it off with `general.check_for_updates = false` or in Settings; "Check
for updates" in the tray menu still works on demand. Updates are signed and
only installed after you agree.

The bookmarks plugin reads your browsers' bookmark files from disk (Firefox's
database is read from a temporary copy that is deleted again) and never writes
to them or sends them anywhere.

If you turn on currency conversion (`[calculator] currency`, off by default),
Sevak also downloads
`https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml` (the European
Central Bank's daily reference rates) in the background, at most once a day
and only when the saved rates are over a day old (the ECB sees your IP address,
nothing else is sent; no cookies, no identifiers). The rates are kept in
`currency-rates.json` in the data folder. With the option off, no such request
is ever made. Unit conversion is always offline.

Clipboard history is off unless you turn it on. When on, copied text is stored
unencrypted in `clipboard-history.json` in Sevak's data folder (readable only by
you on Linux and macOS, in your profile folder on Windows) and never leaves your
machine; see [above](#paste-clipboard-history-and-snippets) for what is skipped.
Anyone with access to your account can read that file, so add password-like
apps to `ignore_apps` and clear the history when in doubt.

Otherwise, the only network traffic is your browser opening a web search URL
when you pick a web search result. (On Windows, the installer may download the
Microsoft WebView2 runtime if it is missing.)

## Build from source

Prerequisites: Rust (stable, 1.90+), Node.js 22+ and the platform libraries
for Tauri v2 (see the [prerequisites](https://v2.tauri.app/start/prerequisites/)):

- **Windows**: Microsoft C++ Build Tools (MSVC) and WebView2 (preinstalled on
  Windows 11).
- **Ubuntu/Debian**: `sudo apt install build-essential curl file patchelf libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev`
- **Fedora**: `sudo dnf install gcc gcc-c++ make curl file patchelf webkit2gtk4.1-devel libayatana-appindicator-gtk3-devel librsvg2-devel libxdo-devel openssl-devel`

```sh
npm ci
npm run tauri dev       # run with hot reload
npx tauri build         # build installers for the current OS
```

See [docs/development.md](docs/development.md) for the architecture, tests and
release process.

## Documentation

- [docs/install.md](docs/install.md): installation, Wayland, tray, RHEL notes, uninstall
- [docs/themes.md](docs/themes.md): accent, fonts, opacity and custom stylesheets
- [docs/plugins.md](docs/plugins.md): how plugins work and how to write one
- [docs/development.md](docs/development.md): architecture, testing, releasing

## Contributing

Bug reports, fixes and plugins are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md)
for the workflow (pull request titles decide the release version) and the
[Code of Conduct](CODE_OF_CONDUCT.md). Report security problems privately as
described in [SECURITY.md](SECURITY.md).

## License

[Apache License 2.0](LICENSE).
