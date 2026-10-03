# Sevak plugins

Everything Sevak shows in its result list comes from a plugin: installed apps,
the calculator, web search, files. This document explains how a query flows
through them, how to write and register a built-in plugin, and sketches how
script-based external plugins could work in the future.

- [Architecture](#architecture)
- [Writing a built-in plugin](#writing-a-built-in-plugin)
- [Toward script-based external plugins](#toward-script-based-external-plugins) (design only, **not implemented**)

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
| Standard action execution | `crates/sevak-plugins/src/actions.rs` |
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
  happen as an `Action`. They do not touch the OS directly.
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
  Other OS entry points are narrow, closed vocabularies instead of strings:
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
  `apps` (looks the key up in its index) and `files` (checks the path exists)
  do. Plugins that only exist as answers to a typed query (calculator, web
  search, uuid) leave it alone.

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
| `uuid` | `uuid` | example plugin, keyword-only |

- `PluginRegistry::builtin()` is the stock set; `register(descriptor)` adds (or
  replaces, by id) a family. This is how a compiled-in third-party plugin joins.
- `instantiate(&config, platform)` builds the enabled plugins. Disabled families
  are not constructed at all. It logs at `info` which plugins were loaded and
  which were skipped. `builtin_plugins(&config, platform)` is shorthand for
  `PluginRegistry::builtin().instantiate(..)`.
- `catalog(&config, platform)` returns a `PluginInfo { id, name, description,
  keyword, enabled }` for **every** instance, disabled ones included (it is
  `Serialize`, snake_case). The planned settings UI uses it for its toggles.

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

## Toward script-based external plugins

> **Status: design only. None of this is implemented.** Today plugins are Rust
> code compiled into Sevak. This section records the intended direction so the
> built-in contract stays compatible with it.

Goal: let a user drop a folder with a script into the config directory and get a
keyword plugin, without building Sevak.

### Discovery and manifest

Each plugin lives in `<config dir>/sevak/plugins/<name>/` and contains a
`plugin.toml` plus whatever the script needs. Sevak scans the directory at
startup and on "Reload index".

```toml
# <config dir>/sevak/plugins/hello/plugin.toml
protocol    = 1                  # protocol version the script speaks
id          = "script:hello"     # stable; must be unique; prefix avoids clashing with built-ins
name        = "Hello"
description = "Greets whoever you type."
keyword     = "hello"
global      = false              # also answer queries without the keyword?
command     = "python"           # resolved via PATH or relative to the plugin folder
args        = ["main.py"]
timeout_ms  = 50                 # soft per-query budget
```

`[plugins] disabled` and `catalog()` apply as for built-ins (the manifest `id`
is the instance id).

### Process model and protocol

A **long-lived child process** per plugin, started lazily on first use (or at
startup), with its working directory set to the plugin folder. Sevak and the
script exchange **newline-delimited JSON** messages over stdin/stdout, one
object per line, in a JSON-RPC-like shape. The script's stderr is captured and
written to Sevak's log, tagged with the plugin id. Messages:

```jsonc
// Sevak -> script
{"type":"initialize","protocol":1,"sevak_version":"0.1.0"}
{"type":"query","request_id":42,"input":"world"}
{"type":"execute","key":"greeting"}
{"type":"shutdown"}

// script -> Sevak
{"type":"ready"}                                      // reply to initialize
{"type":"results","request_id":42,"items":[
  {"key":"greeting","title":"Hello, world!","subtitle":"Enter to copy",
   "icon":{"kind":"builtin","name":"copy"},
   "action":{"type":"copy_text","text":"Hello, world!"}}
]}
{"type":"error","request_id":42,"message":"..."}      // optional
```

- `items[].key` becomes the result id suffix: `<manifest id>:<key>`, so the
  stability rules for keys apply to scripts too.
- `action` is restricted to the **same `Action` enum** the built-ins use
  (`launch`, `open_path`, `open_url`, `copy_text`; plus `custom`, which Sevak
  routes back to the script as `execute`). The JSON shape is the existing serde
  encoding of `Action`, so the schema is already defined by `model.rs`.
- `execute {key}` is only needed for `custom` actions; for standard actions Sevak
  performs the action itself through `execute_action`, so scripts never get to
  run arbitrary OS calls on Sevak's behalf.
- The script may add a `"score"` per item; Sevak clamps it into a sane range
  (for example never above `score::KEYWORD`) so scripts cannot dominate ranking.

### Latency: never stall typing

`Plugin::query` is synchronous and must be fast, but a script is arbitrary
code. The adapter therefore decouples the two:

- `query()` sends `query {request_id, input}`, then waits **up to the soft
  budget** (`timeout_ms`, default 50 ms) for a `results` message with the
  matching `request_id`.
- Inside the budget: return the items. After it: return the **last cached
  results** (or nothing) immediately, and keep reading. When the late answer
  arrives, store it and ask the shell to re-run the current query (an engine
  "results changed" notification, which does not exist yet; see open questions).
  Results for a superseded `request_id` are dropped.
- **Cancellation:** every `query` carries a fresh `request_id`; a newer query
  makes older ids stale. Scripts may check for new input between steps. Sevak
  does not kill the process to cancel.
- A hard timeout (for example 2 s without any reply) counts as a failure.

### Crash and restart policy

- If the process exits or breaks the protocol, mark the plugin unhealthy, return
  no results and restart with exponential backoff (for example 0.5 s, 1 s, 2 s,
  ... up to 60 s).
- After N consecutive failed starts (say 5), disable the plugin until the next
  reload and log a clear message.
- On exit Sevak sends `shutdown`, waits briefly, then kills the process.

### Security model

- Scripts run **with the user's privileges**, like any program the user starts.
  Sevak provides no sandbox.
- Only plugins the user installed into their own config directory are loaded;
  Sevak does not download or update them.
- Every `open_url` action still passes the platform provider's allow-list
  (`http`, `https`, `mailto`); a script cannot make Sevak open `file:` or custom
  schemes. Launching executables is limited to the `Launch` targets Sevak
  already understands.
- Sevak itself makes no network requests for plugins; whatever the script does
  on its own is outside Sevak's control and should be documented by its author.
- Show enabled script plugins (with command path) in settings so the user can
  see what runs.

### The `ScriptPlugin` adapter

A `ScriptPlugin` implements `Plugin` on top of the child process:

- `id/name/description/keyword/global` come from `plugin.toml`.
- `query(input)` as above: send, wait up to the budget, else serve the cache.
  The cache is a `Mutex<Option<(String /*input*/, Vec<ResultItem>)>>`, written by
  a reader thread that owns the child's stdout.
- `execute(item)`: standard actions go to `execute_action`; `Custom` sends
  `execute {key}` to the script.
- `refresh()`: (re)start the process if it is down, resending `initialize`.
- A `ScriptPluginHost` scans the plugins directory and registers one
  `PluginDescriptor` per manifest with the `PluginRegistry`, so scripts reuse
  `instantiate`, `catalog` and the disable list unchanged.

### Versioning

- `protocol` is an integer in the manifest and in `initialize`. Sevak refuses to
  start a plugin that asks for a version it does not support and says so in the
  log and in settings.
- Within a version, only additive changes (new optional fields, new message
  types that scripts may ignore). Unknown fields must be ignored by both sides.

### Minimal example

`plugin.toml` as above, with `main.py`:

```python
import json, sys

for line in sys.stdin:
    msg = json.loads(line)
    kind = msg["type"]
    if kind == "initialize":
        print(json.dumps({"type": "ready"}), flush=True)
    elif kind == "query":
        text = f"Hello, {msg['input'] or 'world'}!"
        item = {
            "key": "greeting",
            "title": text,
            "subtitle": "Enter to copy",
            "icon": {"kind": "builtin", "name": "copy"},
            "action": {"type": "copy_text", "text": text},
        }
        print(json.dumps({"type": "results", "request_id": msg["request_id"],
                          "items": [item]}), flush=True)
    elif kind == "shutdown":
        break
```

### Open questions

- **Async result delivery.** `Plugin::query` is synchronous and the engine has no
  "results changed" channel. Late script answers need one (engine callback or an
  event to the shell), or scripts only ever get one chance per keystroke.
- **Windows command resolution** (`python` vs `py`, `.cmd` shims, quoting) and
  Linux shebang handling.
- **Scores:** should scripts supply scores at all, or only order? Clamping rules.
- **Icons:** allow `file` icons relative to the plugin folder? Resolution and
  path-escape rules.
- **Startup cost:** start lazily on first keyword use vs eagerly; idle shutdown
  of unused processes.
- **Permissions UX:** first-run confirmation when a new plugin appears.
- **`global` scripts:** how to bound their cost per keystroke, since every
  global query would hit every script; probably forbid or heavily throttle.
- **Distribution:** only manual installation for now; signing/registry is out of
  scope.
- **Alternatives:** WASM components would give real sandboxing at the cost of a
  much heavier runtime and authoring story.
