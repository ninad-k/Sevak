# ADR-0002: Tauri shell, Rust core, plugin architecture

**Status:** Accepted

## Context

A launcher must open instantly, answer every keystroke in milliseconds, use
little memory while idle and run on Windows, macOS and Linux. Most of the code
(matching, ranking, plugins) is platform independent; a small part (launching,
icons, hotkeys, pasting) is not.

## Decision

- **Shell:** Tauri v2 with a Svelte 5 front end draws the window and hosts the
  IPC (`src-tauri/`, `ui/`). The page draws results and forwards key presses;
  it is not given file, shell or network permissions
  ([threat model](../security/threat-model.md#webview-and-ipc)).
- **Core in Rust:** `sevak-core` (engine, config, fuzzy matching, usage),
  `sevak-plugins` (the features) and `sevak-platform` (OS access) are separate
  crates. Only `sevak-platform` contains OS-specific code.
- **Every feature is a plugin.** Each one (apps, calculator, files, clipboard
  history, ...) implements the `Plugin` trait in
  `crates/sevak-core/src/plugin.rs`: `query` on a worker thread for every
  keystroke, `execute` for the chosen result, and an optional `confirmation`
  before anything irreversible.
- **Extensible without a rebuild:** script plugins and workflows run as
  separate processes behind the same trait
  ([ADR-0005](0005-approval-before-run.md)).

## Consequences

- Fast startup and low idle cost; one language for the core and the OS
  integration.
- A built-in plugin runs inside the Sevak process, so a bug in one affects the
  whole launcher. Code that is not trusted goes through the script-plugin or
  workflow path, which uses separate processes.
- Tauri uses the system web view (WebView2, WKWebView, WebKitGTK), whose
  security updates arrive through the operating system, not through Sevak.
- Contributors need a Rust toolchain; writing a script plugin does not.
