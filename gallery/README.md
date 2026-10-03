# Sevak galleries

This folder holds the two opt-in online galleries. Sevak reads them from
`https://raw.githubusercontent.com/ninad-k/Sevak/main/gallery/` only when you
ask: nothing is fetched at startup or in the background. Both use the same
download code (HTTPS only, a size limit, a timeout, a `Sevak/<version> (gallery)`
user agent) and install a file only if its SHA-256 matches the one in the index.

| Gallery | Index | Files | Opened from | What the app does |
|---|---|---|---|---|
| Workflows and script plugins | `index.json` | `packages/*.zip` | Settings → Gallery → **Load gallery** | [docs/workflows.md](../docs/workflows.md#the-gallery) |
| Themes | `themes.json` | `themes/*.toml` | Settings → Appearance → Theme editor → **Browse online themes** | [docs/themes.md](../docs/themes.md#theme-gallery) |

## Adding a workflow or script plugin

1. Put the folder in `examples/` (workflows in `examples/workflows/`, script
   plugins in `examples/plugins/`) and pack it:

   ```sh
   cargo run -p sevak-plugins --example gallery_pack -- <folder> gallery/packages/<id>.zip
   ```

   The command prints the zip's SHA-256.
2. Add an entry to `index.json` with `id`, `kind` (`workflow` or `plugin`),
   `name`, `description`, `author`, `version`, `source` (the
   `https://raw.githubusercontent.com/ninad-k/Sevak/main/gallery/packages/<id>.zip`
   address), `sha256` and optionally `homepage`.
3. `cargo test -p sevak-plugins gallery` checks that every package matches its
   hash and the example it was built from. An installed package still has to be
   allowed before anything in it runs.

## Adding a theme

1. Make the theme in Settings → Appearance → Theme editor, then **Export…** it
   (or copy the file from your `themes` folder) to `themes/<Name>.toml`.
   Check that every contrast line in the editor says AA.
2. Add an entry to `themes.json`:

   ```json
   {
     "id": "my-theme",
     "name": "My Theme",
     "author": "you",
     "description": "One sentence.",
     "mode": "dark",
     "url": "https://raw.githubusercontent.com/ninad-k/Sevak/main/gallery/themes/My-Theme.toml",
     "sha256": "<the SHA-256 of the file>"
   }
   ```

   Get the hash with `sha256sum gallery/themes/My-Theme.toml` (macOS:
   `shasum -a 256`; Windows: `Get-FileHash -Algorithm SHA256`). The hash must
   be of the file exactly as committed (the repository stores it with LF line
   endings), and it must be changed whenever the file is.
3. `cargo test -p sevak-core` checks that every entry whose `url` points into
   this folder has the right hash and is a valid theme without warnings.

Only `https://` URLs are accepted by either gallery, and `id`s must be unique
within an index.
