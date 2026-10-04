# Searching and launching

Learn how Sevak works: opening it, typing queries, selecting results and running actions.

## Opening and closing

### Show the launcher

Press your global shortcut (default: ++win+space++ on Windows, ++cmd+space++ on macOS, ++super+space++ on Linux). The search bar appears centered on your screen. Sevak takes this key over from the system; see [Win+Space, Cmd+Space and Super+Space](troubleshooting.md#super-space), including how to go back to ++alt+space++.

On **Linux Wayland**, configure the shortcut first: [Setting up the hotkey](install.md#setting-up-the-hotkey-on-linux).

### Hide the launcher

- Press **++esc++** while the launcher is open (it first closes whatever is open on top: Large Type, the Text View, the action panel, the preview pane, a folder picker or a workflow's text output)
- Click outside the search bar (if "Hide when focus is lost" is on in Settings)
- Run a result and "Hide when focus is lost" is enabled

Hiding does not quit Sevak. It stays in the background until you press the hotkey again.

### Tray menu and system integration

Click the tray icon (Windows) or menu-bar icon (macOS) to open **Settings**
directly. Right-click it to open the menu below. Linux tray icons (AppIndicator)
always open the menu on click.

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

## Preview, Text View and Grid View

### Preview pane

Tap ++shift++ (press and release it alone) or press ++ctrl+y++ (++cmd+y++ on macOS) to open a pane under the list that shows what the selected result is. It follows the selection as you move; tap ++shift++ or press ++ctrl+y++ again, or ++escape++, to close it (the first ++escape++ closes the pane, the next hides Sevak). It shows:

| Result | The pane shows |
|---|---|
| Text and code files | The first 64 KB in a monospace font, with size and modified date |
| Images (PNG, JPEG, GIF, WebP, SVG, BMP, ICO; up to 4 MB) | The picture and its pixel size |
| PDFs (up to 50 MB) | The first page as a picture, with the page count where the system knows it, then kind, size, path and modified date. Linux needs `pdftoppm` (see below) |
| Word, Excel and PowerPoint files (up to 50 MB) and videos | The system's own thumbnail, when it has one (Windows and macOS), then the details |
| Other binary files | Kind, size, path and modified date |
| Folders | The first 100 entries, folders first, and the item count |
| Applications | Kind, path or launch command, and the version when it is cheap to read (macOS apps) |
| Web results and bookmarks | The address, title and site. Nothing is fetched from the network |
| Snippets | The text as ++enter++ would paste it, with `{date}` and the other placeholders filled in |
| Clipboard history entries | The copied text, the full image, or the copied file (several files: their paths) |
| Calculator and conversions | The result and the calculation |
| Emoji | The emoji large, with its name, keywords and code points |

PDF pages and thumbnails are drawn by your operating system, not by Sevak: Windows uses its built-in PDF engine and the Explorer thumbnail, macOS uses Quick Look, and Linux runs `pdftoppm` from poppler-utils (install it with your package manager, for example `sudo apt install poppler-utils`; without it a PDF shows its details and a one-line hint). Each picture is made by a short-lived helper with a ten-second limit, at most 900 pixels wide, shown only while the pane is open, and deleted at once; nothing is cached on disk. A protected, damaged or very large PDF shows its details and a note instead.

The pane reads only the file or folder the selected result refers to, only when it is open, and never reads network locations (`\\server\share`). The window grows to make room and shrinks back when you close the pane; near the bottom of a small screen it moves up so the pane stays visible.

### Text View

A result that carries a long text (a long or multi-line clipboard entry, a snippet, a script plugin's output) can be opened in a scrollable, taller view with ++ctrl+t++. ++arrow-up++ ++arrow-down++ ++pageup++ ++pagedown++ ++home++ ++end++ scroll; ++ctrl+c++ copies the text, ++enter++ runs the result as usual, and ++escape++ or ++arrow-left++ goes back to the list. Rows that exist only to show text (marked by the script that produced them) open the Text View on ++enter++.

### Grid View

Plugins that offer pictures show their results as a grid of tiles instead of a list, when every result of a search is a tile. Move with ++arrow-left++ ++arrow-right++ ++arrow-up++ ++arrow-down++ (++pageup++ / ++pagedown++ jump three rows), ++enter++ runs the selected tile, ++ctrl+k++ opens its actions, and the preview pane (++shift++ or ++ctrl+y++) works as for list rows. The grid shows up to 60 tiles. Built in:

- The [emoji picker](features/emoji.md): `:heart` or `emoji heart`.
- Copied images in [clipboard history](features/clipboard.md): `cb image`.
- Script plugins can request tiles too: see [Views: text and grid](plugins.md#views-text-and-grid).

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

`f` searches **names** in the folders you chose. Default roots: Desktop, Documents, Downloads.

**Whole disk and inside files**: `ff report` asks your computer's own file index (Windows Search, Spotlight, `locate`) for names anywhere, and `in invoice 2026` for words inside documents. Queries stay on your computer. See [Whole-disk and content search](features/files.md#whole-disk-and-content-search).

By default, dot-files and cache folders (node_modules, .git, etc.) are excluded.

**Browsing a path**: type a full path (e.g., `~/Documents/`, `/etc/`, `C:\Users\`) to list a folder directly. **Tab** completes entries; **++shift+tab++** goes up; **++enter++** opens.

**File buffer**: press ++alt+arrow-down++ on file results to collect them, then ++alt+arrow-right++ to open, move, copy, zip or trash them all at once. See [File buffer](features/files.md#file-buffer).

Configure in **Settings → Files & bookmarks** or with `[files]` in config.toml.

More: [Files](features/files.md) plugin.

### Bookmarks

Type **`b `** and part of a bookmark's title or URL to search bookmarks from Chrome, Edge, Firefox, Safari and other browsers.

Sevak reads your bookmarks across all profiles, read-only.

**++enter++** opens the URL in your default browser.

More: [Bookmarks](features/bookmarks.md) plugin.

### Clipboard history

**Optional and off by default.** Enable in **Settings → Clipboard & paste** or with `[clipboard] enabled = true`.

Type **`cb `** to search what you copied recently: text, images and files. **++enter++** pastes the entry into your previous app. `cb image` shows copied images as a grid of thumbnails.

Sevak never records content password managers mark as secret, copies made in the common password managers or in apps listed in `ignore_apps`, very long text or very large images. The history and the images (PNG files) are kept in the local data folder, encrypted for your account on Windows and owner-only elsewhere; turn images off with `[clipboard] images = false`.

More: [Clipboard history](features/clipboard.md) plugin.

### Snippets

Type **`s `** and a snippet name to search your saved text templates. **++enter++** pastes the template (with placeholders filled in) into your previous app.

Placeholders:
- `{date}`, `{time}`, `{datetime}`: today, current time, both
- `{date:FORMAT}`: custom [strftime](https://docs.rs/chrono/latest/chrono/format/strftime/index.html) format, e.g. `{date:%d %B %Y}`
- `{clipboard}`: the current clipboard text
- `{uuid}`: a new random UUID
- `{{`, `}}`: literal `{` and `}`

Configure pasting in **Settings → Clipboard & paste** (or `[paste]`), and snippets with `[[snippet]]` entries in config.toml.

**Expand as you type** (off by default): with `[snippets] auto_expand = true` (or **Settings → Plugins**), typing a snippet's `keyword` in any app replaces it with the snippet. While on, Sevak watches your keystrokes, keeping only the last 64 characters in memory. See [Expand snippets as you type](features/snippets.md#expand-snippets-as-you-type).

More: [Snippets](features/snippets.md) plugin.

### System commands

Type a command's name: `lock`, `sleep`, `restart`, `shutdown`, `logout`, `empty trash`, or settings pages (`bluetooth`, `display`, `wifi`, `sound`, etc.).

Dangerous actions (restart, shut down, logout, empty trash) ask for confirmation first. You can disable the ask or hide commands in **Settings → System & terminal** or with `[system]` in config.toml.

More: [System commands](features/system.md) plugin.

### Automation tasks

Type a task's name, or `t ` to list them all: `dark mode`, `vol 30`, `screenshot`, `quit`, `kill chrome`, `eject`, `awake 45`, `wifi`, `flush dns`… Only the tasks that work on your machine are offered. Force quit, kill and restarting Explorer or Finder ask first.

More: [Automation tasks](features/tasks.md).

### Media controls

Type `play`, `pause`, `next`, `previous` or `stop` to press the media button of whatever is playing. `play ` (with a space) or `music` also shows the current track.

More: [Media controls](features/media.md).

### Shell commands

Type **`> `** (or `>`) followed by a shell command. **++enter++** opens your terminal and runs it.

`> ` (space only) shows your recent commands and an "Open terminal" row.

Nothing runs until you press Enter—the launcher only prepares the command.

Terminal auto-detection by platform; customize in **Settings → System & terminal** or with `[shell]` in config.toml.

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

### Workflows

Chain a trigger (a keyword, a script filter, a hotkey, a Universal Actions entry or `sevak --trigger`) to actions and outputs, built as boxes and connectors in **Settings → Workflows**: type `issue 14` to open issue 14, or select text in any app and have a workflow tidy it and paste it back. Start from one of the templates in **New from template…**.

A workflow that runs scripts or commands asks for your permission first, and again if what it runs changes. **Settings → Gallery** installs ready-made workflows and script plugins, but only after you press **Load gallery** and **Install**.

More: [Workflows](workflows.md).

### Emoji picker

Type `:` and a name (`:heart`) or `emoji ` and a name. Matches are shown as a grid; **++enter++** pastes the emoji, **++shift+enter++** copies it.

More: [Emoji picker](features/emoji.md).

### Contacts and 1Password

Both are **off by default**. `c ada` or `@ada` searches your address book (vCard files and the system address book); **++enter++** copies the e-mail address. `1p github` lists your 1Password logins through the official `op` tool; **++enter++** opens the website. Sevak never reads passwords.

More: [Contacts](features/contacts.md) and [1Password](features/1password.md).

### Dictionary and spelling

`define serendipity` shows definitions; `spell recieve` lists corrections and **++enter++** pastes the right spelling. Everything is offline.

More: [Dictionary and spelling](features/dictionary.md).

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
| **Launcher** | ++win+space++ / ++cmd+space++ / ++super+space++ | Show or hide (or your configured hotkey) |
| **Navigation** | ++arrow-up++ / ++arrow-down++ or ++ctrl+p++ / ++ctrl+n++ | Previous / next result |
| **Running** | ++enter++ | Run selected result |
| **Actions panel** | ++arrow-right++ or ++ctrl+k++ | Open actions panel |
| **Panel** | ++arrow-up++ / ++arrow-down++ | Select action |
| **Completion** | ++tab++ | Complete to selected result |
| **History** | ++arrow-up++ / ++arrow-down++ (empty bar) | Recall searches |
| **Copying** | ++ctrl+c++ | Copy result value/path/URL |
| **Large Type** | ++ctrl+l++ | Show as Large Type |
| **Preview** | ++shift++ (tap) or ++ctrl+y++ | Show or hide the preview pane |
| **Text View** | ++ctrl+t++ | Read the result's long text in full |
| **File buffer** | ++alt+arrow-up++ / ++alt+arrow-down++ | Add the file to the buffer and move |
| **Close** | ++escape++ | Close the innermost panel or view, then hide the launcher |

## Command line

These assume `sevak` is on your path; otherwise use the full path to the executable.

```
sevak                       # Open the launcher
sevak --toggle              # Toggle the launcher (open or hide)
sevak --query "> "          # Open the launcher with text pre-filled
sevak --run system:lock     # Run a result by ID without showing the launcher
sevak --actions             # Universal Actions for the current selection
sevak --trigger my-flow/go some text   # Start a workflow's external trigger
sevak --background          # Start without showing the launcher
sevak --settings            # Open Settings
sevak --quit                # Quit Sevak
sevak --setup-hotkey        # Configure GNOME shortcuts on Linux
sevak --restore-hotkey      # Put back system shortcuts Sevak changed (GNOME, macOS Spotlight)
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
- [Workflows](workflows.md)
- [Configuration guide](configuration.md)
