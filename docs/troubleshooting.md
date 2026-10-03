# Troubleshooting

## Start here

Before diving into specific issues, try these first:

1. **Confirm Sevak is running:** Look for the tray icon (Windows/Linux) or menu bar icon (macOS). If it is missing, open Sevak from your application menu or run `sevak` in Terminal.

2. **Check your hotkey:** Open Settings and look at the configured shortcut under General. Try a different key if the current one is already in use by another app or your desktop.

3. **Reload the index:** After installing a new app or changing file search settings, open the tray menu and choose **Reload index**.

4. **Check the logs:** If the problem persists, open Settings → "Reveal logs folder" and look at the most recent log file. Enable debug logging with `SEVAK_LOG=debug sevak` for more detail.

## The hotkey does not work

**Diagnosis:** Open the tray/menu bar icon and click "Show". If the launcher appears, Sevak is running and the hotkey is not registered correctly.

**Try these steps:**

1. **Choose a different key** in Settings → General. Your current key may be in use by another app or your desktop.

2. **macOS:** The default is ++option+space++ (Alt and Option are the same key on Mac). If it doesn't work, Option+Space might be reserved by Spotlight or another app—try a different key.

3. **Linux Wayland (GNOME):** Sevak cannot register global keys on Wayland. Run this command:

   ```bash
   sevak --setup-hotkey
   ```
   
   This creates a GNOME custom keyboard shortcut. If you get a conflict warning, follow the instructions to free ++alt+space++ or choose another key.

4. **Linux Wayland (other desktops):** Bind ++alt+space++ or another key to `sevak --toggle` in your desktop's keyboard settings (KDE Settings, Sway config, etc.).

5. **Linux X11 on GNOME:** ++alt+space++ is reserved for the window menu. Try ++ctrl+space++ or ++super+space++ (but ++super+space++ switches input sources if you have multiple layouts). Or free ++alt+space++:

   ```bash
   gsettings set org.gnome.desktop.wm.keybindings activate-window-menu "[]"
   ```

## There is no tray icon on Linux

Sevak shows a tray icon through AppIndicator. Some GNOME sessions do not have the AppIndicator extension enabled.

**Ubuntu:** The extension is enabled by default. If there is no tray icon, check that GNOME Extensions are working.

**Fedora:** Install and enable the AppIndicator extension:

```bash
sudo dnf install gnome-shell-extension-appindicator
killall -HUP gnome-shell  # or restart GNOME
```

**Without a tray:** Sevak still works. Use your hotkey to show it, `sevak --quit` to exit, and `sevak --settings` to open Settings.

## An installed app is missing

- Choose **Reload index**, wait for indexing, and search again.
- Confirm the **Applications** plugin is enabled.
- Try the application's full name.
- Confirm the app has a platform launcher entry: a Start Menu shortcut or
  packaged app on Windows, an application bundle on macOS, or a desktop entry
  on Linux. Merely copying an executable into a folder may not create one.

## A file or folder is missing

Check these in order:

| Check | What to do |
|---|---|
| Query length | Use at least two characters after `f ` |
| Indexed root | Add the containing folder in Settings → Files |
| Search depth | Increase depth if the file is nested too deeply |
| Dot-file visibility | Enable hidden-file indexing if needed |
| Excluded directory | Generated/cache directories are deliberately pruned |
| New or moved file | Reload the index |
| Permissions | Ensure your user account can read that directory |
| Index size | Reduce broad roots if you hit the 100,000-entry cap |

File search matches names, not document contents. A word appearing only
inside a PDF or document is not a filename match.
[Full file-search behavior](usage.md#files-and-folders).

## I get a web suggestion instead of a local result

The fallback web engine is offered when an ordinary query has no matches.
It does not mean your local query has already been sent to that provider.
A browser request happens when you execute the web result.

Check local plugins and indexing first, or disable the fallback under
**Settings → Search**.

## A web keyword does not work

Use `g rust traits`, with a space after `g`. Then check that:

- The engine exists and is enabled.
- Its keyword is unique and has no spaces.
- Its URL starts with HTTP(S) and contains `{query}`.
- Your custom TOML engine list includes all the default engines you still want.

Defining `[[web_search]]` entries replaces the default list. See [Web search configuration](configuration.md#web_search) for examples and details.

## A calculation looks unexpected

Check parentheses and operators. `%` means remainder; use `200*15/100`
for 15% of 200. Powers have precedence over a leading minus, so `-2^2` is
`-4`; use `(-2)^2` for `4`. Press Enter to **copy** the answer, then paste it
where you need it.

Sevak uses finite-precision arithmetic. Invalid expressions and operations
such as division by zero do not produce a valid copyable answer.

## The launcher disappears

This is expected with **Hide on blur** enabled: the launcher hides when
another window takes focus. Turn it off under Settings → General if you prefer.
Pressing Esc also hides the launcher; **Quit** in the tray stops the app.

## Settings will not save

The Settings window shows validation errors if there are problems. Fix them in order:

**Common issues:**

- Duplicate web search keywords (each must be unique)
- URL without `{query}` placeholder
- Invalid or empty hotkey
- Fallback web search engine that no longer exists
- Blank fields where text is required

**Hand-edited TOML files:**

Check for syntax errors (unmatched quotes, missing brackets, typos in section names). If the file can't be read, Sevak starts with default settings (or, on **Reload index**, keeps the settings already in use) and writes the reason to the log. See [Files and data](files-and-data.md) for the config path and how to back it up.

## An update check fails

Confirm internet access and that GitHub Releases is reachable. The release
must include a valid update manifest and a compatible package. Automatic
checks run in release builds; development/debug builds skip them.

Use **Check for updates** in the tray to see the immediate outcome. You can
also check the [release page](https://github.com/ninad-k/Sevak/releases) and
follow the [installation guide](install.md). Turning off automatic checks
does not disable the manual menu action.

## Why did the installer show an operating-system warning?

Updater signatures and OS code signing are different mechanisms. An update
can be signed for Sevak's updater while an installer is not yet signed for
Windows or notarized by Apple. Read the
[platform installation notes](install.md) for the current status and make
sure the package came from the project's release page.

## Does Sevak work offline?

App search, indexed file search, calculations and local ranking work without
network access. Web searches, checking for a release, and downloading updates
need an internet connection. Indexes depend on locally accessible applications
and folders.

## Where is my data? Does Sevak collect analytics?

**No telemetry.** Sevak does not collect analytics. All config, usage history, and logs stay on your machine.

**Network activity:**

- **Optional update checks** (default on): Check GitHub for a new version at startup and daily. Disable with `[general] check_for_updates = false`.
- **Web searches:** When you run a web search, your browser contacts that search provider (Google, YouTube, GitHub, etc.). Sevak does not proxy or log these.
- **Currency conversion** (off by default): When enabled, Sevak downloads the European Central Bank's daily rates once per day for currency conversion.

See [Files and data](files-and-data.md) for where everything is stored, and [Configuration](configuration.md) for the settings.

## Reporting a problem

Open an issue through the [repository's issue form](https://github.com/ninad-k/Sevak/issues/new/choose)
and include:

- Sevak version and operating system.
- Linux desktop and X11/Wayland session type, if relevant.
- The query or shortcut that reproduces the issue.
- What you expected and what happened.
- Relevant recent log lines, with private paths or unrelated data removed.

Send security issues through [SECURITY.md](https://github.com/ninad-k/Sevak/blob/main/SECURITY.md) instead of a public issue.
