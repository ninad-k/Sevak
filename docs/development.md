# Developing Sevak

## Layout

```
Cargo.toml              workspace: crates/* and src-tauri
crates/sevak-core       config, fuzzy matcher, search engine, usage stats, Plugin trait
crates/sevak-platform   OS access: launching, icons, app scanning, clipboard, paths,
                        hotkey strategy, GNOME shortcut setup (Windows, macOS and Linux backends)
crates/sevak-plugins    built-in plugins: apps, calculator, files, bookmarks, web search, uuid example
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

- `targets` is `all`, meaning every format the current OS can build. To pick:
  - Windows: `npx tauri build --bundles nsis,msi`
  - macOS: `npx tauri build --bundles app,dmg` (add
    `--target universal-apple-darwin` for an Apple silicon + Intel build, after
    `rustup target add aarch64-apple-darwin x86_64-apple-darwin`)
  - Linux: `npx tauri build --bundles deb,rpm,appimage`
- Output goes to `target/release/bundle/<format>/`.
- Windows: NSIS installs per user (no admin). The first build downloads the NSIS
  and WiX toolsets from their official GitHub releases. The MSI `upgradeCode`
  must never change.
- Linux: the `.desktop` file comes from `packaging/linux/sevak.desktop`
  (Categories, Keywords and a "Show or hide" action). Extra runtime dependencies
  (`libayatana-appindicator`, glib tools) are declared in `bundle.linux.deb.depends`
  and `bundle.linux.rpm.depends`.
- macOS: the app runs as a menu-bar (accessory) app with no Dock icon. It is
  ad-hoc signed (`bundle.macOS.signingIdentity: "-"`), not notarized.
  `app.macOSPrivateApi` (and Tauri's `macos-private-api` feature) give the
  launcher its transparent window.
- Installers are not code-signed yet.

## CI

`.github/workflows/ci.yml` runs on pushes and pull requests:

1. `lint-test`: fmt, clippy, tests and the UI checks on `windows-latest`,
   `macos-latest` and `ubuntu-22.04`.
2. `fedora`: builds and tests in a `fedora` container to prove the Fedora
   toolchain and libraries work. Bump the image tag when the release goes EOL.
3. `bundle` (after `lint-test`): builds the installers (Windows: nsis+msi;
   macOS: app+dmg; ubuntu-22.04: deb+rpm+appimage) and uploads them as
   workflow artifacts.
   Ubuntu 22.04 is the oldest supported base, so the `.deb` and AppImage run on 22.04+.

## Releasing

Releases are automatic: every push to `main` that changes more than docs
(in practice, every merged pull request) publishes a new version.
`.github/workflows/release.yml`:

1. **plan** computes the next version with `scripts/release-version.mjs next`
   from the latest `vX.Y.Z` tag and the [Conventional Commit](https://www.conventionalcommits.org)
   subjects since it, and opens a draft GitHub Release with generated notes:

   | Commits since the last tag | Bump |
   |---|---|
   | any `feat!:` / `fix!:` or a `BREAKING CHANGE:` footer | major (minor while on 0.x) |
   | any `feat:` / `feat(scope):` | minor |
   | anything else | patch |

2. **build** stamps that version into `Cargo.toml`, `tauri.conf.json`,
   `package.json` and `package-lock.json` (`release-version.mjs set`, in the
   runner only; nothing is committed), then builds and uploads the installers:
   Windows (nsis, msi), macOS universal (app, dmg) and Linux (deb, rpm, AppImage).
   `src-tauri/tauri.release.conf.json` turns on `createUpdaterArtifacts`, so each
   installer is signed for the updater (`.sig` next to it).
3. **publish** writes `latest.json` from those signatures
   (`scripts/updater-manifest.mjs`), adds `SHA256SUMS.txt` and publishes the
   release, which creates the `vX.Y.Z` tag on the released commit.

### Auto-update and the signing key

Installed copies poll
`https://github.com/ninad-k/Sevak/releases/latest/download/latest.json`
(`plugins.updater` in `tauri.conf.json`; code in `src-tauri/src/updater.rs`) at
startup and daily, and from the tray's "Check for updates". They install an
update only after the user agrees, and only if its signature matches the
`pubkey` in `tauri.conf.json`.

The matching private key is the repository secret `TAURI_SIGNING_PRIVATE_KEY`
(plus `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` if the key has a password). The
release workflow stops early if it is missing. **Keep an offline backup of the
private key:** if it is lost, existing installs can no longer verify updates
and users must reinstall by hand after you rotate keys. To rotate, generate a
pair with `npx tauri signer generate -w ~/.tauri/sevak-updater.key`, put the
new public key in `tauri.conf.json` and the private key in the secret; the
release that ships the new public key must still be signed with the old key.

Local and CI builds don't sign (no `--config src-tauri/tauri.release.conf.json`),
so they don't need the key. Debug builds never check for updates automatically.

If a build fails, nothing is tagged; the draft is replaced on the next run.
The version in the repository files is only the floor for the first release
and for forced bumps: to jump to a specific version (say `1.0.0`), set it in
`src-tauri/tauri.conf.json` and `Cargo.toml` in a PR. To merge without
releasing, put `[skip release]` in the merge commit message; pushes that only
touch Markdown, `docs/` or `LICENSE` never release.

Preview the next version locally with `node scripts/release-version.mjs next`.
