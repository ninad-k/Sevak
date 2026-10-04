//! Windows: ask UI Automation whether the focused control is a password field
//! (`IsPassword` of the focused element).
//!
//! This sees what the `ES_PASSWORD` edit-control check cannot: password fields
//! of browsers (when their accessibility support is on, which a UI Automation
//! client switches on), WPF, WinUI, UWP, Qt and Electron apps. It does not see
//! everything: an app that does not expose its controls, a canvas-drawn
//! password box, and a process running as administrator when Sevak is not give
//! no answer. The question runs on a helper thread under a 50 ms budget (see
//! [`crate::password_probe`]), never in the keyboard hook's callback, so a slow
//! app costs nothing but the answer.

use std::time::Duration;

use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};

use crate::password_probe::{BudgetedProbe, FocusProbe, PasswordFieldDetector};

/// How long a question may take before the answer is "unknown".
const BUDGET: Duration = Duration::from_millis(50);
/// How long an answer is reused for the same window.
const REUSE: Duration = Duration::from_millis(300);

/// UI Automation's view of the focused element. Lives on the probe thread.
struct UiaProbe {
    automation: IUIAutomation,
    com_initialized: bool,
}

impl UiaProbe {
    fn create() -> Option<Box<dyn FocusProbe>> {
        // SAFETY: COM is initialized on this thread (and balanced in `Drop`
        // when this call added a reference); the interface is created and
        // used on this thread only.
        unsafe {
            let com_initialized = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            match CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER) {
                Ok(automation) => Some(Box::new(Self {
                    automation,
                    com_initialized,
                })),
                Err(err) => {
                    tracing::debug!(%err, "UI Automation is not available");
                    if com_initialized {
                        CoUninitialize();
                    }
                    None
                }
            }
        }
    }
}

impl FocusProbe for UiaProbe {
    fn focused_is_password(&mut self) -> Option<bool> {
        // SAFETY: plain COM calls on interfaces owned by this thread.
        unsafe {
            let element = self.automation.GetFocusedElement().ok()?;
            element
                .CurrentIsPassword()
                .ok()
                .map(|password| password.as_bool())
        }
    }
}

impl Drop for UiaProbe {
    fn drop(&mut self) {
        if self.com_initialized {
            // SAFETY: balances the successful `CoInitializeEx` on this thread.
            unsafe { CoUninitialize() };
        }
    }
}

/// The detector for the focused control of the foreground window: started on
/// first use.
pub(super) fn detector() -> &'static PasswordFieldDetector<BudgetedProbe> {
    static DETECTOR: std::sync::OnceLock<PasswordFieldDetector<BudgetedProbe>> =
        std::sync::OnceLock::new();
    DETECTOR.get_or_init(|| {
        PasswordFieldDetector::new(BudgetedProbe::spawn(BUDGET, UiaProbe::create), REUSE)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// UI Automation against a real password box: an edit control with
    /// `ES_PASSWORD` is focused and must be reported. Needs an interactive
    /// desktop and steals focus for a moment, so it is run by hand:
    /// `cargo test -p sevak-platform uia_sees_a_password_box -- --ignored`
    #[test]
    #[ignore = "needs an interactive desktop"]
    fn uia_sees_a_password_box() {
        use std::sync::mpsc;
        use windows::core::{w, PCWSTR};
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, DispatchMessageW, GetMessageW, PostMessageW,
            TranslateMessage, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WS_OVERLAPPEDWINDOW,
            WS_VISIBLE,
        };

        use super::super::paste;

        const ES_PASSWORD: u32 = 0x20;
        for (style, expected) in [(ES_PASSWORD, true), (0, false)] {
            let (tx, rx) = mpsc::channel();
            let window = std::thread::spawn(move || unsafe {
                let title: Vec<u16> = "sevak uia target".encode_utf16().chain([0]).collect();
                let hwnd = CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("EDIT"),
                    PCWSTR(title.as_ptr()),
                    WS_OVERLAPPEDWINDOW | WS_VISIBLE | WINDOW_STYLE(style),
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
            let hwnd = paste::int_to_hwnd(rx.recv().unwrap());
            assert!(paste::focus(hwnd).is_ok(), "could not focus");

            // Not through the shared detector: a fresh probe with a generous
            // budget, so a slow first UI Automation start does not fail it.
            let probe = BudgetedProbe::spawn(Duration::from_secs(5), UiaProbe::create);
            let answer = crate::password_probe::Ask::ask(&probe);

            unsafe {
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, Default::default(), Default::default());
            }
            drop(window);
            assert_eq!(answer, Some(expected), "ES_PASSWORD style {style:#x}");
        }
    }
}
