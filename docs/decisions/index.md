# Design decisions

These records explain *why* Sevak is built the way it is. Each one is short:
the situation that forced a choice, what was chosen, what it costs, and
whether it still stands. They describe decisions that are visible in the code
today; the code paths named in each are the place to check that a record is
still true.

New decisions are added as the next number. A decision that is replaced is
not deleted: its status changes to *Superseded by ADR-NNNN* and the new record
explains the change.

| ADR | Decision | Status |
|---|---|---|
| [0001](0001-local-first-no-telemetry.md) | Local-first, no telemetry | Accepted |
| [0002](0002-tauri-rust-core-plugins.md) | Tauri shell, Rust core, plugin architecture | Accepted |
| [0003](0003-closed-action-vocabulary.md) | A closed `Action` vocabulary; the platform provider is the only OS layer | Accepted |
| [0004](0004-keyword-routing-and-ranking.md) | Keyword and global routing, and how results are ranked | Accepted |
| [0005](0005-approval-before-run.md) | Approval before a script plugin or workflow runs | Accepted |
| [0006](0006-alfred-script-filter-compat.md) | Alfred Script Filter compatibility | Accepted |
| [0007](0007-release-on-every-merge.md) | Automated release on every merge to `main` | Accepted, with known risks |
| [0008](0008-text-config-toml.md) | Text-only configuration in TOML with comment-preserving saves | Accepted |
| [0009](0009-hotkey-takeover-per-os.md) | Taking over Super+Space differently on each OS | Accepted |
| [0010](0010-opt-in-network-features.md) | Network features only on the user's action or opt-in | Accepted |
| [0011](0011-snippet-expansion-hooks-no-wayland.md) | Snippet expansion through low-level input hooks, and no Wayland support | Accepted |
| [0012](0012-native-extensions-reuse-the-script-trust-model.md) | Native extensions reuse the script plugin trust model; no sandbox, honestly labelled | Accepted |

See also the [threat model](../security/threat-model.md) and
[How Sevak works](../architecture.md).
