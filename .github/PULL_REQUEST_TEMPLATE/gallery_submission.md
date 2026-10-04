## Gallery submission

<!--
Use this template for a pull request that adds or changes something in gallery/
or examples/. Open it with ?template=gallery_submission.md appended to the
"compare" address, or pick it from the template list.
Guides: docs/marketplace/publishing.md and docs/marketplace/review-process.md.
Title: "feat(gallery): add <id>" (new), "fix(gallery): <id> ..." or
"feat(gallery): update <id> to <version>" (update), "chore(gallery): ..." (other).
-->

- **Kind:** workflow / script plugin / native extension / theme
- **Id and version:** `my-id` 1.0.0 (for an update: from 1.0.0 to 1.0.1)
- **Submission issue (if you opened one):** #
- **Source repository:** https://github.com/you/repo
- **Licence (SPDX):**
- **Declared permissions:** none / network / filesystem / subprocess / clipboard

### What it does

<!-- Two or three sentences. Does it run code? Which hosts does it contact, which programs does it start, which files does it write? "None" is a fine answer. -->

### Needs a human to judge

<!-- Does the theme look good? Does the Universal Actions entry appear in the right apps? Anything you could not test, and on which platforms. -->

## Pre-merge checklist

Taken from [gallery/README.md](../../gallery/README.md#pre-merge-checklist). Tick what is true; leave a note for what is not.

- [ ] The description says what the package does, and whether it runs code.
- [ ] No network access, subprocess, file write or `eval` in any script; no secrets.
- [ ] Links open only hosts on the allow list (and the list change is justified).
- [ ] Folder name, `id`, `name` in the file and `name` in the index agree.
- [ ] Re-packed after the last edit; `node scripts/gallery-check.mjs` is clean.
- [ ] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` pass.
- [ ] Themes: contrast lines are all AA; the palette's author and licence are named.
- [ ] Anything a human has to judge is mentioned above.

### Index entry

- [ ] `version` is bumped if any packaged byte changed (`node scripts/gallery-check.mjs --base origin/main` is clean).
- [ ] `license` is an SPDX expression, and it allows the gallery to distribute the work under Apache-2.0.
- [ ] The keyword is not used by a built-in, a search engine or another entry.
- [ ] Tags are 1 to 8 lower case labels, and include `needs-python` or `needs-node` where they apply.

### Native extensions only

- [ ] The source is public at the repository above, at the tag or commit the binaries were built from, and builds with the steps given in the README there.
- [ ] `sevak-ext validate gallery/extensions/<id>/*.sevakext` passes.
- [ ] The `permissions` in the index and in `plugin.toml` are equal, and match what the code does.
- [ ] Nothing is obfuscated, and the program does not download or run other code.

### Attestation

- [ ] I wrote this or have the right to submit it, under a licence compatible with Apache-2.0.
- [ ] It contains no secrets, no personal data and no telemetry.
- [ ] I have read the [acceptance rules](../../gallery/README.md#what-the-gallery-accepts) and understand that an accepted entry reaches users with the next release, and that a maintainer may later deprecate or remove it.

<!--
Do not tick or edit the labels: maintainers apply gallery:* labels as the entry moves through review.
Third-party pull requests are checked by an automated workflow that runs with read-only permissions and no secrets; it cannot approve anything.
-->
