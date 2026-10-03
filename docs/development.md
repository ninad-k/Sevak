# Developing Sevak

## Layout

```
Cargo.toml              workspace: crates/* and src-tauri
crates/sevak-core       config, fuzzy matcher, search engine, usage stats, Plugin trait
crates/sevak-platform   OS access: launching, icons, app scanning, clipboard, paths,
                        hotkey strategy, GNOME shortcut setup (Windows and Linux backends)
crates/sevak-plugins    built-in plugins: apps, calculator, files, web search, uuid example
src-tauri               the Tauri shell: window, hotkey, tray, CLI, IPC commands, bundling config
ui                      Svelte 5 + Vite frontend (builds to ui/dist)
packaging/linux         desktop-entry template used by the .deb and .rpm
scripts                 icon generator, WSL Linux test runner
docs                    plugins, install, development
.github/workflows       ci.yml, release.yml
```

## Architecture

```
 ui (Svelte)  <--- Tauri IPC --->  src-tauri (shell)
                                      |  owns window, tray, hotkey, CLI, single-instance
                                      v
                               sevak-core::SearchEngine
                                      |  routes a query to plugins, ranks, applies usage
                                      v
                               sevak-plugins (Plugin impls)
                                      |  execute -> actions
                                      v
                               sevak-platform::PlatformProvider
                                      Windows / Linux implementations
```

`sevak-core` and `sevak-plugins` are platform independent; only
`sevak-platform` has `cfg(windows)` / `cfg(target_os = "linux")` code. How a
query flows through plugins and how to add one is in [plugins.md](plugins.md).

## Running

Prerequisites are in the [README](../README.md#build-from-source).

```sh
npm ci
npm run tauri dev        # shell + Vite dev server (hot reload on the UI)
npm run check            # svelte-check
npm run build            # build the UI into ui/dist
npm run icons            # regenerate src-tauri/icons (scripts/generate-icons.mjs)
```

A plain `cargo build --release -p sevak` needs `--features custom-protocol` to
embed `ui/dist`; `tauri build` enables it for you.

## Tests and lint

CI runs exactly these (on Windows and Ubuntu 22.04, plus a Fedora container):

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm run check
```

### Linux code from a Windows machine

Type-check the Linux backend without a Linux toolchain:

```sh
rustup target add x86_64-unknown-linux-gnu
cargo clippy -p sevak-platform --target x86_64-unknown-linux-gnu
```

Run the Linux tests inside WSL (no sudo; bootstraps Rust and Zig as a linker
under `$HOME`):

```sh
wsl -d Ubuntu -- bash /mnt/d/PProjects/Sevak/scripts/wsl-linux-test.sh
wsl -d Ubuntu -- bash /mnt/d/PProjects/Sevak/scripts/wsl-linux-test.sh clippy -p sevak-platform --all-targets
```

See the header of `scripts/wsl-linux-test.sh` for options. The Tauri shell
itself (GTK/WebKit) cannot be built that way; CI covers it.

## Packaging

Bundle settings are the `bundle` object in `src-tauri/tauri.conf.json`:

- `targets` lists `nsis`, `msi`, `deb`, `rpm` and `appimage`. Each can only be
  built on its own OS, so always choose per OS with `--bundles`:
  - Windows: `npx tauri build --bundles nsis,msi`
  - Linux: `npx tauri build --bundles deb,rpm,appimage`
- Output goes to `target/release/bundle/<format>/`.
- Windows: NSIS installs per user (no admin). The first build downloads the NSIS
  and WiX toolsets from their official GitHub releases. The MSI `upgradeCode`
  must never change.
- Linux: the `.desktop` file comes from `packaging/linux/sevak.desktop`
  (Categories, Keywords and a "Show or hide" action). Extra runtime dependencies
  (`libayatana-appindicator`, glib tools) are declared in `bundle.linux.deb.depends`
  and `bundle.linux.rpm.depends`.
- Installers are not code-signed yet.

## CI

`.github/workflows/ci.yml` runs on pushes and pull requests:

1. `lint-test`: fmt, clippy, tests and the UI checks on `windows-latest` and `ubuntu-22.04`.
2. `fedora`: builds and tests in a `fedora` container to prove the Fedora
   toolchain and libraries work. Bump the image tag when the release goes EOL.
3. `bundle` (after `lint-test`): builds the installers (Windows: nsis+msi;
   ubuntu-22.04: deb+rpm+appimage) and uploads them as workflow artifacts.
   Ubuntu 22.04 is the oldest supported base, so the `.deb` and AppImage run on 22.04+.

## Releasing

1. Update the version in `Cargo.toml` (`[workspace.package]`),
   `src-tauri/tauri.conf.json` and `package.json`; refresh `Cargo.lock`
   (`cargo check`).
2. Commit and merge to `main`.
3. Tag and push: `git tag vX.Y.Z && git push origin vX.Y.Z`.
4. `.github/workflows/release.yml` builds all bundles on Windows and Ubuntu 22.04
   via `tauri-apps/tauri-action` and creates a **draft** GitHub Release (it
   fails early if the tag does not match the version in `tauri.conf.json`).
5. Check the draft's assets, edit the notes, then publish.
