# Theme gallery

`themes.json` is the index the theme editor reads when someone clicks
**Browse online themes** in Settings → Appearance, and `themes/` holds the theme
files it points to. The eight built-in themes are listed here too, so they can
be reinstalled from the gallery. See [docs/themes.md](../docs/themes.md#theme-gallery)
for what the editor requests and checks.

## Adding a theme

1. Make the theme in Settings → Appearance → Theme editor, then **Export…** it
   (or copy the file from your `themes` folder) to `gallery/themes/<Name>.toml`.
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

Only `https://` URLs are accepted by the editor; `id` and `url` must be unique.
