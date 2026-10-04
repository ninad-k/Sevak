# ADR-0011: Snippet expansion through low-level input hooks, and no Wayland support

**Status:** Accepted

## Context

Typing `;sig` anywhere and getting a signature requires seeing what the user
types in other applications and replacing it. That is keylogger-shaped
capability, even when the intent is benign. The operating systems offer it
through different, sometimes unavailable, interfaces.

## Decision

- Expansion is **off by default** (`[snippets] auto_expand`). While it is off,
  Sevak does not listen to the keyboard for snippets at all.
- When on, it uses the narrowest interface each OS offers: a low-level hook on
  Windows (`WH_KEYBOARD_LL`), a listen-only event tap on macOS (needs the
  Input Monitoring permission), the X server's RECORD extension on X11. The
  recogniser keeps only the last 64 typed characters, in memory, and wipes them
  whenever the text could have changed under them and after every expansion
  (`crates/sevak-plugins/src/snippet_expansion.rs`). They are never written to
  disk, logged or sent anywhere.
- Input Sevak cannot trust to be a typed character is ignored: key events that
  Sevak or another program injected on Windows, Sevak's own windows, terminals
  (unless enabled), apps in `ignore_apps`, and password boxes the OS can detect.
- Expansion deletes the typed keyword with Backspace presses and pastes the
  text, saving and restoring the clipboard and keeping it out of clipboard
  history.
- **Wayland is not supported**, and this will not change from inside Sevak:
  the Wayland security model does not let a client observe keystrokes in other
  clients or synthesise input for them. Working around it would need either
  compositor-specific protocols or privileged input devices (`/dev/input`,
  `uinput`), which would widen what Sevak can do far beyond a launcher. Under
  Wayland the Settings window says so.

## Consequences

- Users of Wayland sessions have no auto-expansion, and no Universal Actions
  selection capture or paste either; the launcher itself works through a
  desktop-bound `sevak --toggle` ([ADR-0009](0009-hotkey-takeover-per-os.md)).
- Security software may flag Sevak when the hook is installed. The privacy page
  and Settings state this when the option is turned on.
- A password field inside a web page cannot be detected on Windows or Linux, so
  the documentation tells users to put their browser in `ignore_apps` or to
  choose a prefix they never type in a password.
- Because the hook sees all typing at Sevak's privilege level, a bug in that
  code is among the most security-sensitive in the project; it is reviewed in
  the [threat model](../security/threat-model.md#windows-keyboard-hook).
