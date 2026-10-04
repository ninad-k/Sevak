# ADR-0012: Native extensions reuse the script plugin trust model

**Status:** Accepted

## Context

Sevak's extensions are scripts and workflows: text a user can read, started
only after a native Allow dialog, with an approval bound to the contents
([ADR-0005](0005-approval-before-run.md)). Two things were missing for an
ecosystem: a way to write an extension that is a fast, self-contained program
(in Rust, with no interpreter on the user's computer), and a way for users to
find and install extensions from inside Sevak instead of copying folders.

A compiled program is the riskier kind of extension: the user cannot read it,
and "the same bytes" is the only thing an approval can mean. The pressure is
to invent a new trust model for it (a permission system, a sandbox, signed
publishers). That would be a second model to explain and to get wrong.

## Decision

- **A native extension is a script plugin.** Its `plugin.toml` has an
  `[extension]` table (version, publisher, licence, minimum Sevak, declared
  permissions, one program per platform) instead of `command`. It runs through
  the existing host: the same protocol, the same approval bound to the bytes of
  the manifest and the program, the same re-check before every start, the same
  scrubbed environment, the same process-tree kill, the same closed set of
  actions. Nothing about starting a program changed.
- **The dialog says what it is.** Titled "native extension", showing the
  publisher, version, licence, source, the permissions the author declared
  (marked as the author's statement), the program's full SHA-256 and that it is
  not sandboxed.
- **No sandbox, and no pretence of one.** Declared permissions are shown to the
  user and never enforced; the documentation says what the user is trusting.
  A real sandbox (OS-level, or a WASM runtime) would be a separate extension
  type later, not a retrofit of this one.
- **The gallery stays the only source, and keeps its rules.** Index format 2 is
  unchanged (a `native` entry is a kind older builds skip); entries list one
  package per platform, read at the build's own release tag, checked by SHA-256
  and then by the package rules. A native package's manifest must agree with its
  index entry, so what the page shows is what the dialog shows.
- **The store adds no trust.** Installing places a new, unapproved folder. Update
  swaps the folder as one step and so asks again (a new program); uninstall
  removes only what a receipt says the store installed.
- **The packaging format is a zip with a checksums file** (`.sevakext`), built
  deterministically, checked on install, and produced by a small separate tool
  (`sevak-ext`) rather than by the `sevak` launcher binary, which has no console
  on Windows.

## Consequences

- One trust model to explain: "an extension does nothing until you allow exactly
  what you were shown", for scripts, workflows and programs alike.
- A user who allows a native extension trusts its publisher entirely. Sevak
  narrows what can be swapped or hidden, not what an allowed program does.
  Declared permissions can be wrong or false without Sevak noticing.
- An update to a native extension always asks again, even for a one-line change.
  That is the cost of binding the approval to the bytes, and it is on purpose.
- Programs Sevak writes carry no download mark, so Gatekeeper and SmartScreen do
  not evaluate them; Sevak's dialog is the only gate.
- The gallery's reviewers become the human check for gallery entries, which is a
  maintenance burden and a trust root. A signed index remains in the backlog.
- Tag pinning means a new entry reaches users with the next release.
- The `sevak-ext` tool links Sevak's own plugin code so that what it validates is
  what the app installs; installing it builds that code.
