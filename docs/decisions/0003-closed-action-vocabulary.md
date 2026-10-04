# ADR-0003: A closed `Action` vocabulary; the platform provider is the only OS layer

**Status:** Accepted

## Context

Results can come from built-in code, from the user's config and from external
scripts and workflows. If a result could carry an arbitrary command, any bug
in parsing, ranking or rendering results would be a way to run code.

## Decision

- What can happen when a result is activated is a closed enum, `Action`
  (`crates/sevak-core/src/model.rs`): `Launch`, `OpenPath`, `OpenUrl`,
  `CopyText`, `PasteText`, `PasteClip`, `CopyClip`, `RevealPath`,
  `RunAsAdmin` and `Custom`. `Custom` is understood only by the plugin that
  produced it. There is no "run this command line" variant; features that run
  commands (the shell plugin, workflows, script plugins) are separate plugins
  with their own consent rules.
- All access to the operating system goes through the `PlatformProvider`
  trait (`crates/sevak-platform/src/provider.rs`), with one implementation per
  OS. Plugins and the shell do not call OS APIs directly.
- System commands (sleep, lock, shut down, ...), settings pages and automation
  tasks are closed enums in the platform layer (`SystemCommand`,
  `SettingsPage`, `TaskKind` in `crates/sevak-platform/src/system.rs` and
  `tasks.rs`) rather than free-form strings, and destructive ones ask for
  confirmation through `Plugin::confirmation`.
- The web view never receives an `Action`. It gets a display-only `ResultDto`
  (title, subtitle, icon, action kind) and names a result by id and search
  ticket; the action stays in the Rust process (`src-tauri/src/commands.rs`).
- Links open only for an allow-list of schemes (`has_allowed_scheme` in
  `crates/sevak-platform/src/open.rs`; `crates/sevak-platform/src/deep_link.rs`).

## Consequences

- Reviewing what Sevak can do to the system means reviewing one enum and one
  trait.
- Adding a capability means adding a variant or a provider method, which is
  visible in review.
- Some integrations are less flexible: an Alfred script that returns a
  `slack://` link gets a copy action instead of an open
  ([ADR-0006](0006-alfred-script-filter-compat.md)).
- The enum is not a sandbox. A plugin that handles `Custom` actions can still
  do whatever its process can do; trust for such plugins comes from
  [ADR-0005](0005-approval-before-run.md).
