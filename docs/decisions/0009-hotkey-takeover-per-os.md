# ADR-0009: Taking over Super+Space differently on each OS

**Status:** Accepted

## Context

The default launcher key, Super+Space, already belongs to the operating system:
Win+Space switches input language on Windows, Cmd+Space is Spotlight on macOS,
and Super+Space switches input sources in GNOME. A launcher that cannot have
its key feels broken, but an app that silently rewrites system settings or
watches the keyboard without saying so is worse.

## Decision

Each OS gets the least invasive mechanism that works, and anything that
changes a system setting needs an explicit yes.

| OS | Mechanism | Consent |
|---|---|---|
| Windows | A low-level keyboard hook (`WH_KEYBOARD_LL`, `crates/sevak-platform/src/windows/keyhook*.rs`, `hotkey_hook.rs`) installed only while a configured shortcut needs it. It matches key presses to configured shortcuts and swallows them. No Windows setting changes. | None needed: nothing is changed or recorded; removed on quit. |
| macOS | Turns off Spotlight's shortcut (symbolic hotkey 64) with `defaults write` (`spotlight.rs`). | Asked once in a dialog; "no" falls back to Option+Space. |
| GNOME (X11 and Wayland) | Moves the input-source shortcut from Super+Space to Ctrl+Super+Space with `gsettings` (`gnome.rs`); on Wayland the desktop binds `sevak --toggle`. | Asked once, shows old and new values. |
| Other X11 | The global-shortcut plugin grabs the key. | None. |
| Other Wayland | Not possible from inside an app; the user binds `sevak --toggle` in the desktop. | None. |

What was decided and what was changed is recorded in `hotkey-takeover.json` so
Sevak never asks twice and `sevak --restore-hotkey` can undo it
(`src-tauri/src/takeover.rs`). Settings of other applications are never
touched. The Windows hook shares one thread with snippet expansion
([ADR-0011](0011-snippet-expansion-hooks-no-wayland.md)) and the shortcut
recogniser is a pure state machine with unit tests, so what the hook may
inspect and report is narrow and testable.

## Consequences

- The default key works out of the box on Windows, and after one question on
  macOS and GNOME.
- On Windows the hook sees every key press (at Sevak's privilege level) while
  it is installed, which is why the hook module keeps nothing but modifier
  state and reports only which configured shortcut fired. Security software
  may flag any keyboard hook.
- On macOS and GNOME Sevak edits system settings. The record file is the
  single source for undoing it; if it is lost, restoring means doing it by hand.
- Wayland users depend on their desktop's shortcut settings; that is a
  platform limit, not a choice ([ADR-0011](0011-snippet-expansion-hooks-no-wayland.md)).
