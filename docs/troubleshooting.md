# Troubleshooting and frequently asked questions

[← Help center](README.md) · [Installation](install.md) · [Configuration](configuration.md)

## Start here

1. Confirm Sevak is running: use the tray/menu-bar icon or open it from your
   application menu.
2. Check your configured shortcut under **Settings → General**.
3. Choose **Reload index** after changing installed apps or indexed files.
4. If the problem persists, open **Settings → Reveal logs folder** and look at
   the most recent entries.

## The shortcut does not open Sevak

**Try Show from the tray.** If that opens the launcher, the app is running and
the problem is likely shortcut registration or a desktop binding.

- Choose a different key combination in Settings if the current one is
  already in use.
- On macOS, the default is **Option+Space**.
- On Linux Wayland, configure a desktop shortcut that runs `sevak --toggle`.
  On GNOME, `sevak --setup-hotkey` can create it for you.
- GNOME commonly uses Alt+Space for its window menu. Pick another combination
  or follow the [hotkey instructions](install.md#setting-up-the-hotkey-on-linux).

## There is no tray icon on Linux

Some GNOME sessions do not provide a tray host. Sevak can still work through
the hotkey or `sevak --toggle`. See
[GNOME tray support](install.md#tray-icon-on-gnome-fedora-ubuntu).
You can open Settings with `sevak --settings` and exit with `sevak --quit`.

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

Defining `[[web_search]]` entries replaces the default list.
[Complete example](configuration.md#add-a-web-search-keyword).

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

Resolve the validation messages in the form. Common causes include duplicate
web keywords, a URL without `{query}`, an invalid shortcut, or a fallback
engine that no longer exists. A Save button can also be disabled simply
because nothing has changed.

For hand-edited TOML, check quotes and section names. On a failed reload,
Sevak retains its current valid configuration. Back up the file before
attempting a reset; see [data locations](configuration.md#data-and-file-locations).

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
need an internet connection, as does the optional workflow gallery. Indexes depend on locally accessible applications
and folders.

## Where is my data? Does Sevak collect analytics?

Sevak has no telemetry or analytics. Config, usage history and logs remain
local. Optional automatic update checks contact GitHub; a web search contacts
the chosen provider through your browser when you select it.
[Locations and update settings](configuration.md#data-and-file-locations).

## Reporting a problem

Open an issue through the [repository's issue form](https://github.com/ninad-k/Sevak/issues/new/choose)
and include:

- Sevak version and operating system.
- Linux desktop and X11/Wayland session type, if relevant.
- The query or shortcut that reproduces the issue.
- What you expected and what happened.
- Relevant recent log lines, with private paths or unrelated data removed.

Send security issues through [SECURITY.md](../SECURITY.md) instead of a public issue.
