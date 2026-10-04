# Sevak galleries

This folder holds the two opt-in online galleries. Sevak reads them from
`https://raw.githubusercontent.com/ninad-k/Sevak/v<version>/gallery/` (the tag
of the running build, not `main`) only when you ask: nothing is fetched at
startup or in the background. Both use the same download code (HTTPS only,
addresses in this repository only, a size limit, a timeout, a
`Sevak/<version> (gallery)` user agent) and install a file only if its SHA-256
matches the one in the index. See
[docs/security/gallery-trust.md](../docs/security/gallery-trust.md).

Index entries name files by a **path relative to the repository root**
(`gallery/packages/<id>.zip`, `gallery/themes/<Name>.toml`), never by an
absolute address; Sevak joins the path to the tag it is reading. A change you
merge here reaches users with the next release, not at once. `index.json` has
`"format": 2` and `themes.json` has `"version": 2`; builds older than that show
"update Sevak".

| Gallery | Index | Files | Opened from | What the app does |
|---|---|---|---|---|
| Workflows and script plugins | `index.json` | `packages/*.zip` | Settings → Gallery → **Load gallery** | [docs/workflows.md](../docs/workflows.md#the-gallery) |
| Native extensions (programs written in Rust) | `index.json` (kind `native`) | `extensions/<id>/*.sevakext` | Settings → Extensions, or `ext` in the launcher | [docs/writing-extensions-in-rust.md](../docs/writing-extensions-in-rust.md) |
| Themes | `themes.json` | `themes/*.toml` | Settings → Appearance → Theme editor → **Browse online themes** | [docs/themes.md](../docs/themes.md#theme-gallery) |

Everything here is written by the Sevak project and licensed under Apache-2.0
like the rest of the repository (palettes of third-party themes: see
[Theme palettes and licences](#theme-palettes-and-licences)).

## What is in it

**Workflows** (source in `examples/workflows/<id>/`). Most run no code. Those
that only open a link or show a result need no permission; a workflow that pastes
into another app (`tidy-text`, `markdown-tools`, `selection-toolkit`) or runs a
script asks for your permission once, because it types into whatever app is in front:

| Id | What it does |
|---|---|
| `duckduckgo` | `ddg words` searches DuckDuckGo |
| `dev-search` | `so`, `mdn`, `crate`, `docsrs`, `npm`, `pypi` search Stack Overflow, MDN, crates.io, docs.rs, npm, PyPI |
| `github-search` | `ghr`, `ghi`, `ghu` search GitHub repositories, issues and pull requests, people |
| `wikipedia-search` | `wiki words` searches Wikipedia; the language is a variable |
| `maps-search` | `map place` (OpenStreetMap) and `gmap place` (Google Maps) |
| `open-selected-link` | Universal Actions: open the selected web address (`javascript:`, `file:` and the like are refused) |
| `tidy-text` | Universal Actions: collapse whitespace in the selection |
| `markdown-tools` | Universal Actions: Markdown link, bold, code, quote, bullet list |
| `decode-tools` | Universal Actions: Base64 and URL decoding to a text view, encoding to the clipboard |
| `selection-toolkit` | Universal Actions: word count, JSON pretty-print and minify, timestamp and date conversion. **Runs a Python 3 script** after you allow it |

**Script plugins** (source in `examples/plugins/<id>/`). All are offline,
standard-library only, and wait for your permission before anything runs:

| Id | Keyword | What it does | Needs |
|---|---|---|---|
| `case-converter-node` | `case` | camelCase, snake_case, kebab-case… | Node.js |
| `password-generator` | `pw` | random passwords, PINs, tokens (`pw 24`) | Python 3 |
| `id-generator` | `id` | UUID v4 and v7, ULID, NanoID | Python 3 |
| `color-converter` | `color` | HEX, RGB, HSL, HSV and WCAG contrast | Python 3 |
| `lorem-ipsum` | `lorem` | placeholder text (`lorem 3 paragraphs`) | Python 3 |
| `hash-calculator` | `hash` | MD5, SHA-1, SHA-256, SHA-512, SHA3, BLAKE2b, CRC-32 of text or a file | Python 3 |

**Themes** (`themes/<Name>.toml`): the eight built-in ones (Sevak Light and
Dark, Nord, Dracula, Solarized Light and Dark, Gruvbox, High Contrast) and
Tokyo Night, Catppuccin Mocha, Catppuccin Latte, Rosé Pine, One Dark, Everforest
Dark, Ayu Mirage, Nightfox, GitHub Light, GitHub Dark and Sevak Amber Glass.

## What the gallery accepts

The gallery is opt-in and every install is a click, but people install what is
listed here, so the bar is deliberate. A change is reviewed against this list,
and `cargo test` enforces most of it (see [Checking your change](#checking-your-change)).

- **No surprises.** A package does what its description says and nothing else.
  A workflow description ends with "Runs no code." or says that it runs a script
  "after you allow it".
- **No network from the content itself.** Workflows may *open* a link in the
  browser on a host from the allow list in `crates/sevak-plugins/tests/gallery_content.rs`
  (adding a host is a review decision), with the user's text percent-encoded.
  Scripts must not use sockets, HTTP, subprocesses, `eval`/`exec` or write files;
  the test checks the imports against a list of safe standard-library modules.
- **Only these workflow nodes:** triggers (keyword, Universal Actions), Transform,
  Conditional, Set variable, Open URL, Copy, Paste, Notification, Large Type,
  Text view, and Run script with a script file from the package. No Launch app,
  System command, Terminal command, Open file or Hotkey.
- **Small and readable.** A script is under 20 KiB, a package under 5 MiB, and
  both can be read in a few minutes. No minified, bundled or binary files.
  The one exception is a [native extension](#adding-a-native-extension): a
  compiled program (a package under 10 MiB), reviewed against its public source.
- **No secrets, no accounts, no telemetry.**
- **Cross-platform or honest about it.** Say in the description what a package
  needs (`needs-python`, `needs-node` tags) and give it a unique keyword that no
  built-in, search engine or other gallery entry uses.
- **Licence.** Your contribution is licensed under Apache-2.0. Do not copy code
  or text from sources whose licence does not allow it.

## Adding a workflow or script plugin

1. Put the folder in `examples/` (workflows in `examples/workflows/`, script
   plugins in `examples/plugins/`). The folder name is the entry's `id`: lower
   case letters, digits and dashes. Write the file with LF line endings.
2. Pack it. The command prints the zip's SHA-256:

   ```sh
   cargo run -p sevak-plugins --example gallery_pack -- <folder> gallery/packages/<id>.zip
   ```

   The zip holds the folder under its own name, files in sorted order, with
   fixed timestamps and permissions, so packing the same folder with the same
   Sevak source gives the same bytes. The hash in the index is of the *committed*
   zip; re-pack and update the hash whenever anything in the folder changes
   (and re-pack every package if the zip library is ever upgraded, since the
   compressed bytes can differ between versions).
3. Add an entry to `index.json`:

   ```json
   {
     "id": "my-workflow",
     "kind": "workflow",
     "name": "My workflow",
     "description": "One or two sentences. Runs no code.",
     "author": "you",
     "version": "1.0",
     "tags": ["search", "no-code"],
     "source": "gallery/packages/my-workflow.zip",
     "sha256": "<the SHA-256 of the zip>",
     "homepage": "https://github.com/ninad-k/Sevak/tree/main/examples/workflows/my-workflow"
   }
   ```

   `kind` is `workflow` or `plugin`; `tags` are one to eight lower case labels
   (`a-z`, `0-9`, `-`); `folder` is optional and defaults to the `id`. `source`
   is the **path** of the zip below the repository root, never a web address:
   Sevak joins it to the release it is reading. You can leave `sha256` as 64
   zeros and let the next step fill it in.
4. Run `node scripts/gallery-check.mjs --update` (also `npm run gallery:check`
   to only check). It copies the real hashes into `index.json` and `themes.json`
   and fails on a wrong path, a missing file, a file the index does not list
   or a stale hash.

An installed package still has to be allowed before anything in it runs.

## Adding a native extension

A native extension is a compiled program, so it is held to more than the rules
above. Read [Writing extensions in Rust](../docs/writing-extensions-in-rust.md)
first (the manifest, what Sevak does and does not protect, versioning); then:

1. Build the program for each platform you support and pack one package per
   platform: `sevak-ext pack . --split --binary <platform>=<program> ...`. The
   files are named `<id>-<version>-<platform>.sevakext`.
2. Put them in `gallery/extensions/<id>/` (at most 10 MiB each; nothing else in
   that folder).
3. Add the entry to `index.json`: `sevak-ext entry gallery/extensions/<id>/*.sevakext`
   prints it. `kind` is `native`; it has `platforms` (each a `source` path and
   a `sha256`) instead of `source` and `sha256`; `author`, `license`,
   `repository` and `version` (like `1.2.3`) are required; `min_sevak` and
   `permissions` say what it needs and what its author declares it does. They
   must equal what the package's `plugin.toml` says: Sevak refuses an install
   where they differ, and a test checks it.
4. `node scripts/gallery-check.mjs` (`--update` fills in the hashes) and
   `cargo test -p sevak-plugins --test gallery_content`.

A reviewer checks that the source is public and builds the committed binaries,
that the declared permissions match the code, the licence, and that nothing is
obfuscated or downloads more code. An entry reaches users with the next release
(builds read the list at their own tag, see
[Gallery trust](../docs/security/gallery-trust.md)). Index format 2 is unchanged:
older Sevaks skip entries of a kind they do not know.

## Adding a theme

1. Make the theme in Settings → Appearance → Theme editor, then **Export…** it
   (or copy the file from your `themes` folder) to `themes/<Name>.toml`.
   Check that every contrast line in the editor says AA.
2. If its colors come from someone else's palette, start the file with a
   comment naming the palette, its author and its licence, e.g.
   `# Palette: Tokyo Night by enkia, MIT licence (https://github.com/...).`, and
   add it to the table below. Only use palettes whose licence allows it; the
   test requires such a comment (naming `MIT` or `Apache-2.0`) on every theme
   that is not built in.
3. Add an entry to `themes.json`:

   ```json
   {
     "id": "my-theme",
     "name": "My Theme",
     "author": "you",
     "description": "One sentence.",
     "mode": "dark",
     "url": "gallery/themes/My-Theme.toml",
     "sha256": "<the SHA-256 of the file>"
   }
   ```

   `mode` is `light` or `dark`, whichever single palette the file has. Get the
   hash with `node scripts/gallery-check.mjs --update`, or by hand with
   `sha256sum gallery/themes/My-Theme.toml` (macOS: `shasum -a 256`; Windows:
   `Get-FileHash -Algorithm SHA256`). The hash must be of the file exactly as
   committed (the repository stores it with LF line endings), and it must be
   changed whenever the file is.

Only paths inside this release are accepted by either gallery (an absolute
address must lie below the same tag), and `id`s must be unique within an index
and may not be a Windows device name such as `con` or `nul`.

## Checking your change

```sh
node scripts/gallery-check.mjs                       # hashes, paths, orphans; no build needed
cargo test -p sevak-core -p sevak-plugins            # the full checks, below
```

`cargo test` checks that

- every package matches its hash, installs through the app's own installer and
  equals the folder in `examples/` it was built from;
- every workflow validates without a warning, only uses allowed nodes and hosts,
  and *runs*: the search workflows open the right encoded address, the
  Universal Actions workflows transform sample selections, and the Python
  scripts are run on sample input (those checks are skipped on a computer
  without Python 3 unless `SEVAK_REQUIRE_PYTHON=1` is set);
- every script plugin has a loadable manifest, a keyword nothing else uses and a
  script that answers in the right shape, and is queried through the real engine;
- every theme parses without a warning, matches its entry, is in canonical form,
  reaches WCAG AA for body text (text on background, selected row, subtext, text
  on accent buttons) and sits in the index exactly once.

### Pre-merge checklist

- [ ] The description says what the package does, and whether it runs code.
- [ ] No network access, subprocess, file write or `eval` in any script; no secrets.
- [ ] Links open only hosts on the allow list (and the list change is justified).
- [ ] Folder name, `id`, `name` in the file and `name` in the index agree.
- [ ] Re-packed after the last edit; `node scripts/gallery-check.mjs` is clean.
- [ ] `cargo +1.99.0 fmt --all --check`, `cargo +1.99.0 clippy --workspace
      --all-targets -- -D warnings` and `cargo test --workspace` pass.
- [ ] Themes: contrast lines are all AA; the palette's author and licence are named.
- [ ] Anything a human has to judge (does the theme look good, does the Universal
      Actions entry appear in the right apps) is mentioned in the pull request.

## Theme palettes and licences

Sevak Light, Sevak Dark and Sevak Amber Glass are original. The other themes use
the colors of well-known palettes, adjusted where needed so that body text reaches
WCAG AA (the notes are in each file's first comment). The palettes are used under
their authors' MIT licences, which ask that the copyright notice travel with
substantial copies of the software; only color values are used here, and each
author is credited:

| Theme | Palette | Licence |
|---|---|---|
| Nord | [Nord](https://github.com/nordtheme/nord), Arctic Ice Studio | MIT |
| Dracula | [Dracula](https://github.com/dracula/dracula-theme) | MIT |
| Solarized Light, Solarized Dark | [Solarized](https://github.com/altercation/solarized), Ethan Schoonover | MIT |
| Gruvbox | [Gruvbox](https://github.com/morhetz/gruvbox), Pavel Pertsev | MIT |
| Tokyo Night | [Tokyo Night](https://github.com/enkia/tokyo-night-vscode-theme), enkia | MIT |
| Catppuccin Mocha, Catppuccin Latte | [Catppuccin](https://github.com/catppuccin/catppuccin) | MIT |
| Rosé Pine | [Rosé Pine](https://github.com/rose-pine/rose-pine-theme) | MIT |
| One Dark | [One Dark](https://github.com/atom/one-dark-syntax), Atom / GitHub | MIT |
| Everforest Dark | [Everforest](https://github.com/sainnhe/everforest), sainnhe | MIT |
| Ayu Mirage | [Ayu](https://github.com/ayu-theme/ayu-colors) | MIT |
| Nightfox | [Nightfox](https://github.com/EdenEast/nightfox.nvim), EdenEast | MIT |
| GitHub Light, GitHub Dark | [Primer primitives](https://github.com/primer/primitives), GitHub | MIT |

The names belong to their owners; Sevak is not affiliated with them. The
licences above are those of each project's repository when the theme was added;
if one is wrong or a palette's owner would rather not be included, open an issue
and the theme is changed or removed.
