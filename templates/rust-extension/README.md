# {{project-name}}

A [Sevak](https://github.com/ninad-k/Sevak) extension written in Rust.

## Build and try it

```sh
cargo build --release
```

Copy the program next to the manifest the way Sevak installs it, into your
plugins folder (Windows `%APPDATA%\sevak\plugins`, macOS
`~/Library/Application Support/sevak/plugins`, Linux `~/.config/sevak/plugins`):

```text
plugins/{{project-name}}/plugin.toml
plugins/{{project-name}}/bin/{{project-name}}-<platform>[.exe]
```

where `<platform>` is the name in `plugin.toml` for your computer
(`windows-x86_64`, `macos-aarch64`, `linux-x86_64`, ...). Choose **Reload index**
in Sevak's tray menu; Sevak shows the **Allow** dialog for a native extension,
with the publisher, permissions and the program's SHA-256. After you allow it,
type `{{keyword}}` and a name.

## Package it

```sh
sevak-ext validate .
sevak-ext pack . --binary linux-x86_64=target/release/{{project-name}} --out dist
```

`pack` writes a `.sevakext` file with the manifest, the program and a
`checksums.sha256`. Add `--split` to write one package per platform, which is
what the gallery lists. See `docs/writing-extensions-in-rust.md` in the Sevak
repository for the whole guide: the manifest, the security model, testing,
submitting to the gallery and versioning.

## Copying this template by hand

The files contain a few markers that `cargo generate` and `sevak-ext init`
replace for you. When you copy the folder yourself, replace them in
`Cargo.toml`, `plugin.toml` and this file:

| Marker | Replace with |
|---|---|
| `{{project-name}}` | the extension's name: lower case letters, digits and `-` |
| `{{keyword}}` | what users type to reach it |
| `{{description}}` | one sentence |
| `{{authors}}` | your name, as it should appear to users |
