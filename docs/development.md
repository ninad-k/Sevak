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
- `bundle.resources` installs `LICENSE` and `THIRD_PARTY_NOTICES.md` with the
  app. `THIRD_PARTY_NOTICES.md` is generated: run
  `node scripts/generate-third-party-notices.mjs` after any dependency change
  (see [Supply chain](security/supply-chain.md#regenerating-the-third-party-notices)).
- After a release is published, `.github/workflows/sbom.yml` attaches the
  CycloneDX SBOMs and `.github/workflows/attest.yml` creates the build-provenance
  attestations (see [Supply chain](security/supply-chain.md)).
- `crates/sevak-plugins/data/wordnet-en.z` is the bundled dictionary (about 2.7
  MB), generated from Princeton WordNet 3.0 by `scripts/build-dictionary.py`
  (`python scripts/build-dictionary.py <path to WordNet-3.0>`, Python 3, no
  dependencies). Regenerate it only to change what is kept; its licence notice is
  inside the file and in `THIRD_PARTY_NOTICES.md`.

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
4. `msrv`: `cargo check --workspace --locked` on the `rust-version` declared in
   `Cargo.toml`, so the stated minimum is really buildable.

The Rust toolchain every job uses comes from `rust-toolchain.toml` (see below).
The security, CodeQL and canary workflows are described in the next section.

## Security and supply chain

| Workflow | Runs | What it does |
| --- | --- | --- |
| `security.yml` | PRs that touch a manifest or lockfile, pushes to `main`, weekly | `cargo deny` (advisories; licences, bans and sources) and `npm audit --omit=dev --audit-level=high`. The weekly run also writes a `cargo audit` report and keeps one `security` issue open while something fails. |
| `codeql.yml` | PRs to `main`, pushes to `main`, weekly | CodeQL for `javascript-typescript` and for the `actions` workflows. Rust is not scanned by CodeQL; clippy (`-D warnings`) and cargo-deny cover it. |
| `toolchain-canary.yml` | weekly, manually | clippy and tests on the newest `stable` and `beta` Rust (Linux only). A failure opens one `toolchain` issue, "New Rust release breaks the build". |

### Dependency scanning locally

```sh
cargo install --locked cargo-deny cargo-audit

cargo deny check                       # advisories, licenses, bans, sources (deny.toml)
cargo deny check licenses              # or a single check
cargo audit                            # wider RustSec report, including unmaintained crates
npm audit --omit=dev --audit-level=high
```

`deny.toml` is the policy: crates.io only, permissive licences only (MIT,
Apache-2.0, BSD, ISC, Zlib, Unicode and similar; MPL-2.0 per crate), duplicate
crate versions warn. Advisories fail the check because cargo-deny cannot filter
by severity; yanked crates and unmaintained *transitive* crates only warn (the
latter appear in `cargo audit`). The advisory database is fetched on every run,
so a clean result today can fail next week without any change here; that is what
the weekly run is for.

### Handling a finding

1. Prefer fixing it: `cargo update -p <crate>` (or bump the requirement), then
   re-run `cargo deny check` and the tests.
2. If no fix exists yet, add a time-boxed exception to `deny.toml`:

   ```toml
   [advisories]
   ignore = [
       { id = "RUSTSEC-2099-0001", reason = "only reachable via X, which Sevak never calls; fix tracked in #123; review-by: 2027-01-31" },
   ]
   ```

   Always give a reason and a `review-by: YYYY-MM-DD` date (at most three months
   out): the Security workflow fails once that date has passed, so the exception
   gets reviewed instead of forgotten. Remove it once the fix lands
   (`unused-ignored-advisory` warns when it is no longer needed).
3. A new licence goes in `[licenses] allow` only if it is permissive and
   compatible with Apache-2.0, with a comment naming the crate. Copyleft that is
   file-level only (MPL-2.0) is allowed per crate under `[licenses] exceptions`;
   anything stronger (GPL, AGPL, LGPL-only) needs a maintainer decision, and the
   dependency is usually better replaced.
4. A new `THIRD_PARTY_NOTICES.md` entry may be needed when a dependency with
   a notice requirement is added.

Known, accepted findings (no exception needed, because cargo-deny only gates
direct dependencies for these classes): `cargo audit` reports
RUSTSEC-2024-0370 (`proc-macro-error`, unmaintained) and RUSTSEC-2024-0429
(`glib` 0.18, unsound `VariantStrIter` iterators). Both come from the GTK3
bindings that Tauri 2 and wry use on Linux only, Sevak does not call the
affected API, and the fix is Tauri moving to newer bindings, so they clear
with a Tauri update.

Dependabot (`.github/dependabot.yml`) opens one grouped pull request per month
for minor and patch updates of Cargo, npm and GitHub Actions, and one pull
request per major update. Third-party GitHub Actions are pinned to a commit SHA
(`uses: owner/repo@<sha> # v1.2.3`; Dependabot keeps both in step). Actions owned
by GitHub (`actions/*`, `github/*`) may use version tags.

### The pinned toolchain

`rust-toolchain.toml` pins an exact Rust version (currently 1.99.0) with the
`clippy` and `rustfmt` components. It is the single source of truth: rustup
picks it up in any checkout, and CI installs it through the
`.github/actions/setup-rust` composite action (which runs `rustup toolchain
install` against that file). A new Rust release therefore never changes CI, or a
release build, by itself; new lints cannot block an unrelated pull request.

To bump it, change `channel` in `rust-toolchain.toml` in a pull request of its
own (Dependabot opens one monthly), fix any new clippy findings, and run the
checks in "Tests and lint". Nothing else needs editing. The `rust-version` in
`Cargo.toml` is the separate, lower minimum supported Rust version; the `msrv` CI
job checks it. Raise it only when the code or a dependency really needs a newer
compiler.

The canary workflow tells you in advance when a bump will need work: if the
latest stable or beta fails clippy or tests, it files (or updates) the
"New Rust release breaks the build" issue, and closes it again when they pass.

### Repository settings (maintainers)

Workflows cannot change repository settings. Enable these under the repository's
Settings > Advanced Security (Code security): Dependency graph, Dependabot
alerts, and Dependabot security updates (without the last one, only the monthly
version PRs arrive). For CodeQL, leave "Code scanning" on "Advanced" or off, not
"Default setup", or GitHub rejects the results from `codeql.yml`. Do not make the
`Security` workflow's jobs required status checks of `main`: they run only when
a manifest or lockfile changes, and a required check that never starts leaves the
pull request waiting forever.

## Releasing

The release flow, the pre-release checklist, the opt-in staged flow (beta, then
promote), rollback and hotfixes are in [Releasing](releasing.md). What follows is how
the default pipeline is built.

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
(`plugins.updater` in `tauri.conf.json`; code in `src-tauri/src/updater.rs`; copies
on the beta channel also read `latest-beta.json`, see [Releasing](releasing.md)) at
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
