//! Windows: replace the text just typed (Backspaces, then Ctrl+V) and tell
//! where typing goes (which app, whether the focused control is a password box).

use std::thread::sleep;
use std::time::Duration;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{INPUT, VIRTUAL_KEY, VK_BACK, VK_CONTROL, VK_V};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetGUIThreadInfo, GetWindowLongPtrW, GUITHREADINFO, GWL_STYLE,
};

use crate::error::Result;
use crate::expand::{self, ExpandDriver, SystemExpandClipboard};
use crate::keyboard::TypingTarget;

use super::capture::wait_for_modifier_release;
use super::keyhook_expand::OWN_EXTRA_INFO;
use super::paste::{
    app_of, foreground_window, is_own_window, key_input, send_inputs, window_owner,
};

/// `ES_PASSWORD`: the edit control shows dots for what is typed.
const ES_PASSWORD: isize = 0x20;

/// Pause between the keys of the paste shortcut.
const KEY_GAP: Duration = Duration::from_millis(40);

/// How many Backspaces go to the OS in one call.
const BACKSPACES_PER_CALL: usize = 64;

/// A key event stamped as Sevak's own, which the keyboard hook skips.
fn own_key(key: VIRTUAL_KEY, up: bool) -> INPUT {
    let mut input = key_input(key, up);
    // `key_input` builds a keyboard event, so `ki` is the active field.
    input.Anonymous.ki.dwExtraInfo = OWN_EXTRA_INFO;
    input
}

pub(crate) fn replace_typed_text(
    delete: usize,
    text: &str,
    still_current: &dyn Fn() -> bool,
) -> Result<bool> {
    expand::replace_typed_text(
        delete,
        text,
        still_current,
        &SystemExpandClipboard,
        &WindowsExpand,
        true,
    )
}

struct WindowsExpand;

impl ExpandDriver for WindowsExpand {
    fn release_modifiers(&self) {
        wait_for_modifier_release();
    }

    fn press_backspaces(&self, count: usize) -> Result<()> {
        let mut remaining = count;
        while remaining > 0 {
            let batch = remaining.min(BACKSPACES_PER_CALL);
            let inputs: Vec<INPUT> = (0..batch)
                .flat_map(|_| [own_key(VK_BACK, false), own_key(VK_BACK, true)])
                .collect();
            send_inputs(&inputs)?;
            remaining -= batch;
        }
        Ok(())
    }

    fn press_paste(&self) -> Result<()> {
        // Apart, with a beat between: some apps read the Ctrl state when they
        // handle the V, not when it was pressed, and would otherwise see Ctrl
        // already up and type a "v".
        send_inputs(&[own_key(VK_CONTROL, false)])?;
        sleep(KEY_GAP);
        send_inputs(&[own_key(VK_V, false), own_key(VK_V, true)])?;
        sleep(KEY_GAP);
        send_inputs(&[own_key(VK_CONTROL, true)])
    }
}

pub(crate) fn typing_target() -> TypingTarget {
    let Some(foreground) = foreground_window() else {
        return TypingTarget::default();
    };
    TypingTarget {
        app: app_of(foreground),
        own_window: is_own_window(foreground),
        private: focus_is_password_box(foreground),
    }
}

/// True if the control with the keyboard focus in `foreground`'s thread is an
/// edit control in password mode. This sees classic and most framework edit
/// controls; it cannot see inside a web page (a browser's password field), which
/// would need UI Automation.
fn focus_is_password_box(foreground: HWND) -> bool {
    let Some((_, thread)) = window_owner(foreground) else {
        return false;
    };
    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..GUITHREADINFO::default()
    };
    // SAFETY: `info` is initialized with the right `cbSize` and outlives the call.
    if unsafe { GetGUIThreadInfo(thread, &mut info) }.is_err() || info.hwndFocus.0.is_null() {
        return false;
    }
    let focus = info.hwndFocus;

    let mut class = [0u16; 64];
    // SAFETY: `class` is valid for its length; a stale handle makes the call fail.
    let len = unsafe { GetClassNameW(focus, &mut class) };
    let len = usize::try_from(len).unwrap_or(0).min(class.len());
    let class = String::from_utf16_lossy(&class[..len]).to_ascii_lowercase();
    if !class.contains("edit") {
        return false;
    }
    // SAFETY: plain Win32 call on a window handle; failure reads as 0.
    let style = unsafe { GetWindowLongPtrW(focus, GWL_STYLE) };
    style & ES_PASSWORD != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_typing_target_can_be_described() {
        // Nothing to assert about which window is in front (a headless CI
        // session may have none); it must just work.
        let _ = typing_target();
    }

    /// Types into a real password box. Needs an interactive desktop and steals
    /// focus for a moment: `cargo test -p sevak-platform
    /// a_password_box_is_private -- --ignored`
    #[test]
    #[ignore = "needs an interactive desktop"]
    fn a_password_box_is_private() {
        use std::sync::mpsc;
        use windows::core::{w, PCWSTR};
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, DispatchMessageW, GetMessageW, PostMessageW,
            TranslateMessage, MSG, WINDOW_EX_STYLE, WM_CLOSE, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
        };

        fn open(style: u32) -> (isize, std::thread::JoinHandle<()>) {
            let (tx, rx) = mpsc::channel();
            let handle = std::thread::spawn(move || unsafe {
                let title: Vec<u16> = "sevak password target".encode_utf16().chain([0]).collect();
                let hwnd = CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("EDIT"),
                    PCWSTR(title.as_ptr()),
                    WS_OVERLAPPEDWINDOW
                        | WS_VISIBLE
                        | windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(style),
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
                tx.send(super::super::paste::hwnd_to_int(hwnd)).unwrap();
                let mut message = MSG::default();
                while GetMessageW(&mut message, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                let _ = DestroyWindow(hwnd);
            });
            (rx.recv().unwrap(), handle)
        }

        for (style, private) in [(ES_PASSWORD as u32, true), (0, false)] {
            let (raw, thread) = open(style);
            let hwnd = super::super::paste::int_to_hwnd(raw);
            assert!(super::super::paste::focus(hwnd).is_ok(), "could not focus");
            let target = typing_target();
            unsafe {
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, Default::default(), Default::default());
            }
            drop(thread);
            assert_eq!(target.private, private, "ES_PASSWORD style {style:#x}");
            assert!(!target.own_window);
        }
    }
}
