# Automation tasks

Ready-made actions for the operating system: toggle dark mode, set the volume, take a screenshot, quit or kill an app, eject a drive, keep the computer awake and more. Type a task's name as you would an app (two or more letters), or type `t ` to list them all. Only the tasks that work on your machine are offered, and they are matched by name even without the `t`.

## How to use it

| Task (try typing) | Windows | macOS | Linux |
|---|---|---|---|
| Toggle dark mode (`dark mode`, `theme`) | app and system theme in the registry, then a settings broadcast | System Events appearance | GNOME `color-scheme` (`gsettings`), GNOME only |
| Show desktop (`desktop`) | ++win+d++ | ++f11++ through System Events | `wmctrl -k`, if installed (X11) |
| Hide other apps (`hide others`) | ++win+home++ (minimizes all but the front window) | ++option+cmd+h++ | not offered |
| Minimize all windows | ++win+m++ | not offered | not offered |
| Take a screenshot (`screenshot`, `snip`) | `ms-screenclip:` (Snipping Tool) | the Screenshot app | `gnome-screenshot -i`, else `spectacle`, `flameshot gui`, `xfce4-screenshooter` |
| Open Downloads folder (`downloads`) | your Downloads folder | your Downloads folder | your Downloads folder |
| Open recent files (`recent`) | `shell:recent` | not offered | `gio open recent:///` |
| Flush DNS cache (`flush dns`) | not offered (needs administrator rights) | not offered (needs root) | `resolvectl flush-caches`; may ask through polkit |
| Restart Explorer / Finder | ends `explorer.exe` (Windows starts it again) | `killall Finder` | not offered |
| Empty clipboard (`clear clipboard`) | yes | yes | yes |
| Mute / Unmute / Volume up / Volume down | the default output device (Core Audio) | Apple events (`set volume`) | `wpctl`, else `pactl`, else `amixer` |
| Toggle Wi-Fi (`wifi`) | Windows radio API, if the PC has a Wi-Fi radio | `networksetup -setairportpower` | `nmcli radio wifi`, else `rfkill toggle wifi` |
| Toggle Bluetooth (`bluetooth on`) | Windows radio API, if the PC has a Bluetooth radio | `blueutil --power toggle`, if [blueutil](https://github.com/toy/blueutil) is installed | `rfkill toggle bluetooth`, else `bluetoothctl power` |
| Keep awake (`caffeinate`, `stay awake`) | a power request held for the time | `caffeinate -d -i -t` | `systemd-inhibit` |
| Stop keeping awake | releases it | stops `caffeinate` | stops `systemd-inhibit` |

### Tasks that take an argument

Some tasks take what you type after them:

| Type | Lists or does |
|---|---|
| `quit` / `quit sla` | The apps that have a window; ++enter++ asks the app to quit, ++shift+enter++ force quits it |
| `force quit sla` (or `fq sla`) | The same list; ++enter++ force quits after asking |
| `kill chrome` | The running processes with that name, grouped, with their CPU and memory; ++enter++ ends them all after asking |
| `eject` / `eject usb` | The removable drives (USB sticks, SD cards, optical drives); ++enter++ ejects or unmounts |
| `vol 30`, `volume 30%` | Sets the volume (0 to 100); `vol up`, `vol down`, `vol mute`, `vol unmute` also work, and a bare `vol` offers presets |
| `awake 45`, `awake 2h`, `awake 1.5 hours` | Keeps the computer awake for that long (one minute to 24 hours); a bare `awake` offers presets |

The list of apps, processes and drives is read from the system in the background, so the first keystroke after a pause may show the list a moment later; typing never waits for it.

Force quit, kill and restarting Explorer or Finder ask for confirmation first, and the system's own processes (`csrss`, `launchd`, `systemd`…) and Sevak itself are never offered.

## Bind a task to a hotkey

Tasks have stable result ids, so a [`[[hotkey]]`](../configuration.md#hotkey) or `sevak --run` can run them directly:

```toml
[[hotkey]]
key = "Ctrl+Alt+D"
run = "tasks:dark_mode"

[[hotkey]]
key = "Ctrl+Alt+M"
run = "tasks:volume:30"      # also "tasks:keep_awake:45", "tasks:kill:chrome.exe"
```

## Options

All of these are in **Settings → Tasks & media**; the hidden-tasks list there is a checklist of the keys below.

| Setting | Default | What it does | Config section |
|---|---|---|---|
| Confirm | `true` | Ask before force quit, kill and restarting Explorer or Finder | [`[tasks] confirm`](../configuration.md#tasks) |
| Disabled | *(empty)* | Hide tasks: `"dark_mode"`, `"screenshot"`, `"kill"`… (the full list is in the configuration reference) | [`[tasks] disabled`](../configuration.md#tasks) |
| Keyword | `t` | `t ` lists the tasks; `""` removes the keyword | [`[tasks] keyword`](../configuration.md#tasks) |
| Global | `true` | Also match task names in ordinary searches | [`[tasks] global`](../configuration.md#tasks) |

To turn the whole plugin off, add `"tasks"` to [`[plugins] disabled`](../configuration.md#plugins).

## Platform notes

=== "Windows"
    Toggling Wi-Fi or Bluetooth needs *Settings → Privacy & security → Radios* to allow desktop apps. The window-shortcut tasks wait a moment so they act on the app you were using, not on Sevak. Quitting an app posts the same close request as its close button, so it may ask to save.

=== "macOS"
    The first use of dark mode, volume, quit or hide-others asks for permission to control System Events; hide-others and show-desktop press keys and need *Privacy & Security → Accessibility* as well. Homebrew tools such as `blueutil` are found in `/opt/homebrew/bin` and `/usr/local/bin`.

=== "Linux"
    Tasks that need a helper (`wmctrl`, `nmcli`, `udisksctl`…) are simply not listed when it is not installed. Quit lists X11 windows (also XWayland); on a pure Wayland session it lists programs started with a display, minus a few desktop helpers.

## Troubleshooting

A task you expected is missing? See [An automation task or media control is missing](../troubleshooting.md#an-automation-task-or-media-control-is-missing).

Media buttons (play, pause, next) are a separate plugin: [Media controls](media.md).
