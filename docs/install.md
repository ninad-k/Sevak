# Installing Sevak

Download Sevak for your operating system from the [Releases page](https://github.com/ninad-k/Sevak/releases).

!!! warning "Unsigned builds"

    Release builds are currently unsigned. Your OS may show a security warning on first run. This is normal; see the platform-specific instructions below.

## Package managers (coming soon)

Packages for winget (`NinadKulkarni.Sevak`), Scoop, Homebrew and the AUR
(`sevak-bin`) are prepared but not published yet; progress is tracked in
[#12](https://github.com/ninad-k/Sevak/issues/12). Until then, use the
installers below.

Once they are available, Scoop and AUR installs will leave updates to the
package manager (Sevak's own update check is off there, and "Check for
updates" in the tray names the command to run), while winget and Homebrew
installs keep Sevak's own updater, which those package managers recognise.

## Windows 10 / 11

1. Download the installer from [Releases](https://github.com/ninad-k/Sevak/releases):
   - **`Sevak_<version>_x64-setup.exe`** (recommended): Per-user install, no admin rights needed.
   - **`Sevak_<version>_x64_en-US.msi`**: Per-machine install for enterprise deployments. Needs administrator rights; MSI updates are automatic.

2. Run the installer and follow the prompts.

3. After installation, Sevak appears in your Start Menu and runs in the background at startup.

!!! warning "SmartScreen warning"

    Windows SmartScreen may show "Windows protected your PC" because the installer is unsigned. Click "More info", then "Run anyway" to proceed.

!!! info "WebView2 runtime"

    Sevak uses Microsoft WebView2. On Windows 11 and recent Windows 10 versions it is already installed. If it is missing, the installer automatically downloads it from Microsoft (internet required).

**Uninstall:** Open Settings → Apps → Installed apps, find Sevak, and click Uninstall. Your config and data in `%APPDATA%\sevak\` are preserved; see [Files and data](files-and-data.md) to delete them.

## macOS 11+

1. Download `Sevak_<version>_universal.dmg` from [Releases](https://github.com/ninad-k/Sevak/releases).
   - The .dmg runs natively on Apple silicon and Intel Macs.

2. Open the .dmg and drag Sevak into Applications.

3. Open Applications and click Sevak to launch it.

!!! warning "Gatekeeper warning"

    macOS blocks the first launch with "Apple could not verify Sevak" because the app is not notarized.
    
    Click **Open Anyway** in System Settings → Privacy & Security (scroll down to find it).
    
    Alternatively, run in Terminal:
    ```bash
    xattr -dr com.apple.quarantine /Applications/Sevak.app
    ```

### Usage

- **Menu bar:** Sevak lives in your menu bar (top-right), not the Dock.
- **Hotkey:** The default is ++alt+space++ (Option+Space). Change it in Settings or the config file.
- **File search permissions:** Sevak asks for permission to index Desktop, Documents and Downloads. Decline if you don't use file search; remove those folders in Settings.
- **Pasting:** Clipboard history and snippets require Accessibility permission to paste with Cmd+V. Without it, they copy to the clipboard instead.
  - Go to System Settings → Privacy & Security → Accessibility and add Sevak.
  - If pasting stops after updating, remove Sevak and re-add it (ad-hoc signed builds are treated as new apps).
- **Contacts:** The optional contacts plugin (`[contacts] enabled = true`) asks for access to your contacts the first time you press Enter on its "Allow" row; nothing is asked at startup. Change the answer under System Settings → Privacy & Security → Contacts.
- **Snippet expansion:** Expanding snippets as you type (opt-in, `[snippets] auto_expand`) also needs **Input Monitoring** to see the keys; macOS asks the first time, and Sevak must be restarted after you allow it.

**Uninstall:** Delete `/Applications/Sevak.app`. Config and data remain at `~/Library/Application Support/sevak/`; see [Files and data](files-and-data.md) to delete them.

## Ubuntu 22.04+ / Debian

Download `Sevak_<version>_amd64.deb` from [Releases](https://github.com/ninad-k/Sevak/releases), then install:

```bash
sudo apt install ./Sevak_<version>_amd64.deb
```

Start Sevak from your application menu, or run:

```bash
sevak
```

**Uninstall:**

```bash
sudo apt remove sevak
```

## Fedora 39+

Download `Sevak-<version>-1.x86_64.rpm` from [Releases](https://github.com/ninad-k/Sevak/releases), then install:

```bash
sudo dnf install ./Sevak-<version>-1.x86_64.rpm
```

**Uninstall:**

```bash
sudo dnf remove Sevak
```

## AppImage (other distributions)

Download `Sevak_<version>_amd64.AppImage` from [Releases](https://github.com/ninad-k/Sevak/releases), then run:

```bash
chmod +x Sevak_<version>_amd64.AppImage
./Sevak_<version>_amd64.AppImage
```

!!! info "Requirements"

    AppImages need FUSE 2 (install `libfuse2` on Ubuntu/Debian, `fuse-libs` on Fedora).
    
    If you cannot install FUSE, extract and run instead:
    ```bash
    ./Sevak_<version>_amd64.AppImage --appimage-extract-and-run
    ```
    
    The AppImage is built on Ubuntu 22.04 and requires glibc 2.35 or newer.

!!! tip "Permanent installation"

    Install the AppImage somewhere permanent (not `/tmp` or a temporary folder) before using `sevak --setup-hotkey`, since desktop shortcuts store the full path to the executable.

## Setting up the hotkey on Linux

### X11 sessions

Sevak registers hotkeys directly. Your configured hotkey (default ++alt+space++) works out of the box.

### Wayland sessions

Applications cannot grab global keys on Wayland. Sevak provides `sevak --setup-hotkey` to ask the desktop to handle keys:

**On GNOME:**

```bash
sevak --setup-hotkey              # use hotkey from config.toml
sevak --setup-hotkey "Ctrl+Space" # or choose a different key
```

This creates GNOME custom keyboard shortcuts for:

- Main hotkey (`[general] hotkey`) → `sevak --toggle`
- Universal Actions hotkey (`[general] actions_hotkey`) → `sevak --actions`
- Each `[[hotkey]]` entry in config.toml → `sevak --query '<text>'` or `sevak --run <id>`

After editing `config.toml`, run `--setup-hotkey` again to update shortcuts. Delete old shortcuts in GNOME Settings → Keyboard Shortcuts if you remove config entries.

**On other desktops (KDE, Sway, etc.):**

Bind a key to `sevak --toggle` in your desktop's keyboard settings. The `--setup-hotkey` command only works on GNOME.

!!! warning "GNOME Alt+Space conflict"

    GNOME binds ++alt+space++ to the window menu by default, which blocks Sevak's hotkey.
    
    Choose a different key (e.g. ++ctrl+space++, ++super+space++, ++ctrl+alt+space++) or free ++alt+space++:
    
    ```bash
    gsettings set org.gnome.desktop.wm.keybindings activate-window-menu "[]"
    ```
    
    Note: ++super+space++ switches input sources if you have multiple keyboard layouts configured.

### Wayland rendering

By default, Sevak uses XWayland (X11 compatibility layer) on Wayland to position itself and take focus reliably. You can use the native Wayland backend instead by setting `[linux] wayland_use_xwayland = false` in your config, though the window may appear off-centre or not accept input.

## Tray icon on GNOME (Ubuntu and Fedora)

Sevak displays a tray icon through the AppIndicator protocol.

**Ubuntu:** The AppIndicator extension is enabled by default. You should see the tray icon.

**Fedora:** Install the AppIndicator extension:

```bash
sudo dnf install gnome-shell-extension-appindicator
```

Then enable it in GNOME Extensions or at extensions.gnome.org, or restart GNOME Shell:

```bash
killall -HUP gnome-shell
```

**Without a tray:** Sevak still runs. Use the hotkey to show it, and run `sevak --quit` to exit. Open Settings with `sevak --settings`.

## Red Hat Enterprise Linux 9 and clones (Rocky, AlmaLinux)

!!! warning "Not supported on RHEL 9 / Rocky 9 / AlmaLinux 9"

    Prebuilt packages do not work on EL9. Sevak uses Tauri v2, which requires WebKitGTK 4.1 (`webkit2gtk4.1`). EL9 distributions only ship WebKitGTK 4.0 (`webkit2gtk3`), which is not compatible and is not available in any EL9 repository (BaseOS, AppStream, CRB, or EPEL 9).
    
    The AppImage also fails to run on EL9 (compiled against glibc 2.35, EL9 has 2.34).

**Workarounds on EL9:**

- Run Sevak in a **Fedora or Ubuntu container** (e.g. distrobox) with display access.
- **Build from source** on a Fedora or newer distribution.
- Use **Fedora or Ubuntu** directly.
- Wait for **EL10 with EPEL**: AlmaLinux 10 reportedly ships `webkit2gtk4.1` in EPEL 10, so the `.rpm` is expected to work on EL10 clones.
