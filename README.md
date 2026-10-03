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

Anything else searches apps (and files, if `files.global` is on); an
expression like `12*7` or a conversion like `10 km in mi` shows the calculator. When nothing matches, the fallback
web search (`g` by default) is offered.

The tray menu has Show, Settings (opens the settings window), Reload index
(re-reads the config and rescans apps and files) and Quit.

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

Usage statistics are in `usage.json` and logs in `logs/`, both under
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
disabled = []          # "apps", "calculator", "files", "web:<keyword>"

[calculator]
currency = false       # true: convert currencies with the ECB's daily rates (network)

[files]
directories = ["~/Desktop", "~/Documents", "~/Downloads"]
max_depth = 4
include_hidden = false
keyword = "f"
global = true

[linux]
wayland_use_xwayland = true

[[web_search]]
keyword = "g"
name = "Google"
url = "https://www.google.com/search?q={query}"
```

Defining any `[[web_search]]` entry replaces the default list. After editing,
choose "Reload index" from the tray menu or restart Sevak.

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

If you turn on currency conversion (`[calculator] currency`, off by default),
Sevak also downloads
`https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml` (the European
Central Bank's daily reference rates) in the background, at most once a day
and only when the saved rates are over a day old (the ECB sees your IP address,
nothing else is sent; no cookies, no identifiers). The rates are kept in
`currency-rates.json` in the data folder. With the option off, no such request
is ever made. Unit conversion is always offline.

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
