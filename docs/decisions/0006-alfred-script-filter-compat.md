# ADR-0006: Alfred Script Filter compatibility

**Status:** Accepted

## Context

Alfred's Script Filter format (a script prints `{"items": [...]}`) is the
best-known way to extend a launcher, and many scripts exist that do not depend
on macOS. Asking those authors to learn a new format would waste that work. At
the same time, Alfred's format has features (`variables`, `rerun`, workflow
objects, arbitrary URL schemes) that would widen what a script result can do.

## Decision

- A one-shot script plugin may declare `format = "alfred"`
  (`crates/sevak-plugins/src/script/alfred.rs`). Sevak reads the script's JSON
  and maps it onto its own `ResultItem`s; nothing in the Alfred document
  reaches the platform layer unmapped.
- The mapping is deliberately conservative. `arg` becomes an action by shape:
  `http://`, `https://` and `mailto:` open in the browser or mail client, an
  existing absolute path (or `~/...`) opens with its default program, and
  anything else is copied. Other schemes are copied, never opened, because the
  platform layer only opens web and mail links ([ADR-0003](0003-closed-action-vocabulary.md)).
- `icon.path` is resolved inside the plugin folder. `quicklookurl`, `variables`,
  `text` and `rerun` are ignored. `mods` become secondary actions; Alfred
  modifier combinations that have no Sevak key are dropped.
- Compatible scripts are still script plugins: the user approves the command
  before it first runs ([ADR-0005](0005-approval-before-run.md)).
- Sevak's own JSON protocol stays the primary, documented format; Alfred
  support is a bridge, not the model for new features.

## Consequences

- Existing cross-platform Alfred scripts run without changes; macOS-only ones
  (AppleScript, `osascript`) do not.
- Scripts that rely on `variables` passing between workflow objects, or on
  opening custom URL schemes, behave differently from Alfred.
- A second input format has to be parsed defensively, because it comes from a
  script Sevak does not control. The parser is the reason `alfred.rs` exists
  as a separate, tested module.
