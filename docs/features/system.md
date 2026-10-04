# System commands

Lock your screen, put the computer to sleep, restart, shut down, and more. Type a command's name as you would an app name (two or more characters); only the commands available on your machine are shown.

## How to use it

Type the command name (or an alias):

| Command | Aliases | What it does |
|---|---|---|
| `lock` | `lock`, `lock computer`, `lock workstation` | Lock the screen or session |
| `sleep` | `suspend`, `standby` | Put the computer to sleep |
| `hibernate` | *(no aliases)* | Hibernate (if supported) |
| `restart` | `reboot` | Restart the computer |
| `shut down` | `shutdown`, `power off`, `poweroff`, `turn off` | Power off the computer |
| `log out` | `logout`, `log off`, `logoff`, `sign out`, `signout` | End your session |
| `empty trash` / `recycle bin` | `empty recycle bin`, `clear trash`, `recycle bin` | Delete all trash (or Recycle Bin on Windows) |

Press ++enter++ to run the command. Destructive commands (restart, shut down, log out, empty trash) ask for confirmation first, unless you turn it off.

### Aliases

Each command has aliases so you can type less. A query of at least two characters finds these commands (one letter alone might match app names like "S" for Slack, so Sevak requires more). Examples:

| Query | Found |
|---|---|
| `lo` | Lock screen, Log out |
| `sl` | Sleep |
| `sh` | Shut down |
| `re` | Restart |

## System settings

Sevak also offers shortcuts to common operating system settings pages. Type the page name:

| Page | Available on | Example queries |
|---|---|---|
| Bluetooth | Windows, macOS, Linux | `bluetooth` |
| Display | Windows, macOS, Linux | `display`, `screen` |
| Wi-Fi | Windows, macOS, Linux | `wifi`, `network` |
| Sound | Windows, macOS, Linux | `sound`, `audio` |
| Network | Linux | `network` |
| Power | Windows, Linux | `power`, `battery` |

## Actions

- **++enter++**: Run the command (with confirmation if configured).
- **++ctrl+k++** (action panel): See all available actions (typically just run).

## Options

Both are in **Settings → System & terminal**, where the hidden commands and settings pages are a checklist.

| Setting | Default | What it does | Config section |
|---|---|---|---|
| Confirm | `true` | Ask before restart, shut down, log out, and empty trash | [`[system] confirm`](../configuration.md#system) |
| Disabled | *(empty)* | Hide specific commands or settings pages: `"restart"`, `"settings:bluetooth"`, etc. | [`[system] disabled`](../configuration.md#system) |

## Platform notes

=== "Windows"
    - **Lock**: `LockWorkStation` (fast)
    - **Sleep**: `SetSuspendState` (hybrid sleep mode)
    - **Hibernate**: `shutdown /h` (only if hibernation is enabled)
    - **Restart**: `shutdown /r /t 0`
    - **Shut down**: `shutdown /s /t 0`
    - **Log out**: `shutdown /l`
    - **Empty Recycle Bin**: `SHEmptyRecycleBin` (permanent delete)
    - **Settings pages**: `ms-settings:` (Bluetooth, Display, Network, Power, Sound, etc.)

=== "macOS"
    - **Lock**: `pmset displaysleepnow` (lock screen; requires "Require password" set to immediately)
    - **Sleep**: `pmset sleepnow`
    - **Hibernate**: Not offered (use sleep instead)
    - **Restart**: Uses System Events (requires Accessibility permission on first use)
    - **Shut down**: Uses System Events (requires Accessibility permission on first use)
    - **Log out**: Uses System Events (requires Accessibility permission on first use)
    - **Empty Trash**: Finder (requires Accessibility permission on first use)
    - **Settings pages**: System Settings panes

=== "Linux"
    - **Lock**: `loginctl lock-session`
    - **Sleep**: `systemctl suspend`
    - **Hibernate**: `systemctl hibernate` (only if swap is set up)
    - **Restart**: `systemctl reboot`
    - **Shut down**: `systemctl poweroff`
    - **Log out**: `gnome-session-quit` (GNOME), or KDE `qdbus`, or `loginctl terminate-session`
    - **Empty Trash**: `gio trash --empty`
    - **Settings pages**: `gnome-control-center <panel>` (if installed; GNOME only)

## Tips and troubleshooting

!!! tip "Disable a command"
    To hide a specific command, add it to [`[system] disabled`](../configuration.md#system):
    ```toml
    [system]
    disabled = ["hibernate", "settings:bluetooth"]
    ```

!!! tip "Disable all confirmations"
    If you do not want to confirm destructive commands, set `confirm = false`:
    ```toml
    [system]
    confirm = false
    ```

!!! warning "Destructive commands ask for confirmation"
    Restart, shut down, log out, and empty trash are destructive and ask for confirmation first (if `confirm = true`). A warning dialog appears whose button names the action (for example **Restart**). Only that button runs it; **Cancel** or closing the dialog does nothing. This also applies when the command is bound to a [custom hotkey](../configuration.md#hotkey).

!!! warning "macOS Accessibility permission"
    Restart, shut down, log out, and empty trash on macOS need *System Settings → Privacy & Security → Accessibility → Sevak*. The first time you use any of these, macOS shows a permission prompt; grant it and try again.
