# Hardening: release notes

This page lists, in one place, every change in the security hardening release
that you can notice. It is meant to be pasted into the release notes. The
reasons are in the [threat model](threat-model.md#hardening-applied); the
details of each area are in [Script and workflow trust](script-workflow-trust.md)
and [Gallery trust](gallery-trust.md). Nothing here needs action for Sevak itself
to keep working; the items marked **You may need to** are things that can
behave differently from before.

## Script plugins and workflows

- **Scripts ask for approval once more.** An approval now covers what would run
  (the manifest, the command, the script files it names and the folder's
  location), not just the plugin's name and command line. Every existing script
  plugin asks again on the first start; editing a script, moving the folder or
  re-installing it asks again too.
- **Workflows that paste ask for approval.** A workflow with a Paste node types
  into whatever app was in front, so it needs your yes like a workflow that runs
  a script. Workflows you already allowed keep working unless they paste. In the
  gallery this affects Tidy up whitespace, Markdown helpers and Selection
  toolkit.
- **Scripts see a scrubbed environment.** A script starts with a small set of
  variables, not Sevak's own. **You may need to** list a variable a script needs
  (a proxy, a token, `JAVA_HOME`) in the plugin's `inherit_env`. Variables that
  change how interpreters and loaders start can no longer be passed or set.
- **`launch` needs a capability.** A script result that launches an app works
  only if the manifest declares `capabilities = ["launch"]`. Results use a
  closed set of actions; anything else is dropped.
- **Stopping a script stops what it started.** A timeout or shutdown ends the
  whole process tree (job object on Windows, process group elsewhere).
- **The Allow dialogs show more and cannot be spoofed.** They list commands with
  their arguments, environment variable names, standard input use and nodes
  that paste or open files and links; control and direction-changing characters
  in author-written text are removed.
- **Manifest file lists use portable paths.** The `files` entries of a manifest
  cannot contain backslashes or a drive prefix on any system.
- **A script's error output is limited** when Sevak logs it.

## Keyboard shortcuts and snippet expansion

- **Remapped keys no longer press Sevak's shortcut (Windows).** The keyboard
  hook ignores key events another program sends. **You may need to** turn on
  `accept_injected_hotkeys` (Settings, General) if AutoHotkey, PowerToys or a
  similar remapper types your shortcut.
- **The keyboard hook recovers.** It is put in again every minute and after an
  unlock, display change or wake from sleep.
- **Snippets do not expand in web browsers by default.** A password field in a
  web page cannot be told from other text boxes. **You may need to** turn on
  `expand_in_browsers` (Settings, Snippets) to expand there.
- **Password fields are detected more widely (Windows)** through UI Automation,
  in addition to the edit-control check.
- **An app Sevak cannot identify is treated as excluded** for snippet expansion
  and clipboard history, instead of being allowed.

## Clipboard history and selection

- **The history moves to the local folder (Windows).** `clipboard-history.json`
  and the `clipboard` folder are now in `%LOCALAPPDATA%\sevak\` instead of the
  roaming `%APPDATA%\sevak\`. The first start moves them and deletes the old
  copies. Clear clipboard history in Settings removes both places.
- **The history is encrypted on Windows** for your account (DPAPI), by default
  (`[clipboard] encrypt`). A history that cannot be decrypted (files from another
  account or computer) is replaced by an empty one, and the first row of `cb`
  says so. macOS and Linux have no encryption: their files are plain and
  readable by you only.
- **Password managers are ignored by default.** Copies from common password
  managers, system credential prompts and ssh/gpg passphrase prompts are never
  recorded (`[clipboard] default_ignore_apps`, Settings, Clipboard).
- **Linux honours the secret marker.** A copy marked `x-kde-passwordManagerHint`
  is skipped on X11 and, with `wl-clipboard` installed, on Wayland.
- **The clipboard is not restored over a newer copy,** and the paste target is
  checked again just before the paste keystroke.
- **Typed characters and captured selections are overwritten** in memory when
  Sevak lets go of them.
- **A selection over 256 KiB or with more than 1000 files is dropped** where it
  is read, more terminals are recognised, and an enormous clipboard image is
  turned away on its header (Windows).

## Galleries

- **The galleries are pinned to your version's release.** Sevak reads the
  workflow and theme lists from the tag of the running version, not `main`, and
  only from Sevak's repository (checked again at every redirect). A build with
  no published release uses the latest release and says so.
- **The list format changed.** Entries name files by a path relative to the
  release (`format` 2 for packages, `version` 2 for themes). Older builds read
  their own tag, which still has the old format; they are not affected.
- **Windows device names** (`CON`, `NUL`, `COM1`, ...) are refused as package
  entries, ids and folder names, and a gallery theme never replaces a theme of
  the same name.

## Files, paths and addresses

- **Network paths are off by default (Windows).** `\\server\share` paths and
  mapped network drives are refused by path browsing, configured folders, the
  file buffer and workflows. **You may need to** set `[files]
  allow_network_paths = true` (Settings, Files) to use them.
- **Addresses are checked more strictly.** Opening a link refuses control
  characters, quotes, angle brackets, backslashes, credentials in the address,
  a missing host and very long addresses, and limits `mailto:` to recipients,
  subject and body.
- **Programs are found only on a safe `PATH`.** An empty or relative `PATH`
  entry is ignored, and on Linux and macOS a bare program name runs the file
  `PATH` finds.
- **Owner-only permissions on Linux and macOS.** The data and log folders are
  `0700` and the files Sevak keeps there `0600`; folders an earlier version
  made wider are tightened.
- **Size limits on files read.** Configuration, manifests, approvals, caches,
  bookmarks, icons and description files above a fixed size are not read.
- **The file buffer never replaces a name that appeared meanwhile** when it
  moves files, and checks the destination before each item.
- **On macOS** icon scratch files are private and exclusive, and `osascript` is
  started by its absolute path.

## What did not change

The webview content security policy and the per-window Tauri capabilities are as
before, updater and release signing are unchanged, and the galleries still have
no signature of their own (see the [backlog](threat-model.md#hardening-backlog)).
