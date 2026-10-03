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

`f` matches names in your indexed folders, not document contents. A file
outside them, or a word that is only inside a document, needs `ff` or `in`
([whole-disk search](usage.md#whole-disk-and-content-search)). If those say
"File index unavailable", start the Windows Search service (`WSearch`), install
`plocate` and run `updatedb`, or install Tracker or Baloo, as the row explains;
the OS only finds what it has indexed, so a file created seconds ago may take a
moment to appear.
[Full file-search behavior](usage.md#files-and-folders).

## I get a web suggestion instead of a local result

The fallback web engine is offered when an ordinary query has no matches.
It does not mean your local query has already been sent to that provider.
A browser request happens when you execute the web result.

Check local plugins and indexing first, or disable the fallback under
**Settings → Search**.

## `c`, `1p`, `define` or `spell` does nothing

- **Contacts and 1Password are off by default.** The row says "Contacts are off"
  or "1Password is off"; add `[contacts] enabled = true` or
  `[onepassword] enabled = true` to `config.toml` and choose **Reload index**.
- **No contacts found.** The plugin reads vCard files from `vcard_files` and, with
  `use_system = true`, the system address book. On macOS the first `c` shows
  "Allow Sevak to read your Contacts": press Enter and answer the system
  question; if you said no, enable Sevak under *System Settings → Privacy &
  Security → Contacts*. On Windows the People store may be empty when your
  accounts' contacts are not synced there; export a `.vcf` file instead. On Linux
  only Evolution's local address books are read.
- **1Password says `op` was not found or is not signed in.** Install the
  [1Password CLI](https://developer.1password.com/docs/cli/get-started/), turn on
  *Settings → Developer → Integrate with 1Password CLI* in the 1Password app, and
  run `op account list` in a terminal to check. If `op` is somewhere unusual, set
  `[onepassword] op_path`. After you dismiss the unlock prompt, press Enter on
  "Try again". Sevak never reads or shows passwords.
- **`define` finds nothing.** The bundled dictionary has single words and their
  common inflections, not phrases or names of people. Check the spelling with
  `spell`.
- **`spell` calls a word wrong that is right.** The bundled word list is general
  English. Specialist words are missing; on Linux the hunspell lists in
  `/usr/share/hunspell` are added when installed.

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

## An automation task or media control is missing

Sevak lists only what can work on your machine. Type `t ` to see the tasks
that are available. On Linux, tasks that need a helper program are hidden until
it is installed: `wmctrl` (show desktop), `gnome-screenshot`, `spectacle` or
`flameshot` (screenshot), `wpctl`, `pactl` or `amixer` (volume), `nmcli` or
`rfkill` (Wi-Fi), `rfkill` or `bluetoothctl` (Bluetooth), `udisksctl` (eject),
`systemd-inhibit` (keep awake) and `playerctl` (the media buttons and the
playing track). On macOS, Bluetooth needs [blueutil](https://github.com/toy/blueutil),
and dark mode, volume, quit and hide-others ask once for permission to control
System Events (hide-others and show-desktop also need the Accessibility
permission). On Windows, Wi-Fi and Bluetooth are listed only when the PC has
the radio, and switching them needs *Settings → Privacy & security → Radios*
to allow desktop apps. Flush DNS is not offered on Windows or macOS because it
needs administrator rights. You can also hide tasks in
[`[tasks]`](configuration.md#automation-tasks).

The list after `quit`, `kill` or `eject` appears a moment after you type the
command, because Sevak asks the system for it in the background.

## A snippet does not expand as I type

Expansion is off until you turn it on (**Settings → Plugins**, or
`[snippets] auto_expand = true`), and the snippet needs a `keyword`. Then:

- Type the keyword in one go, with your `prefix` if you set one. Clicking,
  arrow keys, Enter, Escape, shortcuts and switching windows make Sevak forget
  what you typed. Without a prefix a keyword only works at the start of a word.
- With `expand_on = "delimiter"` finish with a space or punctuation mark.
- Sevak does nothing in its own windows, in terminals (set
  `expand_in_terminals = true`), in apps listed in `ignore_apps`, in a password
  box, or in windows running as administrator when Sevak is not.
- **macOS:** allow Sevak under *Privacy & Security → Input Monitoring* and
  *Accessibility*, then quit and start Sevak again. Settings shows what is
  missing.
- **Linux Wayland:** not possible; Wayland does not allow it. Use X11 or
  paste snippets with `s <name>`.
- Security software may block or warn about the keyboard hook; allow Sevak.
  Turning the setting off removes the hook.

[How expansion works and what it watches](usage.md#expand-snippets-as-you-type).

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

App search, indexed file search, calculations, contacts, the dictionary and
spelling, and local ranking work without network access. Web searches, checking for a release, and downloading updates
need an internet connection. Indexes depend on locally accessible applications
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
