# Your first five minutes with Sevak

A quick walkthrough to get you searching and launching.

## 1. Install and open

Download from [GitHub Releases](https://github.com/ninad-k/Sevak/releases) and follow the [platform-specific instructions](install.md).

On **Linux Wayland**, configure your desktop hotkey first: [Setting up the hotkey on Linux](install.md#setting-up-the-hotkey-on-linux).

Open Sevak once. You'll see a tray or menu-bar icon that keeps it always accessible.

## 2. Bring up the search bar

Press **++alt+space++** (++option+space++ on macOS) to open the launcher.

![An empty Sevak search bar, ready for input](media/launcher-ready.png)

Type immediately—the cursor is already in the search box. Press **++esc++** to hide it.

If ++alt+space++ is already used by another app, open **Settings** from the tray and change it.

## 3. Launch an app

Type the name of an installed app: `code`, `firefox`, `vs code`.

Use **++arrow-up++** / **++arrow-down++** to pick the right result, then press **++enter++** to launch.

![Example app search results](media/launcher-apps.png)

Sevak learns what you use most and ranks results by frequency and recency. Recent searches reappear first.

## 4. Do a quick calculation

Type `12*7` and press **++enter++** to copy the result (`84`).

Paste it anywhere. Try `sqrt(16)`, `2^10`, `(125+75)/4`, or `10 km in mi`.

![Sevak calculates 12 times 7 and shows 84 to copy](media/launcher-calculator.png)

Sevak handles arithmetic, unit conversion, and more. See [Calculator](usage.md#calculator).

## 5. Find a file or search the web

| Query | Result |
|---|---|
| `f project` | Finds files/folders matching "project" in your indexed folders (default: Desktop, Documents, Downloads) |
| `g rust traits` | Opens Google with that search in your browser |
| `yt svelte` | YouTube search |
| `gh tauri` | GitHub search |

**Important**: put a space after the keyword (`f `, `g `, `yt `, `gh `).

![Web search for "svelte tutorial" on YouTube](media/launcher-web.png)

## Make it yours

Five quick customizations in **Settings**:

1. **General**: Change the hotkey and set whether Sevak starts at login
2. **Appearance**: Choose Light, Dark, or System theme; adjust font size and window width
3. **Search**: Set the max number of results and fallback web engine
4. **Files**: Add more folders to search
5. **Plugins**: Turn features on or off (Calculator, Bookmarks, Clipboard history, etc.)

Click **Save** when done.

More options: [Configuration guide](configuration.md).

## Watch it in action

[12-second workflow demo with captions](media/sevak-12s.mp4) (real interface, sample results).

## What's next?

- **Search like a pro**: [Searching and launching](usage.md) — everything you can search and how results are ranked
- **All keyboard shortcuts**: [Keyboard reference](keyboard.md)
- **Universal Actions**: [Selection-based actions](usage.md#universal-actions) — search, transform, calculate or run actions on text, links or files from any app
- **Deep customization**: [Configuration guide](configuration.md) — hotkeys, themes, custom search engines and more
- **Stuck?** [Troubleshooting](troubleshooting.md) or [FAQ](faq.md)

**Enjoy!** Sevak is here to save you time.
