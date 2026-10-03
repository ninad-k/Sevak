# Applications

Launch installed applications by typing their name. Sevak finds applications from your system (Start Menu and packaged apps on Windows, application bundles on macOS, desktop entries on Linux) and ranks matches by how closely they fit your query and how often you use them.

## How to use it

Type any part of an application's name. Fuzzy matching lets you skip letters:

| Input | What you see | What ++enter++ does |
|---|---|---|
| `chrome` | Google Chrome | Launch Chrome |
| `msg` | Microsoft Outlook (messages) | Launch Outlook |
| `calc` | Calculator | Open Calculator |
| `vs co` | Visual Studio Code | Launch VS Code |
| `fire` | Firefox | Launch Firefox |

Matching is case-insensitive. The launcher shows the best matches in order; use the arrow keys to choose a different result.

## Actions

| Key(s) | Action |
|---|---|
| ++enter++ | Launch the application |
| ++ctrl+enter++ | Show the application file in your file manager |
| ++shift+enter++ | Copy the application's full path to your clipboard |
| ++alt+enter++ | Run as administrator (Windows and Linux only; packaged apps cannot be elevated) |

Use ++ctrl+k++ to open the actions panel and see all available actions.

## Tips and troubleshooting

**The app doesn't appear:** Applications are indexed when Sevak starts. Install the app, then restart Sevak or choose **Reload index** from the tray menu.

**Multiple versions of the same app:** If you have several versions installed (e.g., both 32-bit and 64-bit), they all appear as separate results.

**Packaged apps:** On Windows, packaged apps from the Microsoft Store may have long, generated names (like `9P...`) in the launcher even if they display nicely in the Start Menu. Pin your favorite packaged apps in the Start Menu to see them with their display names.

**Custom keyboard shortcut:** You can bind a key to launch a specific app using [hotkeys in config.toml](../configuration.md#hotkey). For example, `run = "apps:firefox.desktop"` on Linux or `run = "apps:Google Chrome"` on Windows.
