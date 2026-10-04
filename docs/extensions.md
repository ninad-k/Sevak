# Extensions for Sevak

Add a focused tool without rebuilding the application. Sevak extensions use
its script-plugin protocol; the Gallery installs them as individual packages.
The first productivity pack includes four tools and an optional theme.

## Browse and install

1. Open **Settings → Gallery → Load gallery**.
2. Search by name, keyword or description. Filter by **Extensions**, **Workflows**,
   or installation status.
3. Read the requirements, then choose **Install**. Sevak verifies the package
   checksum and adds it to your plugins folder.
4. Allow the plugin's displayed command when prompted. Install **Node.js 22+**
   on your command path first; these extensions need it to run.
5. Open the launcher and type the keyword **followed by a space**.

The gallery reads the published `main` branch. In a checkout with unpublished
packages, copy a folder from [`examples/plugins`](https://github.com/ninad-k/Sevak/tree/main/examples/plugins)
to your [plugins folder](features/script-plugins.md#install-a-plugin), then
choose **Reload index**. Keep the entire folder together. No npm install is
needed inside an extension: these tools use only Node's standard library.

## The productivity pack

| Extension | Try | What it does |
|---|---|---|
| **Color tools** | `color #f5b52c` | Copies HEX, RGB or HSL |
| **Color tools** | `color #fff on #111` | Shows text contrast ratings or copies the report |
| **Pomodoro** | `pomo start 25` | Starts a focus timer when you press Enter |
| **Pomodoro** | `pomo status` | Checks remaining time and completed focus sessions |
| **Translate in browser** | `tr fr hello world` | Opens Google Translate with French as the target |
| **Tauri docs** | `tauri updater` | Opens the matching official Tauri 2 guide |
| **Catppuccin Mocha** | Theme gallery | Applies a dark palette with mauve accents |

### Color tools

Use opaque `#RGB`, `#RRGGBB`, `rgb(245, 181, 44)` or
`hsl(41, 91%, 57%)`. Enter copies a conversion. Add `on` or `vs` and another
color to see a WCAG 2.x text contrast report. Decisions use the unrounded
ratio, with separate normal/large-text AA and AAA ratings.

The extension works offline and writes no files. It converts typed colors;
it does not sample screen pixels, handle alpha or check overall accessibility.
[How contrast is calculated](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html).

### Pomodoro

Type `pomo start 25` for focus, `pomo break 5` for a break, `pomo pause`,
`pomo resume`, or `pomo reset`. **Enter confirms the command.** Durations are
whole minutes from 1 to 180. Starting a timer replaces any current one.

The deadline and completed count are saved in `plugins/pomodoro/timer.json`
under Sevak's [data directory](files-and-data.md). Time continues to elapse
across plugin/app restarts. A finished focus session is counted once when
you next query it. Breaks start manually.

Reopen or retype `pomo status` to refresh; the host may cache results for two
seconds. This version has **no live countdown, background alarm or OS notification**.
It makes no network requests. A corrupt saved file produces a help message;
back up or rename that file to reset without losing the original.

### Translate in browser

Use `tr <target language> <text>`, such as `tr en bonjour`, `tr hi good morning`
or `tr ar hello`. Type `tr ` to see supported language codes. Source language
detection happens in Google Translate. Maximum input is 5,000 Unicode characters.

This is a browser shortcut: translations appear on Google's website, not in
the launcher. Typing makes no request; pressing Enter sends the text to Google
through your browser. The extension saves no text or translation history.

### Tauri docs

Search a curated list of official Tauri 2 guide titles and keyword tags.
Try `tauri rust`, `tauri global shortcut` or `tauri clipboard`. All query words
must match a guide. The index search is offline; Enter loads the official page
in your browser. It is not full-text search or an offline documentation mirror.

### Catppuccin Mocha

Go to **Settings → Appearance → Theme editor → Browse online themes**.
Install **Catppuccin Mocha**, then select/apply it in the editor. From an
unpublished checkout, import `gallery/themes/Catppuccin-Mocha.toml` instead.

The palette is from [Catppuccin](https://github.com/catppuccin/palette), under
its MIT license; the Sevak adaptation and license are included in this repo.
It needs no script runtime or plugin approval.

## Tools already built into Sevak

Several common extension categories are already available:

| Need | Existing feature |
|---|---|
| Emoji and symbols | [Emoji picker](features/emoji.md) |
| Prevent sleep | `awake 45` in [automation tasks](features/tasks.md) |
| Quit/kill a process | [Automation tasks](features/tasks.md) |
| Contacts | [Contacts integration](features/contacts.md) |
| Browser bookmarks | [Bookmarks](features/bookmarks.md) |
| Appearance customization | [Themes and the editor](themes.md) |

These are original Sevak-compatible extensions inspired by categories in the
[Asyar catalog](https://asyar.org/extensions). Asyar's extension packages and
SDK are not directly compatible. Steam indexing, paired live browser tabs,
Home Assistant control, speed tests and AI-agent integrations are not part of
this pack.

## Disable or develop an extension

Switch it off under **Settings → Plugins**. IDs for this pack are
`script:color-tools`, `script:pomodoro`, `script:translate` and `script:tauri-docs`.
The `script` family disables all script plugins. Removing the plugin folder
and reloading uninstalls it; saved Pomodoro state stays in the data directory.

Read [Writing plugins](plugins.md#external-plugins) to create another tool.
Each package contains a manifest, source and README. Run
`npm run test:extensions` for behavior tests and
`cargo test -p sevak-plugins gallery` to verify package checksums and contents.
See the [gallery publishing guide](https://github.com/ninad-k/Sevak/tree/main/gallery)
before updating a package.
