# Updating and removing entries

How a published entry changes over time: new versions, deprecation, removal at the
author's request, takedown of something unsafe, and how security reports are
handled. It also says plainly what Sevak itself does and does not do with a
removed entry today, because that decides what a removal can promise.

## Release a new version

An update is a pull request, reviewed like a submission, against the same id.

1. Change the sources in `examples/` (or, for a native extension, build new
   packages from a new tag of your repository).
2. Re-pack, and **bump `version`** in the entry. The version is numeric (`1.2` or
   `1.2.3`; a native extension needs three parts). Use a patch bump for a fix, a
   minor bump for something new, and a major bump when behaviour, keywords or
   permissions change in a way people should notice.
3. For a native extension, add the new `<id>-<version>-<platform>.sevakext`
   packages, **remove the old ones** from `gallery/extensions/<id>/` (the checker
   refuses a package the entry does not list), and update `platforms`, `version`
   and, if they changed, `permissions`, `license` and `min_sevak`. They must equal
   the new package's `plugin.toml`.
4. Run `node scripts/gallery-check.mjs --base origin/main`. It fails if bytes
   changed and the version did not, if the version went down, or if the entry
   changed kind. It warns about a new declared permission, a changed licence or
   author, and a version bump with no change in the package.
5. Open the pull request with `feat(gallery): update <id> to <version>` (or
   `fix(gallery): ...`). The `gallery:update` label is applied from the report.

**What reviewers do with an update.** They read the difference, not just the new
version. Anything that adds permissions, network access, a new host, a new node
type, or changes a program gets the [security review](review-process.md#security-review)
again. A change of author needs the old and the new author to agree on it in the
pull request.

**What users see.** Once a release containing the change ships, Settings compares
the installed version with the listed one and offers **Update**. An update is
written aside and swapped in as one step. It is a changed program or script, so
Sevak asks for permission again, with the dialog saying the contents changed.
Nothing updates by itself.

## Deprecate an entry

Use deprecation when something should no longer be used but removing it would
surprise people: it is superseded, unmaintained, or broken on a platform.

Add two optional fields to the entry in `gallery/index.json`:

```json
{
  "id": "old-tool",
  "deprecated": "Superseded by new-tool, which also handles XYZ.",
  "replaced_by": "new-tool"
}
```

- `deprecated` is a trimmed, plain-text reason (1 to 200 Unicode characters,
  without control characters).
- `replaced_by` is optional, needs `deprecated`, and must be the id of another entry
  in the index (the checker refuses a missing id and an entry that replaces itself,
  and warns when the replacement is itself deprecated). A workflow or script plugin
  cannot name a native extension: the Gallery page does not list native extensions.

**What users see.** Gallery and Extensions show the reason and the replacement's
name (its id when the replacement is not listed).
New installs of deprecated entries are refused before downloading. An existing
Extensions installation can still update to a newer listed version, so a final
fix can reach its users, with the warning remaining visible. Sevak does not
silently install the replacement or remove the original. An invalid reason
causes that entry to be skipped. An invalid `replaced_by` (missing, itself, not
deprecated, or a native extension for a workflow or plugin) only drops the
reference: the entry and its deprecation warning stay, so people who have it
installed still see it, and the list reports the dropped reference.

The index stays format 2. Deprecation changes reach a build through its pinned
release catalog; older builds that do not implement these fields ignore them.
Use a signed advisory for a security issue that needs to reach builds already
released with advisory support.

## Remove an entry at the author's request

An author can withdraw their work at any time.

1. Open a pull request that removes the entry from `gallery/index.json`, its files
   from `gallery/packages/` or `gallery/extensions/<id>/`, and its folder from
   `examples/` (or ask in an issue and a maintainer will do it). A maintainer can
   remove an entry on the author's word if the author cannot open a pull request.
2. A maintainer merges it, labelled `gallery:update`. The checker reports it as
   `removed` and raises a warning, which is expected.
3. The next release no longer lists the entry.

**What a removal does not do.** It does not uninstall anything from anyone's
computer, and it does not change what **already released** builds list: they read
the gallery at their own release tag, and that tag still contains the entry and its
files ([Gallery trust](../security/gallery-trust.md)). Git history keeps the files
too. A removal is a promise about future releases, not a recall.

## Takedown

A takedown is the removal of an entry that is malicious, deceptive, abusive,
infringing or unsafe, without waiting for the author. Reasons include: code that
steals data or does something its description does not say; an author account or
repository that was taken over; a vulnerability that cannot be fixed quickly; a
licence or copyright claim that holds up; harassment or illegal content in names
and descriptions.

How it works:

1. **Report.** For anything that could harm users, report privately (below).
   Otherwise open an issue or contact the maintainer.
2. **Decide.** A maintainer assesses the report. If the entry is plausibly
   dangerous, it is taken down first and discussed afterwards.
3. **Remove.** A pull request (labelled `gallery:takedown`) removes the entry and
   its files, as in a removal at the author's request. It is reviewed as fast as
   the maintainer can, and merging publishes a release, so the entry is gone from
   every build released after that. The pull request says why, unless explaining
   would help an attacker.
4. **Tell people.** For a security problem the maintainer publishes a GitHub
   security advisory naming the entry and version, saying what it did and that
   users should remove it in Settings and treat anything the program could reach
   as exposed. The release notes mention it.
5. **Follow up.** The author may respond and, if the problem was a mistake that
   is fixed, submit again under the normal process (a new review, with the history
   in view). The id may be refused.

!!! warning "Notices require an explicit check"
    Builds with advisory support check a signed channel when users choose
    **Load gallery**, **Load/Refresh the list**, or **Check extension advisories**
    on the Installed tab. **Check extension advisories** runs the same request as
    **Refresh the list**: it also reloads the package and theme lists. Opening the
    page alone uses the local cache, and there is no background check. Known revoked versions cannot be installed or
    updated from the gallery. The Installed tab shows the reason and an
    **Uninstall** action for copies managed by Extensions, including entries
    removed from the catalog. Themes installed from Appearance get the same
    receipt. Older Gallery/manual copies without a receipt must be removed from
    their installation folder: a `"*"` notice still flags them on the Installed
    tab with their location (themes only once the list is loaded), and Gallery
    warns about any notice for an entry whose installed version is unknown.

    Installed copies are not automatically disabled or removed. A non-revoked
    newer version can still be installed as an update. Existing approval remains
    bound to the exact program bytes; it does not prove a bad original is safe.
    Offline users and builds predating advisory support cannot learn new
    notices. A missing, expired or failed feed is visibly **unknown**, and
    previously authenticated notices remain effective. Unknown status does not
    block every package or mean that an extension is safe.

## Publish and renew advisories

The application verifies notices using its embedded updater public key. The
maintainer publishes them separately from pinned package catalogs:

1. Update `gallery/revocations.json` in a reviewed pull request. It contains
   `{"format":1,"revocations":[]}` initially. Each notice has a gallery `id`
   (lower case letters, digits, `-` and `_`), `versions` (exact versions or
   the sole `"*"`), a plain-text `reason`, and optionally `advisory_url` pointing at
   a Sevak GitHub security advisory. Add `"kind": "theme"` for a theme from the
   theme gallery; themes have no versions, so they take the sole `"*"`. Package
   and theme ids are separate: a notice for one never revokes the other.
   Versions match as semantic versions where both sides parse (`1.0` is
   `1.0.0`; build metadata is ignored, pre-releases are not), otherwise as
   exact strings. `"*"` matches every version, **including a later fixed
   one**, so it also blocks updating to the fix. List the exact affected
   versions unless no version may be installed at all.
2. Keep previous notices unless intentionally withdrawing them. A newer valid
   signed feed replaces the previous set, so dropping an id withdraws its
   notice. A takedown pull request alone does not publish a revocation.
3. Run `node scripts/extension-advisories.mjs check gallery/revocations.json`.
   Merge the reviewed source, then manually run the
   [Extension advisories workflow](https://github.com/ninad-k/Sevak/actions/workflows/extension-advisories.yml)
   from `main`. It prepares a **14-day** feed, signs the exact bytes using the
   existing `TAURI_SIGNING_PRIVATE_KEY`, and runs the application's Rust verifier
   with the embedded public key before uploading. A wrong key or invalid feed
   fails before publication. No production key is generated by the tooling.
4. The workflow publishes `extension-revocations.json` and its Tauri-format
   `.sig` to the dedicated **extension-advisories** prerelease. This release is
   a mutable channel, not the latest application release. Keep it exempt from
   release immutability; stable application releases do not need to change.
5. Run the same workflow **before expiry**, even when the notices are unchanged.
   Renewal is manual; no scheduled publication is configured. Review its result
   and the uploaded expiry. During asset replacement, clients reject a transient
   JSON/signature mismatch and retain the previous authenticated notices.

The source starts empty and these changes do not publish a feed. Until the first
successful upload the app reports unavailable/unknown status. Oversized
responses, invalid signatures and older feeds are rejected. A feed accepts at
most 30 days of validity. The local clock never discards authentic notices: a
feed that looks expired, or issued more than five minutes in the future, is
still enforced and shown as stale. See [Gallery trust](../security/gallery-trust.md#6-signed-revocation-notices)
for limits, cache rules and what the signature does not cover.

## Security reports

If you find that a gallery entry is malicious, leaks data, or has a vulnerability:

1. **Do not open a public issue or pull request.** Report privately through
   [GitHub's private vulnerability reporting](https://github.com/ninad-k/Sevak/security/advisories/new)
   (the Security tab of the repository).
2. Say which entry and version, what it does, what an attacker needs, and how to
   reproduce it. Remove private data from logs first.
3. The maintainer's targets are in [SECURITY.md](https://github.com/ninad-k/Sevak/blob/main/SECURITY.md):
   acknowledgement in about 7 days and a first assessment in about 14. These are
   targets, not guarantees. For an entry that is actively harmful, the first step is
   a takedown, which does not have to wait for a fix.
4. The maintainer contacts the author (privately, through the advisory) when the
   problem is a bug rather than an attack, and gives them a reasonable time to
   publish a fixed version, which is reviewed as an [update](#release-a-new-version).
   If the author does not respond or the problem is serious, the entry is
   taken down.
5. When the matter is settled, the advisory is published and the reporter is
   credited unless they ask not to be.

A vulnerability in code the entry only *uses* (a library) belongs to that library;
tell the entry's author as well.

## Quick reference

| I want to... | Do this | Effect |
|---|---|---|
| Ship a fix or feature | PR with a higher `version` and new package | users get it from the next release, after they click Update |
| Mark it obsolete | add `deprecated` (and `replaced_by`) | warning and no fresh install; existing users can get a newer fix |
| Withdraw my entry | PR removing the entry and files | gone from the next release; installed copies stay |
| Report something dangerous | private vulnerability report | takedown and separately signed notice; user chooses Uninstall |
