# Window management

Snap, resize and move the window you were using, send it to another display, and jump to any open window by its title, all from the search bar. It works on Windows, macOS and Linux (X11).

## Arrange the window you were using

Open Sevak with its shortcut, type `win ` and a layout, and press ++enter++. Sevak closes and the window that had focus *before* Sevak opened is moved. (Sevak remembers that window the moment it opens, the same way pasting returns to the right app.)

| Type | What happens |
|---|---|
| `win left`, `win right` | Left or right half of the screen |
| `win top`, `win bottom` | Top or bottom half |
| `win top left`, `tl`, `tr`, `bl`, `br` | A quarter of the screen |
| `win left third`, `center third`, `right third` | A third of the screen |
| `win left two thirds`, `right two thirds` | Two thirds of the screen |
| `win max` | Fills the screen (without the taskbar, dock or menu bar) |
| `win almost-max` | Fills 90 % of the screen and leaves a margin |
| `win center` | Keeps the window's size and puts it in the middle |
| `win restore` | Back to where the window was before Sevak first moved it |
| `win next display`, `win previous display` | To the next or previous display, keeping its relative position and size |

A bare `win ` lists every layout; the **Cheat sheet** in **Settings → Windows** shows them with the exact spelling. Layouts are found by their name or a short alias (`win max`, `win maximise`, `win fill`), and hyphens count as spaces (`almost-max`).

- **Gap.** **Settings → Windows → Gap** (`[window_management] gap`) keeps that many pixels between snapped windows and the screen edge, and between two windows next to each other. It is in logical pixels, so it looks the same on a high-DPI display.
- **Displays.** Layouts use the work area of the display the window is mostly on. *Next display* walks the displays left to right (then top to bottom) and wraps around; with one display it says so.
- **Restore.** Sevak remembers where a window was before its first layout. If you move the window by hand afterwards, that new position becomes the one `win restore` returns to. A window that was maximized comes back at its size and position, not in the maximized state.
- **Maximized and minimized windows** are restored first and then placed. A full-screen window leaves full screen.

### Global shortcuts for layouts

Each layout has a stable result id, `windows:<key>`, so a [`[[hotkey]]`](configuration.md#hotkey) can run it directly. The shortcut acts on the window that has focus when you press it, with no launcher in between:

```toml
[[hotkey]]
key = "Ctrl+Alt+Left"
run = "windows:left"

[[hotkey]]
key = "Ctrl+Alt+Right"
run = "windows:right"

[[hotkey]]
key = "Ctrl+Alt+Enter"
run = "windows:maximize"
```

The keys are `left`, `right`, `top`, `bottom`, `top_left`, `top_right`, `bottom_left`, `bottom_right`, `left_third`, `center_third`, `right_third`, `left_two_thirds`, `right_two_thirds`, `maximize`, `almost_maximize`, `center`, `restore`, `next_display` and `previous_display`. Pick keys that no other app uses (many apps use ++ctrl+alt+arrow-left++, for example). Layouts do not work on Wayland (see below).

## Switch windows

Type `w ` and part of a window's title or of its app's name:

| Type | What happens |
|---|---|
| `w` then space | Lists the open windows, most recently used first |
| `w fire` | Windows whose title or app matches (`Release notes - Firefox`, any window of Firefox) |
| ++enter++ | Brings that window to the front; a minimized window is restored first |

The list is read from the system in the background, so the first keystroke after a pause may show it a moment later. Windows of Sevak itself, tool windows, panels and taskbars are left out. On Windows only the current virtual desktop is listed; on Linux the window manager decides whether activating a window on another workspace switches to it.

## Options

All of these are in **Settings → Windows**.

| Setting | Default | What it does | Config section |
|---|---|---|---|
| Window management | on | Turns the layouts and the switcher on or off together | [`[window_management] enabled`](configuration.md#window_management) |
| Layouts keyword | `win` | `win ` lists the layouts; `""` removes the keyword | [`[window_management] keyword`](configuration.md#window_management) |
| Switcher keyword | `w` | `w ` lists the windows; `""` turns the switcher off | [`[window_management] switcher_keyword`](configuration.md#window_management) |
| Gap | `0` | Pixels between snapped windows and the screen edge (0 to 200) | [`[window_management] gap`](configuration.md#window_management) |
| Global | `false` | Also match layout names in ordinary searches (`snap left`) | [`[window_management] global`](configuration.md#window_management) |

To turn the plugin off, add `"windows"` to [`[plugins] disabled`](configuration.md#plugins) (or untick **Window layouts** and **Window switcher** in **Settings → Plugins**).

## Platform notes

=== "Windows"
    Sevak moves windows with the Win32 API (`SetWindowPos`). Windows 10 and 11 give every window an invisible resize border of a few pixels; Sevak measures it (through DWM) and compensates, so snapped windows touch each other and the screen edge exactly. Layouts use physical pixels of each display and the gap scales with the display's DPI.

    A window that runs **as administrator** cannot be moved by a program that does not (Windows blocks it): Sevak reports "Windows does not let Sevak move that window". Run Sevak as administrator too if you need that. Old DPI-unaware programs may end up a few pixels off, because Windows scales them itself.

=== "macOS"
    Moving other apps' windows needs the **Accessibility** permission for Sevak, in *System Settings → Privacy & Security → Accessibility*. The first use also asks to allow Sevak to control **System Events** (*Privacy & Security → Automation*). Until both are granted, `win ` and `w ` show one row that explains what is missing instead of failing when you press ++enter++; nothing prompts at startup.

    Sevak asks System Events for windows and displays through small `osascript` scripts (nothing is installed, and a window's title is only ever passed as data). The target is the first window on screen of the app that was frontmost when Sevak opened. Windows an app does not expose to Accessibility cannot be moved. Coordinates and the gap are in points. Native full-screen windows leave full screen before they are moved.

=== "Linux"
    Layouts and the switcher work in **X11** sessions with an [EWMH](https://specifications.freedesktop.org/wm-spec/latest/)-compliant window manager (GNOME/Mutter, KDE/KWin, Xfce, i3, Openbox, ...). Sevak lists windows with `_NET_CLIENT_LIST_STACKING`, focuses them with `_NET_ACTIVE_WINDOW`, places them with `_NET_MOVERESIZE_WINDOW` (compensating the decorations from `_NET_FRAME_EXTENTS` and GTK's transparent shadow from `_GTK_FRAME_EXTENTS`) and reads the displays through RandR. X11 has no per-display work area, so Sevak derives it from the panels' struts (`_NET_WM_STRUT_PARTIAL`), falling back to `_NET_WORKAREA`. Terminals that resize in whole characters may end up a few pixels short of the layout.

    **Wayland** does not let an application list or move other applications' windows, and there is no portable protocol for it. On a Wayland session `win ` and `w ` therefore show one row that says so (log in to an X11 session, or use your desktop's own tiling shortcuts such as GNOME's ++super+arrow-left++ or KDE's *Quick Tile*). Sevak does not use compositor-specific interfaces (GNOME Shell extensions, KWin scripts).

## Troubleshooting

- **"Window management is not available"**: the row's second line says why: a Wayland session, a window manager without EWMH, or a missing macOS permission.
- **Nothing moves, no error**: the layout acted on the window that had focus when Sevak opened. If you opened Sevak from the tray or while another Sevak window was in front, there may be no such window ("There is no previous window to arrange").
- **A window ends a few pixels off**: the app enforces a minimum or maximum size, or a size grid (terminals). Sevak places it as close as the app allows.
- **`win restore` says there is nothing to restore**: Sevak only remembers windows it moved, and forgets the oldest after 32.

## Privacy

Everything happens on your computer. Window titles are read when you use the switcher and kept in memory for a couple of seconds; nothing is written to disk or sent anywhere.
