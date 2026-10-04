# ADR-0001: Local-first, no telemetry

**Status:** Accepted

## Context

A launcher sees a great deal: what you type, what you copy, which files and
bookmarks you have, and (with snippet expansion) what you type in other apps.
Users have to trust it with that. Analytics and crash reporting are the usual
way a small project learns what to fix, and each one is a data flow that has
to be explained, secured and defended.

## Decision

Sevak keeps everything on the machine and collects nothing about its users.

- No telemetry, analytics, crash reporting, accounts or identifiers. There is
  no code that sends usage data anywhere and no server that could receive it.
- All state lives in one config folder and one data folder
  ([Files and data](../files-and-data.md)); each file is plain TOML or JSON the
  user can read, edit or delete.
- Typed text, selections and clipboard contents are kept out of logs. Types
  that carry them have hand-written `Debug` implementations that log lengths,
  not text (for example `Launch` in `src-tauri/src/cli.rs`).
- Every request Sevak makes itself is listed in [Privacy](../privacy.md) and
  is governed by [ADR-0010](0010-opt-in-network-features.md).
- Problems are diagnosed from local logs that the user chooses to share.

## Consequences

- The privacy story is short and checkable by reading the source.
- There is no data on how many people use Sevak or which features break, so
  prioritisation relies on issues and direct feedback.
- Features that would need a server (sync, cloud history, galleries with
  accounts) are out of scope by design.
- Clipboard history is stored unencrypted on disk (in a file only the current
  user can read where the platform allows it), and the 1Password list is held
  in memory only. Protection against other software running as the same user is
  the operating system's, not Sevak's; see the
  [threat model](../security/threat-model.md).
