# Keyboard shortcuts

Complete reference to every key Sevak handles.

## Global hotkeys

These work anywhere on your system:

| Shortcut | Action | Configure |
|---|---|---|
| ++alt+space++ | Show or hide the launcher | `[general] hotkey` |
| ++ctrl+alt+space++ | Universal Actions for your current selection | `[general] actions_hotkey` (or empty to disable) |
| Custom `[[hotkey]]` entries | Type a query or run a result without showing the launcher | `[[hotkey]]` in config.toml or Settings → Hotkeys |

On **macOS**, ++alt++ is ++option++, and ++ctrl++ is ++cmd++.

On **Linux Wayland**, apps cannot register global keys; run `sevak --setup-hotkey` to bind the launcher shortcut in GNOME. For the effect of a custom `[[hotkey]]` there, bind a desktop shortcut to `sevak --query "<text>"` or `sevak --run <result-id>` yourself (see [Command line](cli.md)).

## In the launcher window

### Navigation and selection

| Key | Action |
|---|---|
| ++arrow-up++ or ++ctrl+p++ | Previous result |
| ++arrow-down++ or ++ctrl+n++ | Next result |
| ++pageup++ | Previous page (7 results) |
| ++pagedown++ | Next page (7 results) |
| ++tab++ | Complete the input to the selected result's name/value |
| ++shift+tab++ | Go up one folder while browsing a path |

### Running results and actions

| Key | Action |
|---|---|
| ++enter++ | Run the selected result |
| ++ctrl+enter++, ++shift+enter++, ++alt+enter++ | Run the selected result's alternative action bound to that modifier, if it has one. The row shows which ones it offers (for example `Ctrl+↵`), and each [feature page](features/index.md) lists them |
| ++ctrl+1++ to ++ctrl+9++ | Run the 1st to 9th result in the list directly |

### Actions panel

Press ++arrow-right++ (with cursor at the end of text) or ++ctrl+k++ to open; shows every action of the selected result or your selection.

| Key | Action |
|---|---|
| ++arrow-up++ or ++ctrl+p++ | Previous action |
| ++arrow-down++ or ++ctrl+n++ | Next action |
| ++enter++ | Run the selected action |
| ++ctrl+1++ to ++ctrl+9++ | Run the corresponding action (Universal Actions only) |
| ++arrow-left++ or ++ctrl+k++ or ++escape++ | Close the panel (in Universal Actions, ++escape++ hides Sevak, as there is nothing behind the panel) |
| Any other key | Closes the panel and acts as usual (typing goes to the search bar) |

When using Universal Actions, each action's row shows its modifiers (++ctrl++, ++shift++, ++alt++) for alternative behavior.

### Display and copying

| Key | Action |
|---|---|
| ++ctrl+l++ | Show the selected result as Large Type across the screen (dismiss with any key) |
| ++ctrl+c++ | Copy the selected result's path, URL, or value (when nothing is selected in the text input) |

### Search history

On an **empty search bar**:

| Key | Action |
|---|---|
| ++arrow-up++ | Step to the next earlier search (most recent first) |
| ++arrow-down++ | Step to the next later search |

Typing leaves history. Toggle history on or off with `[search] query_history`.

### Closing

| Key | Action |
|---|---|
| ++escape++ | Hide the launcher (or close the actions panel) |

## Settings window

### General navigation

| Key | Action |
|---|---|
| ++ctrl+s++ | Save the current settings |
| ++arrow-down++ or ++arrow-up++ | Move between tabs (when a tab button has focus) |
| ++home++ or ++end++ | Jump to the first or last tab |

### Form controls

- ++tab++ / ++shift+tab++ move between controls
- ++space++ toggles a switch or checkbox
- Arrow keys change a slider or the selected option in a group

## Blocked keys

The following are blocked in production to prevent accidental page reloads:

| Key | Context |
|---|---|
| ++f5++ | Launcher and Settings |
| ++ctrl+r++ | Launcher and Settings |
| ++ctrl+f++ | Launcher and Settings |
| ++ctrl+p++ | Settings (print); in the launcher ++ctrl+p++ means "previous result" |

## Platform differences

| Function | Windows/Linux | macOS |
|---|---|---|
| Ctrl prefix | ++ctrl++ | ++cmd++ |
| Alt prefix | ++alt++ | ++option++ |
| Tray access | Click the icon; no key | Click the menu-bar icon; no key |

## Custom shortcuts

Add global hotkeys in your config file or Settings → Hotkeys:

```toml
[[hotkey]]
key = "Ctrl+Alt+T"
query = "> "                  # open Sevak with this text

[[hotkey]]
key = "Ctrl+Alt+L"
run = "system:lock"           # run this without showing the launcher
```

See [Custom hotkeys](configuration.md#hotkey) for details.
