# How review works

Every entry in the gallery was read by a person before it was merged. This page
describes that process for authors and reviewers: the steps, who decides, what is
rejected and why, and how to appeal. It describes what exists today; where
something is a goal rather than a guarantee, it says so.

## The lifecycle

1. **Submitted.** An author opens a pull request that changes `gallery/` and the
   matching sources in `examples/` (see [Publishing](publishing.md)), optionally
   after a [submission issue](https://github.com/ninad-k/Sevak/issues/new?template=extension_submission.yml).
2. **Automated checks.** Workflows run on the pull request and report back.
   They check files and rules, not intent ([below](#automated-checks)).
3. **Triage.** A maintainer looks at the pull request, confirms it is a real
   submission, and labels it `gallery:needs-review` (applied automatically when
   the paths match), adding `gallery:security-review` if it runs code or a binary.
4. **Human review.** A maintainer reads the change against the
   [reviewer checklist](#reviewer-checklist) and either asks for changes
   (`gallery:changes-requested`), declines (`gallery:rejected`, with the reason in
   the thread) or approves.
5. **Security review**, for anything that runs code or a binary
   ([below](#security-review)). It is a second, closer reading by a maintainer who
   did not write the change, or, while the project has one maintainer, a separate
   pass in which the checklist is worked through in writing on the pull request.
6. **Approved** (`gallery:approved`). All checks pass, the review is done and the
   author's open questions are answered.
7. **Merged.** A maintainer merges; the author does not.
8. **Released.** A merge to `main` that is not documentation-only publishes a
   release automatically ([ADR 0007](../decisions/0007-release-on-every-merge.md)),
   and a Sevak build reads the gallery **at its own release tag**
   ([Gallery trust](../security/gallery-trust.md#2-pinned-to-the-release-of-the-running-build)).
   So an entry is visible to users from the release that contains the merge, and only
   to people who update to it. Nobody sees a merge the moment it lands.

### Target response times

These are goals, not promises. The project is maintained by volunteers in spare time.

| Step | Goal |
|---|---|
| First reply or triage | within 7 days |
| First full review of a workflow or theme | within 14 days |
| Review of anything with code or a binary, including the security review | within 30 days |
| A pull request with no reply from the author | marked stale after 30 days, closed after 60; it can be reopened |

A security report about an existing entry is handled much faster; see
[Updating and removal](updating-and-removal.md#security-reports).

## Automated checks

Two workflows run on a pull request that touches `gallery/**`,
`examples/plugins/**` or `examples/workflows/**`. They are a convenience for the
author and the reviewer. **They are not a security review**: they pass or fail on
format, hashes and rules, and they cannot tell that a script is malicious.

**Gallery review** (`gallery-review.yml`):

- *Automated review report.* `node scripts/gallery-check.mjs --base <base commit>`
  checks that every path exists, every SHA-256 matches, nothing is unlisted, ids
  and tags are well formed (an id may not be a Windows device name), versions are
  numeric, licences are SPDX expressions, keywords are unique, sizes are within
  limits, and `deprecated`/`replaced_by` make sense. Against the base commit it
  checks that a package whose bytes changed has a **higher `version`**, that an id
  did not change kind, and lists what is new, updated or removed. The result is in
  the job summary and uploaded as a small JSON report.
- *Tests.* `node --test scripts/gallery-check.test.mjs`, the example extension
  tests, and `cargo test -p sevak-plugins --test gallery_content`, which installs
  every package through the app's own installer, validates every workflow, runs the
  workflows and Python scripts on sample input, and checks theme contrast.

**Gallery review comment** (`gallery-review-comment.yml`) posts the report as a
comment and adds `gallery:needs-review`, `gallery:submission` or
`gallery:update`, and `gallery:security-review` when the report says an entry runs
code. **Gallery labels** (`gallery-labels.yml`) adds `gallery` and the area labels
from the changed paths.

These workflows are written to be safe with hostile pull requests:

- The review runs on `pull_request`, with a read-only token and no secrets.
  Nothing from a fork can write to the repository.
- The checker that produces the report is taken from the **base branch** and pointed
  at the contributor's files (`--root`), so a pull request cannot weaken the rules
  that judge it. It only reads files.
- The comment is posted by a different workflow, which runs from the default branch
  and only downloads the JSON report. It never checks out or runs the pull request,
  finds the pull request from the commit SHA (not from a number inside the
  report), and writes every string from the report inside a code span.
- The Rust cache is restored but never saved by these runs.

The tests do run the contributor's code, as the repository's ordinary CI does for
every pull request, in a job with a read-only token and no secrets.

## Reviewer checklist

A reviewer works through this on the pull request, and says what they checked.

**Every entry**

- The description says what it does, and a workflow that runs no code says
  "Runs no code."; one that runs a script says it runs after the user allows it.
- Id, folder, `name` in the file and `name` in the index agree. Version is bumped
  if any byte changed. Author and licence are plausible and the licence allows
  Apache-2.0 distribution (the script flags GPL-style, non-commercial and
  unknown licences for a human decision).
- The keyword is not taken by a built-in, a search engine or another entry.
- No secrets, no personal data, no telemetry, no analytics, no tracking links.
- The pull request touches only what it says (no changes to other entries, to the
  allow lists or to the checker, unless that is the point and is explained).
- The package equals the folder in `examples/` it was packed from (the tests
  check this; confirm the folder is what you read).

**Workflows**

- Only allowed node types; links open only hosts on the allow list. A new host is
  justified, and its URL template percent-encodes the user's text.
- A workflow that pastes or runs a script is labelled for the security review.

**Script plugins**

- Standard library only; no sockets, HTTP, subprocesses, `eval`/`exec`; no file
  writes outside `SEVAK_PLUGIN_DATA`; under 20 KiB; readable, not minified.
- Every helper file the manifest needs is listed in `files`.

**Native extensions**

- The repository is public, the licence is open source, and the README says how to
  build. The tagged source builds the committed binary (the packages are
  deterministic; rebuild and compare checksums where practical).
- Declared `permissions` equal what the code does: read the network, file and
  process use in the source. The `plugin.toml` in the package and the index entry
  agree (the tests check it).
- No obfuscation or packing, no secondary downloader or updater, no download of
  more code, no telemetry that is not stated.

**Themes**

- Contrast lines are all AA; the palette's author and licence are named in the file.
- It looks good in the app (a human judgement the tests cannot make).

## Security review

Anything that runs code (a script plugin, a workflow with a script or a paste
step, a native extension) gets the label `gallery:security-review` and may not be
merged on the strength of the automated checks. The reviewer:

1. reads **all** of the code, including build files and scripts of the source
   repository for a native extension;
2. checks the code against the description and the declared permissions;
3. for a native extension, rebuilds from the tagged source and compares the result
   with the committed package, or says why that was not possible;
4. writes the outcome on the pull request: what was read, what was not, and the
   residual risk.

A review is **not an audit**. Sevak does not sandbox native extensions
([Writing extensions in Rust](../writing-extensions-in-rust.md#security-model)): the
user's approval is what permits one to run, and a reviewed entry is still code the
user must trust. The gallery also has no signature yet
([Gallery trust](../security/gallery-trust.md#what-this-does-not-protect-against)).

## What gets rejected, and why

| Reason | Example |
|---|---|
| It does something its description does not say | a "colour converter" that writes a log file |
| Network access from a script or program that is not the point of the entry, or telemetry of any kind | analytics, update checks, phone-home |
| Obfuscated, minified, packed or binary content outside a native extension | a bundled script nobody can read |
| A native extension without public source that builds the committed binary | "trust me" binaries |
| Declared permissions that do not match what the code does | `permissions = []` for a program that opens sockets |
| A licence that does not allow redistribution under Apache-2.0, or content copied without the right to | a GPL program, a palette with no licence |
| Secrets, personal data, or links to hosts that are not on the allow list | a hard-coded API key |
| Impersonation: a name, icon or description that suggests it comes from someone it does not | "Official Google Translate" |
| Duplicates an existing entry without adding anything, or squats an id or keyword | |
| Cannot be reviewed in reasonable time: too big, too clever or too many dependencies | |
| The author cannot be reached to fix a problem found in review | |

Rejection is about the entry, not the person. The reason is written on the pull
request, with what would change the answer, where there is one.

## Appeals

If you think a decision is wrong:

1. Reply on the pull request, saying which point of the decision you disagree with
   and what you changed or can show.
2. If that does not settle it, ask for a second opinion by opening a
   [Discussion](https://github.com/ninad-k/Sevak/discussions) that links the pull
   request. While the project has one maintainer, the second opinion is a fresh
   read of your arguments after a few days, and a fair outcome is not guaranteed.
3. You can always distribute your work yourself. A script plugin or native
   extension from your own repository installs by hand
   ([Try it in Sevak](../writing-extensions-in-rust.md#try-it-in-sevak)), and Sevak
   asks before it runs.

Appeals are not available for a [takedown](updating-and-removal.md#takedown) made
for safety reasons while the safety question is open.

For the maintainer's side, see [Maintainers](maintainers.md).
