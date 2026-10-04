# Developing Sevak

## Layout

```
Cargo.toml              workspace: crates/* and src-tauri
crates/sevak-core       config, theme (appearance settings to CSS), theme files and gallery checks, fuzzy matcher, search engine,
                        usage stats, Plugin trait
crates/sevak-platform   OS access: launching, icons, app scanning, clipboard, pasting into the
                        previous app, capturing the selection (Universal Actions), moving to the trash,
                        whole-disk search through the OS file index (os_search), watching typed
                        keywords and replacing them (snippet expansion), paths, terminal
                        launching, hotkey strategy, GNOME
                        shortcut setup, the Windows keyboard hook that takes Win+Space
                        (hotkey_hook), the macOS Spotlight shortcut (spotlight)
                        (Windows, macOS and Linux backends)
crates/sevak-plugins    built-in plugins: apps, calculator (+ units, currency), files, bookmarks,
                        web search, system commands, automation tasks, media controls, shell,
                        clipboard history, snippets, emoji picker (data/emoji.tsv), uuid example;
                        the file buffer (collect files, act on all); the script plugin host
                        (external plugins); workflows (graph engine, runtime, gallery)
src-tauri               the Tauri shell: window, hotkeys (main, Universal Actions + [[hotkey]] entries,
                        takeover of Win+Space / Cmd+Space / Super+Space), tray, CLI,
                        --query / --run handling (direct.rs), script plugin approval, IPC
                        commands, bundling config
ui                      Svelte 5 + Vite frontend (builds to ui/dist)
packaging/linux         desktop-entry template used by the .deb and .rpm
scripts                 icon generator, emoji list generator, WSL Linux test runner
docs                    plugins, install, development
examples/plugins        example script plugins (Python, PowerShell, Node)
examples/workflows      example workflows (also packaged for the gallery)
gallery                 the opt-in online galleries: index.json and packages/*.zip (workflows and
                        script plugins), themes.json and themes/*.toml (themes)
.github/workflows       ci.yml, release.yml, coverage.yml, bench.yml, docs.yml
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
                                      Windows / macOS / Linux implementations
```

`sevak-core` and `sevak-plugins` are platform independent; only
`sevak-platform` contains the OS-specific backends. How a
query flows through plugins and how to add one is in [plugins.md](plugins.md).

## Running

Use **Rust 1.90+**, **Node.js 22+**, and the native dependencies for your platform:

| Platform | Native prerequisites |
|---|---|
| Windows | Microsoft C++ Build Tools with the Desktop development with C++ workload; WebView2 runtime |
| macOS | Xcode Command Line Tools (`xcode-select --install`) |
| Ubuntu / Debian | WebKitGTK 4.1, AppIndicator and the development packages below |
| Fedora | WebKitGTK 4.1, AppIndicator and the development packages below |

The Linux commands mirror this repository's CI setup. On Ubuntu / Debian:

```sh
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  build-essential curl file patchelf \
  libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev \
  libxdo-dev libssl-dev
```

On Fedora:

```sh
sudo dnf install -y \
  git tar gzip xz curl file gcc gcc-c++ make patchelf pkgconf-pkg-config \
  webkit2gtk4.1-devel libayatana-appindicator-gtk3-devel \
  openssl-devel librsvg2-devel libxdo-devel
```

See [Tauri's prerequisites](https://v2.tauri.app/start/prerequisites/) for
platform setup details. Then run from the repository root:

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

CI runs these checks on Windows, macOS and Ubuntu 22.04. A Fedora container
also builds and tests the Rust workspace:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm run check
npm run build
```

Further commands (what each layer covers, and the manual release checklist, are in
[Testing](testing.md)):

```sh
npm test                          # UI unit tests (Vitest); npm run test:watch to keep them running
npm run test:coverage             # the same with a coverage report in ui/coverage/
cargo test --profile fast-release -p sevak-plugins --test latency -- --nocapture
                                  # search latency budget (skips itself in a debug build)
cargo bench -p sevak-plugins      # search latency and startup benchmark (criterion)
```

Coverage of the Rust crates uses `cargo-llvm-cov` (`cargo install --locked
cargo-llvm-cov`, `rustup component add llvm-tools-preview`); the exact command and
the current numbers are in [Testing](testing.md#coverage). CI runs `npm test` and the
latency budget on Ubuntu on every push, and the `Coverage` workflow (pull requests,
weekly) and `Benchmark` workflow (weekly, manual) publish their tables in the job
summary. Neither gates a merge.

### Linux code from a Windows machine

Type-check the Linux backend without a Linux toolchain:

```sh
rustup target add x86_64-unknown-linux-gnu
cargo clippy -p sevak-platform --target x86_64-unknown-linux-gnu
```

The macOS backend can be type-checked the same way (the crates it uses are pure
Rust, so no Apple SDK is needed to *check*, only to build and run):

```sh
rustup target add aarch64-apple-darwin
cargo clippy -p sevak-platform --target aarch64-apple-darwin
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
- macOS: `src-tauri/Info.plist` is merged into the app's `Info.plist` by Tauri. It
  holds `NSContactsUsageDescription`; macOS ends an app that reads Contacts
  without it, so keep it while the contacts plugin uses the Contacts framework.
- Installers are not code-signed yet.
- `crates/sevak-plugins/data/wordnet-en.z` is the bundled dictionary (about 2.7
  MB), generated from Princeton WordNet 3.0 by `scripts/build-dictionary.py`
  (`python scripts/build-dictionary.py <path to WordNet-3.0>`, Python 3, no
  dependencies). Regenerate it only to change what is kept; its licence notice is
  inside the file and in `THIRD_PARTY_NOTICES.md`.

## CI

`.github/workflows/ci.yml` runs on pushes and pull requests:

1. `lint-test`: fmt, clippy, tests and the UI checks on `windows-latest`,
   `macos-latest` and `ubuntu-22.04`.
2. `ui-tests` and `latency-budget` (Ubuntu): the Vitest suite, and the search
   latency budget in an optimized build.
3. `fedora`: builds and tests in a `fedora` container to prove the Fedora
   toolchain and libraries work. Bump the image tag when the release goes EOL.
4. `bundle` (after `lint-test`): builds the installers (Windows: nsis+msi;
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

### Package managers

After publishing, the `distribute` job renders the manifests in `packaging/`
for the new version (`scripts/package-manifests.mjs`, which fills in versions,
file names and SHA-256 hashes from `SHA256SUMS.txt`) and publishes them. Each
channel runs only when its secret is set, and a failure there never fails the
release:

| Channel | Where | Secret |
|---|---|---|
| Homebrew | `Casks/sevak.rb` in [ninad-k/homebrew-tap](https://github.com/ninad-k/homebrew-tap) | `PACKAGING_TOKEN` |
| Scoop | `bucket/sevak.json` in [ninad-k/scoop-bucket](https://github.com/ninad-k/scoop-bucket) | `PACKAGING_TOKEN` |
| AUR | [`sevak-bin`](https://aur.archlinux.org/packages/sevak-bin), from `packaging/aur/PKGBUILD` | `AUR_SSH_PRIVATE_KEY` |
| winget | a pull request to [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) for `NinadKulkarni.Sevak` | `WINGET_TOKEN` |

- `PACKAGING_TOKEN`: a fine-grained personal access token with **Contents:
  read and write** on `ninad-k/homebrew-tap` and `ninad-k/scoop-bucket` only.
- `AUR_SSH_PRIVATE_KEY`: the private half of an SSH key whose public half is
  added to the AUR account that owns `sevak-bin`.
- `WINGET_TOKEN`: a classic personal access token with the `public_repo` scope,
  from an account with a fork of `microsoft/winget-pkgs`. winget only accepts
  automated updates for packages that already exist, so the first version is
  submitted once by hand from the rendered `packaging/winget` manifests.

Scoop and AUR packages write a `package-manager` marker (next to `sevak.exe`,
or `/usr/share/sevak/package-manager`) that turns Sevak's self-update off; see
`ManagedBy` in `src-tauri/src/updater.rs`. Preview the manifests for any
release with `node scripts/package-manifests.mjs <version> SHA256SUMS.txt out`.

### Auto-update and the signing key

Installed copies poll
`https://github.com/ninad-k/Sevak/releases/latest/download/latest.json`
(`plugins.updater` in `tauri.conf.json`; code in `src-tauri/src/updater.rs`) at
startup, every six hours and when the launcher opens, and from the tray's "Check for updates". They install an
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

## Documentation site

The user and developer guide is built with MkDocs Material and published to GitHub Pages.

**Editing:**

- Documentation source: `docs/` (Markdown files)
- Site configuration: `mkdocs.yml`
- Extra CSS: `docs/stylesheets/extra.css`
- Media (screenshots, images): `docs/media/`

**Preview locally:**

```sh
pip install -r docs/requirements.txt
mkdocs serve
# Open http://127.0.0.1:8000 in your browser
```

**Strict build (no warnings):**

```sh
mkdocs build --strict -f mkdocs.yml
```

**PDF manual:**

The documentation is also exported to PDF (`Sevak-User-Guide.pdf`) for download:

```sh
mkdocs build                    # Generates the site to site/
npm run docs:pdf               # Builds the PDF (see scripts/docs-pdf.mjs)
# Output: docs/pdf/Sevak-User-Guide.pdf
```

The PDF script requires Chrome or Chromium; set `CHROME_PATH` if it's not in the default location.

**Publishing:**

The site is built and deployed to [GitHub Pages](https://ninad-k.github.io/Sevak/docs/) by `.github/workflows/docs.yml` on every push to `main`. The same workflow publishes the product page from `landing/` at the site root, [ninad-k.github.io/Sevak](https://ninad-k.github.io/Sevak/), with the documentation under `/docs/`.

**Linux testing in WSL:**

For contributors on Windows, test the Linux code without a Linux machine:

```sh
# From the Windows project root, type these in PowerShell:
# The scripts bootstrap Rust and Zig inside WSL, no sudo needed.

wsl -d Ubuntu -- bash /mnt/d/PProjects/Sevak/scripts/wsl-setup.sh    # First-time setup
wsl -d Ubuntu -- bash /mnt/d/PProjects/Sevak/scripts/wsl-dev.sh      # Build and test

# Or run specific cargo commands:
wsl -d Ubuntu -- bash /mnt/d/PProjects/Sevak/scripts/wsl-linux-test.sh test -p sevak-platform
wsl -d Ubuntu -- bash /mnt/d/PProjects/Sevak/scripts/wsl-linux-test.sh clippy -p sevak-platform --all-targets
```

See the header of `scripts/wsl-linux-test.sh` for more options.
