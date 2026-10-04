# Backup and restore

Save your settings, snippets, web searches, themes, script plugins and workflows to **one file**, and put them back later or on another computer. Open **Settings → Backup & restore**.

A backup is a plain `.sevakbackup` file (a zip with a `manifest.json` and your configuration files). It is made on your computer, nothing is uploaded, and Sevak does not need an account.

## What goes in

You tick the categories to include. All are ticked by default.

| Category | What it holds | Where it comes from |
|---|---|---|
| **Settings** | Shortcuts, appearance, search and plugin options, extra hotkeys | the tables of `config.toml` (everything but snippets and web searches) |
| **Snippets** | Your `[[snippet]]` entries and the `[snippets]` expansion options | `config.toml` |
| **Web search engines** | The `[[web_search]]` engines and their keywords | `config.toml` |
| **Themes** | Your theme files and the custom stylesheet named in `[appearance] custom_css` | `themes/`, the stylesheet |
| **Script plugins** | Each plugin folder with its `plugin.toml` and scripts, gallery installs included | `plugins/` |
| **Workflows** | Each workflow folder with its `workflow.toml` and scripts, gallery installs included | `workflows/` |

**Show exactly what is included** lists every section, theme and folder before you save. Anything Sevak leaves out of `config.toml` (for example the `[onepassword]` section) or skips in a plugin folder (a file called `.env` or `token.json`, a link, a huge file) is listed there with the reason.

### What never goes in

The list of what can be in a backup is an **allowlist**: a file is only included if its place is named in the code, so a new kind of data stays out until someone adds it on purpose. These are never in a backup:

- AI assistant and other API keys, tokens and passwords (a key in a section that is otherwise included is dropped too: names such as `api_key`, `token`, `secret` and `password` are removed)
- clipboard history and the images you copied
- search and usage history (`usage.json`)
- anything from a password manager or the system credential store, including all 1Password data (the `[onepassword]` section is left out)
- the encrypted history database, if you use one
- logs and diagnostic reports
- **which scripts, plugins and workflows you allowed to run** (see [Scripts are never trusted automatically](#scripts-are-never-trusted-automatically))
- the data folders and caches of plugins

Inside a plugin or workflow folder, Sevak also leaves out hidden files (`.env`, `.git`), `node_modules` and `__pycache__`, and files that look like secrets or databases (`*.pem`, `*.key`, `*.db`, `credentials.json`, `token.json`, …). A script such as `token_counter.py` is code and is kept. A plugin that cannot be backed up whole (a file over 8 MiB, more than 300 files, a manifest that does not load) is left out entirely, with a note, rather than saved broken.

!!! warning "The file is not encrypted"
    A backup is a plain file. It holds your snippet text and your scripts, so anyone who can open it can read them. Sevak creates it readable by you only (mode `0600` on macOS and Linux; on Windows it inherits your profile folder's access). Keep it somewhere you trust. Password-protecting the archive is not offered: it would need a cryptography library Sevak does not ship, and an unreviewed home-made scheme is worse than none. To protect it, store it in an encrypted folder or use a tool such as `age` or 7-Zip on the file.

## Back up

| Button | Result |
|---|---|
| **Save backup as…** | A save dialog; the default name is `sevak-backup-YYYYMMDD-HHMMSS.sevakbackup` |
| **Back up now** | Saves to the backup folder without asking (`Documents/Sevak backups` unless you chose another) |
| **Open backup folder** | Shows the folder |

The page also shows when and where the last backup was made.

## Restore

1. Click **Choose a backup…** and pick a `.sevakbackup` file. Sevak checks it first (see [How a backup is checked](#how-a-backup-is-checked)); a damaged, hostile or too-new file is refused with a clear message.
2. Tick the categories to restore (only those in the file can be ticked).
3. Choose how:

    | Mode | What it does |
    |---|---|
    | **Merge** (default) | Adds what the backup has and overwrites what has the same name or setting. Keeps everything else. Snippets match by name, web searches and hotkeys by keyword or key, theme files by file name, plugins and workflows by folder; a setting the backup contains overrides yours, one it does not contain stays. |
    | **Replace** | Makes the ticked categories exactly like the backup. What the backup does not have is removed (snippets, engines, theme files, plugin and workflow folders) or goes back to its default (settings). |

4. Read **What would change**: for each category how many things are new, changed or already the same (and, in Replace mode, removed), with the list of names and files. Nothing is written yet.
5. Click **Restore**. Replace asks once more when it would remove something.

The restore is **all or nothing**. Sevak first saves a **safety copy** of what will be replaced, then prepares every file, then swaps them in; if any step fails it puts everything back and says so, and your configuration is as it was. A restore is refused before anything is touched if the result would not be valid (for example a shortcut that cannot work, or a web search the settings refer to that is missing).

When it succeeds Sevak **reloads at once**: the new settings, snippets, themes and plugins are in use without a restart. The Settings pages load the restored values too.

### Undo restore

**Undo restore** puts back what the last restore replaced, using the safety copy. Sevak keeps the five newest safety copies in the data folder (`backup-snapshots/`) and shows the button while there is one. Undoing deletes that copy; run the restore again to redo it. (One detail: comments you wrote inside `[[snippet]]` entries are not kept when the snippets change; comments elsewhere in `config.toml` are.)

### Scripts are never trusted automatically

Script plugins and workflows run code with your permissions, so Sevak asks you to **allow** each one the first time. Which ones you allowed is a record in the data folder, and it is **not part of a backup**. A restore therefore never carries trust over:

- on a new computer nothing is approved, so every restored plugin and every workflow that runs a script asks;
- on the same computer, a plugin or workflow the restore adds or changes **has its old approval removed**, even when the command is the same as before (a plugin's approval is bound to its command, so a swapped script would otherwise inherit it; a workflow's is bound to its scripts' contents). Ones the restore leaves alone keep their approval, so you are not asked needlessly;
- the preview lists them (“asks for approval”), and the dialog that follows the reload shows what each one will do.

## Automatic backups

Off by default. In **Settings → Backup & restore → Automatic backups** (or in `backup.toml`, next to `config.toml`):

```toml
# "off", "daily" or "weekly".
schedule = "off"
# Also back up the first time a new version of Sevak starts.
on_update = false
# How many automatic backups to keep (1-50); older ones are deleted.
keep = 5
# Where they go. "" means the "Sevak backups" folder in your Documents folder.
folder = ""
```

| Option | Meaning |
|---|---|
| `schedule` | A backup is made when the last automatic one is a day (or a week) old. Sevak checks a minute and a half after it starts and then every half hour. |
| `on_update` | Makes a backup the first time a different version of Sevak starts, which captures your configuration as it was before the new version touched it. |
| `keep` | Only files named `sevak-auto-*.sevakbackup` are ever deleted. Backups you saved yourself, and any other file in the folder, are never touched. |
| `folder` | A full path, or one starting with `~`. Also used by **Back up now**. |

Automatic backups include every category. They write the same file as a manual backup, with the same exclusions.

## How a backup is checked

A backup file can come from anywhere, so Sevak trusts nothing in it. Before it shows a preview it checks:

- **Size**: the file, every entry, the total once unpacked (64 MiB), the number of entries and the length and depth of paths are capped; entries are read with a hard limit whatever size the zip claims, so a zip bomb is refused.
- **Paths**: every path must be on the allowlist. A path with `..`, an absolute or drive path, a backslash, a hidden or reserved name (`CON`, `.env`), a name that differs from another only in case, or anything outside the six places above is refused. Links and special files are refused.
- **The manifest**: it must be a Sevak backup, list every file once, and match each file's size and SHA-256 checksum; a file it does not list, or one it lists but the archive lacks, is refused. A backup with a **newer format version** than this Sevak understands is refused with “made by a newer version of Sevak … update Sevak to restore it”.
- **Content**: settings, snippets and web searches are parsed against the real configuration schema (a value of the wrong type is refused; unknown sections and secret-looking keys are ignored with a warning); theme files must be valid; each plugin must have a `plugin.toml` Sevak can load, each workflow a valid `workflow.toml`.
- **The result**: the configuration that would result must pass the same checks the Settings window applies when you save.

The file you previewed is the file that is restored: Sevak checks its checksum again before it writes anything.

## From the command line and the launcher

```bash
sevak --backup ~/Documents/my.sevakbackup     # a file, or an existing folder
sevak --restore ~/Documents/my.sevakbackup    # merge
sevak --restore my.sevakbackup --replace      # replace
sevak --undo-restore
```

These work on the files, whether or not Sevak is running (they do not contact the running instance). If Sevak is running, choose **Reload index** in its tray menu, or restart it, to use the restored settings. See [Command line](cli.md#-backup-path).

In the launcher, type `backup settings` to save a backup to the backup folder, or `restore settings` to choose a file and restore it in merge mode after a confirmation that lists what changes. They need six typed characters to appear, so they do not get in the way. The Settings page has every option. Turn them off with `[plugins] disabled = ["backup"]`.

## Using a backup on another computer

1. On the old computer: **Save backup as…**, then copy the file over.
2. Install Sevak on the new one, open **Settings → Backup & restore**, **Choose a backup…**.
3. Use **Replace** for an exact copy of the old setup, or **Merge** to combine it with what is already there.
4. Allow your script plugins and workflows again when Sevak asks.

Folder paths in `[files] directories` and shortcuts are restored as written. A backup made on Windows can be restored on macOS or Linux; check the file search folders afterwards. The execute bit of a script is kept, but a script made on Windows has none, so a plugin that runs the script directly (rather than through `python3 script.py`) may need `chmod +x`.

## Related

- [Configuration file](configuration.md#files-next-to-configtoml) and [Files and data](files-and-data.md): where everything lives
- [Privacy](privacy.md): what Sevak stores and what it never sends
- [Script plugins](features/script-plugins.md) and [Workflows](workflows.md): approval of scripts
