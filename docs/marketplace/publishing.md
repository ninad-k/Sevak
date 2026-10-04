# Publishing to the gallery

This is the path from nothing to a pull request for each thing the Sevak gallery
lists: a **workflow**, a **script plugin**, a **native extension** (a program
written in Rust) and a **theme**. Read [How review works](review-process.md)
afterwards to see what happens to your pull request.

!!! note "What \"published\" means"
    The gallery is a list kept in Sevak's own repository, because that is the only
    place Sevak downloads from ([Gallery trust](../security/gallery-trust.md)).
    Publishing means getting a pull request merged. A released Sevak reads the list
    at its own release tag, so **your entry reaches users with the next release
    after the merge**, not at once. Until then anyone can install your work by hand.
    There is no account, no upload form and no store outside the repository.

## Before you start

1. **Decide whether it belongs.** Read [what the gallery accepts](https://github.com/ninad-k/Sevak/blob/main/gallery/README.md#what-the-gallery-accepts).
   In short: it does what its description says and nothing else, makes no network
   calls of its own, writes nothing outside its own folder, has no secrets or
   telemetry, and is small enough to read in a few minutes.
2. **Check the licence.** The repository is Apache-2.0. A workflow, script plugin
   or theme you put in `examples/` or `gallery/` is contributed under Apache-2.0
   (a theme that uses someone else's palette must name the palette's licence). A
   native extension keeps its own licence, which must be an open-source licence
   that lets the gallery distribute your binaries; give it as an SPDX identifier.
3. **For a native extension, or anything unusual, open a submission issue first.**
   The [Gallery submission form](https://github.com/ninad-k/Sevak/issues/new?template=extension_submission.yml)
   asks what a reviewer needs to know (permissions, network and file behaviour,
   platforms). It saves you from building something that cannot be accepted.
   Questions and half-formed ideas belong in
   [Discussions](https://github.com/ninad-k/Sevak/discussions).
4. **Fork and clone** [ninad-k/Sevak](https://github.com/ninad-k/Sevak), and create a
   branch. You need Node.js 22 or newer for the checker. You need the Rust
   toolchain (pinned in `rust-toolchain.toml`; rustup installs it) to pack a
   package.

Every kind ends the same way, in [Open the pull request](#open-the-pull-request).

## Workflow

A workflow is a folder with a `workflow.toml` (and any script files it runs).
Most run no code: they open a link, transform text or show a result.

1. Build and try it in **Settings → Workflows** (see [Workflows](../workflows.md)).
   Give it a unique keyword.
2. Put the folder in `examples/workflows/<id>/`. The folder name is the entry's
   `id`: lower case letters, digits and dashes, at most 48 characters, and not a
   Windows device name such as `con`, `nul` or `com1`. Use LF line endings.
3. Stay within the gallery's rules for workflows: only the allowed node types
   (triggers, Transform, Conditional, Set variable, Open URL, Copy, Paste,
   Notification, Large Type, Text view, and Run script with a script from the
   package), links only to hosts on the allow list in
   `crates/sevak-plugins/tests/gallery_content.rs` (adding a host is a review
   decision, so say why), and no Launch app, System command, Terminal command, Open
   file or Hotkey node.
4. Pack it. The command prints the zip's SHA-256:

   ```sh
   cargo run -p sevak-plugins --example gallery_pack -- examples/workflows/<id> gallery/packages/<id>.zip
   ```

5. Add the entry to `gallery/index.json` (see [The index entry](#the-index-entry)).
6. Run the [checks](#check-it-before-you-push).

## Script plugin

A script plugin is a `plugin.toml` and a script (Python 3 or Node.js 22+,
standard library only) that Sevak starts when someone types its keyword. See
[Writing plugins](../plugins.md#external-plugins) for the protocol.

1. Put the folder in `examples/plugins/<id>/` (same naming rules as above).
   List every helper file in `files` in the manifest, so the user's approval
   covers it.
2. Keep to the rules for scripts: standard library only, no sockets, HTTP,
   subprocesses, `eval` or `exec`, no file writes (a timer that saves its own
   state in `SEVAK_PLUGIN_DATA` is the one reviewed exception), under 20 KiB, not
   minified. `cargo test -p sevak-plugins --test gallery_content` checks the
   imports against an explicit allow list.
3. Say what it needs: tag it `needs-python` or `needs-node`.
4. Run `node --test scripts/tests/extensions.test.mjs` if you changed a Node
   package, then pack it:

   ```sh
   cargo run -p sevak-plugins --example gallery_pack -- examples/plugins/<id> gallery/packages/<id>.zip
   ```

5. Add the entry to `gallery/index.json` with `"kind": "plugin"`, and run the
   [checks](#check-it-before-you-push).

Because a script plugin runs code on the user's computer, expect the
[security review](review-process.md#security-review) in addition to the normal
one. An installed plugin never runs until the user has allowed it in Sevak's own
dialog, but the reviewer's job is to make sure the dialog is not misleading.

## Native extension

A native extension is a compiled program that speaks Sevak's plugin protocol. It
is held to more than the rules above because it is a binary. Read
[Writing extensions in Rust](../writing-extensions-in-rust.md) first: the
manifest, the security model (a native extension is **not** sandboxed) and
versioning.

1. Install the tool and scaffold a project:

   ```sh
   cargo install --git https://github.com/ninad-k/Sevak sevak-ext --locked
   sevak-ext init my-extension --keyword hi --description "Says hi."
   ```

2. Publish the **source** in a public repository, under an open-source licence,
   with a README that says how to build it. Tag the commit you build from.
3. Build the program for each platform you support (the table of targets is in
   [Building for each platform](../writing-extensions-in-rust.md#building-for-each-platform)),
   then pack one package per platform:

   ```sh
   sevak-ext validate .
   sevak-ext pack . --split --out dist \
       --binary linux-x86_64=build/linux/my-extension \
       --binary windows-x86_64=build/windows/my-extension.exe
   sevak-ext validate dist/my-extension-0.1.0.sevakext
   ```

   Packing is deterministic, so a reviewer who builds from your tag can compare
   checksums. `validate` runs the same checks Sevak runs on install.
4. In your fork of Sevak, copy the packages to `gallery/extensions/<id>/` (at
   most 10 MiB each, nothing else in that folder; names are
   `<id>-<version>-<platform>.sevakext`).
5. Print the entry and add it to `gallery/index.json`:

   ```sh
   sevak-ext entry gallery/extensions/<id>/*.sevakext
   ```

   `version`, `author`, `license`, `repository`, `min_sevak` and `permissions`
   must equal what the package's `plugin.toml` says: Sevak refuses to install where
   they differ, and a test checks it.
6. Run the [checks](#check-it-before-you-push), and
   `cargo test -p sevak-plugins --test gallery_content`, which opens every package
   and compares it with its entry.

`sevak-ext validate` is what an author runs before submitting; there is no separate
`sevak-ext check`. `node scripts/gallery-check.mjs` covers the index and the files
around it.

Every native extension gets the [security review](review-process.md#security-review):
a reviewer reads the public source, checks that the committed binaries are what
that source builds, and compares the declared `permissions` with the code.

## Theme

1. In **Settings → Appearance → Theme editor** make the theme and check that every
   contrast line says AA, then **Export…** it to `gallery/themes/<Name>.toml`.
2. If the colours come from someone else's palette, start the file with a comment
   naming the palette, its author and its licence (MIT or Apache-2.0), for example
   `# Palette: Tokyo Night by enkia, MIT licence (https://github.com/...).`
3. Add an entry to `gallery/themes.json` with `id`, `name`, `author`,
   `description`, `mode` (`light` or `dark`, matching the single palette in the
   file), `url` (`gallery/themes/<Name>.toml`) and `sha256`.
4. Run the [checks](#check-it-before-you-push). `--update` fills in the hash.

A theme runs no code, so it does not need the security review. A reviewer
looks at the licence of the palette and at how it looks.

## The index entry

Workflows and script plugins, in `gallery/index.json`:

```json
{
  "id": "my-workflow",
  "kind": "workflow",
  "name": "My workflow",
  "description": "One or two sentences. Runs no code.",
  "author": "Your Name",
  "version": "1.0.0",
  "license": "Apache-2.0",
  "tags": ["search", "no-code"],
  "source": "gallery/packages/my-workflow.zip",
  "sha256": "<the SHA-256 of the zip>",
  "homepage": "https://github.com/ninad-k/Sevak/tree/main/examples/workflows/my-workflow"
}
```

- `kind` is `workflow`, `plugin` or `native`. `source` is a **path** below the
  repository root, never an address. You can leave `sha256` as 64 zeros and let
  `node scripts/gallery-check.mjs --update` fill it in.
- `version` is numeric (`1.0` or `1.0.0`; a native extension needs three parts).
  Bump it whenever the package's bytes change.
- `license` is an SPDX expression (`MIT`, `Apache-2.0`, `MIT OR Apache-2.0`). It is
  required unless the author is the Sevak project itself, and always for a
  native extension. Licences such as GPL or a non-commercial licence are not
  rejected by the script, but a maintainer has to decide about them.
- `tags` are one to eight lower case labels. `no-code` is only for workflows;
  `official`, `verified` and `featured` are reserved for the maintainers.
- `name` is at most 60 characters and `description` at most 300.
- The keyword in your manifest may not be used by another entry.

## Check it before you push

```sh
node scripts/gallery-check.mjs                    # paths, hashes, orphans, the rules above
node scripts/gallery-check.mjs --base origin/main # also: a changed package needs a higher version
node --test scripts/gallery-check.test.mjs        # only if you changed the checker
cargo test -p sevak-plugins --test gallery_content
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

The checker prints **problems** (they fail the check) and **warnings** (a human will
look at them: a new permission, a licence that needs a decision, an entry that was
removed). Pull requests run the same checks automatically; see
[the automated review](review-process.md#automated-checks).

## Open the pull request

1. Commit with a [Conventional Commit](https://www.conventionalcommits.org) message:
   `feat(gallery): add my-workflow` for a new entry,
   `feat(gallery): update my-workflow to 1.1.0` or `fix(gallery): ...` for a new
   version.
2. Push your branch and open a pull request to `main`. Add
   `?template=gallery_submission.md` to the compare address to get the
   [gallery submission template](https://github.com/ninad-k/Sevak/blob/main/.github/PULL_REQUEST_TEMPLATE/gallery_submission.md)
   with the pre-merge checklist. Link your submission issue if you opened one.
3. Watch the **Gallery review** check. It comments with a report, and a maintainer
   takes it from there.

Keep a pull request to one entry (or one update). Do not touch other entries or
the listing tables in `gallery/README.md`.

Next: [How review works](review-process.md), and later
[updating and removal](updating-and-removal.md).
