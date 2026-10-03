# Sevak plugins

[← Help center](README.md) · [Development](development.md) · [Configuration](configuration.md)

Everything Sevak shows in its result list comes from a plugin: installed apps,
the calculator, web search, files. This document explains how a query flows
through them, how to write and register a built-in plugin, and how to add
plugins without rebuilding Sevak by dropping in a script.

- [Architecture](#architecture)
- [Writing a built-in plugin](#writing-a-built-in-plugin)
- [Universal Actions](#universal-actions): offering actions for what the user selected in another app
- [Contacts, 1Password and dictionary](#contacts-1password-and-dictionary): plugins with two keywords, an external tool, OS data sources and a bundled dictionary
- [External plugins](#external-plugins): script plugins in Python, PowerShell, Node or anything else, including Alfred Script Filter scripts

Code map:

| Concern | Where |
|---|---|
| `Plugin` trait, `PluginError` | `crates/sevak-core/src/plugin.rs` |
| `ResultItem`, `Action`, `IconSource`, `score` constants | `crates/sevak-core/src/model.rs` |
| Routing, ranking, execution | `crates/sevak-core/src/engine.rs` |
| Usage statistics | `crates/sevak-core/src/usage.rs` |
| Fuzzy matcher | `crates/sevak-core/src/fuzzy.rs` |
| Config (`[plugins] disabled`) | `crates/sevak-core/src/config.rs` |
| Built-in plugins, registry | `crates/sevak-plugins/src/` |
| Typed-path browsing (files plugin) | `crates/sevak-plugins/src/path_browse.rs` |
| Script plugins (external) | `crates/sevak-plugins/src/script/` |
| Standard action execution | `crates/sevak-plugins/src/actions.rs` |
| Contacts, 1Password, dictionary | `crates/sevak-plugins/src/{contacts,onepassword,dictionary}/`, `crates/sevak-platform/src/{contacts,deep_link,dictionary}.rs` |
| Universal Actions (selection) | `crates/sevak-core/src/selection.rs`, `crates/sevak-plugins/src/selection/`, `crates/sevak-platform/src/capture.rs`, `src-tauri/src/selection.rs` |
| OS access (`PlatformProvider`) | `crates/sevak-platform/src/provider.rs` |

## Architecture

```
 keystroke
    |
    v
 SearchEngine::query(input)            routing + ranking     (sevak-core)
    |  keyword route, or all global plugins
    v
 Plugin::query(text) -> Vec<ResultItem>                      (sevak-plugins)
    |  merge, boost by usage, dedupe, sort, truncate
    v
 UI shows results; user presses Enter on one
    |
    v
 SearchEngine::execute(item, query)
    |  finds the owning plugin by item.plugin_id
    v
 Plugin::execute(item)  ->  execute_action(platform, &item.action)
    |
    v
 PlatformProvider::{launch, open_path, open_url, set_clipboard_text,
                    paste_text, reveal_path, launch_as_admin}          (sevak-platform)
```

(`SearchEngine::execute_secondary(item, index, query)` is the same path for a
secondary action.)

- **Engine** (`SearchEngine`) owns the plugins (`Vec<Arc<dyn Plugin>>`) and the
  usage store. It never interprets results; it only routes and ranks them.
- **Plugins** answer queries from in-memory data and describe what should
  happen as an `Action`. They do not touch the OS directly. Script plugins (see
  [External plugins](#external-plugins)) are the exception to "in-memory": they ask
  a child process, so they answer within a small time budget and deliver late
  answers through `Plugin::attach_notifier`.
- **Actions** (`Action`) are a closed vocabulary: `Launch`, `OpenPath`,
  `OpenUrl`, `CopyText`, `PasteText` (copy, return to the app that was focused
  before Sevak opened, press Ctrl+V / Cmd+V), `RevealPath` (show in the file
  manager), `RunAsAdmin` (elevated launch; Windows) and `Custom` (plugin-defined payload
  that only the owning plugin understands). `execute_action` in
  `sevak-plugins` maps the standard ones onto the platform provider; `Custom`
  yields `PluginError::Unsupported` there, so a plugin using it must handle it
  itself.
- **Secondary actions.** Besides the `action` Enter runs, a result can carry
  more, added with `ResultItem::with_secondary(label, modifier, action)`. The
  `modifier` (`Modifier::Ctrl`/`Shift`/`Alt`, or `None`) is the key held with
  Enter to run it straight from the list; the action panel (Right arrow or
  `Ctrl+K`) lists all of them. Use at most one action per modifier. Plugins need
  no extra code to support them: the engine's `execute_secondary` hands the
  plugin its item with the chosen action swapped in as `item.action`, so
  `execute_action(.., &item.action)` just works (a `Custom` secondary action
  arrives as `Custom` in `execute`). Usage is recorded for the item either way.
  Offer only what can work here: for example the apps plugin adds
  `RunAsAdmin` only when `PlatformProvider::can_run_as_admin()` is true.
- **Copy text.** `ResultItem::copy_text()` is what `Ctrl+C` copies: the text
  of a `CopyText` or `PasteText` action, the URL of an `OpenUrl`, the path of
  `OpenPath`, `RevealPath` or a launch target. Nothing is copied for `Custom`
  actions or packaged apps. The copy goes through the plugin
  (`SearchEngine::copy` runs `execute` with a `CopyText` of that text), so a
  plugin whose rows carry a template, like snippets, copies the expanded text.
- **Platform provider** (`PlatformProvider`) is the only OS-specific layer
  (Windows Start Menu / packaged apps, Linux `.desktop` entries). It also
  gatekeeps URLs: `open_url` accepts only `http://`, `https://` and `mailto:`.
  Other OS entry points are narrow, closed vocabularies instead of strings
  (`DeepLink` for `tel:` and app links, [below](#the-deeplink-allow-list)):
  `run_system_command(SystemCommand)` and `open_settings_page(SettingsPage)`
  (with `supported_*` methods that report what works on this machine). They
  back the `system` plugin, so settings URIs such as `ms-settings:` never pass
  through `open_url`.

### Routing

1. Whitespace-only input yields nothing.
2. **Keyword route.** If the input is `<kw><whitespace><rest>` and at least
   one plugin has `keyword() == kw` (case-insensitive), only those plugins are
   queried, with `rest` (leading whitespace trimmed; it may be empty, e.g.
   `"g "`). No fallback runs in this mode. The keyword needs a following space:
   a bare `g` is an ordinary global query.
   **Symbol keywords** are the exception: a keyword made only of punctuation or
   symbols (such as the shell plugin's `>`) needs no space, so `>ls` and `> ls`
   both route to the plugin with `ls`, and a bare `>` routes with an empty
   rest. When several symbol keywords match (`>` and `>>`), the longest wins.
   Choose symbol keywords sparingly: the plugin claims every input that starts
   with the symbol.
3. **Global route.** Otherwise every plugin with `global() == true` is queried
   with the trimmed input. The default is `global() == keyword().is_none()`, so
   plugins with a keyword are keyword-only unless they opt in (the files and
   bookmarks plugins do when `[files] global` / `[bookmarks] global` is true).

### Ranking

- Fuzzy matches use nucleo's scale: roughly 16-20 points per matched
  character plus bonuses for word starts and consecutive runs.
- Non-fuzzy plugins use the constants in `sevak_core::model::score`:
  `EXACT_ANSWER` = 10 000 (calculator), `KEYWORD` = 5 000 (keyword-triggered
  rows), `FALLBACK` = 0.
- **Secondary weight.** A keyword plugin that answers a *global* query has its
  scores multiplied by `GLOBAL_SECONDARY_WEIGHT` = 0.5 (only for scores below
  `KEYWORD`), so installed apps stay above loosely matching files.
- **Usage boost.** For scores below `KEYWORD`, the engine adds a boost from
  usage statistics: `25 * ln(1 + launches)`, plus a recency bonus of up to 40
  that halves every 72 hours, plus a flat 80 when the typed text is a prefix of
  a query previously used to pick that item. Results at or above `KEYWORD` are
  never boosted, so exact answers and keyword results keep the order the
  plugin gave them.
- Results are deduplicated by `id` (best score wins), sorted by score
  descending (ties: title case-insensitively, then id) and truncated to
  `[search] max_results`.
- **Fallback.** If a *global* query produced nothing, the configured fallback
  plugins (`[search] fallback_web_search`, a keyword or a list of keywords,
  default `g`) are queried with the full input and their results are shown
  instead, in the configured order.
- **Autocomplete (Tab).** A result may carry `autocomplete`
  (`ResultItem::with_autocomplete`), the text the input becomes when the user
  presses Tab on it. It is relative to the plugin's own input: for a keyword
  route (`f ~/Doc`) the engine puts the typed keyword back in front, so a plugin
  never has to know it. `ResultItem::new` is unchanged; the field defaults to
  `None`.
- **Keyword hints.** A global query that is exactly a plugin's keyword (`g`)
  also shows that plugin's `Plugin::keyword_row()` (default `None`) after the
  real matches. Its `autocomplete` is the full replacement input (`g `). Web
  search uses it; hints never replace the fallback.
- **Query history.** The engine records every executed query (the whole input,
  keyword included) in the usage file for Up/Down recall, unless
  `[search] query_history = false`.

The engine logs a warning for any query slower than 16 ms, naming the slowest
plugin. Plugin panics are **not** caught; the release profile aborts the
process, so plugins must never panic.

### Ids and usage statistics

- A plugin's `id()` is unique and **stable forever**. It appears in result ids,
  usage statistics and `[plugins] disabled`.
- A result id is `<plugin id>:<key>` (`ResultItem::new` builds it). Usage is
  recorded per result id and persisted in `<data dir>/sevak/usage.json`. Keys
  should therefore identify *what the thing is* (`firefox.desktop`), not
  volatile data (a random UUID, a timestamp), or the history fills with ids that
  never come back.
- Plugins with several instances use `family:instance` ids (`web:g`, `web:yt`).
- The exception is a plugin whose results *are* user-typed text. The shell
  plugin (`> ls -la`) uses the command text as the key (`shell:ls -la`): the
  command is what the result is, and it is how earlier commands come back.
- **Restoring history.** When the engine is built (startup and "Reload
  index"), it calls `Plugin::restore_history(keys)` on every plugin with the
  keys, most recent first (at most 50), of that plugin's results found in the
  usage statistics. Most plugins ignore it; the shell plugin uses it to offer
  recent commands again. Results run in this session are tracked by the plugin
  itself. Empty keys are never passed.

- **Running a result by id.** A `[[hotkey]] run = "<id>"` entry (and
  `sevak --run <id>`) executes a result without a query. The engine asks the
  plugin that owns the id prefix to rebuild the result through
  `Plugin::resolve(id)`. The default returns `None` ("not resolvable"); implement
  it when your keys name something that can be found again without a query, as
  `apps` (looks the key up in its index), `files` (checks the path exists),
  `bookmarks` (the URL hash in its index), `system` (a command or settings page
  that is available here), `snippets` (the snippet's key) and `shell` (the key
  is the command) do. Plugins that only exist as answers to a typed query
  (calculator, web search, uuid) leave it alone; so does the clipboard history,
  whose entries come and go.

### Registry and enabling/disabling

`sevak_plugins::PluginRegistry` holds `PluginDescriptor`s, one per plugin
*family*:

| Family id | Instances | Notes |
|---|---|---|
| `apps` | `apps` | |
| `calculator` | `calculator` | also converts units (`units.rs`) and, with `[calculator] currency`, currencies (`currency.rs`) |
| `web` | `web:<keyword>` per `[[web_search]]` engine | |
| `files` | `files` | also browses typed paths ([below](#path-browsing-in-the-files-plugin)) |
| `bookmarks` | `bookmarks` | see [Bookmarks](#bookmarks) |
| `system` | `system` | lock, sleep, restart, settings pages; global |
| `shell` | `shell` | `> command` runs in a terminal; see below |
| `clipboard` | `clipboard` | `cb`, clipboard history; opt-in through `[clipboard] enabled` (see below) |
| `snippets` | `snippets` | `s`, `[[snippet]]` entries pasted with placeholders expanded |
| `selection` | `selection` | Universal Actions for the text, URL or files selected in another app; no keyword ([below](#universal-actions)) |
| `contacts` | `contacts`, `contacts:at` | `c` and `@`, opt-in through `[contacts] enabled` ([below](#contacts-1password-and-dictionary)) |
| `1password` | `1password` | `1p`, opt-in through `[onepassword] enabled` |
| `dict` | `dict`, `dict:spell` | `define` and `spell`, offline |
| `uuid` | `uuid` | example plugin, keyword-only |

- `PluginRegistry::builtin()` is the stock set; `register(descriptor)` adds (or
  replaces, by id) a family. This is how a compiled-in third-party plugin joins.
- `instantiate(&config, platform)` builds the enabled plugins. Disabled families
  are not constructed at all. It logs at `info` which plugins were loaded and
  which were skipped. `builtin_plugins(&config, platform)` is shorthand for
  `PluginRegistry::builtin().instantiate(..)`.
- `catalog(&config, platform)` returns a `PluginInfo { id, name, description,
  keyword, enabled }` for **every** instance, disabled ones included (it is
  `Serialize`, snake_case). The settings UI uses it for its toggles.

To disable plugins, list ids in `config.toml` (`<config dir>/sevak/config.toml`),
then choose "Reload index" in the tray (or restart):

```toml
[plugins]
# a family id disables all its instances; an instance id disables one
disabled = ["web:yt", "uuid"]
```

### Path browsing in the files plugin

Input that starts like a path (`~/`, `~\` on Windows, `/`, `C:\` or `C:/` on
Windows, `\\server\share\` on Windows) is not searched in the index. The plugin
lists the one directory named by everything up to the last separator and filters
its entries by the text after it. Folders sort above files; hidden (dot) entries
appear only with `[files] include_hidden` or when the typed segment starts with
a dot. Each row's `autocomplete` is the typed directory plus the entry name,
with the typed separator appended for folders, which is what Tab inserts.

Rows score above `score::KEYWORD`, so they are neither halved as secondary
global results nor reordered by usage. The listing never recurses, is capped at
5 000 entries, and runs on a helper thread the query waits on for at most
150 ms, so a stalled network share yields no rows instead of a stalled UI (at
most four such listings may be outstanding at once, and the last listing is
reused for 1.5 s while the user types the filter). A UNC path needs both a
server and a share before anything is read.

### The shell plugin

`> some command` (`crates/sevak-plugins/src/shell.rs`) shows "Run `some command`
in terminal"; on Enter it calls `PlatformProvider::run_in_terminal(command,
&config.shell)`. It is the reference for a plugin that

- uses a symbol keyword (`>`) and `global() == false`;
- returns `Action::Custom` and handles it itself (the command text is the
  payload; the platform call is not one of the standard actions);
- keeps rows at or above `score::KEYWORD` so the engine keeps its order: the
  typed command first, then recents matching the typed prefix, newest first;
  with nothing typed, recents followed by "Open terminal";
- implements `restore_history` (above).

Nothing runs while typing; only Enter on a row executes. The terminal and
argument construction lives in `crates/sevak-platform/src/terminal.rs`: pure
`plan_windows` / `plan_macos` / `plan_linux` functions build the program and
arguments (and are unit-tested on every OS without launching anything), and
`run_in_terminal` spawns the result through `process.rs`. PowerShell receives
the command as `-EncodedCommand`, Windows Terminal gets `;` escaped as `\;`,
`cmd` gets `/S /K "..."`, POSIX shells get `-c`, and macOS gets an escaped
AppleScript string, so the command text is never re-parsed by Sevak.

### Bookmarks

The `bookmarks` plugin (`crates/sevak-plugins/src/bookmarks/`) searches the
bookmarks of the web browsers on the machine, read-only and offline. Typing
`b <text>` searches only bookmarks; with `[bookmarks] global = true` they also
appear (down-weighted like files) for plain queries.

- **Where the browsers are** is the platform layer's job:
  `PlatformProvider::browser_roots()` (`sevak-platform/src/browsers.rs`) lists
  the user-data folders that exist, per OS (Windows `%LOCALAPPDATA%` /
  `%APPDATA%`, macOS `~/Library/Application Support`, Linux `~/.config`,
  `~/.mozilla` plus Flatpak and Snap copies).
- **Chromium family** (Chrome, Edge, Brave, Vivaldi, Chromium, Opera, Opera GX):
  the `Bookmarks` JSON file of every profile folder.
- **Firefox family** (Firefox, LibreWolf, Zen): `places.sqlite` of every profile
  in `profiles.ini`. Firefox keeps the database locked, so Sevak copies it and
  its `-wal` file into a private temporary folder, reads the copy with a
  bundled SQLite and deletes it again. The browser's own files are never
  written. Tag entries are skipped.
- `[bookmarks] browsers = []` means every browser found; list ids (`"chrome"`,
  `"edge"`, `"brave"`, `"vivaldi"`, `"chromium"`, `"opera"`, `"opera-gx"`,
  `"firefox"`, `"librewolf"`, `"zen"`) to restrict it.
- **Indexing.** `refresh` runs at startup, on "Reload index" and every ten
  minutes, on a background thread. A source file is re-read only when its
  modification time or size changed (Firefox: the database or its `-wal`), so
  an idle refresh is a handful of `stat` calls. A file that cannot be read
  keeps its previous contents. Queries use an in-memory snapshot.
- **Results.** Fuzzy match on the title, falling back to title + URL (so the
  domain and path match too), with bonuses for a title or domain that starts
  with or contains the input. Enter opens the URL (`Action::OpenUrl`). Only
  `http(s)` bookmarks are indexed; `javascript:`, `chrome:`, `file:` and the like
  are skipped. The subtitle is `folder path · domain · browsers`.
- **Ids.** The result key is a hash (FNV-1a, 64 bit, hex) of the URL, so usage
  statistics follow the page across browsers, profiles and restarts. Identical
  URLs from different browsers or profiles are merged into one result that lists
  the browsers.

## Writing a built-in plugin

The worked example is `crates/sevak-plugins/src/example_uuid.rs`: type `uuid `
and get a fresh random UUID; `uuid 5` gives five; `uuid upper` uppercases;
Enter copies. Its source comments spell out every trait method's contract; read
it alongside this section.

### 1. Create the file

`crates/sevak-plugins/src/my_plugin.rs`, declared in `lib.rs`:

```rust
pub mod my_plugin;
pub use my_plugin::MyPlugin;
```

Implement `sevak_core::Plugin`. Required methods: `id`, `name`, `keyword`,
`query`, `execute`. Defaulted: `description` (empty), `global`, `restore_history`,
`refresh`.

```rust
pub struct UuidPlugin { platform: Arc<dyn PlatformProvider> }

impl Plugin for UuidPlugin {
    fn id(&self) -> &str { "uuid" }                  // stable forever
    fn name(&self) -> &str { "UUID generator" }
    fn description(&self) -> &str { "Type `uuid` to generate random UUIDs; Enter copies one." }
    fn keyword(&self) -> Option<&str> { Some("uuid") }
    fn global(&self) -> bool { false }               // only answers "uuid ..."
    fn query(&self, input: &str) -> Vec<ResultItem> { /* see the file */ }
    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        execute_action(self.platform.as_ref(), &item.action)
    }
}
```

Rules of thumb (all spelled out in the example):

- `query` runs on a worker thread **for every keystroke**: keep it well under a
  millisecond, no I/O, no blocking, never panic. Load slow data in `refresh`
  (called on a background thread at startup, on "Reload index" and
  periodically) into an in-memory index, and swap it in under a short lock.
- Put everything `execute` needs into the result's `Action`; do not rely on
  remembering the last `query`.
- Delegate standard actions to `execute_action`. Use `Action::Custom` only when
  none fits, and handle it in your own `execute`.
- Add `with_secondary(..)` actions where they are natural (a path to reveal or
  copy, a URL to copy). Secondary actions share the primary's `execute`.
- Icons are `IconSource::builtin(name)` (a UI glyph: `app`, `calculator`,
  `web`, `file`, `folder`, `copy`, `terminal`, `plugin`, `lock`, `sleep`,
  `restart`, `power`, `logout`, `trash`, `settings`), or `File` / `Shell` for real
  images.
- For an action that cannot be undone, override `Plugin::confirmation(item)` to
  return the question to ask. The shell shows it in a native dialog before
  `execute` runs and skips the action if the user declines. The `system`
  plugin does this for restart, shut down, log out and emptying the trash.
- Pick scores deliberately: fuzzy score for fuzzy matches, `score::KEYWORD` for
  rows the user asked for by keyword, `score::EXACT_ANSWER` for answers.

### 2. Register a descriptor

Add the family to `PluginRegistry::builtin()` in `registry.rs`:

```rust
registry.register(PluginDescriptor::new(
    "uuid",                                   // family id (config + catalog)
    "UUID generator",
    "Type `uuid ` to generate random UUIDs; Enter copies one.",
    |_config, platform| vec![Arc::new(UuidPlugin::new(platform.clone()))],
));
```

The factory is a plain `fn(&Config, &Arc<dyn PlatformProvider>) -> Vec<Arc<dyn
Plugin>>`. Return one instance, several (one per configured item, like web
search) or none. Keep it cheap: no scanning; leave that to `refresh`.

If your plugin needs configuration, add a section to `Config` in
`sevak-core/src/config.rs` (with `#[serde(default)]`) and read it in the
factory. An embedding application can also build its own registry:
`let mut r = PluginRegistry::builtin(); r.register(...);
r.instantiate(&config, platform)`.

### 3. Enable and disable

New plugins are enabled by default. Users switch them off with
`[plugins] disabled = ["uuid"]`. `catalog()` will list the plugin
automatically for the settings UI.

### 4. Test it

`crates/sevak-plugins/src/test_util.rs` provides `MockPlatform`, a
`PlatformProvider` that records what was launched, opened or copied instead of
doing it. A typical test:

```rust
#[test]
fn execute_copies_to_the_clipboard() {
    let platform = MockPlatform::empty();
    let plugin = UuidPlugin::new(platform.clone());
    let item = plugin.query("").remove(0);
    plugin.execute(&item).unwrap();
    assert_eq!(*platform.clipboard.lock().unwrap(), vec![item.title]);
}
```

Also test: metadata (`id`, `keyword`, `global`), empty and malformed input,
score and action of each result, and that the registry lists and instantiates
it (see `registry.rs` tests). Verify with:

```
cargo test -p sevak-core -p sevak-plugins
cargo clippy -p sevak-core -p sevak-plugins --all-targets -- -D warnings
```

## Pasting, clipboard history and snippets

Two built-in plugins go beyond "describe an action": `clipboard` (`cb`) and
`snippets` (`s`). They show how to use `Action::PasteText` and, for the first,
a background thread. User documentation is in the README.

### Pasting into the previous app

`Action::PasteText { text, restore_clipboard }` is executed by
`PlatformProvider::paste_text`, which:

1. copies `text` (asking the OS to keep it out of its own clipboard history);
2. brings back the window remembered by `remember_foreground_app`, which the
   shell calls just before it shows Sevak's window, while the user's app still
   has focus;
3. synthesizes Ctrl+V / Cmd+V;
4. optionally puts the previous clipboard text back.

The shell hides Sevak's window *before* executing a `PasteText` (as it does for
`Launch`/`OpenPath`/`OpenUrl`). Implementations: `windows/paste.rs`
(`GetForegroundWindow`, `SetForegroundWindow`, `SendInput`), `macos/paste.rs`
(`NSWorkspace`, `CGEvent`; needs the Accessibility permission) and
`linux/paste.rs` (`_NET_ACTIVE_WINDOW` and XTest through `x11rb`; X11 only).
`paste.rs` holds the shared order of operations.

Not every system can paste, so a plugin should ask `paste_support()` while
building rows: for `PasteSupport::CopyOnly(reason)` return `Action::CopyText`
and say "Copies to clipboard" plus the reason in the subtitle, so the row never
promises more than Enter does. `paste_text` itself also degrades to copying
(`PasteOutcome::CopiedOnly`) if the situation changed since the query.

### `clipboard`: a plugin with a thread

The constructor and `query` stay cheap; the recording thread is started by
`refresh`, never by the constructor, because the settings window also builds
plugins just to list them (`catalog`). Plugins are rebuilt on every config
reload, so the history and its thread live in a `Shared` that a process-wide
table hands to the new plugin while the old one is alive; the thread holds a
`Weak` and ends when the last plugin is dropped. The platform supplies
`clipboard_sequence()` (change counter), `read_clipboard()` (text plus the
"secret" flag) and `foreground_app()` (source app, matched against
`ignore_apps`). Text Sevak wrote itself is recognised through
`sevak_platform::clipboard::take_own_write` and skipped.

### `snippets`: expanding at execution time

A snippet's row carries the *template* in its `PasteText` action; `execute`
expands the placeholders (`{time}`, `{clipboard}`, ...) at the moment of
pasting, looking the snippet up by its result id so a config reload between
query and Enter uses the new text. Expansion is the pure function
`snippets::expand`, tested without a platform.

## Universal Actions

The `selection` plugin (`crates/sevak-plugins/src/selection/`) answers the
Universal Actions hotkey. It has no keyword and never answers typed queries; it
implements two optional methods of `Plugin` instead:

```rust
/// Actions for what the user selected in another app, in listing order.
fn selection_actions(&self, selection: &Selection) -> Vec<ResultItem> { Vec::new() }

/// False keeps this plugin's results out of usage.json and the search history.
fn tracks_usage(&self) -> bool { true }
```

The flow is: the shell (`src-tauri/src/selection.rs`) calls
`PlatformProvider::capture_selection`, builds a `Selection` (text *or* files,
never serialized, `Debug` leaves the content out), asks
`SearchEngine::selection_actions` to collect every plugin's rows, stores them
under a search ticket and shows them in the action panel. Picking one goes
through the ordinary `execute` command, so a row is a normal `ResultItem` whose
`Action` the plugin's `execute` understands: `OpenUrl`, `OpenPath`,
`RevealPath`, `CopyText`, `PasteText`, `RunAsAdmin`, or a `Custom` payload for
"open in terminal". Two `Custom` payloads are carried out by the launcher window
itself rather than the shell (Large Type and "send to Sevak"); see
`selection::ui_request`.

Rules for a plugin that offers selection actions:

- Build every row from the selection when `selection_actions` is called: a
  transform carries its finished result (`PasteText { text: "HELLO" }`), so
  `execute` needs no copy of the selection.
- Keep the selection out of result ids (`selection:search:g`, not
  `selection:search:rust traits`): ids are stable keys and the selection is
  private. Return `false` from `tracks_usage` so running a row records nothing.
- Prefer `PasteText` with a `Ctrl` secondary `CopyText`, and check
  `paste_support()` first: the selection is still highlighted in the app, so the
  paste replaces it. Where pasting is unavailable use `CopyText`.
- Do not log the selection or put it in an error message.

Capturing lives in `sevak-platform` (`capture.rs` holds the order of operations
and its fake-driven tests; `windows/`, `macos/` and `linux/capture.rs` hold the
key presses and modifier handling). `PlatformProvider::capture_selection`
returns `Selected`, `Nothing` or `Unavailable(reason)`.

**Script plugins and Universal Actions: design only.** Script plugins do not
receive the selection yet. The plan, so the manifest can stay stable: a script
plugin would declare `accepts = ["text", "url", "file"]` in `plugin.toml`; when a
selection of one of those kinds is captured, the host would send the persistent
script a request `{"selection": {"kind": "text", "text": "..."}}` (or `"url"`,
`"files": [...]`) and show the Alfred-style items it answers with after the
built-in actions, running them with the same `Action` mapping as query results.
The open questions are the wait (scripts are normally answered asynchronously,
but this panel is built once, so a deadline of about 300 ms would apply) and
telling the user, in the approval dialog, that the plugin will see their
selection. Until then a script plugin cannot read it, and a selection is never
passed to any script.

## Contacts, 1Password and dictionary

Three built-in plugins (`crates/sevak-plugins/src/contacts/`, `onepassword/`,
`dictionary/`) show patterns the simpler ones do not. User documentation is in
[usage.md](usage.md#contacts).

**Several keywords, one data set.** `Plugin::keyword` is a single string, so a
plugin with two keywords is two instances sharing an `Arc` of the data:
`contacts` (`c`, configurable) and `contacts:at` (`@`), and `dict` (`define`) and
`dict:spell` (`spell`). The factory returns both; instance ids use the
`family:instance` form, so `[plugins] disabled = ["contacts"]` turns both off.
Only the family instance loads data in `refresh`.

**Opt-in plugins** are built even when disabled in the config (so the settings
window can list them) but hold no data and answer every query with one row that
copies the line to add to `config.toml`; the clipboard plugin does the same.

**Usage tracking.** All three return `false` from `tracks_usage`, so names,
login titles and looked-up words never reach `usage.json` or the search history.

**Large Type.** `ResultItem::with_large_text` sets what `Ctrl+L` shows instead of
the title (a contact's phone number).

### Platform pieces

| Need | Where |
|---|---|
| Address book | `PlatformProvider::{contacts_access, request_contacts_access, system_contacts}`. macOS: `macos/contacts.rs` (`CNContactStore`, behind `NSContactsUsageDescription` in `src-tauri/Info.plist`). Windows: `windows/people.rs` (`Windows.ApplicationModel.Contacts`, read-only). Linux: `evolution_address_books()` returns Evolution's `contacts.db` paths and the plugin reads their vCards (`contacts/sources.rs`) from a private copy, like Firefox's bookmarks. |
| vCard 2.1, 3.0 and 4.0 | `sevak_platform::contacts::parse_vcards` (unit tested; no crate) |
| macOS permission | Never asked at startup or while typing. `contacts_access()` only reads the status; a row's Enter calls `request_contacts_access()`. |
| Non-web links | `sevak_platform::DeepLink`, opened with `PlatformProvider::open_link`. |
| Definitions | `PlatformProvider::system_definition` (macOS Dictionary Services, `macos/dictionary.rs`) |
| Spelling | `PlatformProvider::system_spelling` (Windows `ISpellChecker`, `windows/spell.rs`, on a worker thread with a 120 ms answer limit) |

### The `DeepLink` allow-list

`open_url` accepts only `http(s):` and `mailto:` and must stay that way. Three
plugin features need one more scheme each, so instead of loosening `open_url`
there is a closed type, `DeepLink`, that can only be built by constructors that
validate every piece and build the whole URL themselves:

| Constructor | URL | Validation |
|---|---|---|
| `DeepLink::tel(number)` | `tel:+15551234567` | digits, a leading `+`, spaces and `-.()`; 3 to 20 digits |
| `DeepLink::address_book_card(id)` | `addressbook://<id>` | letters, digits, `:`, `-`, `_`; at most 100 characters |
| `DeepLink::onepassword_item(account, vault, item)` | `onepassword://view-item/?a=..&v=..&i=..` | each id letters and digits only, 20 to 40 characters |

Plugins carry the pieces in an `Action::Custom` payload (`call:+44 20 ...`) and
build the link in `execute`, so a tampered payload is refused there. To add a
link, add a constructor with tests next to the others; do not add a scheme to
`open_url`.

### 1Password and `op`

`onepassword/op.rs` holds the parsing (tested against JSON fixtures) and the
`OpRunner` trait; `CliRunner` is the real implementation, and tests use a fake.
Rules the plugin keeps:

- Only `op item list --categories Login --format json` and `op account list
  --format json` are ever run. `Login` has no field for a secret, and the parser
  reads only `id`, `title`, `vault`, `urls` and `additional_information`.
- `op` is started only from `query` (the keyword route), on a background thread,
  when the in-memory list is missing or older than `cache_minutes`. `refresh` does
  nothing, so startup and "Reload index" never trigger a biometric prompt. A
  failure is remembered and not retried until the user presses Enter on the retry
  row. The plugin tells the launcher the answer arrived through the notifier.
- An item's "open in 1Password" link needs the account id (`op account list`); with
  several accounts and no `[onepassword] account` the link is left out.

### The dictionary

`dictionary/lexicon.rs` is a small engine over `data/wordnet-en.z`: a sorted text
index with binary search, WordNet-style inflection rules checked against the part
of speech (`-ed` needs a verb) and consonant doubling (`stopped`, not `stoped`),
and spelling suggestions by Damerau-Levenshtein distance, pre-filtered by length
and letter set, ranked by distance, rearranged letters, first letter, frequency.
The data is derived from Princeton WordNet 3.0 by
`python scripts/build-dictionary.py <WordNet-3.0 folder>`; the licence is kept in
the file's header and in `THIRD_PARTY_NOTICES.md`. It adds about 2.7 MB to the
program and 8 MB of memory when unpacked (once per process, in `refresh`).

## External plugins

You can add a keyword plugin without building Sevak: drop a folder with a
`plugin.toml` and a script into the plugins folder and Sevak runs the script to
answer queries. Scripts can be written in anything that reads and writes text
(Python, PowerShell, Node, a shell script, a compiled program), and a one-shot
mode runs many existing **Alfred Script Filter** scripts unchanged.

- [Quick start](#quick-start)
- [The manifest](#the-manifest-plugintoml)
- [Starting the script](#starting-the-script)
- [Persistent plugins: the protocol](#persistent-plugins-the-protocol)
- [One-shot plugins](#one-shot-plugins)
- [Alfred Script Filter scripts](#alfred-script-filter-scripts)
- [Speed: queries never wait for scripts](#speed-queries-never-wait-for-scripts)
- [Lifecycle, crashes and restarts](#lifecycle-crashes-and-restarts)
- [Security](#security)
- [Tips for plugin authors](#tips-for-plugin-authors)
- [Design decisions](#design-decisions)
- [For contributors](#for-contributors)

### Quick start

1. Copy a folder from [`examples/plugins/`](../examples/plugins) into the
   plugins folder, which sits next to `config.toml`:

   | OS | Plugins folder |
   |---|---|
   | Windows | `%APPDATA%\sevak\plugins` |
   | macOS | `~/Library/Application Support/sevak/plugins` |
   | Linux | `~/.config/sevak/plugins` |

2. Choose **Reload index** in the tray menu (or restart Sevak).
3. Sevak shows a dialog naming the plugin and the exact command it will run.
   Choose **Allow**. (Nothing runs before that; see [Security](#security).)
4. Open Sevak and type the plugin's keyword and a space: `hello Ada`.

The examples:

| Folder | Mode | Language | Keyword |
|---|---|---|---|
| `hello-python` | persistent | Python | `hello` |
| `timestamp-powershell` | one-shot | PowerShell | `ts` |
| `case-converter-node` | one-shot, Alfred format | Node.js | `case` |

### The manifest (`plugin.toml`)

Each plugin is a folder `<plugins folder>/<name>/` containing `plugin.toml` and
whatever the script needs. Sevak scans the folder at startup and on **Reload
index**. Folders starting with `.` and folders without a `plugin.toml` are
ignored.

```toml
protocol    = 1                  # required: the protocol version the plugin speaks
keyword     = "hello"            # required: type "hello " to use it (no spaces)
script      = "main.py"          # how to run it: `script` or `command`, not both
# command   = ["python3", "main.py"]

id          = "script:hello"     # default: "script:" + the folder name; must start with "script:"
name        = "Hello"            # default: the folder name
description = "Greets whoever you type."

mode        = "persistent"       # "persistent" (default) or "oneshot"
format      = "sevak"            # one-shot output: "sevak" (default) or "alfred"

timeout_ms        = 50           # how long a query waits for the script (10-1000)
hard_timeout_ms   = 3000         # unanswered this long = hung, restart it (500-60000)
idle_timeout_secs = 300          # persistent: stop after this much inactivity (0 = never)
```

- `id` is **stable forever**, like every plugin id: it is part of result ids and
  usage statistics and is what `[plugins] disabled` lists. Ids must be unique;
  when two folders claim one id the first (by folder name) wins.
- Keywords are matched case-insensitively. A keyword that another plugin (built-in
  or script) also uses queries both and merges their results, so pick one
  that is not taken (`g`, `yt`, `gh`, `f`, `b`, `>`, `cb`, `s`, `c`, `@`, `1p`, `define`, `spell` and `uuid` are by
  default).
- Unknown keys are ignored, so a manifest written for a newer Sevak still loads.
- A `plugin.toml` that is invalid is skipped with a message in the log and shown
  in Settings > Plugins as "Not loaded: ...".
- `global = true` is accepted and ignored: script plugins only answer their
  keyword, because a global script would be asked on every keystroke.
- `protocol` must be a version this Sevak speaks (currently `1`); otherwise the
  plugin is not loaded and says so.

Disable a plugin with its id, or all of them with the family id:

```toml
[plugins]
disabled = ["script:hello"]    # one plugin
# disabled = ["script"]        # every script plugin
```

### Starting the script

Sevak never uses a shell. Pick one of two ways to say what to run:

- `command = ["program", "arg", ...]`: runs exactly this. The program is looked
  up on `PATH` unless it contains a path separator or starts with `.`, in which
  case it is a path relative to the plugin folder (absolute paths are used as
  they are). Relative paths may not contain `..`.
- `script = "file"`: a file in the plugin folder. Sevak picks the interpreter
  from the extension:

  | Extension | Runs as |
  |---|---|
  | `.py` | `py -3 -u file` on Windows (`python -u`, `python3 -u` as fallbacks); `python3 -u file`, then `python -u file` elsewhere |
  | `.ps1` | `pwsh -NoProfile -NonInteractive -File file`; on Windows also `powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File file` |
  | `.js`, `.mjs`, `.cjs` | `node file` |
  | `.sh` | `sh file` (Linux and macOS) |
  | anything else | the file itself: an executable, a `.cmd`/`.bat` on Windows, a script with a `#!` line and the executable bit on Linux and macOS |

  If the interpreter is not installed the plugin loads but cannot start, and the
  log says which programs it looked for.

On Windows, `python` and `python3` can be Microsoft Store stubs that open the
Store instead of running Python, which is why `.py` prefers the `py` launcher.
Use `command = ["C:\\Path\\to\\python.exe", "main.py"]` to pin an interpreter.

The script runs with the **plugin folder as its working directory** and these
environment variables (on top of Sevak's own environment):

| Variable | Value |
|---|---|
| `SEVAK_PLUGIN_ID` | the plugin id |
| `SEVAK_PLUGIN_DIR` | the plugin folder |
| `SEVAK_PLUGIN_DATA` | a folder for the script's own files (created on demand, `<data dir>/plugins/<folder>`) |
| `SEVAK_VERSION` | Sevak's version |
| `alfred_workflow_bundleid`, `alfred_workflow_name`, `alfred_workflow_data`, `alfred_workflow_cache` | only with `format = "alfred"`, the Alfred equivalents |

On Windows the script gets no console window.

### Persistent plugins: the protocol

The default mode. Sevak starts **one long-lived process** the first time the
keyword is used and talks to it with **newline-delimited JSON**: one object per
line on stdin (Sevak to script) and on stdout (script to Sevak), UTF-8. stdout
is for the protocol only; **stderr is captured into Sevak's log** tagged with
the plugin id (the first 200 lines per process).

```jsonc
// Sevak -> script
{"type":"initialize","protocol":1,"sevak_version":"0.1.0","plugin_id":"script:hello"}
{"type":"query","request_id":42,"input":"world"}
{"type":"execute","key":"remember","payload":"world"}
{"type":"shutdown"}

// script -> Sevak
{"type":"ready"}                                   // optional reply to initialize
{"type":"results","request_id":42,"items":[
  {"key":"greeting","title":"Hello, world!","subtitle":"Enter to copy",
   "icon":{"kind":"builtin","name":"copy"},
   "action":{"type":"copy_text","text":"Hello, world!"},
   "score":100}
]}
{"type":"error","request_id":42,"message":"..."}   // logged; the query gets no results
```

Messages Sevak sends never contain raw non-ASCII characters (they are written
as JSON `\uXXXX` escapes), so a script that reads its pipe with the wrong
encoding, as Windows consoles do by default, still sees the right text.

- `initialize` is sent first. Replying `ready` is optional; Sevak does not wait
  for it, and queries may follow immediately.
- `query` carries the text after the keyword (`hello Ada` sends `"Ada"`; `hello `
  sends `""`) and a `request_id`. Reply with a `results` message **using the same
  `request_id`**. Answers to older requests are dropped (see
  [Speed](#speed-queries-never-wait-for-scripts)). Scripts may notice a newer
  `query` already waiting on stdin and skip the work for the old one; Sevak never
  kills a script to cancel a query.
- `execute` is sent when the user picks an item whose action is `custom`. `key`
  is the item's key, `payload` the action's payload. There is no reply.
- `shutdown` asks the script to exit. It then has half a second before it is
  killed. Closing stdin means the same.
- Unknown message types and unknown fields are ignored in both directions.
  A stdout line that is not a JSON message is ignored too (the first few are
  logged as a hint to print debugging output to stderr).

#### Items

| Field | | |
|---|---|---|
| `title` | required | shown in the list (200 characters) |
| `key` | | stable id of this row within the plugin; the result id is `<plugin id>:<key>`. Defaults to the title. Duplicate keys in one answer get `#2`, `#3`... |
| `subtitle` | | 300 characters |
| `icon` | | `{"kind":"builtin","name":"copy"}` (a UI glyph: `app`, `calculator`, `web`, `file`, `folder`, `copy`, `plugin`) or `{"kind":"file","path":"icon.png"}` |
| `action` | | what Enter does; defaults to copying the title |
| `score` | | a number; see below |

At most 50 items per answer are kept. An item that is malformed is skipped, the
rest of the answer is used, and the log says why.

**Keys** follow the same rule as built-in plugins: identify what the thing *is*
(`greeting`, `project-sevak`), not volatile data, because usage statistics are
keyed by result id.

**Actions** use the same JSON as Sevak's `Action` type:

| `action` | Enter does |
|---|---|
| `{"type":"copy_text","text":"..."}` | copies text |
| `{"type":"open_url","url":"https://..."}` | opens a web or `mailto:` link (other schemes are refused by Sevak) |
| `{"type":"open_path","path":"..."}` | opens a file or folder with its default program; a relative path is relative to the plugin folder |
| `{"type":"launch","target":{...}}` | starts an application (the `LaunchTarget` shapes in `model.rs`) |
| `{"type":"custom","payload":"..."}` | sends `execute` back to the script (persistent mode only) |

Sevak itself performs every standard action; a script cannot make Sevak call
anything beyond this list. A `custom` action is the way to do work in the
script, such as saving a note.

**Scores.** Scores are optional. Without one, an item keeps the order the script
gave it (the gap between rows is wide enough that usage statistics do not
reshuffle them). A given score is clamped into `0` to `4999`, just below the
score Sevak reserves for its own keyword rows, so a script can order its rows
but never outrank built-in answers. Give rows close scores if you want Sevak to
reorder them by how often you pick them.

**Icons.** A `file` icon is a path **inside the plugin folder** (`png`, `svg`,
`ico`, `jpg`, `webp`). Absolute paths, `..`, and symlinks that lead outside the
folder are rejected, and the row shows the default glyph instead.

### One-shot plugins

`mode = "oneshot"` runs the script **once per query** with the query appended
as the last argument, and reads one JSON document from stdout:

```json
{"items":[{"key":"iso","title":"2023-11-14T22:13:20Z","action":{"type":"copy_text","text":"2023-11-14T22:13:20Z"}}]}
```

The items are the same as in the persistent protocol (a bare array of items also
works), except that `custom` actions are not available because no process stays
alive. This is the simplest way to write a plugin: no loop, no protocol. The cost
is a process start per query, so Sevak waits 30 ms before starting one (to let a
burst of typing settle) and kills a run as soon as a newer query exists.
`hard_timeout_ms` is the longest a run may take (default 3 s; raise it for slow
starters such as PowerShell). The exit code is ignored; output that is not valid
JSON is logged and shows no results.

### Alfred Script Filter scripts

`mode = "oneshot"` with `format = "alfred"` reads
[Alfred's Script Filter JSON](https://www.alfredapp.com/help/workflows/inputs/script-filter/json/).
Many existing Alfred workflow scripts that are not specific to macOS can be
dropped into a folder with a four-line manifest:

```toml
protocol = 1
keyword  = "gh"
mode     = "oneshot"
format   = "alfred"
command  = ["python3", "script_filter.py"]
```

Alfred runs a Script Filter with the query as argument, as Sevak does in this
mode. Mapping:

| Alfred item field | In Sevak |
|---|---|
| `title`, `subtitle` | the same |
| `uid` | the result key (stable ids for usage learning); without it the title is used |
| `arg` (the first one, if an array) | the action, by shape: an `http://`, `https://` or `mailto:` address opens; an existing absolute path (or `~/...`) opens with its default program; anything else is copied |
| `type` of `file` or `file:skipcheck` | `arg` is a path to open even if it does not exist |
| `icon.path` | an icon, if the file is inside the plugin folder |
| `valid: false`, or no `arg` | the row is shown; Enter copies its title |
| item order | kept (Sevak's scores follow the order) |

Ignored: `autocomplete`, `quicklookurl`, `mods`, `variables`, `text`, `match`,
`icon.type` (`fileicon` and `filetype` ask macOS for a file's icon), and the
top-level `rerun`, `variables` and `cache`. A scheme other than web or mail in
`arg` (`slack://`, `obsidian://`) is copied rather than opened because Sevak
does not open arbitrary schemes. Scripts that call `osascript`, read
`~/Library`, or expect an Alfred preferences file need changes to run elsewhere.
Only Script Filters are supported, not whole `.alfredworkflow` packages with
their other node types.

### Speed: queries never wait for scripts

`Plugin::query` runs on every keystroke and must be quick, but a script is
arbitrary code. A script plugin therefore treats `timeout_ms` (default 50 ms) as
a **budget, not a deadline**:

1. Every query is numbered with a *request id*, the query generation.
2. The plugin sends the request and waits up to the budget for the answer.
3. **In time:** the results are returned like any other plugin's.
4. **Too late:** the list is shown without them, or, if the previous answer
   was for a related text (the user typed one more letter or deleted one), with
   the previous answer so the list does not flash empty. Nothing else waits.
5. When the late answer arrives and it is **still for the newest request**, Sevak
   emits `sevak:results` and the launcher runs its current query again; the plugin
   now finds the answer cached for exactly that text (kept for two seconds) and
   returns it at once, so the selection stays where the user left it.
6. An answer for a request that is no longer the newest is **dropped**: the user
   has already typed past it.

The engine side of this is `Plugin::attach_notifier`: a plugin that can answer
late is handed a callback (`ResultsNotifier`) and calls it with its id. Built-in
plugins never use it.

### Lifecycle, crashes and restarts

- **Lazy start.** A persistent script starts when its keyword is first used, not
  at startup, so unused plugins cost nothing.
- **Idle stop.** After `idle_timeout_secs` without queries (default five
  minutes; `0` disables) Sevak sends `shutdown`, closes stdin and, after half a
  second, kills the process. The next query starts a new one.
- **Crashes and protocol errors.** If the process exits or Sevak has to stop it
  (a line over 1 MiB), the plugin returns nothing and restarts on a later
  query after a growing delay: 0.5 s, 1 s, 2 s, 4 s. A good answer resets the
  count. After **five failures in a row** the plugin stays off until the next
  reload ("Reload index" in the tray), and the log says so.
- **Hung scripts.** If a query stays unanswered for `hard_timeout_ms` (default
  3 s) the process is killed and counts as a failure. A slow but alive script
  that answers other queries in time is not affected.
- **Quitting and reloading.** Sevak sends `shutdown` to every script when it
  quits, and when a reload replaces the plugin instances.
- Everything is in Sevak's log (`Open log folder` in Settings), tagged with the
  plugin id.

### Security

- Scripts run **with your user account's privileges**, exactly like any program
  you start. Sevak does not sandbox them. Install only plugins you trust.
- A new plugin does **not run until you allow it**. Sevak asks once per plugin,
  in a native dialog that shows the name, the folder and the exact command. The
  answer is stored in `<data dir>/script-plugin-approvals.json`, bound to the
  plugin id **and** its command line, mode and format: if `plugin.toml` changes
  what runs, Sevak asks again. (Editing the script file itself is not detected;
  the manifest is what you approve.) "Not now" asks again at the next start.
  Disabled plugins are never asked about.
- Only folders in your own config directory are loaded. Sevak does not download,
  update or install plugins, and makes no network requests for them. What a
  script does on its own is outside Sevak's control and should be stated by its
  author.
- Scripts cannot make Sevak do more than the fixed list of actions. `open_url`
  still passes the platform allow-list (`http`, `https`, `mailto`), so a script
  cannot open `file:` or custom schemes. Icons are restricted to the plugin folder.
- Everything a script sends is validated: sizes are capped, scores clamped,
  malformed items skipped. A misbehaving script cannot crash Sevak or stall typing.
- The settings window lists script plugins (waiting ones marked as such) so you
  can see what is installed and switch each off.

### Tips for plugin authors

- **Print protocol messages with a flush** (`print(..., flush=True)` in Python,
  `[Console]::Out.Flush()` in PowerShell). A buffered reply looks like a hung script.
  Sevak starts `.py` scripts with `-u` for this reason.
- **Keep stdout clean**; use stderr for debugging.
- **Answer the newest query first.** If several queries are queued on stdin,
  skip to the last one.
- **Do slow work off the query path.** Cache what you can; a script that needs
  the network should answer from a local cache and refresh in the background.
- **Make keys stable** and scores either absent or consistent.
- **One-shot output is read as UTF-8.** In Windows PowerShell set
  `[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)`
  (see `timestamp.ps1`).
- Test a persistent plugin by hand:
  `echo '{"type":"query","request_id":1,"input":"x"}' | python3 main.py`.

### Design decisions

These settle what an earlier design draft left open.

| Question | Decision |
|---|---|
| Async results | Query generations plus a shell notification (`Plugin::attach_notifier`, the `sevak:results` event); late answers for stale generations are dropped. |
| Windows command resolution | Explicit `command` array, no shell; `script` with interpreter shims by extension (`py -3`, `pwsh`/`powershell`, `node`). |
| Scores | Optional, clamped below the keyword score; default is the script's order. |
| Icons | `file` icons relative to the plugin folder, with path-escape and symlink checks. |
| Startup cost | Lazy start on first use; idle shutdown after five minutes by default. |
| Permissions UX | A native dialog on first sight of a plugin or a changed command; approvals in a state file, not in `config.toml`. |
| `global` scripts | Not allowed. Script plugins are keyword-only. |
| Registry | A `ScriptPluginHost` next to the registry rather than one descriptor per manifest, because descriptor factories are plain function pointers with no access to the config directory. It honours `[plugins] disabled` itself and feeds the settings catalog. |
| Manifest | `command` is one array (not `command` plus `args`) so the whole command line is explicit and approvable. |
| Alfred | Supported as a one-shot output format, not as a separate plugin type. |
| Distribution | Manual installation only; signing or a registry is out of scope. |
| Sandboxing | None. WASM components would give real isolation at the price of a much heavier runtime and authoring story; that is a possible future *additional* plugin type. |

Versioning: `protocol` is an integer in the manifest and in `initialize`. Within
a version only additive changes happen (new optional fields, new message types
scripts may ignore). Unknown fields must be ignored by both sides.

### For contributors

| Concern | Where |
|---|---|
| Manifest, command resolution | `crates/sevak-plugins/src/script/manifest.rs` |
| Wire messages | `.../script/protocol.rs` |
| Items to results, scores, icons | `.../script/items.rs` |
| Alfred mapping | `.../script/alfred.rs` |
| Query generations and late answers | `.../script/delivery.rs` |
| Process lifecycle | `.../script/runner.rs` (persistent), `.../script/oneshot.rs` |
| `ScriptPlugin` | `.../script/plugin.rs` |
| Discovery, disabled list, approvals | `.../script/host.rs`, `.../script/approvals.rs` |
| Approval dialog, wiring into the engine | `src-tauri/src/script_plugins.rs`, `src-tauri/src/search.rs` |
| Interpreter choice, console-less spawn | `crates/sevak-platform/src/process.rs` |
| End-to-end tests | `crates/sevak-plugins/tests/script_plugins.rs` with the helper `tests/fixtures/script_fixture.rs` |

The integration tests start real processes through the real search engine. The
script is a small Rust binary (`sevak-script-fixture`) so the tests run the same
on Windows, macOS and Linux without Python or Node. A last test runs the
bundled examples wherever their interpreter is installed.

```
cargo test -p sevak-plugins
```
