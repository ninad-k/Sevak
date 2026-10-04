# Files and data locations

Sevak stores configuration, themes, script plugins, workflows, logs, usage history and clipboard history in well-known locations on each platform. You can override these with environment variables or command-line flags.

## Default locations

| Item | Windows | macOS | Linux |
|---|---|---|---|
| **Config folder** | `%APPDATA%\sevak\` | `~/Library/Application Support/sevak/` | `~/.config/sevak/` |
| **Config file** | `%APPDATA%\sevak\config.toml` | `~/Library/Application Support/sevak/config.toml` | `~/.config/sevak/config.toml` |
| **Data folder** | `%APPDATA%\sevak\` | `~/Library/Application Support/sevak/` | `~/.local/share/sevak/` |
| **Logs** | `%APPDATA%\sevak\logs\` | `~/Library/Application Support/sevak/logs/` | `~/.local/share/sevak/logs/` |
| **Usage history** | `%APPDATA%\sevak\usage.json` | `~/Library/Application Support/sevak/usage.json` | `~/.local/share/sevak/usage.json` |
| **Clipboard history** | `%APPDATA%\sevak\clipboard-history.json` | `~/Library/Application Support/sevak/clipboard-history.json` | `~/.local/share/sevak/clipboard-history.json` |
| **Clipboard images** | `%APPDATA%\sevak\clipboard\` | `~/Library/Application Support/sevak/clipboard/` | `~/.local/share/sevak/clipboard/` |
| **Theme files** | `%APPDATA%\sevak\themes\` | `~/Library/Application Support/sevak/themes/` | `~/.config/sevak/themes/` |
| **Script plugins** | `%APPDATA%\sevak\plugins\` | `~/Library/Application Support/sevak/plugins/` | `~/.config/sevak/plugins/` |
| **Workflows** | `%APPDATA%\sevak\workflows\` | `~/Library/Application Support/sevak/workflows/` | `~/.config/sevak/workflows/` |
| **Shortcut takeover record** | `%APPDATA%\sevak\hotkey-takeover.json` | `~/Library/Application Support/sevak/hotkey-takeover.json` | `~/.local/share/sevak/hotkey-takeover.json` |
| **Backup options** | `%APPDATA%\sevakackup.toml` | `~/Library/Application Support/sevak/backup.toml` | `~/.config/sevak/backup.toml` |
| **Backup bookkeeping and safety copies** | `%APPDATA%\sevakackup-state.json`, `backup-snapshots\` | `~/Library/Application Support/sevak/backup-state.json`, `backup-snapshots/` | `~/.local/share/sevak/backup-state.json`, `backup-snapshots/` |

Theme files, script plugins and workflows sit next to `config.toml` in the config folder; clipboard images, approvals and the data folders of plugins and workflows are in the data folder. On Windows and macOS the two folders are the same.

Expand `~` to your home directory and `%APPDATA%` to your roaming app data folder.

## Override locations

### Config folder

=== "Command line"

    ```bash
    sevak --config ~/my-sevak
    sevak --config /etc/sevak/config.toml
    ```
    
    === "Windows"
        
        ```powershell
        sevak --config $env:USERPROFILE\Sync\sevak
        ```

=== "Environment variable"

    ```bash
    export SEVAK_CONFIG_DIR=~/my-sevak
    sevak
    ```
    
    === "Windows"
        
        ```powershell
        $env:SEVAK_CONFIG_DIR = "$env:USERPROFILE\Sync\sevak"
        sevak
        ```

The `--config` flag takes precedence over the environment variable.

### Data folder (logs, usage, clipboard history, approvals)

=== "Environment variable"

    ```bash
    export SEVAK_DATA_DIR=~/.local/share/my-sevak
    sevak
    ```
    
    === "Windows"
        
        ```powershell
        $env:SEVAK_DATA_DIR = "D:\Data\sevak"
        sevak
        ```

Note: the data folder cannot be overridden via command line; only the `SEVAK_DATA_DIR` environment variable works.

## What each file contains

### config.toml

Your configuration: shortcuts, appearance, enabled plugins, file search roots, etc. See [Configuration file reference](configuration.md).

**Edited by:** You (manually or through the Settings window).

**When to back up:** Before making large manual edits or testing unusual configurations.

### logs/

Day-to-day logs from Sevak (one file per day, dated). Useful for troubleshooting. For more detail, set `SEVAK_LOG=debug` before starting Sevak.

**Automatic cleanup:** Logs older than 7 days are deleted automatically.

To share logs without sharing private details, use the [diagnostics report](privacy.md#diagnostics-report): it holds the last 100 lines with your user name, home folder and anything that looks like a secret removed. **Settings → Help → Open logs folder** shows the raw files.

**Size:** Typically a few kilobytes per day under normal use.

### usage.json

Records searches and actions you perform: what you typed, which results you ran, timestamps. Used for:

- Query history: press Up/Down on an empty search bar to recall your last ~50 searches.
- Usage statistics and analytics (local only; nothing is sent to Sevak developers).

**Edited by:** Sevak (you can manually edit it, but Sevak will overwrite it).

**Privacy:** Text you searched is recorded. Select what to delete (see "Clear history" below).

### clipboard-history.json and clipboard/

Optional clipboard history (only if `[clipboard] enabled = true`). `clipboard-history.json` records the text you copy and the paths of files you copy, with timestamps. The `clipboard/` folder next to it holds each copied image as a PNG file plus a small thumbnail (only if `[clipboard] images = true`, the default once the history is on). Both are unencrypted.

**Edited by:** Sevak (when you copy, and when you type `cb clear` and run **Clear clipboard history**, which deletes the entries and the image files).

**Privacy:** Disabled by default because it records what you copy. Apps that mark their content as secret (password managers, etc.) are never recorded. Configure `[clipboard] ignore_apps` to never record copies from specific apps, and `[clipboard] images = false` / `files = false` to record text only.

### backup.toml, backup-state.json and backup-snapshots/

`backup.toml` holds the options of [automatic backups](backup-and-restore.md#automatic-backups) (off by default) and sits in the config folder. `backup-state.json` in the data folder records when the last backup was made and the Sevak version that last looked at the schedule. `backup-snapshots/` holds the safety copies Sevak takes just before a restore so that **Undo restore** works (the five newest are kept). A snapshot is an ordinary backup file of what the restore replaced, with the same exclusions. None of these three is part of a backup.

### currency-rates.json

Cached European Central Bank exchange rates, only if currency conversion (`[calculator] currency`) is on.

### script-plugin-approvals.json

The script plugins and workflows you allowed to run. Delete it to be asked again for each of them.

### plugins/ and workflows/ (data folder)

`plugins/<name>/` and `workflows/<name>/` in the data folder are where script plugins and workflow scripts may keep their own files (`SEVAK_PLUGIN_DATA`, `SEVAK_WORKFLOW_DATA`). Sevak creates the folder when the script first runs; what goes in it is up to the script.

### themes/, plugins/ and workflows/ (config folder)

Your [theme files](themes.md#theme-files-and-the-editor), [script plugins](features/script-plugins.md) and [workflows](workflows.md) (each a folder with a `workflow.toml` and its scripts). They travel with the config folder, so a synced config folder brings them along.

### What is never written to disk

Contacts and the list of 1Password logins are kept in memory only. The keystrokes watched for [snippet expansion](features/snippets.md#expand-snippets-as-you-type) (the last 64 characters) are kept in memory only, never logged. The Universal Actions selection is never stored.

## Back up your data

The easy way is **Settings → Backup & restore** (or `sevak --backup FILE`): one file with your settings, snippets, web searches, themes, script plugins and workflows, left out of which are keys, history and the list of scripts you allowed. See [Backup and restore](backup-and-restore.md). To copy *everything*, history included, copy the folders yourself:

```bash
cp -r ~/.local/share/sevak ~/backups/sevak-backup
```

or on Windows:

```powershell
Copy-Item $env:APPDATA\sevak $env:USERPROFILE\backups\sevak-backup -Recurse
```

Restore from a backup by copying the folder back.

## Reset to defaults

To reset Sevak to a fresh install while keeping your data:

1. Delete `config.toml` (or move it aside first). Sevak will write a new default config on next start.
2. Start Sevak.

To also clear history:

1. Delete `config.toml`.
2. Delete `usage.json` (or just the queries and actions you want to forget).
3. If clipboard history is enabled, delete `clipboard-history.json` and the `clipboard` folder.
4. Start Sevak.

## Clear search history

Search history is stored in `usage.json`. To clear it entirely:

```bash
rm ~/.local/share/sevak/usage.json
```

On Windows:

```powershell
Remove-Item "$env:APPDATA\sevak\usage.json"
```

Sevak will create a fresh file on next start.

To selectively delete searches, edit `usage.json` in a text editor (it is JSON format) and remove entries from the `searches` array.

## Clear clipboard history

To delete all saved clipboard items, type `cb clear` in Sevak and press ++enter++ on **Clear clipboard history**. It deletes the entries and the image files.

Or, with Sevak closed, delete the file and the image folder yourself:

```bash
rm -r ~/.local/share/sevak/clipboard-history.json ~/.local/share/sevak/clipboard
```

On Windows:

```powershell
Remove-Item "$env:APPDATA\sevak\clipboard-history.json", "$env:APPDATA\sevak\clipboard" -Recurse
```

Sevak will create a fresh file on next use.

## Uninstall cleanly

### Windows

1. Open Settings → Apps → Installed apps, find Sevak, and click Uninstall.
   - The uninstaller removes Sevak itself but keeps config and data. Its "Also delete the web cache" option only clears the web view's cache.
2. To also remove config and data:

   ```powershell
   Remove-Item "$env:APPDATA\sevak" -Recurse
   ```

### macOS

1. Delete the Sevak app from Applications.
   - This does not remove config and data.
2. To also remove config and data:

   ```bash
   rm -rf ~/Library/Application\ Support/sevak
   ```

### Linux

Uninstall using your package manager:

=== "Ubuntu/Debian"

    ```bash
    sudo apt remove sevak
    ```

=== "Fedora"

    ```bash
    sudo dnf remove Sevak
    ```

=== "Other distributions"

    ```bash
    # Delete the AppImage
    rm ~/Sevak_*.AppImage
    ```

To also remove config and data:

```bash
rm -rf ~/.config/sevak ~/.local/share/sevak
```

## Portable setup

You can place Sevak's config and data anywhere and point to it with environment variables:

```bash
export SEVAK_CONFIG_DIR=/mnt/portable/sevak/config
export SEVAK_DATA_DIR=/mnt/portable/sevak/data
sevak
```

or

```bash
sevak --config /mnt/portable/sevak/config
```

This is useful for running Sevak from a USB drive or shared network folder. The `--config` flag overrides the default but not the environment variable if both are set; the flag takes precedence.

## Troubleshooting file access

**"Cannot access config file"** error:

- Check file permissions. Your user must be able to read and write the config folder.
- Check disk space. If the drive is full, writing a new config may fail.
- Check paths with special characters (non-ASCII, spaces). Ensure the path is valid on your system.

**Logs are not being written:**

- Check that `~/.local/share/sevak/logs/` (or equivalent) exists and is writable.
- Set `SEVAK_LOG=debug` to enable debug logging, but this is stored in memory unless persisted to a file.
- Restart Sevak after changing the data folder.

**Old clipboard history or usage data still showing:**

- Sevak loads these files at startup. Changes made externally are not live-reloaded.
- Restart Sevak after editing or deleting `usage.json` or `clipboard-history.json`.
