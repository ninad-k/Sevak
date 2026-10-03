# Your first five minutes with Sevak

[← Help center](README.md) · [Full user guide](usage.md)

## 1. Install and open

Download a package from [GitHub Releases](https://github.com/ninad-k/Sevak/releases)
and follow the [platform instructions](install.md). Open Sevak from your
application menu once. Its tray or menu-bar icon keeps it accessible.

On Linux Wayland, configure a desktop shortcut first:
[Wayland shortcut setup](install.md#setting-up-the-hotkey-on-linux).

## 2. Bring up the search bar

Press **Alt+Space**. On macOS, this is **Option+Space**. Type into the search
bar immediately; press **Esc** when you want to hide it.

![An empty Sevak search bar, ready for input](media/launcher-ready.png)

If the key is already used by another application or your desktop, open
**Settings → General** from Sevak's tray and choose another shortcut.

## 3. Open an app

Type the name of an app installed on your machine, such as `code` for Visual
Studio Code. Use **↑ / ↓** to select it and press **Enter** to launch.

![Example app results for the query code](media/launcher-apps.png)

These images use sample results. Your list reflects installed apps and usage.

## 4. Try a calculation

Type `12*7`. Sevak shows **84**. Press **Enter** to copy the result, then paste
it into the document or app you were working in.

![Sevak calculates 12 times 7 and offers 84 to copy](media/launcher-calculator.png)

You can also try `sqrt(16)`, `2^10` or `(125+75)/4`.

## 5. Find a file or search the web

| Type | What happens |
|---|---|
| `f project` | Finds matching file/folder names in your indexed folders |
| `g rust traits` | Offers a Google search; Enter opens your browser |
| `yt svelte tutorial` | Offers a YouTube search |
| `gh tauri` | Offers a GitHub search |

Put a **space after the keyword**. File search initially covers Desktop,
Documents and Downloads. To include another location, open **Settings → Files**,
add the folder and click **Save**. Wait for indexing, then search again.

## Make it comfortable

- **General:** choose a shortcut and whether Sevak starts at login.
- **Appearance:** choose System, Light or Dark.
- **Search:** set the result limit and fallback web engine.
- **Plugins:** turn result sources on or off.
- **Web search:** add your own keyword shortcuts.
- **Files:** choose folders and indexing depth.

[Configuration examples](configuration.md) explain the options. If something
fails, use the [troubleshooting guide](troubleshooting.md).

## Watch the workflow

[12-second vertical walkthrough with captions and subtle sound](media/sevak-12s.mp4).
The animation uses the real interface components with sample results.
