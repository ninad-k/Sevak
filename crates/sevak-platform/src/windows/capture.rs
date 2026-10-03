//! Windows: wait for the hotkey's modifiers to come up, then send Ctrl+C.

use std::thread::sleep;
use std::time::{Duration, Instant};

use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, VIRTUAL_KEY, VK_C, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT,
    VK_LWIN, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN,
};

use crate::capture::{
    self, CaptureDriver, CaptureOptions, SelectionCapture, SystemClipboardCapture, MODIFIER_TIMEOUT,
};
use crate::error::Result;
use crate::paste::ForegroundApp;

use super::paste::{self, key_input, send_inputs};

/// The keys the hotkey is made of.
const MODIFIERS: [VIRTUAL_KEY; 8] = [
    VK_LCONTROL,
    VK_RCONTROL,
    VK_LMENU,
    VK_RMENU,
    VK_LSHIFT,
    VK_RSHIFT,
    VK_LWIN,
    VK_RWIN,
];

pub(crate) fn capture_selection(_options: &CaptureOptions) -> SelectionCapture {
    let clipboard = SystemClipboardCapture {
        sequence: paste::clipboard_sequence,
        read: paste::read_clipboard,
    };
    capture::capture_by_copy(&clipboard, &WindowsCapture, true)
}

struct WindowsCapture;

/// The modifier keys that are physically (or synthetically) down right now.
fn held_modifiers() -> Vec<VIRTUAL_KEY> {
    MODIFIERS
        .into_iter()
        // SAFETY: plain Win32 call taking a virtual-key code. The top bit of
        // the result is "currently down".
        .filter(|key| unsafe { GetAsyncKeyState(i32::from(key.0)) } as u16 & 0x8000 != 0)
        .collect()
}

/// Waits for the user to let go of Ctrl, Alt, Shift and Win, and if they are
/// still down after [`MODIFIER_TIMEOUT`], lets go of them for the app.
pub(super) fn wait_for_modifier_release() {
    let deadline = Instant::now() + MODIFIER_TIMEOUT;
    loop {
        let held = held_modifiers();
        if held.is_empty() {
            return;
        }
        if Instant::now() >= deadline {
            // Still held (a stuck or very slow release): let go of them
            // for the app, or it would see Ctrl+Alt+C.
            let ups: Vec<INPUT> = held.iter().map(|key| key_input(*key, true)).collect();
            if let Err(err) = send_inputs(&ups) {
                tracing::debug!("could not release the modifier keys: {err}");
            }
            return;
        }
        sleep(Duration::from_millis(10));
    }
}

impl CaptureDriver for WindowsCapture {
    fn foreground_app(&self) -> Option<ForegroundApp> {
        paste::foreground_app()
    }

    fn release_modifiers(&self) {
        wait_for_modifier_release();
    }

    fn press_copy(&self) -> Result<()> {
        send_inputs(&[
            key_input(VK_CONTROL, false),
            key_input(VK_C, false),
            key_input(VK_C, true),
            key_input(VK_CONTROL, true),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifier_state_can_be_read() {
        // Nothing to assert about which keys are down; it must just work.
        let _ = held_modifiers();
    }

    /// Captures the selection of a real window: an edit control with its text
    /// selected is focused, and the clipboard must come back as it was.
    /// Needs an interactive desktop and steals focus for a moment, so it is run
    /// by hand: `cargo test -p sevak-platform captures_a_real_selection -- --ignored`
    #[test]
    #[ignore = "needs an interactive desktop"]
    fn captures_a_real_selection() {
        use std::sync::mpsc;
        use windows::core::{w, PCWSTR};
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, DispatchMessageW, GetMessageW, PostMessageW,
            SendMessageW, TranslateMessage, MSG, WINDOW_EX_STYLE, WM_CLOSE, WS_OVERLAPPEDWINDOW,
            WS_VISIBLE,
        };

        /// `EM_SETSEL` (it lives in a Windows feature the crate does not use).
        const EM_SETSEL: u32 = 0x00B1;
        const TITLE: &str = "sevak capture target";
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
            tx.send(paste::hwnd_to_int(hwnd)).unwrap();
            let mut message = MSG::default();
            while GetMessageW(&mut message, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            let _ = DestroyWindow(hwnd);
        });
        let target = paste::int_to_hwnd(rx.recv().unwrap());

        let before = arboard::Clipboard::new()
            .and_then(|mut clipboard| {
                let saved = clipboard.get_text().ok();
                clipboard.set_text("sevak-test clipboard before")?;
                Ok(saved)
            })
            .ok();
        assert!(paste::focus(target).is_ok(), "could not focus the window");
        unsafe {
            SendMessageW(target, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
        }

        let outcome = capture_selection(&CaptureOptions::default());
        let after = crate::clipboard::get_text().unwrap();

        // The window and the user's clipboard go back before anything is asserted.
        unsafe {
            let _ = PostMessageW(
                Some(target),
                WM_CLOSE,
                Default::default(),
                Default::default(),
            );
        }
        drop(window);
        if let Some(Some(saved)) = before {
            let _ = crate::clipboard::set_text(&saved);
        }

        match outcome {
            SelectionCapture::Selected(selection) => assert_eq!(selection.text(), Some(TITLE)),
            other => panic!("expected the selection, got {other:?}"),
        }
        assert_eq!(after.as_deref(), Some("sevak-test clipboard before"));
    }
}
