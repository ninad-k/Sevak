# Recent projects

Reopen a project in the editor you last used it in. Type `code ` and part of a folder name or path: `code sevak`, `code api`.

The plugin is on by default and reads the recent lists of:

| Editor | Id in `editors` |
|---|---|
| VS Code | `vscode` |
| VS Code Insiders | `vscode-insiders` |
| VSCodium | `vscodium` |
| Cursor | `cursor` |
| JetBrains IDEs (IntelliJ IDEA, PyCharm, WebStorm, GoLand, CLion, Rider, RustRover, PhpStorm, RubyMine, DataGrip, DataSpell, Aqua) and Android Studio | `jetbrains` |

## How to use it

An empty query lists your projects, most recently opened first. Each row names the editor it came from ("VS Code", "GoLand", "VS Code workspace"); a folder you opened in two editors has a row for each.

| Key | Action |
|---|---|
| ++enter++ | Open the project in that editor |
| ++ctrl+enter++ | Open the folder in the file manager |
| ++alt+enter++ | Copy the path |

If Sevak cannot find the editor's program (a portable or unusual install), the row says so and ++enter++ opens the folder instead. Programs are looked for here:

| | VS Code family | JetBrains |
|---|---|---|
| Windows | `%LOCALAPPDATA%\Programs\<editor>` and `%ProgramFiles%\<editor>` | `%LOCALAPPDATA%\Programs\<IDE>`, `%ProgramFiles%\JetBrains\<IDE>`, `%ProgramFiles%\Android\Android Studio` |
| macOS | `/Applications` and `~/Applications` (opened with `open -a`) | the same |
| Linux | `code`, `code-insiders`, `codium`, `cursor` on `PATH` or in `/snap/bin` | the launcher script on `PATH`, in Toolbox's `scripts` folder or `/snap/bin` |

## Where the lists come from

`<config>` is `%APPDATA%` on Windows, `~/Library/Application Support` on macOS and `$XDG_CONFIG_HOME` (or `~/.config`) on Linux.

- **VS Code and its forks**: `<config>/<Code | Code - Insiders | VSCodium | Cursor>/User/globalStorage/state.vscdb`, an SQLite database, opened read-only. If the editor has it locked, Sevak reads a private temporary copy instead and deletes it straight away. Older versions kept the list in `storage.json`, which is read when there is no database.
- **JetBrains**: `<config>/JetBrains/<Product><version>/options/recentProjects.xml` (and `recentSolutions.xml` for Rider); Android Studio's is under `<config>/Google/`.

Only folders and workspaces on this computer are listed: files, remote projects (SSH, WSL, dev containers), network shares and projects whose folder no longer exists are left out.

## Privacy

- Each file is read only from inside the editor's own settings folder (a symbolic link that leads elsewhere is not followed), with a size limit, on a background thread.
- The list is kept in memory only; the log has counts, never paths. Nothing is sent anywhere.

## Configuration

```toml
[projects]
keyword = "code"
editors = []          # e.g. ["vscode", "jetbrains"]; [] reads every editor found
```

See [`[projects]`](../configuration.md#projects). Turn the plugin off with `"projects"` in `[plugins] disabled` or under **Settings → Plugins**.
