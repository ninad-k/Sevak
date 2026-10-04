//! Windows: notices the events after which a low-level hook may have stopped
//! working, for the hook watchdog (see [`crate::hook_watchdog`]): the session
//! being unlocked or reconnected, the display configuration changing, and the
//! machine waking from sleep.
//!
//! The hook thread makes a hidden top-level window of its own (never shown; a
//! top-level window is what receives the system's broadcasts) and registers it
//! for session notifications. The window procedure only sets a flag; the hook
//! thread's loop reads the flags and tells the watchdog. Nothing about the user
//! is read.

use std::sync::atomic::{AtomicU32, Ordering};

use windows::core::w;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::RemoteDesktop::{
    WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassExW, PBT_APMRESUMEAUTOMATIC,
    PBT_APMRESUMESUSPEND, WINDOW_EX_STYLE, WM_DISPLAYCHANGE, WM_POWERBROADCAST,
    WM_WTSSESSION_CHANGE, WNDCLASSEXW, WS_POPUP,
};

use crate::hook_watchdog::Reason;

/// `WM_WTSSESSION_CHANGE` codes after which the hook is put in again: the console
/// or a remote session connected, a user logged on, the session was unlocked.
const WTS_CONSOLE_CONNECT: u32 = 0x1;
const WTS_REMOTE_CONNECT: u32 = 0x3;
const WTS_SESSION_LOGON: u32 = 0x5;
const WTS_SESSION_UNLOCK: u32 = 0x8;

const SESSION: u32 = 1;
const DISPLAY: u32 = 2;
const RESUME: u32 = 4;

/// What happened since the hook thread last looked (bits of the constants above).
static EVENTS: AtomicU32 = AtomicU32::new(0);

/// The flag a window message sets, if it is one of the events we care about.
fn flag_for(message: u32, wparam: usize) -> Option<u32> {
    let code = u32::try_from(wparam).ok()?;
    match message {
        WM_WTSSESSION_CHANGE
            if matches!(
                code,
                WTS_CONSOLE_CONNECT | WTS_REMOTE_CONNECT | WTS_SESSION_LOGON | WTS_SESSION_UNLOCK
            ) =>
        {
            Some(SESSION)
        }
        WM_DISPLAYCHANGE => Some(DISPLAY),
        WM_POWERBROADCAST if matches!(code, PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND) => {
            Some(RESUME)
        }
        _ => None,
    }
}

/// The events since the last call, as watchdog reasons.
pub(super) fn take_events() -> Vec<Reason> {
    let bits = EVENTS.swap(0, Ordering::SeqCst);
    [
        (SESSION, Reason::Session),
        (DISPLAY, Reason::Display),
        (RESUME, Reason::Resume),
    ]
    .into_iter()
    .filter(|(bit, _)| bits & bit != 0)
    .map(|(_, reason)| reason)
    .collect()
}

unsafe extern "system" fn watch_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if let Some(flag) = flag_for(message, wparam.0) {
        EVENTS.fetch_or(flag, Ordering::SeqCst);
    }
    // SAFETY: forwarding the arguments we were given.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

/// The hidden window and its session registration. Lives on the hook thread.
pub(super) struct Watch {
    hwnd: Option<HWND>,
    registered: bool,
}

impl Watch {
    /// Makes the window. Failing is not fatal: the periodic reinstall still
    /// runs, only the quicker reaction to these events is lost.
    ///
    /// # Safety
    /// Must be called on the hook thread, which then runs the message loop that
    /// serves the window.
    pub(super) unsafe fn open(module: Option<HINSTANCE>) -> Self {
        let class_name = w!("SevakHookWatchdog");
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(watch_proc),
            hInstance: module.unwrap_or_default(),
            lpszClassName: class_name,
            ..Default::default()
        };
        // SAFETY: `class` is fully initialized and its strings are static.
        // (A second registration after a restart of the hook thread fails with
        // "class exists", which is what we want.)
        let _ = unsafe { RegisterClassExW(&class) };
        // SAFETY: plain window creation on this thread; the window is never shown.
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class_name,
                w!(""),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                module,
                None,
            )
        }
        .ok();
        let registered = hwnd.is_some_and(|hwnd| {
            // SAFETY: `hwnd` is the window created above, on this thread.
            unsafe { WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) }.is_ok()
        });
        if hwnd.is_none() {
            tracing::debug!("the hook watchdog window could not be created");
        }
        Self { hwnd, registered }
    }

    /// Unregisters and destroys the window.
    ///
    /// # Safety
    /// Must be called on the thread that called [`Watch::open`].
    pub(super) unsafe fn close(self) {
        let Some(hwnd) = self.hwnd else { return };
        if self.registered {
            // SAFETY: registered for this window on this thread.
            let _ = unsafe { WTSUnRegisterSessionNotification(hwnd) };
        }
        // SAFETY: the window belongs to this thread.
        let _ = unsafe { DestroyWindow(hwnd) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wparam(code: u32) -> usize {
        code as usize
    }

    #[test]
    fn unlocks_and_reconnects_are_session_events_but_locking_is_not() {
        for code in [
            WTS_CONSOLE_CONNECT,
            WTS_REMOTE_CONNECT,
            WTS_SESSION_LOGON,
            WTS_SESSION_UNLOCK,
        ] {
            assert_eq!(
                flag_for(WM_WTSSESSION_CHANGE, wparam(code)),
                Some(SESSION),
                "{code}"
            );
        }
        // WTS_SESSION_LOCK (7), WTS_CONSOLE_DISCONNECT (2), WTS_SESSION_LOGOFF (6).
        for code in [7, 2, 6] {
            assert_eq!(flag_for(WM_WTSSESSION_CHANGE, wparam(code)), None, "{code}");
        }
    }

    #[test]
    fn display_changes_and_wake_ups_are_events() {
        assert_eq!(flag_for(WM_DISPLAYCHANGE, 32), Some(DISPLAY));
        assert_eq!(
            flag_for(WM_POWERBROADCAST, wparam(PBT_APMRESUMEAUTOMATIC)),
            Some(RESUME)
        );
        assert_eq!(
            flag_for(WM_POWERBROADCAST, wparam(PBT_APMRESUMESUSPEND)),
            Some(RESUME)
        );
        // PBT_APMSUSPEND (4) and other messages are not.
        assert_eq!(flag_for(WM_POWERBROADCAST, 4), None);
        assert_eq!(flag_for(0x0001, 0), None);
    }

    #[test]
    fn events_are_taken_once_in_a_fixed_order() {
        // The only test that touches the flags.
        EVENTS.fetch_or(RESUME | SESSION, Ordering::SeqCst);
        assert_eq!(take_events(), [Reason::Session, Reason::Resume]);
        assert!(take_events().is_empty());
    }
}
