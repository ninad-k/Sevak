# Writing extensions in Rust

A **native extension** is a keyword extension written as an ordinary Rust
program. Sevak starts it, sends it what the user typed after its keyword, and
shows the rows it answers with. It speaks the same JSON protocol as a
[script plugin](plugins.md#external-plugins) and goes through the same approval,
so everything in the [script plugin guide](plugins.md#persistent-plugins-the-protocol)
applies. What changes is the language, a compiled program per platform, and the
honest consequences of shipping a binary (see [Security model](#security-model)).

You write a function from a query to a list of rows. The
[`sevak-extension-sdk`](https://github.com/ninad-k/Sevak/tree/main/crates/sevak-extension-sdk)
crate (only `serde` and `serde_json` as dependencies) does the protocol.

| | Workflow | Script plugin | Native extension |
|---|---|---|---|
| Written in | the visual builder | Python, Node, PowerShell, anything | Rust |
| Ships as | a folder | a folder | a folder with one program per platform |
| Needs on the user's computer | nothing | the interpreter | nothing |
| Can you read it before running it? | yes | yes | no, it is compiled |
| Sandboxed? | no | no | no |

Choose a native extension when you want speed, a single self-contained
program, or an existing Rust library. For a few lines of glue, a script plugin
is easier to read and to review.

## Quick start

```sh
cargo install --git https://github.com/ninad-k/Sevak sevak-ext --locked   # once
sevak-ext init my-extension --keyword hi --description "Says hi."
cd my-extension
cargo build --release
```

(`sevak-ext` builds Sevak's own plugin code, so it needs a C compiler like the
app does. In a clone of the Sevak repository use `cargo run -p sevak-ext --` in
place of `sevak-ext`.)

Three ways to start, all from the same template
([`templates/rust-extension/`](https://github.com/ninad-k/Sevak/tree/main/templates/rust-extension)):

- `sevak-ext init <folder>` writes the project.
- `cargo generate --git https://github.com/ninad-k/Sevak templates/rust-extension --name my-extension`
  (asks for the keyword and description).
- Copy the folder and replace the four markers `{{project-name}}`,
  `{{keyword}}`, `{{description}}` and `{{authors}}` by hand (listed in its
  `README.md`).

The template is a complete, working extension and the same code as
[`examples/rust-hello`](https://github.com/ninad-k/Sevak/tree/main/examples/rust-hello)
(a test keeps them identical), which is built and tested in Sevak's own CI.

### Try it in Sevak

Put the folder where Sevak looks for plugins, with the program named as
`plugin.toml` says for your computer:

```text
<plugins folder>/my-extension/plugin.toml
<plugins folder>/my-extension/bin/my-extension-linux-x86_64        (your platform's name)
```

| OS | Plugins folder |
|---|---|
| Windows | `%APPDATA%\sevak\plugins` |
| macOS | `~/Library/Application Support/sevak/plugins` |
| Linux | `~/.config/sevak/plugins` |

Choose **Reload index** in the tray menu. Sevak shows the **Sevak: new native
extension** dialog (publisher, version, declared permissions, the program's
SHA-256, what it runs). Choose **Allow**, then type your keyword and a space.

Rebuilding changes the program, which changes what the approval covers, so
Sevak asks again after each rebuild (and refuses to start a program that
changed since it was allowed). That is the same rule that protects users from a
swapped binary.

## The SDK

```rust
use sevak_extension_sdk::{run, Item, Query};

fn main() {
    run(|query: &Query| {
        if query.is_empty() {
            return Ok(vec![Item::new("Type your name").subtitle("Say hello")]);
        }
        Ok(vec![Item::new(format!("Hello, {}!", query.text())).copy_on_enter()])
    });
}
```

| Piece | What it is |
|---|---|
| `run(f)` | the whole protocol for the common case; runs until Sevak sends `shutdown` or closes stdin |
| `Extension::new(f).on_execute(g).on_initialize(h).run()` | also handle `custom` actions coming back, and the `initialize` message |
| `run_oneshot(f)` | for `mode = "oneshot"`: the query is the last argument, one `{"items": [...]}` document is printed |
| `Query` | `raw()`, `text()` (trimmed), `is_empty()`, `words()`, `word(n)`, `rest_from(n)`, `split_first()`, `args()` (a `"quoted phrase"` is one argument), `parse::<T>()` |
| `Item` | `Item::new(title)` then `.key()`, `.subtitle()`, `.icon()`, `.action()`, `.copy_on_enter()`, `.score()`, `.text_view()`, `.tile()`; `.problems()` lists what Sevak would cut or drop |
| `Action` | the **closed set**: `copy_text`, `open_url` (only `http`, `https`, `mailto`), `open_path`, `custom` (comes back to your program), `launch` (only with `capabilities = ["launch"]`) |
| `Icon` | `Icon::builtin("copy")` or `Icon::file("icon.png")` (inside your folder) |
| `Error` | return `Err(Error::new("..."))`, or use `?` on any `std::error::Error`; Sevak logs it with your extension's id and shows no rows for that query |
| `log(text)` | stderr, which Sevak captures into its log; **never print to stdout**, it carries the protocol |
| `plugin_dir()`, `plugin_data_dir()`, `plugin_id()`, `sevak_version()` | what Sevak tells you through the environment |

What the SDK does for you: titles, subtitles and keys are cut to the lengths
Sevak keeps, scores are clamped below Sevak's own keyword rows, an answer holds
at most 50 rows and a line never exceeds the host's 1 MiB limit; if several
queries are waiting only the newest is answered (Sevak drops answers to older
ones anyway); a panic in your function becomes an error for that query and the
loop keeps running. In a debug build the SDK prints each row's `problems()` to
stderr.

Things to design for (the same as for any [persistent plugin](plugins.md#tips-for-plugin-authors)):

- **Answer quickly.** A query waits `timeout_ms` (default 50 ms); a later answer
  still arrives and the list updates. Keep slow work off the query path and answer
  from a cache.
- **Keep state on disk.** An idle process is stopped after `idle_timeout_secs`
  (default 300) and started again on the next query. Use `plugin_data_dir()`.
  Sevak removes that folder when the extension is uninstalled.
- **Make keys stable** (`project-sevak`, not a timestamp): usage statistics are
  keyed by them.

### Testing without Sevak

The protocol loop is a function of a reader and a writer, so the extension's
logic and the whole conversation are testable in `cargo test`:

```rust
use std::io::Cursor;
use sevak_extension_sdk::{Extension, Item};

let mut out = Vec::new();
let input = "{\"type\":\"query\",\"request_id\":1,\"input\":\"hi\"}
".to_owned();
Extension::new(|query| Ok(vec![Item::new(query.text())]))
    .serve(Cursor::new(input), &mut out)
    .unwrap();
assert!(String::from_utf8(out).unwrap().contains("\"title\":\"hi\""));
```

[`examples/rust-hello/tests/host.rs`](https://github.com/ninad-k/Sevak/blob/main/examples/rust-hello/tests/host.rs)
goes further: it runs the built program through Sevak's real script-plugin host
(approval, pipes, the closed actions, a `custom` action coming back) in a
temporary folder. Use it as a pattern if you depend on `sevak-plugins` from a
checkout of the repository; outside the repository the unit tests above are the
right level.

## The manifest

A native extension is a script plugin folder whose `plugin.toml` has an
`[extension]` table instead of `command` / `script`. All the script plugin keys
([the manifest](plugins.md#the-manifest-plugintoml): `keyword`, `name`,
`description`, `mode`, `timeout_ms`, `inherit_env`, `capabilities`, `files`, ...)
work as they do there.

```toml
protocol    = 1
id          = "script:my-extension"     # required, see below
keyword     = "hi"
name        = "My extension"
description = "Says hi."

[extension]
version     = "0.1.0"                   # required, semver; bump it for every release
author      = "Ada Lovelace"            # required; the publisher shown in the Allow dialog
license     = "Apache-2.0"              # required, an SPDX expression
min_sevak   = "0.1.0"                   # the oldest Sevak you tested with
repository  = "https://github.com/ada/my-extension"
homepage    = "https://example.com"     # optional
permissions = ["network"]               # see below

[extension.binaries]
windows-x86_64  = "bin/my-extension-windows-x86_64.exe"
macos-aarch64   = "bin/my-extension-macos-aarch64"
linux-x86_64    = "bin/my-extension-linux-x86_64"
```

| Field | Rules |
|---|---|
| `id` | **Required** for a package: `script:<name>` where `<name>` is lower case letters, digits and `-` (at most 48), the same as the gallery id and the folder it installs to. The package names itself so the folder cannot be chosen by whoever serves it. Never change it: it is part of result ids and usage statistics. |
| `extension.version` | semver (`1.2.3`). The gallery and Settings compare versions to offer updates. |
| `extension.author` | the publisher, plain text (control characters are refused, direction-changing characters are removed when shown). It is **your word, not a verified identity**. |
| `extension.license` | an SPDX expression such as `Apache-2.0` or `MIT OR Apache-2.0`. |
| `extension.min_sevak` | Sevak refuses to load the extension on an older version, and Settings does not offer it to one. |
| `extension.repository` / `homepage` | `https://` addresses, shown as text; Sevak never opens or fetches them. |
| `extension.permissions` | lower case words, at most 8, each at most 24 characters. Known: `network` (connects to the internet or your network), `filesystem` (reads or writes files outside its own folders), `processes` (starts other programs), `clipboard` (reads or changes the clipboard itself), `system` (changes system settings). Others are shown as written, labelled as the author's own. An empty list says "needs none". **Declared, not enforced** (see below). |
| `extension.binaries` | platform to the program's path inside the folder. Platforms: `windows-x86_64`, `windows-aarch64`, `macos-x86_64`, `macos-aarch64`, `linux-x86_64`, `linux-aarch64`. Paths are plain relative paths (no `..`, no drive, no backslash); a Windows program ends in `.exe` and the others do not; two platforms cannot share a path. List only the platforms you build. |

Do not set `command` or `script` as well: Sevak runs the program for the
platform it is on, and a computer with no build for it shows the extension as
"Not loaded: this extension has no build for ...".

Because `plugin.toml` is part of what the approval covers, the declared
publisher, version and permissions are bound to the allowance, and so is the
program (its bytes are hashed) and the folder's location.

## Security model

State this to your users; Sevak does.

**What Sevak does** (the same rules as [script plugins](plugins.md#security) and
[workflows](security/script-workflow-trust.md), reused, not reinvented):

- **Never runs on install.** An installed or copied native extension is new and
  unapproved. Nothing starts until the user chooses **Allow** in a dialog titled
  **Sevak: new native extension**. Declined ("Not now") asks again at the next
  start; disabled extensions are never asked about.
- **The dialog shows what the user is deciding.** The name and keyword, the
  publisher, version and licence, the source address, the permissions you
  declared (marked "the author's statement; Sevak does not enforce them"), the
  program's full SHA-256, the folder and location, the exact command, the
  environment it receives, and a plain statement that it is not sandboxed. Text
  an author wrote is stripped of control and direction-changing characters and
  cut to length, so it cannot fake lines or hide.
- **The approval is bound to the program's bytes** (and the manifest, and the
  folder's location). A rebuilt program, a changed manifest or a moved folder
  asks again. Before each start Sevak checks again that nothing changed since
  the allowance and refuses to start if it did (a swap between the dialog and
  the start does not run).
- **Scrubbed environment.** The program gets a small base set (`PATH`, home and
  temp folders, locale, display variables, what Windows programs need) plus
  `SEVAK_PLUGIN_ID`, `SEVAK_PLUGIN_DIR`, `SEVAK_PLUGIN_DATA`, `SEVAK_VERSION` and
  only the variables your manifest lists in `inherit_env` (shown in the dialog).
  API keys, tokens and credentials in Sevak's own environment do not reach it.
- **Process tree kill.** When Sevak stops the extension, restarts it after a
  crash, or quits, it ends the whole tree of processes it started.
- **A closed set of actions.** Your rows can copy text, open a web or mail
  link, open a path, send a payload back to you, and (with the `launch`
  capability, shown in the dialog) start an application. Sevak performs these
  itself and drops anything else; it cannot be made to paste, elevate or run a
  command through your rows.
- **Every answer is validated:** sizes capped, scores clamped, bad rows skipped.
  A misbehaving extension cannot crash Sevak or stall typing.

**What Sevak does not do.** A native extension is **not sandboxed** beyond the
above. After the user allows it, it is a program running with the user's
account permissions, exactly like any program they installed. It can read and
write the user's files, use the network, start other programs and read the
clipboard, whatever it declared. The declared permissions are information for
the person deciding, not a limit: nothing stops a program that declares none
from using the network. Sevak does not inspect the program, check it against
malware lists, verify who published it, or watch what it does after it starts.

**What the user is trusting** when they allow your extension:

1. that the publisher named in the dialog is who they think it is (Sevak cannot
   check; the gallery's review is the check for gallery entries);
2. that the program does what its description says and nothing else;
3. that the program they were shown (its SHA-256) is the one you built from the
   source you linked, if they care to check;
4. for a gallery install, the same trust as for every gallery file: Sevak's
   repository and its release process (see [Gallery trust](security/gallery-trust.md)).

Operating systems add checks to programs that were downloaded by a browser
(macOS Gatekeeper and notarization, Windows SmartScreen). A program that Sevak
writes into the plugins folder carries no such mark, so those checks do not
apply to it: Sevak's own dialog is the only gate. On macOS on Apple silicon the
program must carry at least an ad-hoc signature to run; Rust's linker adds one
for `aarch64-apple-darwin` builds, so a normal `cargo build` is enough.

Keep your side honest: declare every permission your program uses, link the
source, keep dependencies few, publish builds made from tagged source in CI, and
say in your description what it contacts.

## Package it

```sh
sevak-ext validate .                                    # check plugin.toml (and which programs are built)
sevak-ext pack . --binary linux-x86_64=target/release/my-extension --out dist
sevak-ext pack . --split --out dist \
    --binary linux-x86_64=build/linux/my-extension \
    --binary windows-x86_64=build/windows/my-extension.exe
sevak-ext validate dist/my-extension-0.1.0.sevakext     # check a whole package
```

A `.sevakext` file is a zip with a flat layout:

```text
plugin.toml          the manifest, byte for byte what is installed
checksums.sha256     "<sha256>  <path>" for every other file (sha256sum's format)
bin/<program>        one program per platform the package carries
...                  optional extra files (--include icon.png)
```

Packing is deterministic (sorted entries, fixed timestamps and permissions), so
the same inputs give the same bytes and the same checksum. `--split` writes one
package per platform (`<id>-<version>-<platform>.sevakext`), which is what the
gallery lists, so a user downloads only the build for their computer. Without
it, one package carries every program you passed; Sevak installs only the
program for the platform it runs on.

`validate` and `pack` run Sevak's own checks (they link the same code as the
app), so what they accept is what Sevak installs: safe paths, no links, size
limits (a package up to 32 MiB, a file up to 64 MiB, 128 MiB unpacked, 100 files),
every file covered by `checksums.sha256` and matching it, a manifest valid for
every platform it declares, and **only the programs the manifest declares** (an
undeclared `bin/` file or `.exe` is refused). Files other than the program and
`plugin.toml` are not covered by the approval unless you list them in `files`;
list data your program depends on.

### Building for each platform

| Platform key | Rust target |
|---|---|
| `windows-x86_64` | `x86_64-pc-windows-msvc` |
| `windows-aarch64` | `aarch64-pc-windows-msvc` |
| `macos-x86_64` | `x86_64-apple-darwin` |
| `macos-aarch64` | `aarch64-apple-darwin` |
| `linux-x86_64` | `x86_64-unknown-linux-gnu` (or `-musl` for the widest reach) |
| `linux-aarch64` | `aarch64-unknown-linux-gnu` |

The template's release profile (`opt-level = "z"`, `lto`, `codegen-units = 1`,
`strip`) keeps programs small; do not use `panic = "abort"`, because the SDK
turns a panic into an error for that query. A sketch of a CI job (adjust it; it
is an example, not part of Sevak):

```yaml
jobs:
  build:
    strategy:
      matrix:
        include:
          - { os: ubuntu-latest,  target: x86_64-unknown-linux-gnu,  key: linux-x86_64,   ext: "" }
          - { os: windows-latest, target: x86_64-pc-windows-msvc,    key: windows-x86_64, ext: ".exe" }
          - { os: macos-latest,   target: aarch64-apple-darwin,      key: macos-aarch64,  ext: "" }
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - run: rustup target add ${{ matrix.target }}
      - run: cargo build --release --locked --target ${{ matrix.target }}
      - uses: actions/upload-artifact@v4
        with:
          name: ${{ matrix.key }}
          path: target/${{ matrix.target }}/release/my-extension${{ matrix.ext }}
  package:
    needs: build
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/download-artifact@v4
      - run: |
          cargo install --git https://github.com/ninad-k/Sevak sevak-ext --locked
          sevak-ext pack . --split --out dist \
            --binary linux-x86_64=linux-x86_64/my-extension \
            --binary windows-x86_64=windows-x86_64/my-extension.exe \
            --binary macos-aarch64=macos-aarch64/my-extension
```

## Submit it to the gallery

The gallery (Settings > Extensions) lists extensions kept in Sevak's own
repository, because that is the only place Sevak downloads from (see
[Gallery trust](security/gallery-trust.md)). To propose yours, open a pull
request that adds:

1. **The packages**, one per platform you support, in
   `gallery/extensions/<id>/` named `<id>-<version>-<platform>.sevakext`
   (`sevak-ext pack --split` writes exactly these names). Each is at most 10 MiB.
2. **An entry in `gallery/index.json`.** `sevak-ext entry <the packages>` prints
   it, with the SHA-256 of each file:

   ```json
   {
     "id": "my-extension",
     "kind": "native",
     "name": "My extension",
     "description": "One or two sentences. Says what it contacts, if anything.",
     "author": "Ada Lovelace",
     "version": "0.1.0",
     "license": "Apache-2.0",
     "tags": ["native"],
     "min_sevak": "0.1.0",
     "permissions": ["network"],
     "repository": "https://github.com/ada/my-extension",
     "homepage": "https://github.com/ada/my-extension",
     "platforms": {
       "linux-x86_64": {"source": "gallery/extensions/my-extension/my-extension-0.1.0-linux-x86_64.sevakext", "sha256": "..."},
       "windows-x86_64": {"source": "gallery/extensions/my-extension/my-extension-0.1.0-windows-x86_64.sevakext", "sha256": "..."}
     }
   }
   ```

   `source` is a path relative to the repository root, never an address. The
   entry's version, publisher, licence, minimum Sevak, repository and permissions
   must equal what the package's `plugin.toml` says: Sevak refuses an install
   where the page and the manifest disagree, and a test checks it for every entry.
3. **Checks:** `node scripts/gallery-check.mjs` (addresses, files, hashes, no
   orphans; `--update` refreshes the hashes) and `cargo test -p sevak-plugins
   --test gallery_content` (opens every package and compares it with its entry).

What reviewers look for, because people install what is listed:

- the source is public at the linked address, builds with a documented command,
  and the committed binaries are what that source produces at a tagged commit
  (reviewers may rebuild and compare checksums; say what toolchain you used);
- declared permissions match what the code does (they read the network, file and
  process use in the source), and the description says what the extension
  contacts;
- an open-source licence, no obfuscation or packing, no bundled secondary
  downloader or updater, no telemetry without saying so;
- a unique keyword that no built-in, search engine or other entry uses;
- small: one package at most 10 MiB, usually under 2.

Two limits to plan for:

- **A release carries the list.** A build reads the list from the tag of its own
  version, not from `main`, so an entry added to the repository reaches users
  with the next Sevak release (see [Gallery trust](security/gallery-trust.md)).
  Users can always install your extension by hand from your own releases in the
  meantime: download the package, copy the folder as in [Try it in
  Sevak](#try-it-in-sevak) (or unzip the `.sevakext`), and Sevak asks before it runs.
- **There is no signature yet.** The checksum in the list comes from the same
  repository as the file; see the limits listed in Gallery trust.

## Versioning and updates

- Your extension's `extension.version` is semver. Bump it for every release,
  including a rebuild with a different program: Settings compares it with the
  installed version to offer **Update**.
- An update replaces the folder as one step (the new version is checked and
  written aside first; if anything fails the old version stays). It is **a new
  program**, so Sevak asks for permission again, with the dialog saying the
  contents changed.
- `protocol = 1` is the wire protocol; within a version only additive changes
  happen, so a program built today keeps working. `min_sevak` is how you say you
  need a newer Sevak.
- The SDK has its own version (`sevak-extension-sdk` 0.x). A breaking SDK change
  does not break programs already built: the protocol is the contract.

## Publishing the SDK (maintainers)

`sevak-extension-sdk` is written to be published (Apache-2.0, a README, only
`serde` and `serde_json` as dependencies, `publish` is not disabled) but nothing
publishes it automatically. When the maintainers decide to:

1. Check the crate on its own: `cargo publish -p sevak-extension-sdk --dry-run`,
   and `cargo package -p sevak-extension-sdk --list` for the file list.
2. Publish it from the repository's release workflow or by hand with a token that
   is not stored in the repository.
3. In `templates/rust-extension/Cargo.toml` replace the `git` dependency with
   `sevak-extension-sdk = "0.1"`, and do the same in `docs/`.

Until then the template depends on the Sevak repository (`branch = "main"`); pin
a `rev` for builds you want to reproduce.

## Troubleshooting

| Symptom | Likely cause |
|---|---|
| Settings > Plugins lists it as "Not loaded: ... no build for linux-x86_64" | `plugin.toml` has no `[extension.binaries]` entry for this computer's platform |
| "Not loaded: needs Sevak 0.3.0 or newer" | `min_sevak` is newer than the running Sevak |
| The Allow dialog appears again after every rebuild | by design: the approval is bound to the program's bytes |
| "refuses to start: changed since you allowed it" in the log | the program or manifest changed after the dialog; choose Reload index |
| No rows, `error` in the log | your function returned `Err` or panicked; stderr lines are tagged with your id |
| Rows show but Enter does nothing | the action was outside the closed set (the log says which); `open_url` accepts only `http`, `https` and `mailto` |
| `sevak-ext validate` says "needs an `id`" | add `id = "script:<name>"` to `plugin.toml` |
| macOS: "killed" when started | an unsigned arm64 binary: build with the default Rust toolchain for `aarch64-apple-darwin` (it signs ad hoc) |
