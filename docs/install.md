# Installing Sevak

Download packages from the [Releases page](https://github.com/ninad-k/Sevak/releases).
Release builds are currently **unsigned**.

## Package managers

| Platform | Install | Update |
|---|---|---|
| Windows (winget) | `winget install NinadKulkarni.Sevak` | Sevak updates itself, or `winget upgrade NinadKulkarni.Sevak` |
| Windows (Scoop) | `scoop bucket add ninad-k https://github.com/ninad-k/scoop-bucket`, then `scoop install sevak` | `scoop update sevak` |
| macOS (Homebrew) | `brew install --cask ninad-k/tap/sevak` | Sevak updates itself, or `brew upgrade --cask sevak` |
| Arch Linux (AUR) | `yay -S sevak-bin` | `yay -Syu` |

Scoop and AUR installs leave updates to the package manager: Sevak's own
update check is off there, and "Check for updates" in the tray tells you which
command to run. winget and Homebrew installs keep Sevak's own updater, which
those package managers recognise.

## Windows 10 / 11

| File | Notes |
|---|---|
| `Sevak_<version>_x64-setup.exe` (NSIS) | Recommended. Installs per user into `%LOCALAPPDATA%`; no administrator rights needed. Start Menu folder: "Sevak". |
| `Sevak_<version>_x64_en-US.msi` (WiX) | Per-machine install (needs administrator rights), for managed deployments. The MSI upgrade code is fixed, so newer MSIs upgrade older ones. |

- Because the installers are unsigned, Windows SmartScreen may show "Windows
  protected your PC". Choose "More info" then "Run anyway".
- Sevak needs the Microsoft **WebView2** runtime. It is preinstalled on Windows
  11 and current Windows 10; if missing, the installer downloads the
  Evergreen bootstrapper from Microsoft (internet needed during install).
- Uninstall from Settings, Apps. Your config and usage data in
  `%APPDATA%\sevak\` are kept; delete the folder to remove them.

## macOS 11+

Download `Sevak_<version>_universal.dmg` (runs natively on Apple silicon and
Intel), open it and drag Sevak into Applications.

- The app is not notarized by Apple yet, so the first launch is blocked with
  "Apple could not verify Sevak". Open **System Settings → Privacy & Security**,
  scroll down and click **Open Anyway** (or run
  `xattr -dr com.apple.quarantine /Applications/Sevak.app`).
- Sevak lives in the menu bar (no Dock icon). The default hotkey is
  `Alt+Space` (Option+Space); change it in Settings.
- File search asks for permission the first time it indexes Desktop, Documents
  or Downloads. Decline and remove those folders in Settings if you don't use it.
- Config and usage data: `~/Library/Application Support/sevak/`.
- Pasting from clipboard history and snippets sends Cmd+V to the app you were
  using, which macOS only allows once you turn Sevak on under **System Settings
  → Privacy & Security → Accessibility**. Until then those results copy instead
  and say "Copies to clipboard". If you update Sevak and pasting stops, remove
  Sevak from that list and add it again (the build is ad-hoc signed, so macOS
  may treat each version as a new app).

## Ubuntu 22.04+ / Debian

```sh
sudo apt install ./Sevak_<version>_amd64.deb
```

Dependencies (WebKitGTK 4.1, GTK 3, `libayatana-appindicator3-1`, glib tools)
are pulled in by apt. Then start "Sevak" from the application menu, or run `sevak`.

## Fedora 39+

```sh
sudo dnf install ./Sevak-<version>-1.x86_64.rpm
```

Dependencies include `webkit2gtk4.1`, `gtk3`, `libayatana-appindicator-gtk3`
and `glib2`.

## AppImage (other distributions)

```sh
chmod +x Sevak_<version>_amd64.AppImage
./Sevak_<version>_amd64.AppImage
```

AppImages need FUSE 2 (`libfuse2` on Ubuntu/Debian, `fuse-libs` on Fedora). If
you cannot install it, run with `--appimage-extract-and-run`. The AppImage is
built on Ubuntu 22.04 and needs glibc 2.35 or newer. Install the AppImage
somewhere permanent before using `sevak --setup-hotkey`, since the shortcut
runs the command it was set up from.

## Setting up the hotkey on Linux

- **X11** sessions: `Alt+Space` (the `general.hotkey` setting) works directly.
- **Wayland** sessions: applications cannot grab global keys. On GNOME run:

  ```sh
  sevak --setup-hotkey              # uses the hotkey from config.toml
  sevak --setup-hotkey Ctrl+Space   # or choose another
  ```

  It adds a GNOME custom keyboard shortcut that runs `sevak --toggle`. On
  other desktops (KDE, Sway, ...) bind a key to `sevak --toggle` yourself in
  the desktop's keyboard settings.
- **GNOME binds `Alt+Space` to the window menu.** `--setup-hotkey` warns when a
  GNOME shortcut already uses the key. Either free it:

  ```sh
  gsettings set org.gnome.desktop.wm.keybindings activate-window-menu "[]"
  ```

  or pick another key (`Ctrl+Space`, `Super+Space`, `Ctrl+Alt+Space`). Note
  that `Super+Space` switches input sources on GNOME when several layouts
  are configured.
- By default, on Wayland Sevak draws its window through XWayland so that it can
  center itself and take focus (`linux.wayland_use_xwayland = true`).

## Tray icon on GNOME (Fedora, Ubuntu)

Sevak shows a tray icon (Show, Settings, Reload index, Quit) through the
AppIndicator protocol. Ubuntu ships an AppIndicator extension enabled. Stock
Fedora GNOME does not: install **"AppIndicator and KStatusNotifierItem
Support"** (package `gnome-shell-extension-appindicator`, or from
extensions.gnome.org) and enable it. Without a tray Sevak still runs; use the
hotkey, and `sevak --quit` to exit.

## Red Hat Enterprise Linux 9 and clones (Rocky, AlmaLinux)

**The prebuilt packages do not run on RHEL 9 / Rocky 9 / AlmaLinux 9.**

Sevak uses Tauri v2, which needs the WebKitGTK **4.1** API
(`webkit2gtk4.1`). Findings, from distribution package listings and reports of
others (not verified by installing on an EL9 machine):

- RHEL 9, Rocky 9 and AlmaLinux 9 ship only `webkit2gtk3` (the 4.0 API).
  `webkit2gtk4.1` is in none of BaseOS, AppStream, CRB or EPEL 9, so the `.rpm`
  cannot satisfy its dependency.
- `libayatana-appindicator-gtk3` is available from EPEL 9 (0.5.94), so the tray
  library itself is not the problem.
- The AppImage bundles its own WebKit but is built against glibc 2.35 and EL9
  has glibc 2.34, so it fails to start.
- AlmaLinux 10 reportedly carries `webkit2gtk4.1` in EPEL 10, so the `.rpm`
  is expected to work on EL10 clones with EPEL enabled (also unverified; RHEL 10
  proper is not confirmed).

Options on EL9: run Sevak in a Fedora or Ubuntu container (for example
distrobox) with display access, build from source on a newer distribution, or
use Fedora/Ubuntu. "EL10 with EPEL" is the realistic enterprise-Linux target.

## Troubleshooting

- **Nothing happens when I press the hotkey on Linux**: you are probably on
  Wayland. Run `sevak --setup-hotkey`.
- **Updates**: Sevak checks for a new version at startup and daily and asks
  before installing it (tray menu: "Check for updates"). Turn this off with
  `general.check_for_updates = false` or in Settings. On Windows the update runs
  the installer in passive mode; for `.deb` and `.rpm` installs you are asked
  for your password.
- **Logs**: `%APPDATA%\sevak\logs\` (Windows),
  `~/Library/Application Support/sevak/logs/` (macOS), `~/.local/share/sevak/logs/` (Linux).
- **Start in the background at login**: set `general.launch_at_login = true`, or
  add `sevak --background` to your session's autostart.
