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
- **Web search keywords**: `g`, `yt` and `gh` search Google, YouTube and GitHub
  (`g rust traits`). Add your own engines in the config.
- **File search**: `f <name>` searches files and folders under the folders you
  choose (Desktop, Documents and Downloads by default). Optionally also shown
  for plain queries.
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
- **Settings window** (tray menu or `sevak --settings`), tray icon, hide-on-blur,
  launch at login, light/dark theme, and a plain-text config file whose comments
  survive saves from the settings window.

## Install

Download the installer for your platform from the
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

  This registers a GNOME custom shortcut that runs `sevak --toggle`.
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
| `Ctrl+1` ... `Ctrl+9` | Run the Nth visible result |
| `Esc` | Hide Sevak |

Keywords (type the keyword, then a space):

| Keyword | Does |
|---|---|
| `g <terms>` | Google search |
| `yt <terms>` | YouTube search |
| `gh <terms>` | GitHub search |
| `f <name>` | Search files and folders only |
| `uuid`, `uuid 5`, `uuid upper` | Generate random UUIDs and copy one (example plugin) |
| `cb <text>` | Clipboard history, newest first; Enter pastes (needs `[clipboard] enabled = true`) |
| `cb clear` | Shows a "Clear clipboard history" row at the bottom |
| `s <name>` | Snippets by name or keyword; Enter pastes the expanded text |

Enter on a clipboard or snippet result hides Sevak, returns to the app you were
using and presses Ctrl+V (Cmd+V on macOS). Where that is not possible (Wayland,
or macOS without Accessibility permission) the text is copied instead, and the
result says "Copies to clipboard". See [Paste, clipboard history and
snippets](#paste-clipboard-history-and-snippets).

Anything else searches apps (and files, if `files.global` is on); an
expression like `12*7` shows the calculator. When nothing matches, the fallback
web search (`g` by default) is offered.

The tray menu has Show, Settings (opens the settings window), Reload index
(re-reads the config and rescans apps and files) and Quit.

## Configuration

Sevak creates a commented config file on first run:

| OS | Path |
|---|---|
| Windows | `%APPDATA%\sevak\config.toml` |
| Linux | `~/.config/sevak/config.toml` |

Usage statistics are in `usage.json`, the clipboard history (if enabled) in
`clipboard-history.json`, and logs in `logs/`, all under
`%APPDATA%\sevak\` (Windows) or `~/.local/share/sevak/` (Linux).

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
fallback_web_search = "g"

[appearance]
theme = "system"       # "system", "light" or "dark"

[plugins]
disabled = []          # "apps", "calculator", "files", "web:<keyword>", "clipboard", "snippets"

[files]
directories = ["~/Desktop", "~/Documents", "~/Downloads"]
max_depth = 4
include_hidden = false
keyword = "f"
global = true

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
sevak --background        Start without showing the window
sevak --settings          Open the settings window
sevak --quit              Quit the running instance
sevak --setup-hotkey [KEY]  Bind KEY (default: config hotkey) to `sevak --toggle` in GNOME
sevak -h, --help          Print help
sevak -V, --version       Print the version
```

Only one instance runs at a time; running `sevak` again forwards the request
to the running instance.

## Privacy

Sevak has no telemetry or analytics. Config, usage statistics and logs stay on
your machine. Sevak makes one kind of request on its own: at startup and once a
day it downloads `latest.json` from this repository's GitHub Releases to see if
there is a new version (GitHub sees your IP address, nothing else is sent).
Turn it off with `general.check_for_updates = false` or in Settings; "Check
for updates" in the tray menu still works on demand. Updates are signed and
only installed after you agree.

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
- [docs/plugins.md](docs/plugins.md): how plugins work and how to write one
- [docs/development.md](docs/development.md): architecture, testing, releasing

## Contributing

Bug reports, fixes and plugins are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md)
for the workflow (pull request titles decide the release version) and the
[Code of Conduct](CODE_OF_CONDUCT.md). Report security problems privately as
described in [SECURITY.md](SECURITY.md).

## License

[Apache License 2.0](LICENSE).
