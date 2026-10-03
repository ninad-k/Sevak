# Searching and launching

Learn how Sevak works: opening it, typing queries, selecting results and running actions.

## Opening and closing

### Show the launcher

Press your global shortcut (default: ++alt+space++, ++option+space++ on macOS). The search bar appears centered on your screen.

On **Linux Wayland**, configure the shortcut first: [Setting up the hotkey](install.md#setting-up-the-hotkey-on-linux).

### Hide the launcher

- Press **++esc++** while the launcher is open
- Click outside the search bar (if "Hide when focus is lost" is on in Settings)
- Run a result and "Hide when focus is lost" is enabled

Hiding does not quit Sevak. It stays in the background until you press the hotkey again.

### Tray menu and system integration

Click the tray icon (Windows, Linux) or menu-bar icon (macOS) to access:

| Menu item | Action |
|---|---|
| **Show** | Open the launcher |
| **Settings** | Open the Settings window |
| **Reload index** | Re-read config, rebuild app/file/bookmark indexes, load new script plugins |
| **Check for updates** | Look for a new version on GitHub |
| **Quit** | Exit Sevak |

## Searching

### Typing a query

The cursor is in the search box as soon as Sevak opens. Start typing immediately—no need to click.

Sevak searches as you type. Results update in real time.

### How results are ranked

Sevak combines three factors:

1. **Query match**: how well your typing matches a result (fuzzy matching allows typos and abbreviations)
2. **Frequency**: how often you have run that result recently
3. **Recency**: how long ago you last ran it

Results using frequently and recently rank higher. This means your most-used apps and files appear first, even with a short query.

Examples:
- Type `cod` → sees `code` (VS Code), `codeserver`, and `mycode.txt`; if you use VS Code most, it ranks first
- Type `pro` → apps like `Prometheus` mix with files like `project.md`, `proposal.docx`; your recent choices decide the order

**New installations**: on first run, all results are equally ranked. As you use Sevak, personalization kicks in.

### Query history

On an **empty search bar**, press **++arrow-up++** and **++arrow-down++** to step through your last 50 searches, newest first.

Typing leaves history. Toggle this on or off in **Settings → Search → Remember searches** or with `[search] query_history`.

## Viewing results

The launcher shows a list of matching results. Each row displays:

- **Icon**: the app, file type, or plugin icon
- **Title**: the name of the app, file, or result
- **Subtitle**: additional info (folder path, app description, calculation result, etc.)
- **Action hint**: what pressing **++enter++** will do (Launch, Open, Copy, etc.)

Use **++arrow-up++** / **++arrow-down++** or **++ctrl+p++** / **++ctrl+n++** to select a result.

Press **++pageup++** or **++pagedown++** to jump by a page (7 results).

### Running a result

Press **++enter++** to run the selected result:

- **Apps**: launch the application
- **Files**: open with the default application (or folder in file manager)
- **Web search**: open the URL in your browser
- **Calculations**: copy the result to your clipboard
- **Clipboard history**: paste the text into the app that had focus before Sevak opened
- **Snippets**: paste the template (with placeholders filled) into the previous app
- **Custom results**: defined by each plugin

### Running from keyboard numbers

Press **++ctrl+1++** through **++ctrl+9++** to run the corresponding result in the list without arrow keys. The numbers match the rows 1–9.

## Result actions

Most results can be used in more than one way. The row at the bottom of the list shows what the modifier keys do:

| Shortcut | Effect |
|---|---|
| **++enter++** | Run the primary action (Launch, Open, etc.) |
| **++ctrl+enter++** | Alternative action bound to Ctrl, if available |
| **++shift+enter++** | Alternative action bound to Shift, if available |
| **++alt+enter++** | Alternative action bound to Alt, if available |

### Action panel

Press **++arrow-right++** (with the cursor at the end of the text) or **++ctrl+k++** to open the action panel, which lists **every available action** for the selected result.

In the action panel:

- **++arrow-up++** / **++arrow-down++** or **++ctrl+p++** / **++ctrl+n++**: select an action
- **++enter++**: run the selected action
- **++ctrl+1++** through **++ctrl+9++** (Universal Actions only): run the numbered action
- **++escape++** or **++arrow-left++** or **++ctrl+k++**: close the panel

### Copying and Large Type

**Copy a result**: press **++ctrl+c++** (when nothing is selected in the text input) to copy the result's value, path, or URL to your clipboard.

**Large Type**: press **++ctrl+l++** to show the selected result's text huge across the screen. Press any key to dismiss it. Useful for reading:
- Long file paths
- URLs
- Calculation results
- Large text from the selection (Universal Actions)

## Tab completion

Press **++tab++** to complete the input to the selected result's name or value. The text in the search box is replaced.

Examples:
- Searching apps: **Tab** fills in the selected app's full name
- File browsing: **Tab** completes to the selected file or folder name (folders get a trailing slash so you can keep drilling)
- Web search: **Tab** completes to `keyword ` (the keyword with a space, ready for your search terms)

When browsing a folder path, **++shift+tab++** goes back up one level.

## Plugins and features

Sevak includes several built-in result sources. Each can be turned on or off in **Settings → Plugins**:

### Applications

Type part of an installed application's name. Fuzzy matching lets a short query find a longer name.

Sources differ by platform:
- **Windows**: Start Menu shortcuts and installed packages
- **macOS**: application bundles
- **Linux**: desktop entries

After installing a new app, choose **Reload index** from the tray menu if it doesn't appear immediately.

More: [Applications](features/apps.md) plugin.

### Calculator

Enter an expression directly; no keyword is required. Results update as you type.

**++enter++** copies the number to your clipboard.

Examples:
- Arithmetic: `12*7`, `(125+75)/4`
- Powers: `2^10`
- Functions: `sqrt(16)`, `sin(1.5)`, `ln(100)`
- Factorial: `5!`
- Remainder: `17%5`

### Unit conversion

Type `<amount> <unit> (in|to|as|=) <unit>`. The amount can be any expression: `(2+3) km in m`.

Supported: length, mass, temperature, volume, area, speed, data, time, pressure, energy, angle.

Examples:
- `10 km in mi`, `5'11" to cm`, `3 ft 4 in to cm`
- `100°F in C`, `0 c to k`
- `1 kg in lb`, `8 oz to g`
- `5 GB in MiB`, `100 Mbit to MB`
- `90 min to h`

For data: `MB` is megabytes (1,000,000 bytes); `MiB` is mebibytes (1,048,576 bytes). `KB`/`MB`/`GB` are powers of 1000; `KiB`/`MiB`/`GiB` are powers of 1024.

### Currency conversion

**Optional and off by default** (requires network). Enable in **Settings → Plugins → Currency conversion**.

When on, `100 usd in eur` works. Use ISO codes, signs or words: `100 usd in eur`, `50 € to $`, `$100 in eur`.

The subtitle shows the exchange rate and the European Central Bank's publication date.

Rates are downloaded at most once per day and cached locally.

More: [Calculator](features/calculator.md) plugin.

### Files and folders

Type **`f `** (keyword + space) followed by a filename or folder name to search indexed directories.

Sevak searches **names only**, not document contents. Default roots: Desktop, Documents, Downloads.

By default, dot-files and cache folders (node_modules, .git, etc.) are excluded.

**Browsing a path**: type a full path (e.g., `~/Documents/`, `/etc/`, `C:\Users\`) to list a folder directly. **Tab** completes entries; **++shift+tab++** goes up; **++enter++** opens.

Configure in **Settings → Files** or with `[files]` in config.toml.

More: [Files](features/files.md) plugin.

### Bookmarks

Type **`b `** and part of a bookmark's title or URL to search bookmarks from Chrome, Edge, Firefox, Safari and other browsers.

Sevak reads your bookmarks across all profiles, read-only.

**++enter++** opens the URL in your default browser.

More: [Bookmarks](features/bookmarks.md) plugin.

### Clipboard history

**Optional and off by default.** Enable in **Settings → Plugins** or with `[clipboard] enabled = true`.

Type **`cb `** to search recent copied text. Results you paste into your previous app with **++enter++**.

Sevak never records content password managers mark as secret, copies made in apps listed in `ignore_apps`, very long text, or images.

More: [Clipboard history](features/clipboard.md) plugin.

### Snippets

Type **`s `** and a snippet name to search your saved text templates. **++enter++** pastes the template (with placeholders filled in) into your previous app.

Placeholders:
- `{date}`, `{time}`, `{datetime}`: today, current time, both
- `{date:FORMAT}`: custom [strftime](https://docs.rs/chrono/latest/chrono/format/strftime/index.html) format, e.g. `{date:%d %B %Y}`
- `{clipboard}`: the current clipboard text
- `{uuid}`: a new random UUID
- `{{`, `}}`: literal `{` and `}`

Configure in `[paste]` and with `[[snippet]]` entries in config.toml.

More: [Snippets](features/snippets.md) plugin.

### System commands

Type a command's name: `lock`, `sleep`, `restart`, `shutdown`, `logout`, `empty trash`, or settings pages (`bluetooth`, `display`, `wifi`, `sound`, etc.).

Dangerous actions (restart, shut down, logout, empty trash) ask for confirmation first. You can disable the ask or hide commands in **Settings → Plugins** or with `[system]` in config.toml.

More: [System commands](features/system.md) plugin.

### Shell commands

Type **`> `** (or `>`) followed by a shell command. **++enter++** opens your terminal and runs it.

`> ` (space only) shows your recent commands and an "Open terminal" row.

Nothing runs until you press Enter—the launcher only prepares the command.

Terminal auto-detection by platform; customize in **Settings** or with `[shell]` in config.toml.

More: [Shell](features/shell.md) plugin.

### Web search

Type a **keyword and your terms**: `g rust traits`, `yt svelte tutorial`, `gh tauri`.

Built-in keywords:
- `g`: Google
- `yt`: YouTube
- `gh`: GitHub

**++enter++** opens the constructed URL in your browser.

When a plain query has no results, a fallback web search is offered (default: Google). Configure or add custom engines in **Settings → Web search** or with `[[web_search]]` in config.toml.

More: [Web search](features/web-search.md) plugin and [adding custom engines](configuration.md#web_search).

### Script plugins

Add your own keywords without building Sevak. Put a folder with `plugin.toml` and a script in the `plugins` folder next to `config.toml`, then **Reload index**.

Scripts run with your permissions (not sandboxed), so install only plugins you trust.

Three examples are in `examples/plugins/`. The full guide is [Plugins](plugins.md#external-plugins).

## Universal Actions {#universal-actions}

Select something in any app—text, a link, files—press ++ctrl+alt+space++ (or your configured hotkey) and Sevak shows what you can do with it.

### Reading the selection

Sevak reads your selection by:

1. Saving your clipboard
2. Pressing ++ctrl+c++ (++cmd+c++ on macOS) in the app you were using
3. Waiting for the copy (up to ~0.3 seconds)
4. Reading the text or files
5. Putting your clipboard back

**Nothing is stored.** The selection is never logged or recorded. Your OS clipboard history (Windows Win+V, etc.) or a clipboard manager *might* see it because the app itself makes the copy.

On **terminal windows** (Windows Terminal, xterm, etc.), Sevak skips the copy to avoid interrupting a running program; use the mouse and the clipboard fallback instead.

On **Wayland**, app-to-app clipboard reading is not possible. Enable **Use the clipboard if the selection can't be read** in Settings to act on the clipboard instead.

### What you can do

| Selection | Actions |
|---|---|
| **Text** | Search with each web engine, show as Large Type, copy, paste as plain text, calculate (if it's a math expression or unit conversion), transform: Uppercase, Lowercase, Title Case, Trim, URL-encode, URL-decode, Base64 encode, Base64 decode, Pretty-print JSON, Minify JSON |
| **URLs** (`http://`, `https://`, `mailto:`, `www.`) | Open, copy, Large Type |
| **Files and folders** | Open, show in file manager, copy path(s), open in terminal (folders), run as admin (Windows programs), send to Sevak (fills search box with the path) |
| **Path as text** | File actions above, then text actions |

### Running actions

Actions are shown in the panel. Use:

- **++arrow-up++** / **++arrow-down++**: select an action
- **++enter++**: run the selected action
- **++ctrl+1++** through **++ctrl+9++**: run the numbered action directly
- **++ctrl+enter++**: run the action but copy instead of pasting/replacing
- **++shift+enter++** (web search): copy the URL instead of opening it
- **++escape++**: close the panel and return to the launcher

Where pasting works (Windows, macOS outside Wayland, Linux X11):
- Transformations and calculator results **replace the selected text** in the app
- ++ctrl+enter++ copies instead of pasting

Where pasting doesn't work (macOS without permission, Wayland):
- Results are **copied** only

More: [Universal Actions](features/selection.md).

## Keyboard shortcuts {#keyboard-shortcuts}

Complete reference: [Keyboard shortcuts](keyboard.md).

Quick summary:

| Context | Shortcut | Action |
|---|---|---|
| **Launcher** | ++alt+space++ | Show or hide (or your configured hotkey) |
| **Navigation** | ++arrow-up++ / ++arrow-down++ or ++ctrl+p++ / ++ctrl+n++ | Previous / next result |
| **Running** | ++enter++ | Run selected result |
| **Actions panel** | ++arrow-right++ or ++ctrl+k++ | Open actions panel |
| **Panel** | ++arrow-up++ / ++arrow-down++ | Select action |
| **Completion** | ++tab++ | Complete to selected result |
| **History** | ++arrow-up++ / ++arrow-down++ (empty bar) | Recall searches |
| **Copying** | ++ctrl+c++ | Copy result value/path/URL |
| **Large Type** | ++ctrl+l++ | Show as Large Type |
| **Close** | ++escape++ | Hide launcher or close panel |

## Command line

These assume `sevak` is on your path; otherwise use the full path to the executable.

```
sevak                       # Open the launcher
sevak --toggle              # Toggle the launcher (open or hide)
sevak --query "> "          # Open the launcher with text pre-filled
sevak --run system:lock     # Run a result by ID without showing the launcher
sevak --actions             # Universal Actions for the current selection
sevak --background          # Start without showing the launcher
sevak --settings            # Open Settings
sevak --quit                # Quit Sevak
sevak --setup-hotkey        # Configure GNOME shortcuts on Linux
sevak --setup-hotkey Ctrl+Space   # Set up a specific key on Linux
sevak --config ~/Dropbox/sevak    # Use another config folder
sevak --help
sevak --version
```

Only one instance runs at a time; subsequent invocations forward their action.

## See also

- [Keyboard reference](keyboard.md) — every key Sevak handles
- [Universal Actions in depth](features/selection.md)
- [All plugins and features](features/index.md)
- [Configuration guide](configuration.md)
