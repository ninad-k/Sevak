# Extensions for Sevak

Add a focused tool without rebuilding the application. This productivity pack uses
Sevak's script-plugin protocol; the Gallery installs each tool individually.
For native extensions and the unified manager, see [Extensions](features/extensions.md).
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

Released builds use the catalog pinned to their release, so new packages appear
with the next release. To try packages from this checkout now, copy a folder from [`examples/plugins`](https://github.com/ninad-k/Sevak/tree/main/examples/plugins)
to your [plugins folder](features/script-plugins.md#install-a-plugin), then
choose **Reload index**. Keep the entire folder together. No npm install is
needed inside an extension: these tools use only Node's standard library.

## The productivity pack

| Extension | Try | What it does |
|---|---|---|
| **Color tools** | `colors #f5b52c` | Copies HEX, RGB or HSL |
| **Color tools** | `colors #fff on #111` | Shows text contrast ratings or copies the report |
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

## The developer and everyday utilities pack

Ten more offline script plugins, each a single Python 3 file using only the
standard library. They never touch the network, run no program and write no
file. Install **Python 3.8+** on your command path first; every keyword below
needs a space after it, and each shows example rows when you type it alone.

| Extension | Try | What it does |
|---|---|---|
| **JWT decoder** | `jwt eyJhbGci...` | Header, payload and `exp`/`iat`/`nbf` in plain words. Decodes only: the signature is **not** verified |
| **Number base converter** | `base 0xff`, `base ff 16` | Binary, octal, decimal, hex, base 32 and base 36 |
| **Cron explainer** | `cron 30 9 * * mon-fri` | Explains the expression and lists the next five runs |
| **Regex tester** | `rx (\d+)-(\d+) :: call 555-1234` | Every match and capture group (Python `re` syntax) |
| **Unicode lookup** | `uni U+1F600`, `uni right arrow` | Name, UTF-8 bytes, HTML entity and escapes; searches names |
| **HTTP status codes** | `http 404`, `http redirect` | What a status code or a group of codes means |
| **Port reference** | `port 5432`, `port ssh` | What a TCP/UDP port is used for; common dev-server ports |
| **chmod calculator** | `chmod 755`, `chmod rwxr-xr-x` | Octal and symbolic permissions, setuid/setgid/sticky |
| **Date calculator** | `days 2026-12-25`, `days +90` | Days until or between dates, and date plus or minus days |
| **Slugify and text stats** | `slug Hello World!` | URL slug, safe file name, word and reading-time counts |

Notes on behavior: the cron explainer uses this computer's clock and cron's
rule that day-of-month and weekday match when *either* does; the regex tester
caps the text at 2,000 characters so a pathological pattern cannot hang the
launcher; the date calculator counts weekdays without knowing any holidays.
Enter copies the row you picked.

### Research basis

These were chosen by looking at what people install and recommend in other
launchers' stores and picking developer and everyday utilities that Sevak does
not already have (it already covers apps, calculator, units and currency,
clipboard, snippets, emoji, windows, passwords, ids, colors, hashes, lorem
ipsum, case conversion and the like), that work fully offline and that fit the
[gallery rules](https://github.com/ninad-k/Sevak/tree/main/gallery#what-the-gallery-accepts):

- **Raycast Store**: JWT Decoder, Regex Tester, Cron Description, Unicode
  Symbols Search and the many Base64, JSON and number-base developer utilities
  are among its popular developer extensions.
- **PowerToys Run third-party plugins**: UnicodeInput and PowerHexInspector
  (character lookup, number bases) are on Microsoft's list of community plugins.
- **Flow Launcher plugin store**: its popular plugins include Colors, a
  unit-aware calculator and currency conversion, which shows demand for small
  offline converters; Sevak already has those, so this pack adds the developer
  references around them.
- **Alfred Gallery and workflows**: has community equivalents of the same
  categories (encoders, lookups, date math); not individually checked.

Time-zone conversion was considered and left out: Python's `zoneinfo` needs a
time-zone database that Windows does not ship, so it would not be cross-platform
without a bundled data file. Two modules were added to the allow list in
`crates/sevak-plugins/tests/gallery_content.rs` for this pack: `base64` (decoding
a JWT's segments) and `unicodedata` (character names, accent folding). Both are
pure in-memory data transforms.

## Native tools (compiled Rust)

Three tools are native extensions: compiled programs, so they need no Node.js,
Python or other runtime. They appear in **Settings → Extensions** (and `ext` in
the launcher), and Sevak asks before the first run, showing the program's
SHA-256. All three work offline and declare **no permissions**; their source is
in [`examples/native`](https://github.com/ninad-k/Sevak/tree/main/examples/native).
A native extension is not sandboxed (see the
[security model](writing-extensions-in-rust.md#security-model)); reading the
short source is the way to check these.

| Extension | Try | What it does |
|---|---|---|
| **JWT decoder** | `jwt eyJhbGciOi...` | Shows whether a token is expired, its algorithm, header, claims (times as dates) and signature. Enter copies a value. The signature is **not** verified |
| **Cron explainer** | `cron */15 9-17 * * 1-5` | Says what a cron expression means and lists the next five run times (UTC, or add an offset such as `+05:30`) |
| **Regex tester** | `regex (\d+)-(\d+) => call 555-1234` | Lists every match with its position and capture groups. Rust `regex` syntax: no lookaround or backreferences, but no pattern can hang |

Each package is built per platform by
[CI](writing-extensions-in-rust.md#building-and-publishing-native-extensions-with-ci).
A computer with no build for its platform sees "no build for your platform".
The online catalog of a released Sevak lists what was committed at that
release's tag; the current list is in the
[gallery README](https://github.com/ninad-k/Sevak/tree/main/gallery).

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
`script:color-tools`, `script:pomodoro`, `script:translate` and `script:tauri-docs`
(and `script:jwt`, `script:cron` and `script:regex` for the native tools).
The `script` family disables all script plugins. Removing the plugin folder
and reloading uninstalls it; saved Pomodoro state stays in the data directory.

Read [Writing plugins](plugins.md#external-plugins) to create another tool.
Each package contains a manifest, source and README. Run
`npm run test:extensions` for behavior tests and
`cargo test -p sevak-plugins gallery` to verify package checksums and contents.
See the [gallery publishing guide](https://github.com/ninad-k/Sevak/tree/main/gallery)
before updating a package.
