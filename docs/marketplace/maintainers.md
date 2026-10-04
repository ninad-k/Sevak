# For maintainers

How to run the gallery's review queue: the labels, what to merge and when, and who
may approve what. Authors want [Publishing](publishing.md); the steps and criteria
are in [How review works](review-process.md).

## Labels

The labels are defined in `.github/labels.json` and created with
`scripts/sync-labels.sh` (it needs `gh`, and is run by hand; run it once after
this change lands, since the issue form and workflows use the new labels).

| Label | Set by | Meaning |
|---|---|---|
| `gallery` | automatic (paths) | the pull request touches `gallery/`, `examples/` or `templates/` |
| `gallery:submission` | automatic (report) | the report says an entry is new |
| `gallery:update` | automatic (report) | an existing entry changes, is deprecated or is removed |
| `gallery:needs-review` | automatic | waiting for a human. Remove it when you start; the label is re-added by every push, which is right: a new push needs a new look |
| `gallery:security-review` | automatic (report) | the change runs code or a binary (a plugin, a native extension, a workflow with a script or paste step); not mergeable without the security review written up |
| `gallery:changes-requested` | maintainer | the author must change something; say what, in the thread |
| `gallery:approved` | maintainer | reviewed and OK to merge **at the commit you reviewed**; a later push means look again |
| `gallery:rejected` | maintainer | declined; the reason and what would change it are in the thread; then close |
| `gallery:takedown` | maintainer | the change removes or revokes an entry for safety, abuse or legal reasons |

Labels are an aid to the queue. They are not the gate: branch protection and
CODEOWNERS are, and the labels set by workflows are only what a report claims
(the labeler and the comment workflow cannot approve anything). Never trust a
label to mean a pull request is safe.

The same filters help: `is:pr is:open label:gallery:needs-review`, or
`label:gallery:security-review -label:gallery:approved`.

## Triage

1. **Is it a gallery change?** If a pull request mixes gallery and unrelated
   changes, ask for a split. One entry or one update per pull request.
2. **Read the automated report** (the comment, or the job summary). Fix-able
   problems go back to the author with `gallery:changes-requested`. Do not review
   a pull request whose checks are red unless the author asks for help with a
   failure.
3. **Check the table in the report.** `new` and `updated` rows say whether the entry
   needs the security review. A `removed` row on a pull request that is not a
   removal, an author or licence change, or a new permission is a reason to stop
   and ask.
4. **A submission issue** (the `gallery:submission` form) is a proposal. Reply, say
   whether it looks acceptable, and point at the [publishing guide](publishing.md).
   Close it when the pull request is opened or after 60 days of silence.
5. Work through the [reviewer checklist](review-process.md#reviewer-checklist), and
   for `gallery:security-review` write the [security review](review-process.md#security-review)
   as a comment: what you read, what you did not, the result.

## Merge rules

Merge a gallery pull request only when **all** of these are true:

- CI is green: the repository's **CI** workflow and **Gallery review** (both jobs).
  The automated report has no problems; every warning has been looked at and
  answered in the thread.
- A code owner has reviewed it (CODEOWNERS requires `@ninad-k` for `/gallery/`,
  `/examples/`, `/templates/` and the gallery workflows), and the review was of the
  **current head commit**.
- If `gallery:security-review` is set, the written security review is on the pull
  request and its outcome is "approve". The author of a change does not
  perform its security review. While there is one maintainer, an entry the
  maintainer wrote themselves is reviewed against the same checklist in
  writing, after a night's sleep, and the pull request says so.
- The branch is up to date enough that the checker's `--base` comparison was against
  the current `main` (re-run the check if `main` moved the index).
- The title is a Conventional Commit (`feat(gallery): add <id>`,
  `feat(gallery): update <id> to <version>`, `fix(gallery): ...`,
  `chore(gallery): remove <id>`), because merging publishes a release and the title
  decides its version ([CONTRIBUTING](https://github.com/ninad-k/Sevak/blob/main/CONTRIBUTING.md#pull-request-titles-decide-the-version)).
- Squash-merge, so one commit on `main` carries the review's final state.

Never merge on the strength of the automated checks alone, never merge a change to
the allow lists or the checker as a side effect of a submission, and never merge
something you have not read because the author is well known.

After merging, set `gallery:approved` if you have not, and look at the release the
merge triggers: a failed release leaves the entry unpublished until the next
successful one.

## Who may approve what

| Change | May approve | Needs |
|---|---|---|
| A theme | a code owner | licence of the palette named; contrast is AA |
| A workflow that runs no code | a code owner | checklist |
| A workflow with a script or a paste step, a script plugin | a code owner who is not its author | written security review |
| A native extension (new or updated binary) | a code owner who is not its author | written security review, source rebuilt and compared where practical |
| Deprecation, or removal at the author's request | a code owner | the author's word, or the maintainer's reason |
| A takedown | a code owner, without waiting for a second review | reason in the pull request; advisory for a security problem |
| An addition to an allow list in `gallery_content.rs`, a change to `gallery-check.mjs`, the gallery workflows, `labels.json` or CODEOWNERS | a code owner who is not its author | justification in the pull request |

Today the only code owner is `@ninad-k`, so the "who is not its author" rows mean that
the maintainer's own changes get the written, delayed self-review described above
until a second reviewer exists. Adding a co-maintainer means adding them to
`.github/CODEOWNERS` for the gallery paths (and naming who may do security reviews
in this table).

## Settings this process assumes

These are repository settings, not files, so they are not part of this change and
should be checked in **Settings → Branches / Rules**:

- `main` is protected: a pull request and the required checks (CI, and Gallery
  review once it has run) are required; direct pushes are not allowed.
- **Require review from Code Owners** is on, and **dismiss stale approvals when new
  commits are pushed**.
- Release tags (`v*`) are protected, because the gallery pin is only as good as the
  tag ([Gallery trust](../security/gallery-trust.md#what-this-does-not-protect-against)).
- Private vulnerability reporting is enabled (the issue form's security link and
  [SECURITY.md](https://github.com/ninad-k/Sevak/blob/main/SECURITY.md) rely on it).
- Pull requests from first-time contributors need approval before workflows run
  (the default for public repositories). Keep it that way.

## Handling the workflows

- `gallery-review.yml` runs on `pull_request` with a read-only token. Do not change
  it to `pull_request_target`, do not give it secrets, and do not make it run the
  checker from the pull request's own tree: the point is that the rules come from
  `main`.
- `gallery-review-comment.yml` is the only place that writes a comment for the
  review, and it must stay free of any checkout or execution of pull request
  content. It reads one JSON artifact as data.
- When you change the checker's rules, the new rules judge pull requests only after
  your change is on `main`.
- Actions are pinned to commit SHAs with the version in a comment, like the other
  workflows; Dependabot updates them.

## Takedown and security reports

Follow [Updating and removal](updating-and-removal.md#takedown). The short version:
remove first if the entry could hurt people, label the pull request
`gallery:takedown`, merge it with a Conventional Commit title that starts the
release, publish an advisory for a security problem, and write down what the app
can and cannot do (it has no recall for installed copies or older builds; see the
warning on that page). Do not describe a takedown as stronger than it is.
