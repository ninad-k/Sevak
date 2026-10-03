# Command-line reference

Sevak can be controlled from the command line or automated with scripts. When Sevak is already running, a new command forwards its request to the running instance (single-instance architecture).

## Quick reference

| Flag | Effect |
|---|---|
| (none) | Start Sevak; show the search bar if not already visible |
| `--toggle` | Show the search bar, or hide it if visible |
| `--query TEXT` | Show the search bar with TEXT already typed |
| `--run ID` | Run a result by its id without showing the search bar |
| `--actions` | Universal Actions: capture the selection in the foreground app |
| `--background` | Start Sevak without showing the window |
| `--settings` | Open the Settings window |
| `--quit` | Quit the running instance |
| `--setup-hotkey [KEY]` | Bind hotkeys to GNOME (Wayland desktop on Linux) |
| `--config PATH` | Use a custom config folder or file; overrides `SEVAK_CONFIG_DIR` |
| `-h`, `--help` | Print help |
| `-V`, `--version` | Print version |

## Environment variables

| Variable | Purpose |
|---|---|
| `SEVAK_CONFIG_DIR` | Path to the config folder (overridden by `--config`). If this is set and the folder exists, the config is read from there. |
| `SEVAK_DATA_DIR` | Path to the data folder (usage statistics, logs). If set, overrides the default platform-specific location. |
| `SEVAK_LOG` | Log level filter. Examples: `SEVAK_LOG=debug` for detailed output, `SEVAK_LOG=warn` for warnings and errors only. Defaults to `info`. |

## Flags

### Start or show

#### (no arguments)

```bash
sevak
```

Start Sevak in the background if it is not running. If it is already running and the search bar is hidden, show it. Does nothing if the bar is already visible.

#### `--toggle`

```bash
sevak --toggle
```

Toggle the search bar: show it if hidden, hide it if visible. This is the primary way to bind Sevak to a global hotkey.

On Wayland, where applications cannot grab global keys, bind this to a key in your desktop settings (or use `sevak --setup-hotkey` on GNOME).

#### `--query TEXT`

```bash
sevak --query "firefox"
sevak --query "> pip install requests"
sevak --query ""
```

Show the search bar with TEXT already typed. Used to:

- Open Sevak with a specific search, e.g. `"firefox"` to search for Firefox
- Pre-fill a shell prompt with `"> "` or `">"` (see [Shell commands](features/shell.md))
- Open an empty launcher with an empty string `""`

#### `--background`

```bash
sevak --background
```

Start Sevak in the background. The search bar does not appear. Useful for the `launch_at_login` setting or autostart scripts.

#### `--run ID`

```bash
sevak --run "apps:firefox.desktop"
sevak --run "files:/home/user/Documents"
sevak --run "system:lock"
```

Run a result by its id directly without showing the search bar. The launcher stays hidden. Useful for hotkey bindings, e.g. bind `Ctrl+Alt+L` to `sevak --run "system:lock"`.

See [custom hotkeys](configuration.md#hotkey) for result id formats.

#### `--actions`

```bash
sevak --actions
```

Universal Actions: capture what you have selected in the currently focused app (text, a URL, files) and offer actions on it. On Wayland, bind this to a key in your desktop settings or use `sevak --setup-hotkey` on GNOME.

### Interaction

#### `--settings`

```bash
sevak --settings
```

Open the Settings window. If Sevak is not running, start it with the Settings window shown.

#### `--quit`

```bash
sevak --quit
```

Quit the running instance. If Sevak is not running, this does nothing and exits cleanly.

### Configuration

#### `--config PATH`

```bash
sevak --config ~/.local/share/sevak
sevak --config /etc/sevak/config.toml
sevak --config .\Config
```

Use a custom config folder or config file instead of the default location. Takes precedence over the `SEVAK_CONFIG_DIR` environment variable.

- If `PATH` ends in `.toml` and is not an existing directory, it is treated as the config file itself; the folder containing it is the config folder.
- Otherwise, `PATH` is treated as the config folder; the file is `config.toml` inside it.
- `~` is expanded to your home directory.
- Relative paths are resolved relative to the current working directory.

This flag only affects a newly started instance. If Sevak is already running, it keeps its own config folder; the flag is noted in the log for reference.

#### `--setup-hotkey [KEY]`

(Linux Wayland with GNOME only)

```bash
sevak --setup-hotkey              # use the hotkey from config.toml
sevak --setup-hotkey "Ctrl+Space"
sevak --setup-hotkey "Super+F"
```

Create GNOME custom keyboard shortcuts that bind your hotkeys to Sevak commands. On Wayland, where applications cannot register global keys, this command asks GNOME to handle key events and forward them to Sevak.

It creates shortcuts for:

- The main hotkey (`[general] hotkey`) → `sevak --toggle`
- Universal Actions hotkey (`[general] actions_hotkey`) → `sevak --actions`
- Each `[[hotkey]]` entry → `sevak --query '<text>'` or `sevak --run <id>`

If you pass a KEY, it overrides the hotkey from `config.toml` just for this setup. The config file is not modified.

On other desktops (KDE, Sway, etc.), bind hotkeys through your desktop's keyboard settings. On X11, hotkeys are registered directly by Sevak and this command is not needed.

After editing `config.toml`, run `--setup-hotkey` again to update GNOME shortcuts. Delete old shortcuts in GNOME's keyboard settings if you remove entries from the config.

### Information

#### `-h`, `--help`

```bash
sevak --help
sevak -h
```

Print the help message and exit.

#### `-V`, `--version`

```bash
sevak --version
sevak -V
```

Print the version and exit.

## Single-instance forwarding

When you run `sevak` and the app is already running, your command is forwarded to the running instance. Here is how it works:

```mermaid
sequenceDiagram
    participant User as User shell
    participant NewProcess as New sevak process
    participant RunningInstance as Running Sevak instance
    
    User->>NewProcess: sevak --toggle
    NewProcess->>RunningInstance: Forward --toggle via IPC
    RunningInstance->>RunningInstance: Process --toggle
    RunningInstance-->>User: Show/hide launcher
    NewProcess->>NewProcess: Exit
```

This means:

- Only one instance of Sevak runs at a time.
- Commands from new invocations are processed by the running instance.
- If Sevak is not running, the new invocation starts it.
- The `--config` flag is ignored by the running instance (it keeps its own config path).

Exit codes:

| Code | Meaning |
|---|---|
| 0 | Success (including `--help` and `--version`) |
| 1 | Sevak could not start or run (the error is printed and logged) |
| 2 | Invalid command line: unknown flag, missing value or conflicting flags (the usage text is printed) |

## Examples

**Bind Alt+Space to show/hide Sevak on macOS:**

Add to your keyboard shortcuts under System Settings:

```
/Applications/Sevak.app/Contents/MacOS/sevak --toggle
```

**Bind Ctrl+Alt+T to open a shell prompt on Windows:**

Use Task Scheduler or a tool like AutoHotkey to run:

```powershell
sevak --query "> "
```

**Lock the desktop from anywhere:**

```bash
sevak --run "system:lock"
```

**Search for a specific file from a script:**

```bash
sevak --query "f my-project"
```

**Start Sevak on login (Linux):**

Add to your `.bash_profile` or autostart script:

```bash
sevak --background
```

**Check Sevak version in a script:**

```bash
sevak --version
```

## Development and debugging

**Enable debug logging:**

=== "Bash/Linux/macOS"

    ```bash
    SEVAK_LOG=debug sevak
    ```

=== "PowerShell/Windows"

    ```powershell
    $env:SEVAK_LOG = "debug"
    sevak
    ```

=== "Windows CMD"

    ```cmd
    set SEVAK_LOG=debug
    sevak
    ```

Logs are written to:

=== "Windows"

    `%APPDATA%\sevak\logs\`

=== "macOS"

    `~/Library/Application Support/sevak/logs/`

=== "Linux"

    `~/.local/share/sevak/logs/`

**Test a config file before deployment:**

```bash
sevak --config /path/to/test/config.toml --version
```

This loads the config without starting the UI. Parsing errors are printed to the terminal.
