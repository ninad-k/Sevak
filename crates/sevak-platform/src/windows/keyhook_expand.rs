//! Windows: a low-level keyboard hook (`WH_KEYBOARD_LL`) and a mouse hook
//! (`WH_MOUSE_LL`) that feed snippet expansion with [`KeyEvent`]s.
//!
//! This module exists only for that feature and is deliberately self-contained
//! (no other module touches these hooks), so it can be folded into a shared
//! keyboard hook later. Only installed while `[snippets] auto_expand` is on.
//!
//! What the hooks do and do not do:
//!
//! - A hook procedure must return quickly or Windows drops the hook, so it only
//!   translates the key and hands a [`KeyEvent`] to the sink (a channel send).
//!   It never swallows a key: `CallNextHookEx` is always called.
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

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Mutex;
use std::thread::JoinHandle;

use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyState, GetKeyboardLayout, ToUnicodeEx, HKL, VK_BACK, VK_CAPITAL,
    VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL, VK_RMENU,
    VK_RSHIFT, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PeekMessageW, PostThreadMessageW,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, HC_ACTION, KBDLLHOOKSTRUCT,
    LLKHF_INJECTED, LLKHF_LOWER_IL_INJECTED, MSG, PM_NOREMOVE, WH_KEYBOARD_LL, WH_MOUSE_LL,
    WM_KEYDOWN, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_NCLBUTTONDOWN, WM_NCRBUTTONDOWN, WM_QUIT,
    WM_RBUTTONDOWN, WM_SYSKEYDOWN, WM_USER, WM_XBUTTONDOWN,
};

use crate::error::{PlatformError, Result};
use crate::keyboard::{compose_dead_key, KeyEvent, KeyListener, KeySink};

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

/// Stamped on every key event Sevak injects (`dwExtraInfo`), so its own
/// Backspaces and paste are never mistaken for typing.
pub(super) const OWN_EXTRA_INFO: usize = 0x5345_5641;

/// Debug builds can be told to treat injected key events as typing, so the
/// feature can be driven by a script (`SendInput`) in a manual test. Release
/// builds have no such switch: injected events are always ignored.
#[cfg(debug_assertions)]
static ACCEPT_INJECTED: AtomicBool = AtomicBool::new(false);

#[cfg(debug_assertions)]
fn accept_injected() -> bool {
    ACCEPT_INJECTED.load(Ordering::SeqCst)
}

#[cfg(not(debug_assertions))]
fn accept_injected() -> bool {
    false
}

/// Only one hook pair can be installed at a time.
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

fn on_key(message: u32, info: &KBDLLHOOKSTRUCT) {
    if message != WM_KEYDOWN && message != WM_SYSKEYDOWN {
        return;
    }
    let injected = info.flags.0 & (LLKHF_INJECTED.0 | LLKHF_LOWER_IL_INJECTED.0) != 0;
    if info.dwExtraInfo == OWN_EXTRA_INFO || (injected && !accept_injected()) {
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

fn on_mouse(message: u32) {
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

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && lparam.0 != 0 {
        // SAFETY: for HC_ACTION, `lparam` points to a KBDLLHOOKSTRUCT that is
        // valid for the duration of this call.
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        // A panic must not unwind into Windows.
        let _ = catch_unwind(AssertUnwindSafe(|| on_key(wparam.0 as u32, info)));
    }
    // SAFETY: forwarding the arguments we were given.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let _ = catch_unwind(AssertUnwindSafe(|| on_mouse(wparam.0 as u32)));
    }
    // SAFETY: forwarding the arguments we were given.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// What the hook thread reports once it has tried to install the hooks.
type Started = std::result::Result<u32, String>;

/// Installs the hooks on a thread of their own (a low-level hook is called
/// through that thread's message loop) and returns the handle that removes them.
pub(crate) fn start(sink: KeySink) -> Result<KeyListener> {
    #[cfg(debug_assertions)]
    ACCEPT_INJECTED.store(
        std::env::var_os("SEVAK_TEST_ACCEPT_INJECTED_KEYS").is_some(),
        Ordering::SeqCst,
    );
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

    let (started_tx, started_rx) = mpsc::channel::<Started>();
    let thread = std::thread::Builder::new()
        .name("sevak-keyhook".to_owned())
        .spawn(move || hook_thread(&started_tx));
    let handle = match thread {
        Ok(handle) => handle,
        Err(err) => {
            release();
            return Err(PlatformError::Io(err));
        }
    };

    match started_rx.recv() {
        Ok(Ok(thread_id)) => Ok(KeyListener::new(move || stop(thread_id, handle))),
        Ok(Err(message)) => {
            let _ = handle.join();
            release();
            Err(PlatformError::Os {
                operation: "SetWindowsHookExW",
                message,
            })
        }
        Err(_) => {
            let _ = handle.join();
            release();
            Err(PlatformError::Os {
                operation: "SetWindowsHookExW",
                message: "the hook thread ended unexpectedly".to_owned(),
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

fn stop(thread_id: u32, handle: JoinHandle<()>) {
    // SAFETY: plain Win32 call; the thread has a message queue (it made one
    // before reporting its id) and WM_QUIT ends its loop.
    unsafe {
        let _ = PostThreadMessageW(thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
    }
    let _ = handle.join();
    release();
}

fn hook_thread(started: &mpsc::Sender<Started>) {
    // SAFETY: Win32 hook installation and the message loop that serves it, all
    // on this thread; the hooks are removed before the thread ends.
    unsafe {
        let module = GetModuleHandleW(None)
            .ok()
            .map(|module| HINSTANCE(module.0));
        let keyboard = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0) {
            Ok(hook) => hook,
            Err(err) => {
                let _ = started.send(Err(err.to_string()));
                return;
            }
        };
        // Clicks only make the buffer forget; without this hook a click that
        // moves the caret would go unnoticed, so it is optional but wanted.
        let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0).ok();
        if mouse.is_none() {
            tracing::warn!("the mouse hook could not be installed; clicks will not reset typing");
        }

        // A thread gets its message queue on first use; `stop` needs it.
        let mut message = MSG::default();
        let _ = PeekMessageW(&mut message, None, WM_USER, WM_USER, PM_NOREMOVE);
        let _ = started.send(Ok(GetCurrentThreadId()));

        while GetMessageW(&mut message, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }

        let _ = UnhookWindowsHookEx(keyboard);
        if let Some(mouse) = mouse {
            let _ = UnhookWindowsHookEx(mouse);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// The whole path with a real window: the hook sees `SendInput` keys (this
    /// debug-build switch lets it), a stand-in matcher spots "sig", and
    /// `replace_typed_text` swaps it for other text in the focused edit control.
    /// Needs an interactive desktop, steals focus for a moment and overwrites the
    /// clipboard text briefly (it is restored): `cargo test -p sevak-platform
    /// typing_a_keyword_is_replaced_in_a_real_window -- --ignored`
    #[test]
    #[ignore = "needs an interactive desktop"]
    fn typing_a_keyword_is_replaced_in_a_real_window() {
        use std::sync::mpsc;
        use std::time::Duration;
        use windows::core::{w, PCWSTR};
        use windows::Win32::UI::Input::KeyboardAndMouse::{VK_G, VK_I, VK_S, VK_SPACE, VK_X};
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, DispatchMessageW, GetMessageW, GetWindowTextW,
            PostMessageW, TranslateMessage, MSG, WINDOW_EX_STYLE, WM_CLOSE, WS_OVERLAPPEDWINDOW,
            WS_VISIBLE,
        };

        use super::super::paste::{focus, hwnd_to_int, int_to_hwnd, key_input, send_inputs};

        const TITLE: &str = "sevak expand target";
        let (tx, rx) = mpsc::channel();
        let window = std::thread::spawn(move || unsafe {
            let title: Vec<u16> = TITLE.encode_utf16().chain([0]).collect();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("EDIT"),
                PCWSTR(title.as_ptr()),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                100,
                100,
                400,
                200,
                None,
                None,
                None,
                None,
            )
            .expect("create an edit window");
            tx.send(hwnd_to_int(hwnd)).unwrap();
            let mut message = MSG::default();
            while GetMessageW(&mut message, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            let _ = DestroyWindow(hwnd);
        });
        let target = int_to_hwnd(rx.recv().unwrap());
        assert!(focus(target).is_ok(), "could not focus the window");

        // A matcher in miniature: the last characters typed, and a signal when
        // they end with the keyword.
        let (found_tx, found_rx) = mpsc::channel();
        let typed = Mutex::new(String::new());
        std::env::set_var("SEVAK_TEST_ACCEPT_INJECTED_KEYS", "1");
        let listener = start(Box::new(move |event| {
            let mut typed = typed.lock().unwrap();
            match event {
                KeyEvent::Char(c) => typed.push(c),
                KeyEvent::Backspace => {
                    typed.pop();
                }
                KeyEvent::Reset => typed.clear(),
            }
            if typed.ends_with("sig") {
                typed.clear();
                let _ = found_tx.send(());
            }
        }))
        .expect("the hooks install");

        let press = |key| {
            send_inputs(&[key_input(key, false), key_input(key, true)]).unwrap();
            std::thread::sleep(Duration::from_millis(30));
        };
        for key in [VK_X, VK_SPACE, VK_S, VK_I, VK_G] {
            press(key);
        }
        let fired = found_rx.recv_timeout(Duration::from_secs(3)).is_ok();
        if fired {
            let done = super::super::expand::replace_typed_text(3, "Best regards", &|| true);
            assert!(done.unwrap());
        }
        std::thread::sleep(Duration::from_millis(200));

        let mut buffer = [0u16; 128];
        let len = unsafe { GetWindowTextW(target, &mut buffer) } as usize;
        let text = String::from_utf16_lossy(&buffer[..len.min(buffer.len())]);
        drop(listener);
        unsafe {
            let _ = PostMessageW(
                Some(target),
                WM_CLOSE,
                Default::default(),
                Default::default(),
            );
        }
        drop(window);

        assert!(fired, "the hook never reported the keyword");
        // Typed at the caret, in front of the title text the control started with.
        assert_eq!(text, format!("x Best regards{TITLE}"));
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
