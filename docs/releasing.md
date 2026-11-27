# Releasing

How Sevak versions are published, how to publish them more carefully, and how to
take one back. For the build, signing key and package-manager secrets see
[Development](development.md#releasing); this page is the maintainer's runbook.

## The two flows

| | **Auto** (default) | **Staged** (opt-in) |
|---|---|---|
| A merge to `main` publishes | a **stable** release `vX.Y.Z` | a **beta** pre-release `vX.Y.Z-beta.N` |
| `latest.json` (stable users) | updated at once | untouched until you promote |
| `latest-beta.json` (beta users) | not touched | updated |
| winget, Scoop, Homebrew, AUR | updated | updated only when you promote |
| Stable users get it | within about a day | after you run **Promote** |

Nothing changes until you switch: with no setup, every merge publishes a stable
release exactly as before.

### Auto flow

`.github/workflows/release.yml` runs on every push to `main` that changes more
than docs: it works out the version, builds on Windows, macOS and Linux, signs the
installers for the updater, writes `latest.json` and `SHA256SUMS.txt`, publishes
the release and then updates the package managers. It also calls
`sbom.yml` and `attest.yml`, which attach the SBOMs and the build-provenance
attestations to the published release. Details are in
[Development](development.md#releasing) and
[Supply chain](security/supply-chain.md).

### Staged flow

1. **Switch it on** (once): set the repository variable `RELEASE_MODE` to `staged`
   (Settings, Secrets and variables, Actions, Variables, or
   `gh variable set RELEASE_MODE --body staged`). Setting it to `auto`, or deleting
   it, goes back to the default. Any other value is ignored with a warning and
   releases stable.
2. **Merges publish betas.** The same workflow now publishes `vX.Y.Z-beta.N` as a
   GitHub **pre-release** with `latest-beta.json` and `SHA256SUMS.txt`. `X.Y.Z` is
   the version the next stable release would get; `N` counts up from 1 for that
   `X.Y.Z`. People on the beta channel (Settings, General, Update channel) are
   offered it. Stable users, `latest.json` and the package managers are not touched.
3. **Soak it.** Install the beta yourself, run the [checklist](#before-promoting-a-beta),
   and give beta users some time. A sensible default is two days with no regression
   reports.
4. **Promote.** Actions, **Promote**, enter the beta tag (for example
   `v1.3.0-beta.2`). The workflow downloads the release's assets, verifies
   `SHA256SUMS.txt` and the signatures, writes `latest.json` from them, makes the
   release the newest stable one, updates the package managers and attaches the
   SBOMs and attestations (betas get neither before this). **Nothing is rebuilt**: the
   files stable users get are the files beta users tested.

You can also cut a beta without switching modes: Actions, **Release**, Run workflow,
`channel` = `beta` (builds from `main`), or `channel` = `stable` to cut a stable
release while `RELEASE_MODE` is `staged`. `channel` = `default` follows the variable.

!!! note "A promoted build keeps its beta version"
    The installers are not rebuilt, so the version inside them stays
    `1.3.0-beta.2` and stable users see that string in "About" and in update
    prompts. It is a valid, higher version than any earlier stable (`1.2.9` <
    `1.3.0-beta.2`), and the next real `1.3.0` or `1.3.1` is higher again, so
    updates keep working. If you want a plain `1.3.0` instead, run **Release** with
    `channel` = `stable` on the same commit: it rebuilds and ships `v1.3.0` through
    the normal path, at the cost of shipping bits that were not the ones tested.

### How the beta channel is wired

GitHub's `releases/latest/download/` shortcut, which stable installs use, skips
pre-releases. So a pre-release can't be found that way. Each beta publish also
uploads `latest-beta.json` to one fixed pre-release tagged `channel-beta` (titled
"Beta channel (do not download)", created on first use). Beta installs read
`https://github.com/ninad-k/Sevak/releases/download/channel-beta/latest-beta.json`
and also the stable `latest.json`, and offer the newer of the two. That is why a
stable release that follows the last beta still reaches beta users, and why a beta
user who switches back to Stable is never moved to an older version: the updater
only offers a version higher than the one installed.

## Version numbers and commit types

The version comes from the Conventional Commit prefixes in the pull request title
(and commit messages) since the newest stable `vX.Y.Z` tag
(`scripts/release-version.mjs`):

| Prefix | Bump |
|---|---|
| `feat!:`, `fix!:` or a `BREAKING CHANGE:` footer | major (minor while on 0.x) |
| `feat:` | minor |
| `fix:`, `docs:`, `chore:`, `ci:`, `refactor:`, `test:`, `perf:` | patch |

Betas use the version the next stable release would get plus `-beta.N`. Beta tags
never move the baseline for stable versions. A pull-request title also sets a
label (`.github/workflows/pr-labels.yml`) that groups the generated release notes
(`.github/release.yml`) into Breaking changes, New features, Fixes, Performance,
Documentation and Maintenance; label a PR `skip-changelog` to leave it out.

Preview locally: `node scripts/release-version.mjs next` (stable),
`... next-beta`, `... compare 1.3.0-beta.2 1.3.0`.

### Release notes

The release starts as a draft with generated notes plus a **Known issues and
upgrade notes** section ("None known for this release."). The draft exists while the
installers build, so you can edit that section then; the workflow copies the notes
into the update manifest when it publishes. In the staged flow the beta stays a
published pre-release until you promote it: edit its notes before you run **Promote**,
which copies them into `latest.json` as the update notes (editing later changes the
release page but not what the updater shows).

**Security hardening.** The first release that contains the security hardening
changes (approvals bound to contents, scrubbed script environment, encrypted
clipboard history in the local folder, tag-pinned galleries and the rest) changes
behaviour you will want users to hear about: scripts and paste workflows ask for
approval once more, remapped shortcuts need `accept_injected_hotkeys`, snippets
stop expanding in browsers, and network paths are off. Paste the bullets of
[Hardening: release notes](security/hardening-release-notes.md) into **Known
issues and upgrade notes** for that release.

### Failed builds and unverified draft assets

Each build job uploads its installers to the **draft** release (tauri-action
builds and uploads in one step) and only then runs **Verify publisher signatures
before publication**. Verifying first would mean re-implementing tauri-action's
asset naming, which `latest.json` depends on, so the order stays and two rules
cover the gap:

- When that verification fails, the job deletes the draft, so its unsigned or
  wrongly signed files cannot be published. The next run creates a new draft.
- If a run fails or is cancelled in any other way, the draft can still hold
  some platforms' files, unverified and without `latest.json` or
  `SHA256SUMS.txt`. **Never publish a draft by hand.** Fix the cause and re-run
  Release; `plan` deletes the stale draft for that tag first.

With publisher signing not configured, the build log shows a warning
annotation ("publisher signing not configured") on Windows and macOS. That is
expected until signing is provisioned. Set `REQUIRE_WINDOWS_SIGNING` /
`REQUIRE_MACOS_SIGNING` to `true` to turn it into an error
([Publisher signing](publisher-signing.md)).

### Extension advisories

`extension-advisories.yml` (run by hand from `main`) signs the extension
revocation feed with the **updater private key**, the same key that authorizes
application updates. To keep that key contained:

- The signing job runs in the `extension-advisories` environment. **Configure
  it before the first run:** Settings, Environments, `extension-advisories`,
  add **required reviewers** (maintainers) and limit deployment branches to
  `main`. GitHub creates a missing environment without protection, so an
  unconfigured one gives no protection. Optionally store
  `TAURI_SIGNING_PRIVATE_KEY` and its password as secrets of this environment
  as well; Release still needs the repository secrets.
- The signing job has a read-only token and no dependency caches. A separate
  `publish` job, which never sees the key, verifies the feed with the app's
  parser and uploads it with `contents: write`.
- After signing, the job requires the minisign trusted comment to contain
  `file:extension-revocations.json`, so the two uses of the key stay
  distinguishable.

### Gallery index signatures

Every Release run also signs the two gallery lists that builds read at the new
tag, `gallery/index.json` and `gallery/themes.json`, and attaches
`index.json.sig` and `themes.json.sig` to the release. Sevak verifies a list
against the embedded updater key before reading it
([Gallery trust](security/gallery-trust.md#7-signed-indexes)). No setup is
needed beyond the `TAURI_SIGNING_PRIVATE_KEY` secret Release already requires.

- `sign-gallery` (read-only token, no caches, `npm ci --ignore-scripts`) copies
  each list to `target/gallery-signatures/<tag>-<file>`, signs it with
  `npx tauri signer sign`, and requires the trusted comment to name
  `<tag>-<file>`. That name binds the signature to the release and the list.
- `attach-gallery-signatures` (write token, never the key) runs
  `cargo run --locked -p sevak-plugins --example verify_gallery_index -- <tag> gallery/<file> <sig>`
  (the app's key, policy and parsers, against the files committed at the release
  commit) and uploads the two `.sig` files to the draft. **Publish** waits for
  it, so a release cannot go out with unsigned or unverifiable lists, and
  `SHA256SUMS.txt` covers the `.sig` files.
- To check a published release by hand: download `index.json.sig` from the
  release and run the same example with
  `gallery/index.json` from a checkout of the tag.

**Rollout.** `SIGNED_SINCE` in `crates/sevak-plugins/src/index_signature.rs`
(`v0.14.1`, the version after the newest release when signing was added) is the
first tag whose lists must be signed. Builds accept a list without a signature
only from older tags, and refuse an invalid signature from any tag.

- **Before merging the change that introduces signing**, make sure no release
  has been published at or after `SIGNED_SINCE` without signatures. If one has
  (another pull request merged first), raise `SIGNED_SINCE` to the version this
  merge will be released as. Otherwise builds that fall back to that unsigned
  release (development builds and betas, which read the latest stable release)
  show "publishes no signature" instead of the gallery. That fails closed; it
  is not a security hole, but it is an outage for those builds.
- Never lower `SIGNED_SINCE` after a release, and never delete a `.sig` from a
  release: builds of that release would then refuse their own gallery.
- Re-running only the signing jobs on an existing draft fails, because the
  assets already exist; re-run **Release**, which replaces the stale draft.

## Before promoting a beta

Tick these before you run **Promote** (or, in the auto flow, before merging
anything that touches the risk areas below). The detailed manual cases are in
[Testing](testing.md#release-test-checklist); use the risk-area list here to
choose the checks that apply to the release.

**Always**

- [ ] CI is green on `main` on Windows, macOS and Linux (lint, tests, bundle build).
- [ ] The release run finished: all three build jobs succeeded and the release
      has installers, `.sig` files (including `index.json.sig` and
      `themes.json.sig`), `latest-beta.json` and `SHA256SUMS.txt`.
- [ ] Beta notes read well; fill in **Known issues and upgrade notes**.

**By risk area** (do the ones your changes touch)

| Area | Check |
|---|---|
| Installer and upgrade | Install the new build **over the previous stable** on Windows (NSIS and, if changed, MSI), macOS (drag to Applications) and Linux (`.deb`/AppImage). Config and data survive; the old version is replaced, not duplicated. |
| Startup and installer changes | Use the staged flow while validating OS integration. Attach the artifact-inspection and disposable-VM reports from [Installer validation](installer-validation.md). Record actual sign-in/restart and alternate-admin UAC results separately; a compiled MSI check or a silent VM test does not establish those outcomes. |
| Hotkey | The global hotkey opens Sevak after a fresh install and after an upgrade; the Universal Actions hotkey works; Wayland and GNOME paths if touched. |
| Updater | From the previous **stable**, switch Update channel to Beta and use the tray's Check for updates: the beta is offered, the signature verifies, install and restart work, the new version number shows. Then switch back to Stable and check no downgrade is offered. Also check a plain stable-to-stable update on the previous release. |
| Signature | The `.sig` files exist for every installer, `latest-beta.json` lists all platforms with signatures, and `node scripts/verify-release.mjs <dir> <version> latest-beta.json` passes on the downloaded assets (Promote runs this too). |
| Publisher identity | If configured, the release's Authenticode or Developer ID/notarization checks passed. If unconfigured, the release remains unsigned/ad-hoc; updater `.sig` files do not change that. See [Publisher signing](publisher-signing.md). |
| Config and migrations | Start the new build with the previous release's `config.toml`; nothing is reset and no warning appears. |
| Platform integration | Autostart, tray, single-instance, clipboard and file search on each OS you changed. |
| Packages | If packaging changed, render the manifests: `node scripts/package-manifests.mjs <version> SHA256SUMS.txt out`. |

## After a release

1. **Download.** Fetch one installer per OS from the release page and run it.
2. **Checksum.** `sha256sum -c SHA256SUMS.txt` (or `shasum -a 256 -c`) in the
   download folder reports every file OK.
3. **Manifest.** Open `https://github.com/ninad-k/Sevak/releases/latest/download/latest.json`
   (stable) or `.../releases/download/channel-beta/latest-beta.json` (beta). The
   `version` is the one you released, every platform has a `url` under the new tag
   and a `signature`. Promote and Rollback print this check at the end of the run;
   GitHub can take a minute to serve a new file.
4. **Updater.** On a machine with the previous version, Check for updates offers
   the new one and installing it works.
5. **Package managers** (stable releases): the winget pull request, the Scoop and
   Homebrew commits and the AUR push appear; each is skipped while its secret is unset.
6. **SBOMs and attestations** (stable releases): the release has
   `sevak-sbom-rust.cdx.json`, `sevak-sbom-npm.cdx.json` and `SBOM-SHA256SUMS.txt`,
   and `gh attestation verify <installer> --repo ninad-k/Sevak` succeeds. If the
   `SBOM` or `Attest` job failed, the release itself is fine: re-run the job, or run
   the workflow from the Actions tab with the tag.

## Rolling back

`.github/workflows/rollback.yml` (Actions, **Rollback**). Inputs: `tag` (a
known-good earlier release to serve as stable again), `bad_tag` (the release to
withdraw) and a one-line `reason`.

What it does:

1. Downloads the earlier release and verifies it: checksums, a signature next to
   every installer, and a `latest.json` that agrees with them. It refuses if any of
   that is missing.
2. Makes that release the newest stable release again, so
   `releases/latest/download/latest.json` serves it.
3. Marks the bad release as a pre-release with a "withdrawn" note on top (its files
   stay for investigation).
4. If the beta channel was serving the bad version, points it at the earlier
   release too.

### What rollback cannot do

**Installed copies of the bad version are not moved back.** The updater only offers
a version higher than the installed one, on purpose: a downgrade switch would let
anyone who can serve a crafted manifest push people to an old, genuinely signed but
vulnerable build, because the manifest itself is not signed (only the installer is).
Sevak keeps that guard, and there is no downgrade flag in `latest.json`. So:

- People who have **not** updated yet stay on their good version: they are no
  longer offered the bad one. This is what rollback is for, and why you do it fast.
- People who **already** updated to the bad version stay on it until a newer
  version exists. **The real fix is to publish a fixed newer version quickly**: merge
  a `fix:` (a patch bump above the bad one, even though the bad tag is now a
  pre-release, since version numbers never go down).
- Package managers are not touched by the workflow. If winget, Scoop, Homebrew or
  the AUR already carry the bad version, revert the Scoop and Homebrew commits by
  hand, push the earlier `PKGBUILD` to the AUR, and close or amend the winget pull
  request. The next release overwrites them anyway.
- In an emergency, someone can still reinstall the earlier version by hand from its
  release page. Say so in the withdrawn note.

### Procedure

1. Decide fast: is the problem in the update path itself, data loss, or a crash on
   start? Roll back first, investigate second.
2. Run **Rollback** with the last good tag and the bad tag.
3. Check `latest.json` serves the good version ([After a release](#after-a-release)).
4. Merge the fix (see [Hotfix](#hotfix)). In the staged flow the fix goes out as a
   beta first; for an urgent fix, run **Release** with `channel` = `stable`.
5. Write down what happened in the withdrawn release's notes and, if useful, an issue.

## Hotfix

1. Branch from `main` (the bad code is on `main`, so the fix goes on top; there are
   no release branches).
2. Pull request titled `fix: ...` with a test where possible; CI must pass.
3. Merge. **Auto flow:** it is published as the next patch release and reaches
   everyone within the day, including people who took the bad version.
   **Staged flow:** it is published as a beta; to ship it straight away run
   **Release** with `channel` = `stable` (it builds and ships the stable release
   from `main` immediately) rather than waiting for a soak.
4. Verify as in [After a release](#after-a-release).

## Who does what

| Task | Who |
|---|---|
| Review and merge pull requests, choose the title prefix that sets the bump | Maintainer |
| Switch `RELEASE_MODE`, run Release, Promote and Rollback | Maintainer (needs write access to the repository) |
| Soak a beta and run the checklist | Maintainer, optionally testers on the beta channel |
| Signing key, `PACKAGING_TOKEN`, `WINGET_TOKEN`, `AUR_SSH_PRIVATE_KEY` | Repository secrets; see [Development](development.md#auto-update-and-the-signing-key) |

## What the scripts and workflows are

| File | Purpose |
|---|---|
| `.github/workflows/release.yml` | Plan, build, publish, distribute; stable or beta |
| `.github/workflows/promote.yml` | Publish an existing release as stable, without rebuilding |
| `.github/workflows/sbom.yml`, `.github/workflows/attest.yml` | SBOMs and build-provenance attestations; called by Release and Promote for stable releases, or run by hand for a tag |
| `.github/workflows/rollback.yml` | Serve an earlier release as stable again |
| `.github/workflows/pr-labels.yml`, `.github/release.yml` | Group the generated release notes by commit type |
| `.github/workflows/release-checks.yml` | Tests the scripts below and parses the workflows on pull requests |
| `.github/workflows/extension-advisories.yml` | Signs and publishes the extension advisory feed; see [Extension advisories](#extension-advisories) |
| `crates/sevak-plugins/examples/verify_gallery_index.rs` | Verifies a gallery list's signature with the app's verifier before Release attaches it; see [Gallery index signatures](#gallery-index-signatures) |
| `scripts/release-signing.mjs` | Publisher-signing configuration (`prepare`) and verification (`verify`) |
| `scripts/release-version.mjs` | `next`, `next-beta`, `previous-tag`, `compare`, `set` |
| `scripts/updater-manifest.mjs` | Writes `latest.json` / `latest-beta.json` |
| `scripts/verify-release.mjs` | Checks checksums, signatures and the manifest of a downloaded release |

Run the script tests with `npm run test:scripts`.
