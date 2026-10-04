//! Windows: what snippet expansion takes from the keyboard and mouse hooks.
//!
//! The hooks themselves (`WH_KEYBOARD_LL`, `WH_MOUSE_LL`) and their thread are
//! shared with the global hotkeys: see [`keyhook`]. This module turns the key
//! events the shared hook passes it into [`KeyEvent`]s for the expansion
//! feature, which is opt-in (`[snippets] auto_expand`): it is only on the hook
//! while that setting is on.
//!
//! What it does and does not do:
//!
//! - A hook procedure must return quickly or Windows drops the hook, so it only
//!   translates the key and hands a [`KeyEvent`] to the sink (a channel send).
//!   It never swallows a key.
//! - Keys are translated with `ToUnicodeEx` and the layout of the thread that
//!   owns the foreground window, with flag 4 ("do not change the keyboard
//!   state"), so the dead keys of the app being typed into keep working. For the
//!   same reason a dead key is followed by hand: see [`DeadKeys`].
//! - Events carrying `LLKHF_INJECTED` (Sevak's own Backspaces and paste, and
//!   other programs' `SendInput`) are ignored.
//! - A change of foreground window, a mouse click, and any key that is not plain
//!   typing become [`KeyEvent::Reset`]. IME composition (`VK_PROCESSKEY`) is a
//!   reset too: Sevak never tries to follow it.
//! - Nothing about a key or a character is logged.
//!
//! Windows only delivers events from windows at Sevak's own integrity level or
//! lower, so typing into an app running as administrator is not seen when Sevak
//! is not (and expansion could not press keys there anyway).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyState, GetKeyboardLayout, ToUnicodeEx, HKL, VK_BACK, VK_CAPITAL,
    VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL, VK_RMENU,
    VK_RSHIFT, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    KBDLLHOOKSTRUCT, KBDLLHOOKSTRUCT_FLAGS, LLKHF_INJECTED, LLKHF_LOWER_IL_INJECTED, WM_KEYDOWN,
    WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_NCLBUTTONDOWN, WM_NCRBUTTONDOWN, WM_RBUTTONDOWN,
    WM_SYSKEYDOWN, WM_XBUTTONDOWN,
};

use crate::error::{PlatformError, Result};
use crate::keyboard::{compose_dead_key, KeyEvent, KeyListener, KeySink};

use super::keyhook::{self, Client, OWN_EXTRA_INFO};
use super::paste::{foreground_window, hwnd_to_int, window_owner};

/// `ToUnicodeEx` flag: do not change the keyboard state (Windows 10 1607+).
const TO_UNICODE_KEEP_STATE: u32 = 0x4;

/// Virtual-key codes the `windows` crate does not name as `VIRTUAL_KEY`s we use
/// in a `match` on a plain number.
const VK_NUMLOCK: u32 = 0x90;
const VK_SCROLL: u32 = 0x91;
const VK_APPS: u32 = 0x5D;
const VK_PROCESSKEY: u32 = 0xE5;
const VK_PACKET: u32 = 0xE7;

/// Whether a key event is not the user's typing: Sevak's own (stamped with
/// [`OWN_EXTRA_INFO`]) or injected by any program (`SendInput`, remote tools,
/// automation). Injected events are always ignored, in every build: nothing
/// but real key presses can make Sevak expand a snippet.
fn is_ignored(flags: KBDLLHOOKSTRUCT_FLAGS, extra_info: usize) -> bool {
    extra_info == OWN_EXTRA_INFO || flags.0 & (LLKHF_INJECTED.0 | LLKHF_LOWER_IL_INJECTED.0) != 0
}

/// Only one listener can run at a time.
static ACTIVE: AtomicBool = AtomicBool::new(false);
static STATE: Mutex<Option<HookState>> = Mutex::new(None);

struct HookState {
    sink: KeySink,
    /// The foreground window (an `HWND` as an integer) of the previous event.
    foreground: isize,
    dead: DeadKeys,
}

impl HookState {
    fn emit(&mut self, event: KeyEvent) {
        (self.sink)(event);
    }

    fn reset(&mut self) {
        self.dead.clear();
        self.emit(KeyEvent::Reset);
    }

    /// A reset if the foreground window is not the one the last event was for.
    fn note_foreground(&mut self) {
        let current = foreground_window().map_or(0, hwnd_to_int);
        if current != self.foreground {
            self.foreground = current;
            self.reset();
        }
    }
}

/// The modifier keys that are down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Modifiers {
    shift: bool,
    ctrl: bool,
    alt: bool,
    win: bool,
}

/// What a key press means for text expansion, before any translation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyClass {
    /// A modifier or lock key by itself: not typing, but not a reason to forget
    /// what was typed either.
    Ignore,
    /// Something that moves the caret, edits otherwise, or starts a shortcut.
    Reset,
    Backspace,
    /// Try to translate it to characters.
    Text,
}

fn classify(vk: u32, mods: Modifiers) -> KeyClass {
    match vk {
        v if v == u32::from(VK_SHIFT.0)
            || v == u32::from(VK_LSHIFT.0)
            || v == u32::from(VK_RSHIFT.0)
            || v == u32::from(VK_CONTROL.0)
            || v == u32::from(VK_LCONTROL.0)
            || v == u32::from(VK_RCONTROL.0)
            || v == u32::from(VK_MENU.0)
            || v == u32::from(VK_LMENU.0)
            || v == u32::from(VK_RMENU.0)
            || v == u32::from(VK_CAPITAL.0)
            || v == VK_NUMLOCK
            || v == VK_SCROLL =>
        {
            KeyClass::Ignore
        }
        v if v == u32::from(VK_LWIN.0) || v == u32::from(VK_RWIN.0) || v == VK_APPS => {
            KeyClass::Reset
        }
        VK_PROCESSKEY | VK_PACKET => KeyClass::Reset,
        _ if mods.win => KeyClass::Reset,
        // Ctrl+Alt together is how AltGr reaches us; either one alone is a shortcut.
        _ if mods.ctrl != mods.alt => KeyClass::Reset,
        v if v == u32::from(VK_BACK.0) => {
            if mods.ctrl {
                // Ctrl+Alt+Backspace.
                KeyClass::Reset
            } else {
                KeyClass::Backspace
            }
        }
        _ => KeyClass::Text,
    }
}

/// What `ToUnicodeEx` made of a key.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Translation {
    /// The characters typed.
    Chars(String),
    /// A dead key: nothing is typed yet; this is the accent it waits to add.
    Dead(char),
    /// The key types nothing.
    Nothing,
}

/// Follows a dead key by hand. Translating with the keyboard state left alone
/// (so the app's own dead-key handling is undisturbed) means `ToUnicodeEx`
/// never composes `´` then `e` for us.
#[derive(Debug, Default)]
struct DeadKeys {
    pending: Option<char>,
}

impl DeadKeys {
    fn clear(&mut self) {
        self.pending = None;
    }

    /// The events for one translated key press.
    fn feed(&mut self, translation: Translation) -> Vec<KeyEvent> {
        let pending = self.pending.take();
        match translation {
            Translation::Nothing => vec![KeyEvent::Reset],
            Translation::Dead(accent) => {
                if pending.is_some() {
                    // Two dead keys in a row: the layout decides; we do not know.
                    return vec![KeyEvent::Reset];
                }
                self.pending = Some(accent);
                Vec::new()
            }
            Translation::Chars(text) => {
                if text.chars().any(char::is_control) {
                    return vec![KeyEvent::Reset];
                }
                let mut chars: Vec<char> = text.chars().collect();
                if let Some(accent) = pending {
                    // The accent combines with the first character typed.
                    let Some(first) = chars.first().copied() else {
                        return vec![KeyEvent::Reset];
                    };
                    // (A space gives the accent itself.)
                    match compose_dead_key(accent, first) {
                        Some(composed) => chars[0] = composed,
                        None => return vec![KeyEvent::Reset],
                    }
                }
                chars.into_iter().map(KeyEvent::Char).collect()
            }
        }
    }
}

fn is_down(key: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY) -> bool {
    // SAFETY: plain Win32 call taking a virtual-key code. The top bit of the
    // result is "currently down".
    let state = unsafe { GetAsyncKeyState(i32::from(key.0)) };
    state as u16 & 0x8000 != 0
}

fn modifiers() -> Modifiers {
    Modifiers {
        shift: is_down(VK_SHIFT),
        ctrl: is_down(VK_CONTROL),
        alt: is_down(VK_MENU),
        win: is_down(VK_LWIN) || is_down(VK_RWIN),
    }
}

/// Translates a key press with the layout of the foreground window's thread.
fn translate(info: &KBDLLHOOKSTRUCT, mods: Modifiers) -> Translation {
    let mut keyboard = [0u8; 256];
    let mut set = |key: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY, on: bool| {
        if on {
            keyboard[usize::from(key.0)] = 0x80;
        }
    };
    set(VK_SHIFT, mods.shift);
    set(VK_CONTROL, mods.ctrl);
    set(VK_MENU, mods.alt);
    // SAFETY: plain Win32 call taking a virtual-key code; bit 0 is the toggle.
    let caps = unsafe { GetKeyState(i32::from(VK_CAPITAL.0)) } & 1 != 0;
    keyboard[usize::from(VK_CAPITAL.0)] = u8::from(caps);

    let thread = foreground_window()
        .and_then(window_owner)
        .map_or(0, |(_, thread)| thread);
    // SAFETY: plain Win32 call; thread 0 means the calling thread's layout.
    let layout: HKL = unsafe { GetKeyboardLayout(thread) };

    let mut buffer = [0u16; 8];
    // SAFETY: `keyboard` is the 256-byte array the call requires and `buffer`
    // is valid for its length; both outlive the call.
    let written = unsafe {
        ToUnicodeEx(
            info.vkCode,
            info.scanCode,
            &keyboard,
            &mut buffer,
            TO_UNICODE_KEEP_STATE,
            Some(layout),
        )
    };
    match written {
        0 => Translation::Nothing,
        n if n < 0 => char::decode_utf16(buffer.iter().copied().take(1))
            .next()
            .and_then(std::result::Result::ok)
            .map_or(Translation::Nothing, Translation::Dead),
        n => {
            let len = usize::try_from(n).unwrap_or(0).min(buffer.len());
            Translation::Chars(String::from_utf16_lossy(&buffer[..len]))
        }
    }
}

/// Called by the shared hook for every keyboard event.
pub(super) fn on_key(message: u32, info: &KBDLLHOOKSTRUCT) {
    if message != WM_KEYDOWN && message != WM_SYSKEYDOWN {
        return;
    }
    if is_ignored(info.flags, info.dwExtraInfo) {
        return;
    }
    let mut guard = STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(state) = guard.as_mut() else { return };

    state.note_foreground();
    let mods = modifiers();
    match classify(info.vkCode, mods) {
        KeyClass::Ignore => {}
        KeyClass::Reset => state.reset(),
        KeyClass::Backspace => {
            state.dead.clear();
            state.emit(KeyEvent::Backspace);
        }
        KeyClass::Text => {
            let translation = translate(info, mods);
            for event in state.dead.feed(translation) {
                state.emit(event);
            }
        }
    }
}

/// Called by the shared hook for every mouse event.
pub(super) fn on_mouse(message: u32) {
    let clicked = matches!(
        message,
        WM_LBUTTONDOWN
            | WM_RBUTTONDOWN
            | WM_MBUTTONDOWN
            | WM_XBUTTONDOWN
            | WM_NCLBUTTONDOWN
            | WM_NCRBUTTONDOWN
    );
    if !clicked {
        return;
    }
    let mut guard = STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(state) = guard.as_mut() {
        state.reset();
    }
}

/// Puts the listener on the shared keyboard hook thread (see [`keyhook`]) and
/// returns the handle that takes it off again.
pub(crate) fn start(sink: KeySink) -> Result<KeyListener> {
    if ACTIVE.swap(true, Ordering::SeqCst) {
        return Err(PlatformError::Os {
            operation: "SetWindowsHookExW",
            message: "the keyboard listener is already running".to_owned(),
        });
    }
    *STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(HookState {
        sink,
        foreground: foreground_window().map_or(0, hwnd_to_int),
        dead: DeadKeys::default(),
    });

    match keyhook::acquire(Client::Expansion) {
        Ok(()) => Ok(KeyListener::new(stop)),
        Err(message) => {
            release();
            Err(PlatformError::Os {
                operation: "SetWindowsHookExW",
                message,
            })
        }
    }
}

fn release() {
    *STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    ACTIVE.store(false, Ordering::SeqCst);
}

fn stop() {
    // The hook thread may be inside `on_key` waiting for `STATE`, so `STATE`
    // is only cleared once the listener is off the hook thread.
    keyhook::release(Client::Expansion);
    release();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    const NONE: Modifiers = Modifiers {
        shift: false,
        ctrl: false,
        alt: false,
        win: false,
    };

    fn with(f: impl FnOnce(&mut Modifiers)) -> Modifiers {
        let mut mods = NONE;
        f(&mut mods);
        mods
    }

    #[test]
    fn letters_and_digits_are_text() {
        assert_eq!(classify(u32::from(b'A'), NONE), KeyClass::Text);
        assert_eq!(classify(u32::from(b'7'), NONE), KeyClass::Text);
        assert_eq!(classify(0x20, NONE), KeyClass::Text); // space
        assert_eq!(
            classify(u32::from(b'A'), with(|m| m.shift = true)),
            KeyClass::Text
        );
    }

    #[test]
    fn modifier_and_lock_keys_alone_change_nothing() {
        for vk in [
            0x10, 0xA0, 0xA1, 0x11, 0xA2, 0xA3, 0x12, 0xA4, 0xA5, 0x14, 0x90, 0x91,
        ] {
            assert_eq!(classify(vk, NONE), KeyClass::Ignore, "{vk:#x}");
        }
    }

    #[test]
    fn the_windows_key_and_ime_input_reset() {
        for vk in [0x5B, 0x5C, 0x5D, VK_PROCESSKEY, VK_PACKET] {
            assert_eq!(classify(vk, NONE), KeyClass::Reset, "{vk:#x}");
        }
    }

    #[test]
    fn shortcuts_reset() {
        let letter = u32::from(b'C');
        assert_eq!(classify(letter, with(|m| m.ctrl = true)), KeyClass::Reset);
        assert_eq!(classify(letter, with(|m| m.alt = true)), KeyClass::Reset);
        assert_eq!(classify(letter, with(|m| m.win = true)), KeyClass::Reset);
        assert_eq!(
            classify(
                letter,
                with(|m| {
                    m.ctrl = true;
                    m.shift = true;
                })
            ),
            KeyClass::Reset
        );
    }

    #[test]
    fn altgr_is_ctrl_plus_alt_and_types() {
        let altgr = with(|m| {
            m.ctrl = true;
            m.alt = true;
        });
        assert_eq!(classify(u32::from(b'E'), altgr), KeyClass::Text);
    }

    #[test]
    fn backspace_is_tracked_but_ctrl_backspace_is_not() {
        assert_eq!(classify(0x08, NONE), KeyClass::Backspace);
        assert_eq!(
            classify(0x08, with(|m| m.shift = true)),
            KeyClass::Backspace
        );
        assert_eq!(classify(0x08, with(|m| m.ctrl = true)), KeyClass::Reset);
    }

    fn chars(text: &str) -> Translation {
        Translation::Chars(text.to_owned())
    }

    #[test]
    fn plain_characters_pass_through() {
        let mut dead = DeadKeys::default();
        assert_eq!(dead.feed(chars("a")), [KeyEvent::Char('a')]);
        assert_eq!(
            dead.feed(chars("ab")),
            [KeyEvent::Char('a'), KeyEvent::Char('b')]
        );
    }

    #[test]
    fn control_characters_and_untypable_keys_reset() {
        let mut dead = DeadKeys::default();
        assert_eq!(dead.feed(chars("\r")), [KeyEvent::Reset]);
        assert_eq!(dead.feed(chars("\u{1b}")), [KeyEvent::Reset]);
        assert_eq!(dead.feed(Translation::Nothing), [KeyEvent::Reset]);
    }

    #[test]
    fn a_dead_key_waits_and_then_composes() {
        let mut dead = DeadKeys::default();
        assert_eq!(dead.feed(Translation::Dead('´')), []);
        assert_eq!(dead.feed(chars("e")), [KeyEvent::Char('é')]);
        // And it is over: the next letter is itself.
        assert_eq!(dead.feed(chars("e")), [KeyEvent::Char('e')]);
    }

    #[test]
    fn a_dead_key_then_space_types_the_accent() {
        let mut dead = DeadKeys::default();
        dead.feed(Translation::Dead('^'));
        assert_eq!(dead.feed(chars(" ")), [KeyEvent::Char('^')]);
    }

    #[test]
    fn an_unknown_combination_resets() {
        let mut dead = DeadKeys::default();
        dead.feed(Translation::Dead('´'));
        assert_eq!(dead.feed(chars("x")), [KeyEvent::Reset]);
        let mut dead = DeadKeys::default();
        dead.feed(Translation::Dead('´'));
        assert_eq!(dead.feed(Translation::Dead('`')), [KeyEvent::Reset]);
    }

    #[test]
    fn clearing_forgets_a_pending_accent() {
        let mut dead = DeadKeys::default();
        dead.feed(Translation::Dead('´'));
        dead.clear();
        assert_eq!(dead.feed(chars("e")), [KeyEvent::Char('e')]);
    }

    #[test]
    fn injected_and_own_key_events_are_always_ignored() {
        let none = KBDLLHOOKSTRUCT_FLAGS(0);
        assert!(!is_ignored(none, 0), "a real key press is typing");
        assert!(is_ignored(LLKHF_INJECTED, 0));
        assert!(is_ignored(LLKHF_LOWER_IL_INJECTED, 0));
        assert!(is_ignored(
            KBDLLHOOKSTRUCT_FLAGS(LLKHF_INJECTED.0 | LLKHF_LOWER_IL_INJECTED.0),
            7
        ));
        // Sevak's own events, injected or not.
        assert!(is_ignored(none, OWN_EXTRA_INFO));
        assert!(is_ignored(LLKHF_INJECTED, OWN_EXTRA_INFO));
    }

    /// Installs the real hooks and checks that they come up and go down again,
    /// without typing anything. `cargo test -p sevak-platform
    /// the_hooks_install_and_uninstall -- --ignored`
    #[test]
    #[ignore = "installs a global keyboard hook"]
    fn the_hooks_install_and_uninstall() {
        let listener = start(Box::new(|_| {})).expect("the hooks install");
        // Only one at a time.
        assert!(start(Box::new(|_| {})).is_err());
        drop(listener);
        // And they can be installed again.
        drop(start(Box::new(|_| {})).expect("the hooks install again"));
    }

    /// The interactive check: install the hooks, then press keys by hand in any
    /// window for ten seconds. Prints what the hook reported (here, in a test
    /// about the hook itself, characters may be shown). `cargo test -p
    /// sevak-platform watching_real_typing -- --ignored --nocapture`
    #[test]
    #[ignore = "needs a human typing at the keyboard"]
    fn watching_real_typing() {
        let (tx, rx) = mpsc::channel();
        let listener = start(Box::new(move |event| {
            let _ = tx.send(event);
        }))
        .expect("the hooks install");
        println!("type something, in any window, for 10 seconds");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut typed = String::new();
        while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(KeyEvent::Char(c)) => typed.push(c),
                Ok(KeyEvent::Backspace) => {
                    typed.pop();
                }
                Ok(KeyEvent::Reset) => typed.push('|'),
                Err(_) => break,
            }
        }
        drop(listener);
        println!("the hook saw: {typed:?}");
    }
}
