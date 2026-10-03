# Sevak

**Your desktop. At your service.**

[![CI](https://github.com/ninad-k/Sevak/actions/workflows/ci.yml/badge.svg)](https://github.com/ninad-k/Sevak/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/ninad-k/Sevak)](https://github.com/ninad-k/Sevak/releases/latest)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)

[**Download Sevak**](https://github.com/ninad-k/Sevak/releases/latest) ·
[Quick start](docs/quickstart.md) · [User guide](docs/usage.md) ·
[Help](docs/troubleshooting.md) · [Contribute](CONTRIBUTING.md)

![Sevak: a keyboard-first desktop launcher with app search and the Alt+Space shortcut.](docs/media/sevak-overview.png)

Sevak brings everyday desktop actions into one search bar. Launch an app,
find a file, calculate an answer, or open a web search without navigating
through menus. Press **Alt+Space**, type what you need, then press **Enter**.

Built for **Windows, macOS and Linux** with Rust, Tauri 2 and Svelte 5.
App and file searches, calculations, and usage ranking run locally. Web
searches open in your browser, and optional update checks contact GitHub.

## Why “Sevak”?

*Sevak* (सेवक) means **“one who serves”**, a Sanskrit-derived word used in Hindi
([Hindwi dictionary](https://www.hindwi.org/hindi-dictionary/meaning-of-sevak)).
The name reflects the app's purpose: stay close at hand, help with a small task,
and let you return to your work.

The amber **S-shaped flame** and lamp express that same idea of helpfulness and
clarity. [The story behind the name and logo →](docs/brand.md)

## How it works

![Three steps: open with Alt+Space, type a query, then press Enter to launch, open or copy.](docs/media/sevak-workflow.svg)

1. **Open** — press `Alt+Space` (`Option+Space` on macOS).
2. **Search** — type an app name, calculation, or keyword such as `f` or `g`.
3. **Act** — choose a result with `↑` / `↓`, then press `Enter`.

The shortcut is configurable. On Linux Wayland, bind it through your desktop;
see [Linux shortcut setup](docs/install.md#setting-up-the-hotkey-on-linux).

### A 12-second introduction

<p align="center">
  <a href="docs/media/sevak-12s.mp4">
    <img src="docs/media/sevak-demo.gif" width="280" alt="Animated Sevak walkthrough: open with Alt+Space, find an app, and calculate 12 times 7." />
  </a>
</p>

[**Watch or download the vertical reel — MP4, 12 seconds, captions and subtle sound**](docs/media/sevak-12s.mp4)

The reel is an illustrative walkthrough. Interface images are rendered from
Sevak's real Svelte components using example results, not personal files.
They show the workflow rather than measured search or launch times.

## What you can do

![Four examples: launch an app, calculate and copy, find a project file, and search the web.](docs/media/sevak-features.png)

| Task | Try typing | What Enter does |
|---|---|---|
| Launch an installed app | `code` | Launches the selected application |
| Calculate | `12*7` or `sqrt(16)` | Copies the answer |
| Find a file or folder | `f project` | Opens the selected result |
| Search Google | `g rust traits` | Opens the search in your browser |
| Search YouTube | `yt svelte tutorial` | Opens YouTube search |
| Search GitHub | `gh tauri` | Opens GitHub search |
| Convert units | `10 km in mi`, `72°F to C` | Copies the converted value |
| Open a bookmark | `b github` | Opens it in your browser |
| Browse a folder | `~/Documents/` then `Tab` | Opens the entry; `Tab` drills down |
| Act on selected text or files in any app | select, then `Ctrl+Alt+Space` | Searches, transforms, opens or copies it — you pick |
| Lock, sleep, restart… | `lock`, `restart`, `bluetooth` | Runs the system command (restart, shut down, log out and empty trash ask first) |
| Run a terminal command | `> git status` | Opens your terminal and runs it |
| Paste earlier clipboard text | `cb invoice` | Pastes into the app you were using (opt-in) |
| Paste a snippet | `s sig` | Pastes the snippet with `{date}` etc. filled in |
| Generate a UUID | `uuid ` or `uuid 5` | Copies a UUID when the plugin is enabled |

App results depend on what is installed. File search covers the folders you
choose and matches **names**, not document contents. Put a space after a
keyword to activate it (`>` is the exception: `>ls` works too).

Sevak also includes:

- **Universal Actions** — select text, a link or files in any app, press
  `Ctrl+Alt+Space`, and pick: web search, Large Type, transform (case,
  Base64, URL encoding, JSON) and paste back, calculate, open, show in folder,
  open in a terminal. [How it works →](docs/usage.md#universal-actions)
- **Result actions** — `Ctrl+Enter` shows a file or app in its folder,
  `Shift+Enter` copies its path or URL, `Alt+Enter` runs an app as
  administrator (Windows); `→` or `Ctrl+K` lists every action. `Ctrl+C` copies
  a result and `Ctrl+L` shows it as Large Type.
- **Fuzzy matching and usage ranking** — frequent and recent selections help
  order relevant results. `↑` on an empty bar recalls earlier searches.
- **Custom search engines** — create your own keyword and URL template; list
  several fallback engines for queries with no match.
- **Plugins** — enable the sources you need, build a compiled-in extension, or
  drop in a [script plugin](docs/plugins.md#external-plugins) (Python,
  PowerShell, Node…). Many Alfred Script Filter scripts run unchanged.
- **Custom hotkeys** — extra global keys that open Sevak with text typed in
  (`> `, `g `) or run a result directly.
- **Themes** — light, dark or system, plus accent color, font, radius, opacity
  and your own stylesheet. A visual theme editor with a live preview, eight
  built-in themes and an optional online theme gallery are in Settings →
  Appearance ([themes guide](docs/themes.md)).
- **Settings and TOML** — use the settings window or a commented config file,
  which can live in a synced folder.
- **Tray access and launch at login** — keep Sevak available in the background.
- **Optional update checks** — check at startup and daily; installation
  requires your agreement and verifies an update signature.

[Explore the complete user guide →](docs/usage.md)

## Make it yours

![Sevak's Settings interface showing shortcut, startup, hide-on-blur and update preferences.](docs/media/sevak-settings.png)

Open **Settings** from the tray or run `sevak --settings`. Choose your shortcut,
search folders, result limit, theme, web engines, and plugins. Click **Save**
to apply your changes.

![The same Sevak app search shown in light and dark themes.](docs/media/sevak-themes.png)

Choose **System**, **Light** or **Dark** under Settings → Appearance.
[Configuration reference and examples →](docs/configuration.md)

## Install

Choose a package from [GitHub Releases](https://github.com/ninad-k/Sevak/releases).

| Platform | Package | Instructions |
|---|---|---|
| Windows 10 / 11 | Per-user `.exe` installer or `.msi` | [Windows](docs/install.md#windows-10--11) |
| macOS 11+ | Universal `.dmg` for Apple silicon and Intel | [macOS](docs/install.md#macos-11) |
| Ubuntu 22.04+ / Debian | `.deb` | [Ubuntu / Debian](docs/install.md#ubuntu-2204--debian) |
| Fedora 39+ | `.rpm` | [Fedora](docs/install.md#fedora-39) |
| Other compatible Linux distributions | `.AppImage` | [AppImage](docs/install.md#appimage-other-distributions) |

Package availability follows the assets attached to each release. See the
installation guide for runtime requirements and platform signing status.
Prebuilt packages are not compatible with RHEL 9 and its clones; see
[compatibility notes](docs/install.md#red-hat-enterprise-linux-9-and-clones-rocky-almalinux).

After installing, follow [Your first five minutes with Sevak](docs/quickstart.md).

## Keyboard reference

| Key | Action |
|---|---|
| `Alt+Space` / `Option+Space` | Show or hide Sevak; configurable |
| `↑` / `↓` | Select the previous / next result |
| `Ctrl+P` / `Ctrl+N` | Alternative selection keys |
| `PageUp` / `PageDown` | Move through results by a page |
| `Enter` | Launch, open, or copy the selected result |
| `Ctrl+Enter` / `Shift+Enter` / `Alt+Enter` | Run the result's alternative action (shown under the list) |
| `→` or `Ctrl+K` | Open the action panel for the selected result |
| `Tab` / `Shift+Tab` | Complete the input to the selected result / go up one folder |
| `↑` / `↓` on an empty bar | Recall earlier searches |
| `Ctrl+C` / `Ctrl+L` | Copy the selected result / show it as Large Type |
| `Ctrl+Alt+Space` (in any app) | Universal Actions for what you have selected; configurable |
| `Ctrl+1` … `Ctrl+9` | Run the corresponding result when available |
| `Esc` | Hide the launcher |

On macOS, `Command` takes the place of `Ctrl`.
[Full workflow and CLI reference →](docs/usage.md)

## Privacy and updates

Sevak has no telemetry or analytics. Configuration, local usage history and logs
stay on your machine. The launcher does not send your local search queries to
a cloud search service. Usage history includes your recent searches and the
`>` commands you ran; clipboard history (off by default) is stored unencrypted
in the data folder. Bookmarks are read from your browsers' files, read-only.

There are explicit network uses:

- Selecting a web result opens its URL in your browser, where the chosen
  provider receives your search terms.
- Automatic update checks fetch release information from GitHub after startup
  and once a day. Disable them in **Settings → General** or set
  `general.check_for_updates = false`.
- Installing an update downloads its package after you agree. Windows
  installation may also download WebView2 if it is missing.
- Currency conversion, if you turn it on (`[calculator] currency`, off by
  default), downloads the European Central Bank's daily reference rates at
  most once a day. Unit conversion is always offline.
- Universal Actions reads your selection only when you press its shortcut, by
  briefly borrowing the clipboard and restoring it. The selection is never
  written to disk, logged or sent anywhere (web search actions open your browser
  with the text, like any web search).
- The theme gallery (Settings → Appearance → Theme editor) contacts the
  network only when you click **Browse online themes**: one request for
  `https://raw.githubusercontent.com/ninad-k/Sevak/main/gallery/themes.json`,
  with no cookies or identifying data. Clicking **Install** on a theme then
  downloads that one theme file, and Sevak saves it only if its SHA-256 matches
  the one in the list. The built-in themes and importing or exporting a theme
  file work offline.
- Script plugins you install and allow run with your permissions; what they do
  on the network is up to them. Sevak never downloads plugins itself.

Update signatures are separate from Windows installer signing and macOS
notarization. See [installation notes](docs/install.md) for packaging details
and [data locations](docs/configuration.md#data-and-file-locations) for local files.

## Documentation

| I want to… | Read |
|---|---|
| Get started quickly | [Quick start](docs/quickstart.md) |
| Learn the commands and workflows | [User guide](docs/usage.md) |
| Change folders, shortcuts, themes or search engines | [Configuration](docs/configuration.md) |
| Restyle the launcher with your own CSS | [Themes](docs/themes.md) |
| Fix a shortcut, search or update problem | [Troubleshooting & FAQ](docs/troubleshooting.md) |
| Install on another platform | [Installation](docs/install.md) |
| Understand the name and visual identity | [Brand story](docs/brand.md) |
| Write a plugin (Rust or a script) | [Plugin guide](docs/plugins.md) |
| Build, test or package Sevak | [Development](docs/development.md) |
| Browse every help page | [Documentation index](docs/README.md) |

## Build from source

You need **Rust 1.90+**, **Node.js 22+**, and the native libraries required by
[Tauri 2](https://v2.tauri.app/start/prerequisites/). See the
[development guide](docs/development.md#running) for platform dependencies.

```sh
npm ci
npm run tauri dev        # Desktop app with frontend hot reload
npm run check            # Svelte and TypeScript checks
npm run build            # Production frontend
npx tauri build          # Packages for the current platform
```

## Contributing

Bug reports, documentation improvements and plugins are welcome. Read
[CONTRIBUTING.md](CONTRIBUTING.md) and the [Code of Conduct](CODE_OF_CONDUCT.md).
Include your Sevak version, operating system and reproduction steps in issue
reports. Report security issues through [SECURITY.md](SECURITY.md).

[Apache License 2.0](LICENSE) · Built with Rust, Tauri and Svelte.
