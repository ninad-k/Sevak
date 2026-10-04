# Browser bookmarks

Search bookmarks from all your browsers and profiles. Everything is read locally from disk; nothing is sent anywhere. ++enter++ opens the bookmarked URL in your browser.

## How to use it

Type `b ` followed by a bookmark name or part of a URL:

| Input | What you see | What ++enter++ does |
|---|---|---|
| `b github` | Bookmarks with "github" in the title or URL | Open github.com |
| `b documentation` | Pages bookmarked as "Documentation" | Open the URL |
| `b stackoverflow` | Stack Overflow bookmarks | Open the URL |
| `documentation` (no keyword) | Bookmarks matching "documentation" + apps + files | Open the bookmark (if global is enabled) |

Sevak searches bookmark titles and URLs. Bookmarks are deduplicated across browsers and profiles; if the same URL exists in Chrome and Firefox, they merge into one result showing both browser names.

## Supported browsers

**Chromium family** (Chrome, Edge, Brave, Vivaldi, Chromium, Opera, Opera GX on Windows and macOS):
- Reads the `Bookmarks` JSON file from each profile
- All profiles of each browser are searched automatically

**Firefox family** (Firefox, LibreWolf, Zen):
- Reads `places.sqlite` from each profile
- Profiles listed in `profiles.ini` are searched automatically

**Safari** (macOS only):
- Reads `~/Library/Safari/Bookmarks.plist`: the Favorites bar, the Bookmarks Menu and every folder inside them (shown as `Favorites / Dev` in the result's subtitle)
- The Reading List and the History entry are not bookmarks and are skipped
- macOS keeps this file behind **Full Disk Access**; see [Safari needs Full Disk Access](#safari-needs-full-disk-access)

## Actions

| Key(s) | Action |
|---|---|
| ++enter++ | Open the bookmarked URL |
| ++shift+enter++ | Copy the URL to your clipboard |

Use ++ctrl+k++ to see all available actions. If a bookmark is stored in multiple browsers, the browser names appear in the result's subtitle.

## Options

| Setting | Default | What it does | Config section |
|---|---|---|---|
| Browsers | `[]` (auto-detect) | Which browsers to search (`[]` = all found, or `["chrome", "firefox"]`) | [`[bookmarks] browsers`](../configuration.md#bookmarks) |
| Keyword | `b` | Keyword to search only bookmarks | [`[bookmarks] keyword`](../configuration.md#bookmarks) |
| Global | `true` | Also show bookmarks in ordinary searches without the keyword | [`[bookmarks] global`](../configuration.md#bookmarks) |

### Choosing which browsers to search

By default, Sevak searches every installed browser and all profiles. To limit it:

```toml
[bookmarks]
browsers = ["chrome", "firefox"]     # search Chrome and Firefox only
browsers = []                        # search all browsers (default)
browsers = ["edge"]                  # search Edge only
```

Valid browser ids: `chrome`, `edge`, `brave`, `vivaldi`, `chromium`, `opera`, `opera-gx`, `firefox`, `librewolf`, `zen`, `safari` (macOS only).

### Safari needs Full Disk Access

macOS protects Safari's bookmarks file (it is covered by the same privacy rule as Mail and Messages), so Sevak can read it only after you allow it once:

1. Open **System Settings → Privacy & Security → Full Disk Access**.
2. Switch **Sevak** on (add it with **+** if it is not in the list).
3. Quit Sevak and start it again; macOS applies the permission to programs started afterwards.

Until then, Safari's bookmarks are simply missing. Sevak never asks at startup and never interrupts a search. Only when you type the keyword and nothing else (`b `) does it show one row, **Safari bookmarks need Full Disk Access**; ++enter++ on it opens the Full Disk Access list in System Settings. Sevak does nothing else with the permission: it reads only the Safari bookmarks file, and only to search it.

If you do not use Safari, either ignore the row or list the browsers you do use, which also hides it:

```toml
[bookmarks]
browsers = ["chrome", "firefox"]
```

If Safari has never been used there is no bookmarks file, and nothing is shown.

## How indexing works

Bookmarks are indexed at startup and every few minutes in the background. Sevak reads each browser's bookmarks file only when it changes (checked by file modification time), so an idle refresh is cheap.

**Limits:** The index is capped at 50,000 bookmarks to bound memory. If you exceed this, older bookmarks may not appear.

**Excluded:** Bookmarks whose URL is not `http(s)` (e.g., `javascript:`, `chrome:`, `file:`, `place:`) are skipped, and so are Safari's Reading List and History.

## Platform notes

=== "Windows"

    Browsers store bookmarks in `%APPDATA%\<browser>\User Data\Default\Bookmarks` (Chrome family) or `%APPDATA%\<browser>\Profiles\<profile>\places.sqlite` (Firefox). Sevak reads from these locations automatically.

=== "macOS"

    Bookmarks are stored in `~/Library/Application Support/<browser>/`. Safari's are in `~/Library/Safari/Bookmarks.plist` and need [Full Disk Access](#safari-needs-full-disk-access); the other browsers need no permission.

=== "Linux"

    Bookmarks are stored in `~/.config/<browser>/` or `~/.mozilla/firefox/`. Sevak reads from these locations automatically.

## Tips and troubleshooting

**Bookmarks don't appear:** Open **Settings** and check that bookmarks are enabled under **Plugins**. If the browser was just installed, choose **Reload index** from the tray menu.

**Safari's bookmarks don't appear (macOS):** Allow Sevak in **System Settings → Privacy & Security → Full Disk Access**, then quit and start Sevak again. See [Safari needs Full Disk Access](#safari-needs-full-disk-access).

**Browser not detected:** Sevak looks for browsers in their default locations. If you installed a browser somewhere unusual, it may not be found. In that case, bookmark a page in your default browser or use path browsing to navigate to the bookmark file manually.

**Too many bookmarks:** If you have 50,000+ bookmarks (rare!), the oldest or least-used ones may not be indexed. Reduce the number of bookmarks or add more of your favorites to the top level of your bookmark structure so they stay in the index.

**Merging profiles:** If you use multiple profiles in the same browser, Sevak searches all of them. Bookmarks with the same URL appear as one result showing both profile names in the subtitle.

**Bookmarks not updating:** The index refreshes every few minutes. If you just added a bookmark, wait a moment or choose **Reload index** from the tray menu to see it immediately.
