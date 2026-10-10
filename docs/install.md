# Installing Sevak

Download Sevak for your operating system from the [Releases page](https://github.com/ninad-k/Sevak/releases).

!!! warning "Unsigned builds"

    Release builds are currently unsigned. Your OS may show a security warning on first run. This is normal; see the platform-specific instructions below.

## Verify your download

Every release lists the SHA-256 of each file in `SHA256SUMS.txt`, and its
installers carry a build-provenance attestation. Because the installers are not
code-signed yet, it is worth a minute:

```bash
# Linux:   sha256sum --check --ignore-missing SHA256SUMS.txt
# macOS:   shasum -a 256 Sevak_<version>_universal.dmg   (compare with SHA256SUMS.txt)
# Windows: Get-FileHash .\Sevak_<version>_x64-setup.exe  (compare with SHA256SUMS.txt)
gh attestation verify <downloaded file> --repo ninad-k/Sevak
```

What each check proves, the SBOM files and the licence list are explained in
[Supply chain](security/supply-chain.md).

## Package managers (coming soon)

Packages for winget (`NinadKulkarni.Sevak`), Scoop, Homebrew and the AUR
(`sevak-bin`) are prepared but not published yet; progress is tracked in
[#12](https://github.com/ninad-k/Sevak/issues/12). Until then, use the
installers below.

Once they are available, Scoop and AUR installs will leave updates to the
package manager (Sevak's own update check is off there, and "Check for
updates" in the tray names the command to run), while winget and Homebrew
installs keep Sevak's own updater, which those package managers recognise.

## Updates and the beta channel

Sevak checks GitHub for new versions and asks before installing one. By default it follows
the **stable** channel. To try new builds earlier, set **Update channel** to **Beta** in
Settings, General (or `update_channel = "beta"` under `[general]`). Beta builds are named
like `1.3.0-beta.2`, are published as pre-releases and may be less tested. Switching back
to Stable never downgrades you; you get the next stable version that is newer than the
one you have. See [Releasing](releasing.md) for how releases move from beta to stable.

## Windows 10 / 11

1. Download the installer from [Releases](https://github.com/ninad-k/Sevak/releases):
   - **`Sevak_<version>_x64-setup.exe`** (recommended): a guided installer that
     installs for you alone (no admin rights) or for everyone on the PC.
   - **`Sevak_<version>_x64_en-US.msi`**: per-machine MSI for enterprise deployments. Needs administrator rights; MSI updates are automatic.

2. Run the installer and follow the prompts (below).

3. After installation, Sevak appears in **Start → All apps** and **Settings → Apps → Installed apps**. The finish page offers to start it in the tray, open Settings, and add a desktop shortcut. Automatic startup is a separate opt-in; see [Start Sevak after sign-in](#start-sevak-after-sign-in).

!!! warning "SmartScreen warning"

    Windows SmartScreen may show "Windows protected your PC" because the installer is unsigned. Click "More info", then "Run anyway" to proceed.

!!! info "WebView2 runtime"

    Sevak uses Microsoft WebView2. On Windows 11 and recent Windows 10 versions it is already installed. If it is missing, the installer automatically downloads it from Microsoft (internet required).

### What the installer asks

| Page | What it asks |
|---|---|
| Welcome, License | Nothing to decide: what Sevak is, and the Apache 2.0 licence. |
| **Who is Sevak for?** | **Install for me only** (the default): no administrator permission, installs to `%LOCALAPPDATA%\Sevak`. Or **Install for all users on this PC**: Windows asks for administrator permission (UAC) and Sevak goes in `C:\Program Files\Sevak`. **Start Sevak when I sign in** is optional and applies only to the account running the wizard. |
| **Sevak is already installed** | Only shown when Sevak is already there; see [Upgrading](#upgrading-and-reinstalling). |
| Install folder, Start Menu folder | The usual; the folder is not asked again when you upgrade in place. |
| Finish | **Start Sevak now** (on by default), **Open Sevak Settings**, **Create a desktop shortcut**. |

Per user and per PC differ in where Sevak is installed and who can run it:

| | For me only | All users |
|---|---|---|
| Needs administrator permission | No | Yes, a UAC prompt |
| Folder | `%LOCALAPPDATA%\Sevak` | `%ProgramFiles%\Sevak` |
| Apps list entry | Current user's | Every user's (needs admin to uninstall) |
| Start Menu | Your Start Menu | All users' Start Menu |

Settings and data are the same either way: each user's own `%APPDATA%\sevak`.

The startup checkbox is unchecked for a new account and reflects the current
startup state when upgrading. Leaving an existing, OS-disabled entry unchecked
preserves that disable. If the entry is missing, finishing the wizard with the
checkbox unchecked saves startup off so an old saved opt-in cannot restore it.
It never enables startup for every account on a PC. Each other user can opt in
from **Settings → General** after launching Sevak.

The MSI offers the same **Start Sevak when I sign in** choice on its finish
page, separately from launching Sevak now. For a silent MSI installation use
`SEVAK_AUTOSTART=on` or `SEVAK_AUTOSTART=off` to make an explicit choice; omitting
it preserves startup. Startup is still per user, even though MSI installs the
app for the PC. A deployment running as a Windows service account cannot opt
other users in: they must enable it from their own desktop sessions.

Start the EXE installer normally and choose **Install for all users** inside
the wizard if needed. If you instead launch it with **Run as administrator**,
the startup checkbox is disabled because setup cannot safely identify the
original desktop account. Use **Settings → General** after installation. An
explicit `/AUTOSTART` request in this already-elevated case reports a failure
to save startup instead of enrolling the administrator's account.

If you choose **all users** and are not an administrator, Windows asks for an administrator's credentials. If you decline the prompt, the installer returns to the choice so you can pick **for me only**.

### Upgrading and reinstalling

Run a newer installer over an existing installation and the **Sevak is already installed** page names the version you have and offers:

- **Upgrade to Sevak X.Y.Z** (when yours is older; the default),
- **Reinstall Sevak X.Y.Z** (same version),
- **Downgrade to Sevak X.Y.Z (not recommended)** (when yours is newer; an older Sevak may not understand every setting a newer one saved),
- **Uninstall Sevak**.

Upgrading installs over the old files, in the same folder and the same scope, and keeps everything in `%APPDATA%\sevak`: your settings, themes, plugins, workflows, history. If Sevak is running, the installer asks before closing it (a clean quit, so nothing is cut off) and you can start it again from the finish page.

**Sevak's own updates** (the notification and **Check for updates**) run the installer in a quiet "passive" mode and keep the installation where it is: a per-user copy is updated without any prompt, and a per-PC copy shows the Windows administrator prompt, because it is in Program Files.

**Moving between per-user and per-PC.** If Sevak is installed for one scope and you pick the other, the installer warns that two copies would compete for the hotkey and offers to **replace** the existing copy (recommended: it is removed, your settings stay) or **keep both**. Removing a per-PC copy from a per-user install asks for administrator permission. If both copies already exist when you upgrade one, a check box offers to remove the other.

When the install location changes, setup updates the executable path in an
existing startup entry, keeping its arguments, custom configuration path and
Windows-disabled state. This refresh does not opt you in or create an entry.
Explicit startup choices and path refresh complete before setup automatically
launches Sevak.

### Silent and scripted installs

The installer accepts these switches, so it can be deployed without a wizard:

| Switch | Effect |
|---|---|
| `/S` | Silent: no windows at all. Without `/ALLUSERS`, a silent install is **per user**, or in the scope of the copy already installed. |
| `/P` | Passive: a progress bar and no questions. This is what Sevak's own updater uses. |
| `/CURRENTUSER` | Install for the current user. |
| `/ALLUSERS` | Install for all users. Windows shows the administrator prompt unless the installer already runs elevated (an elevated command prompt, a deployment tool). If the prompt is refused the exit code is 1223. |
| `/D=C:\Some\Folder` | Install folder. Must be the last argument, with no quotes, even if the path has spaces. |
| `/NS` | Do not create shortcuts. Without it, a silent or passive first install creates the Start Menu and desktop shortcuts. |
| `/UNINSTALLOTHER` | Also remove a copy installed in the other scope. Without it, a silent install leaves the other copy alone. |
| `/R` | After a `/S` or `/P` install, start Sevak. |
| `/AUTOSTART=on` or `/AUTOSTART=off` | Explicitly enable or disable start at sign-in for the installing desktop user. Without this switch, silent/passive installs preserve the existing preference. This is separate from `/R`, which only starts Sevak now. |

```powershell
# For the current user, no prompts (what winget runs)
.\Sevak_<version>_x64-setup.exe /S

# For every user (run from an elevated prompt, or accept the UAC prompt)
.\Sevak_<version>_x64-setup.exe /S /ALLUSERS

# A specific folder
.\Sevak_<version>_x64-setup.exe /S /CURRENTUSER /D=D:\Tools\Sevak

# For the current user, with automatic start after sign-in
.\Sevak_<version>_x64-setup.exe /S /CURRENTUSER /AUTOSTART=on
```

Silent installs never wait for an answer: if Sevak is running it is asked to quit and then closed by Windows' Restart Manager, and the installer carries on. A silent install over an existing copy upgrades it in place, whatever its version.

### Uninstalling

Open Settings → Apps → Installed apps, find Sevak, and click Uninstall (a per-PC copy asks for administrator permission first). You can also run the installer again and choose **Uninstall Sevak**, or run `winget uninstall NinadKulkarni.Sevak` once Sevak is on winget.

The uninstaller offers **Also delete the web cache**, which removes only the web view's cache folders (`%LOCALAPPDATA%\com.ninad.sevak` and `%APPDATA%\com.ninad.sevak`). Your Sevak settings, history and plugins in `%APPDATA%\sevak\` are always preserved; see [Files and data](files-and-data.md) to delete them.

For scripts: `"%LOCALAPPDATA%\Sevak\uninstall.exe" /CURRENTUSER /S` (per user) or `"%ProgramFiles%\Sevak\uninstall.exe" /ALLUSERS /S` (per PC, with the administrator prompt).

Startup registration is per user too. The Windows uninstaller removes the
current user's entry only when it points at the copy being removed; it must
not remove an entry for a different installation. Other users should turn off
startup in their own Settings before a machine-wide uninstall. Turn it off
before deleting the macOS app or removing a Linux package as well; removing
an application file does not reliably remove a user's startup configuration.

## Start Sevak after sign-in

Automatic startup is optional and off by default. On any supported OS:

1. Open Sevak's **Settings → General**.
2. Turn on **Start Sevak when I sign in** and click **Save**.
3. Sign out and back in to test it. Sevak should be running in the tray or menu
   bar with the launcher hidden; press your configured shortcut to open it.

This also works after a restart, once you sign in. Turn the option off and
**Save** to stop automatic starts. It controls only your account. The Windows
installer checkbox and this setting write the same preference; **Start Sevak
now** on the finish page does not enable future startup.

| OS | Where startup is registered and managed |
|---|---|
| Windows | A per-user entry shown under **Settings → Apps → Startup** and **Task Manager → Startup apps**. Installing Sevak also adds its separate Start menu and Installed apps entries, whether startup is on or off. |
| macOS | A user LaunchAgent. macOS can restrict it in **System Settings → General → Login Items** (the section's name varies with macOS version). Move Sevak to Applications before enabling it. |
| Linux | A desktop autostart entry for your account. Desktop environments expose this in their startup applications settings. Keep an AppImage in a permanent folder before enabling it. |

macOS `.dmg` files and Linux packages do not run the Windows installer wizard.
Use the in-app setting after installing and launching Sevak. Linux autostart
needs a graphical desktop session that supports the XDG autostart standard.

Settings shows an error if it cannot update startup registration. It also
warns if a saved opt-in is disabled by the OS or its entry is missing. In
Windows Startup apps or Linux startup settings, enable Sevak again, or save
the in-app option off and then on. Administrative policies may still block
startup; an enabled preference cannot override those policies. See
[Settings](settings.md#start-sevak-when-i-sign-in) for more detail.

### Legacy Windows startup

An older Sevak build run as administrator could register startup for every
account. If Sevak finds that machine-wide entry pointing at this executable,
Settings reports it instead of claiming that the per-user switch can disable
it. Sevak does not change machine-wide startup entries automatically.

Ask an administrator to inspect the **Sevak** value under
`HKEY_LOCAL_MACHINE\Software\Microsoft\Windows\CurrentVersion\Run`, confirm that
it points to the old Sevak installation, and remove **only that value**. Then
use **Settings → General → Start Sevak when I sign in** for each account that
wants startup. This is a migration step for affected older installations; a
new installation uses per-user startup.

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
- **Hotkey:** The default is ++cmd+space++, which Spotlight uses. If Spotlight's shortcut is on, Sevak asks once whether it may turn that shortcut off; say No and Sevak uses ++option+space++ instead. Change it any time in Settings or the config file, and restore Spotlight's shortcut from Settings. See [Cmd+Space and Spotlight](troubleshooting.md#super-space).
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

Sevak registers hotkeys directly. Your configured hotkey works out of the box, except the default ++super+space++ on GNOME, which switches input sources: Sevak then asks once whether it may move that GNOME shortcut (see below).

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

!!! warning "GNOME and Super+Space"

    GNOME binds ++super+space++ to switching input sources, which blocks Sevak's default hotkey. `sevak --setup-hotkey` (and the **Set up GNOME shortcut** button in Settings) offers to move that shortcut to ++ctrl+super+space++, and only does it after you confirm. It saves the old value; `sevak --restore-hotkey` or the **Restore** button puts it back. On X11, Sevak asks the same question the first time it cannot register ++super+space++.

    Prefer to leave GNOME alone? Choose ++ctrl+space++ or ++ctrl+alt+space++ in Settings.

!!! warning "GNOME Alt+Space conflict"

    GNOME binds ++alt+space++ to the window menu by default, which blocks Sevak's hotkey if you choose it. Pick a different key or free ++alt+space++:

    ```bash
    gsettings set org.gnome.desktop.wm.keybindings activate-window-menu "[]"
    ```

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
