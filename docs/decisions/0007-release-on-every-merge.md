# ADR-0007: Automated release on every merge to `main`

**Status:** Accepted, with known risks

## Context

Sevak has one maintainer and no release manager. Manual releases drift: fixes
sit on `main` for weeks, installed copies stay vulnerable, and version numbers
and changelogs are done late or wrongly. Users also get fixes only if the
update path is trivial.

## Decision

Every push to `main` ships a release, unless it touches only documentation or
the head commit contains `[skip release]`
(`.github/workflows/release.yml`).

1. **Plan.** The next version is computed from the commit subjects since the
   last `vX.Y.Z` tag (`scripts/release-version.mjs`): `feat:` raises the minor
   version, a breaking change (`feat!:` or a `BREAKING CHANGE:` footer) the
   major one (the minor one while on 0.x), anything else the patch version. A
   draft GitHub release is opened for the commit.
2. **Build.** Installers are built on Windows (NSIS, MSI), macOS (universal
   app and dmg) and Linux (deb, rpm, AppImage) and signed for the updater with
   a minisign key held in repository secrets (`TAURI_SIGNING_PRIVATE_KEY`).
3. **Publish.** `latest.json` (the manifest installed copies poll) and
   `SHA256SUMS.txt` are attached and the draft is published, which creates the
   tag. A failed build leaves only a draft and no tag.
4. **Distribute.** winget, Scoop, Homebrew and the AUR are updated when their
   secrets are configured.

The workflow relies on `main` being protected and on CI having passed on the
pull request that was merged (its header says so; the repository settings are
not part of this repository), so the release is of reviewed, tested code. The
workflow does not re-run the tests. Conventional Commit
subjects are therefore a release mechanism: a wrongly typed commit changes the
version number.

## Consequences

Benefits:

- Fixes reach users in hours, with no release ceremony; security fixes use the
  same path as any other change.
- Version numbers are derived, so they cannot be forgotten.

Risks, accepted knowingly:

- **A bad merge is a bad release.** There is no soak period, staged rollout or
  channel: whatever lands on `main` is offered to every installed copy at its
  next update check (every six hours, and when the launcher opens after an
  hour). The mitigations are required CI, the user's own consent before an
  update installs, and the speed of shipping a fix. Rolling back means
  releasing a higher version.
- **The pipeline is the crown jewel.** Anyone who can merge to `main`, or who
  can change the workflow or read the signing secrets, can ship code to every
  user, signed with the one key the updater trusts. Protecting the account,
  branch protection, secrets and the actions the workflow uses is part of
  Sevak's security, not only of its hygiene (see the
  [threat model](../security/threat-model.md#release-and-update)).
- **No second factor on integrity.** `SHA256SUMS.txt` is published beside the
  files it covers, so it detects corruption but not a compromised release.
  Windows and macOS builds are not yet code-signed or notarized, so the OS
  warnings users see cannot distinguish a genuine from a tampered build either.
- **Frequent releases are noisy** for package-manager maintainers and for
  users who keep every update prompt; `[general] check_for_updates` can be
  turned off, and package-manager installs update through the manager.
