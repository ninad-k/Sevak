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
crates/sevak-backup     settings backup and restore: the .sevakbackup archive, the allowlist of what
                        may go in, validation of hostile archives, merge / replace, safety snapshot
                        and undo, automatic backups (no Tauri code; see docs/backup-and-restore.md)
src-tauri               the Tauri shell: window, hotkeys (main, Universal Actions + [[hotkey]] entries,
                        takeover of Win+Space / Cmd+Space / Super+Space), tray, CLI,
                        --query / --run handling (direct.rs), script plugin approval, IPC
                        commands, bundling config
ui                      Svelte 5 + Vite frontend (builds to ui/dist)
packaging/linux         desktop-entry template used by the .deb and .rpm
scripts                 icon generator, emoji list generator, gallery-check.mjs (gallery hashes), WSL Linux test runner
docs                    plugins, install, development
examples/plugins        example script plugins (Python, PowerShell, Node)
examples/workflows      example workflows (also packaged for the gallery)
gallery                 the opt-in online galleries: index.json and packages/*.zip (workflows and
                        script plugins), themes.json and themes/*.toml (themes)
.github/workflows       ci.yml, release.yml, promote.yml, rollback.yml, sbom.yml, attest.yml, security.yml,
                        codeql.yml, toolchain-canary.yml, coverage.yml, bench.yml, docs.yml, release-checks.yml,
                        pr-labels.yml (.github/actions/setup-rust installs the toolchain pinned in rust-toolchain.toml)
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
npm run test:scripts     # checks on the Windows installer sources and package manifests
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
npm run gallery:check                                  # gallery checksums, themes and tags
npm run build
npm run test:scripts
node scripts/generate-third-party-notices.mjs --check  # THIRD_PARTY_NOTICES.md is current (Linux job)
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
- Windows: the NSIS installer is Sevak's own (a customised copy of Tauri's
  template: it installs per user without administrator rights, or per PC after a
  UAC prompt; see [The Windows installer](#the-windows-installer)). The first
  build downloads the NSIS and WiX toolsets from their official GitHub releases.
  The MSI `upgradeCode` must never change.
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
- Once a stable release is published, `.github/workflows/sbom.yml` attaches the
  CycloneDX SBOMs and `.github/workflows/attest.yml` creates the build-provenance
  attestations. `release.yml` and `promote.yml` call them (a release published
  with `GITHUB_TOKEN` does not trigger them by itself); see
  [Supply chain](security/supply-chain.md).
- `crates/sevak-plugins/data/wordnet-en.z` is the bundled dictionary (about 2.7
  MB), generated from Princeton WordNet 3.0 by `scripts/build-dictionary.py`
  (`python scripts/build-dictionary.py <path to WordNet-3.0>`, Python 3, no
  dependencies). Regenerate it only to change what is kept; its licence notice is
  inside the file and in `THIRD_PARTY_NOTICES.md`.

### The Windows installer

Everything about the Windows installers is in `src-tauri/installer/`, wired up in
`bundle.windows` of `src-tauri/tauri.conf.json`:

| File | What it is |
|---|---|
| `installer.nsi` | Sevak's copy of Tauri's NSIS template (`nsis.template`): pages, install scope, upgrades, uninstall |
| `upstream/installer.nsi` | The unmodified Tauri template it was copied from, kept to diff and merge against |
| `English.nsh` | Every installer string (`nsis.customLanguageFiles`); the only language, no selector |
| `upstream/English.nsh` | The unmodified Tauri strings, same purpose |
| `hooks.nsh` | `NSIS_HOOK_PREINSTALL` and `NSIS_HOOK_PREUNINSTALL` (`nsis.installerHooks`): ask a running Sevak to quit before files change |
| `sidebar.bmp`, `header.bmp` | NSIS Welcome/Finish artwork (164x314) and page header (150x57) |
| `wix-banner.bmp`, `wix-dialog.bmp` | The same artwork for the MSI (493x58, 493x312) |

**Why a template copy.** Tauri's `installMode: "both"` runs the installer
elevated (`RequestExecutionLevel highest`), so administrators get a UAC prompt
even for a per-user install, silent installs and the in-app updater default to
per-machine for them, and the install-mode page is not shown to anyone else.
Sevak wants per-user to need no administrator ever, silent installs and updates
to stay in the scope they are in, and elevation only when "all users" is chosen.
No option of Tauri's NSIS config does that. The branding itself (images, icon,
text) uses only config options, the language file and the hooks.

**How it behaves** (what users see is in [Installing Sevak](install.md#windows-10-11)):

- The installer runs as the invoking user. The scope comes from `/ALLUSERS` or
  `/CURRENTUSER`, otherwise from where Sevak is already installed
  (`HKCU` and `HKLM` `Software\Microsoft\Windows\CurrentVersion\Uninstall\Sevak`),
  otherwise per user. The in-app updater passes neither switch (`/P /UPDATE /R`),
  which is why an update stays where it is.
- Per-machine needs elevation. A wizard run asks on the scope page and then starts
  itself again with `ShellExecute runas` (`RelaunchElevated`; the new copy carries
  `/ELEVATED` and skips the pages already answered). Silent, passive and
  `/ALLUSERS` runs elevate at start-up. A refused prompt exits with 1223.
- The uninstaller does the same, since `uninstall.exe` has no administrator
  manifest: for a per-machine copy, `un.onInit` starts it again elevated and quits.
  Both register the scope in `UninstallString`
  (`"...\uninstall.exe" /ALLUSERS` or `/CURRENTUSER`).
- Nothing is uninstalled before an upgrade; the new files go over the old ones. A
  copy in the other scope is removed only when asked (the page, or
  `/UNINSTALLOTHER`), by running its uninstaller with `/S /MOVE`; `/MOVE` keeps the
  autostart entry and all data while removing shortcuts. An earlier MSI install is
  removed first, as in Tauri's template.
- Switch names must not start with another switch's name: Tauri finds `/R` and `/P`
  with a substring search.
- Custom pages never show in silent or passive mode; their decisions are switches.

**Regenerating the images.** `scripts/generate-installer-images.mjs` draws the
four BMPs from `assets/sevak-icon.png` (the flame is cut out of it by its red
channel, so the installer shows the real icon pixels). CI does not run it; the BMPs
are committed. It needs `@napi-rs/canvas`, which is optional like the
documentation media tooling (`docs/media/README.md`), and Segoe UI, so run it on
Windows:

```sh
npm install --no-save --package-lock=false @napi-rs/canvas
node scripts/generate-installer-images.mjs --preview   # PNG copies in target/installer-preview/
```

The images use the app's palette (`#2b2870` to `#17152b` indigo, `#f59e0b` amber, and
`#fcfcfd`/`#1c1b2e` for the light header and pages: `MUI_BGCOLOR` and
`MUI_TEXTCOLOR` in `installer.nsi`). Pages stay light on purpose: NSIS cannot
recolour the text of its check boxes, so a dark page would make them unreadable.
Edit the layout in the script. NSIS and WiX only take uncompressed 24-bit BMP, which
`scripts/bmp.mjs` writes.

**After a Tauri upgrade, re-sync the template.** `installer.nsi` and `English.nsh`
are copies, so new Tauri behaviour (and fixes) reach Sevak only by merging them.
`utils.nsh` and `FileAssociation.nsh` are not copies: Tauri writes its own next to
the template at build time, so the macros the template calls (`CheckIfAppIsRunning`
and friends) are always the current ones.

1. Find the bundler's version: Tauri's `Cargo.toml` at the tag `tauri-cli-v<CLI version>`
   (`npx tauri --version`) names `tauri-bundler`. The files are in
   `crates/tauri-bundler/src/bundle/windows/nsis/` at that tag.
2. Merge the upstream change into Sevak's copy (the pristine copy is the common
   ancestor):

   ```sh
   V=2.13.0   # the new @tauri-apps/cli version
   B=https://raw.githubusercontent.com/tauri-apps/tauri/tauri-cli-v$V/crates/tauri-bundler/src/bundle/windows/nsis
   curl -fsSL $B/installer.nsi -o new-installer.nsi
   curl -fsSL $B/languages/English.nsh -o new-English.nsh
   git merge-file src-tauri/installer/installer.nsi src-tauri/installer/upstream/installer.nsi new-installer.nsi
   git merge-file src-tauri/installer/English.nsh src-tauri/installer/upstream/English.nsh new-English.nsh
   mv new-installer.nsi src-tauri/installer/upstream/installer.nsi
   mv new-English.nsh src-tauri/installer/upstream/English.nsh
   ```

   Resolve any conflict markers. Every Sevak change sits between `SEVAK:` comments
   (`diff src-tauri/installer/upstream/installer.nsi src-tauri/installer/installer.nsi`
   lists them all). Update the version in the header comment of `installer.nsi`.
3. Build (`npx tauri build --bundles nsis`; makensis must report no warning) and
   run `npm run test:scripts`, which checks that every string the template uses is
   defined and that every upstream string is still in `English.nsh`.
4. Test by hand, since CI cannot run an installer, on a machine that is not your
   daily one (or in a VM): a fresh per-user install; a fresh all-users install (UAC);
   upgrading a per-user and an all-users install with Sevak running, from the wizard
   and with `/S`; the in-app update path (`/P /UPDATE /R`); per-user over all-users
   and the reverse (the move page, and `/UNINSTALLOTHER`); uninstalling from
   Settings > Apps for both scopes; silent `/S` and `/S /ALLUSERS`.

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
