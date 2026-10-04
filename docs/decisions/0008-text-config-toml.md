# ADR-0008: Text-only configuration in TOML with comment-preserving saves

**Status:** Accepted

## Context

Sevak has many options, and its users are comfortable with text files:
snippets with multi-line text, web search engines, folders to search, themes.
A settings window is also needed for people who do not want to edit a file.
Two editors for one file tend to destroy each other's work: a UI that rewrites
the file drops the comments and ordering the user wrote by hand.

## Decision

- Configuration is one human-readable `config.toml` (`crates/sevak-core/src/config.rs`),
  documented in [Configuration](../configuration.md). There is no database and
  no hidden settings store; per-feature state files (`usage.json`,
  `clipboard-history.json`, approvals, takeover record) are small JSON files
  that hold data, not preferences.
- The Settings window and the file are two views of the same document. Saving
  parses the existing file with `toml_edit`, merges the changed values into it
  (`merge_document` in `config.rs`) and writes the result to a temporary file
  that is renamed over the original. Comments, ordering and unknown keys the
  user wrote survive a save; the file is never half-written.
- An invalid file is never overwritten. If `config.toml` cannot be parsed,
  Sevak runs with defaults, logs why and leaves the file alone so the user can
  fix the typo (`src-tauri/src/main.rs`).
- Values are clamped on load and save (`Config::normalized`: window width,
  result count, clipboard limits) so a typo or hostile value cannot ask for
  unbounded memory.
- Things that are too fiddly for a form, such as snippet bodies, are edited in
  the file; Settings has a button that opens it.

## Consequences

- The config can be versioned, synced and shared (`SEVAK_CONFIG_DIR`), and a
  user can see everything Sevak will do by reading one file.
- A config file is trusted input: it can define shell commands and snippets.
  Anyone who can write it already runs code as the user, so this is not a
  privilege boundary ([threat model](../security/threat-model.md)). Anything
  that must be consented to separately (script plugins, workflows) lives
  outside `config.toml` for that reason.
- The merge code is the price: comment preservation needs careful handling of
  arrays of tables and has its own tests.
- A shared or synced config folder is a shared trust domain.
