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

- `deprecated` is a short reason (1 to 200 characters).
- `replaced_by` is optional, needs `deprecated`, and must be the id of another entry
  in the index (the checker refuses a missing id and an entry that replaces itself,
  and warns when the replacement is itself deprecated).

**What this does today.** The index format stays 2, and older Sevaks keep working:
the app reads entries into a fixed set of fields and ignores any other, which a
test (`fields_this_build_does_not_know_are_ignored`) pins down. That also means
**the app does not show or act on `deprecated` yet**. A deprecated entry is still
listed and installable in the app. The fields are information for people (and for
a gallery web page that chooses to show them); they are not enforcement. Showing
them in the app is on the [roadmap](https://github.com/ninad-k/Sevak/blob/main/ROADMAP.md).

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

!!! warning "What Sevak can and cannot do about a revoked entry today"
    **There is no revocation mechanism in the app.** Sevak never checks in the
    background ([Privacy](../privacy.md)), so it does not learn that an entry was
    taken down. Today:

    - A build released **before** the takedown keeps listing the entry, and can
      still install it, because it reads the list at its own release tag. Updating
      Sevak to a release after the takedown removes it from the list.
    - A copy **already installed** keeps running (after the user allowed it).
      Sevak does not remove or disable it. The user has to uninstall it, which is
      why the advisory tells them to.
    - The user's approval is bound to the exact bytes; it protects against a later
      silent change, not against a bad original.

    As a last resort for a dangerous entry in an old release, a maintainer could
    delete that release's tag. A build whose tag does not exist reads the latest
    stable release's list instead ([Gallery trust, rule 3](../security/gallery-trust.md#3-builds-without-a-tag)).
    That also removes the release and its pin for everything else in it, so it is
    a destructive emergency measure, not a routine tool, and it has not been rehearsed.

    A real fix is on the [roadmap](https://github.com/ninad-k/Sevak/blob/main/ROADMAP.md):
    a signed revocation list that Sevak checks when the user opens the gallery or
    the extensions page, with a notice and an offer to uninstall. It is **not built**,
    and there is no date.

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
| Mark it obsolete | add `deprecated` (and `replaced_by`) | information only; the app does not show it yet |
| Withdraw my entry | PR removing the entry and files | gone from the next release; installed copies stay |
| Report something dangerous | private vulnerability report | takedown and advisory; no in-app recall yet |
