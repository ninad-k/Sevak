//! macOS: watch typing with a listen-only `CGEventTap`, replace typed text with
//! `CGEvent` Backspaces and a Cmd+V.
//!
//! Two permissions are involved, and both are the user's to grant in System
//! Settings > Privacy & Security:
//!
//! - **Input Monitoring** lets the tap see keys at all (without it
//!   `CGEventTapCreate` fails). macOS may only start delivering events after
//!   Sevak is restarted once it has been granted.
//! - **Accessibility** lets Sevak post the Backspaces and the paste, as for
//!   pasting a snippet from the launcher.
//!
//! The tap is listen-only: it cannot change or swallow a key. While "secure
//! event input" is on (a password field, or a terminal's "Secure Keyboard
//! Entry") macOS withholds the keys from taps anyway, and
//! [`typing_target`] reports it as private. Events Sevak posts itself carry a
//! marker in their user-data field and are ignored.
//!
//! Like the other backends this turns input into [`KeyEvent`]s and nothing else.
//! Typing with Command, Control or Option held is a shortcut or a dead-key
//! sequence that the tap cannot follow, so it resets; so does anything that does
//! not type a plain character. Nothing about a key is logged.

use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr::NonNull;
use std::sync::mpsc;
use std::thread::JoinHandle;

use objc2_core_foundation::{kCFRunLoopCommonModes, CFMachPort, CFRetained, CFRunLoop};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation,
    CGEventTapOptions, CGEventTapPlacement, CGEventTapProxy, CGEventType,
    CGPreflightListenEventAccess, CGRequestListenEventAccess,
};

use crate::error::{PlatformError, Result};
use crate::expand::{self, ExpandDriver, SystemExpandClipboard};
use crate::keyboard::{KeyEvent, KeyListener, KeyListenerSupport, KeySink, TypingTarget};

use super::capture::wait_for_modifier_release;
use super::paste;

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    /// Whether some process has switched keyboard input to secure mode.
    fn IsSecureEventInputEnabled() -> u8;
}

/// `kVK_Delete` (Backspace), `kVK_ANSI_V`. As elsewhere, `V` assumes a layout
/// where that key types `v`.
const KEYCODE_DELETE: u16 = 51;
const KEYCODE_V: u16 = 9;

/// Stored in the user-data field of the events Sevak posts, so the tap can tell
/// them from typing.
const INJECTED_TAG: i64 = 0x5345_5641; // "SEVA"

/// Longest string one key event is asked for.
const MAX_CHARS: usize = 8;

const NEEDS_INPUT_MONITORING: &str =
    "Allow Sevak in System Settings > Privacy & Security > Input Monitoring to expand snippets as you type, then restart Sevak";
const NEEDS_ACCESSIBILITY: &str =
    "Allow Sevak in System Settings > Privacy & Security > Accessibility to type snippets into other apps";

pub(crate) fn key_listener_support() -> KeyListenerSupport {
    if !CGPreflightListenEventAccess() {
        return KeyListenerSupport::NeedsPermission(NEEDS_INPUT_MONITORING.to_owned());
    }
    if !paste::accessibility_granted() {
        return KeyListenerSupport::NeedsPermission(NEEDS_ACCESSIBILITY.to_owned());
    }
    KeyListenerSupport::Available
}

/// Shows the system's Input Monitoring prompt (once; macOS remembers the
/// answer), so Sevak appears in the list the user has to switch it on in.
pub(crate) fn request_key_listener_permission() {
    if !CGPreflightListenEventAccess() {
        let _ = CGRequestListenEventAccess();
    }
}

/// A run loop, which Core Foundation documents as usable from any thread
/// (`CFRunLoopStop` in particular).
struct SendRunLoop(CFRetained<CFRunLoop>);

// SAFETY: see above; the handle is only used to call `CFRunLoopStop`.
unsafe impl Send for SendRunLoop {}

pub(crate) fn start(sink: KeySink) -> Result<KeyListener> {
    if let KeyListenerSupport::NeedsPermission(reason) | KeyListenerSupport::Unavailable(reason) =
        key_listener_support()
    {
        return Err(PlatformError::Os {
            operation: "CGEventTapCreate",
            message: reason,
        });
    }

    let (started_tx, started_rx) = mpsc::channel::<std::result::Result<SendRunLoop, String>>();
    let handle = std::thread::Builder::new()
        .name("sevak-eventtap".to_owned())
        .spawn(move || tap_thread(sink, &started_tx))
        .map_err(PlatformError::Io)?;

    match started_rx.recv() {
        Ok(Ok(run_loop)) => Ok(KeyListener::new(move || stop(&run_loop, handle))),
        Ok(Err(message)) => {
            let _ = handle.join();
            Err(PlatformError::Os {
                operation: "CGEventTapCreate",
                message,
            })
        }
        Err(_) => {
            let _ = handle.join();
            Err(PlatformError::Os {
                operation: "CGEventTapCreate",
                message: "the event tap thread ended unexpectedly".to_owned(),
            })
        }
    }
}

fn stop(run_loop: &SendRunLoop, handle: JoinHandle<()>) {
    run_loop.0.stop();
    let _ = handle.join();
}

/// What the tap callback works with; owned by the tap thread and reached
/// through the callback's `user_info` pointer.
struct TapState {
    sink: KeySink,
    /// Process id of the frontmost app at the previous key.
    frontmost: i32,
    port: Option<CFRetained<CFMachPort>>,
}

impl TapState {
    fn emit(&self, event: KeyEvent) {
        (self.sink)(event);
    }

    fn handle(&mut self, kind: CGEventType, event: &CGEvent) {
        match kind {
            CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
                // macOS switches a slow or interrupted tap off; keys may have
                // been missed, so forget what was typed and carry on.
                if let Some(port) = &self.port {
                    CGEvent::tap_enable(port, true);
                }
                self.emit(KeyEvent::Reset);
            }
            CGEventType::LeftMouseDown
            | CGEventType::RightMouseDown
            | CGEventType::OtherMouseDown => self.emit(KeyEvent::Reset),
            CGEventType::KeyDown => self.key(event),
            _ => {}
        }
    }

    fn key(&mut self, event: &CGEvent) {
        if CGEvent::integer_value_field(Some(event), CGEventField::EventSourceUserData)
            == INJECTED_TAG
        {
            return;
        }
        let front = paste::frontmost_pid().unwrap_or(0);
        if front != self.frontmost {
            self.frontmost = front;
            self.emit(KeyEvent::Reset);
        }

        let flags = CGEvent::flags(Some(event)).0;
        let shortcut =
            (CGEventFlags::MaskCommand | CGEventFlags::MaskControl | CGEventFlags::MaskAlternate).0;
        if flags & shortcut != 0 {
            self.emit(KeyEvent::Reset);
            return;
        }
        let keycode = CGEvent::integer_value_field(Some(event), CGEventField::KeyboardEventKeycode);
        if keycode == i64::from(KEYCODE_DELETE) {
            self.emit(KeyEvent::Backspace);
            return;
        }

        let mut buffer = [0u16; MAX_CHARS];
        let mut length = 0u64;
        // SAFETY: `buffer` holds MAX_CHARS UniChars and `length` receives how
        // many were written; both outlive the call.
        unsafe {
            CGEvent::keyboard_get_unicode_string(
                Some(event),
                MAX_CHARS as u64,
                &mut length,
                buffer.as_mut_ptr(),
            );
        }
        let written = usize::try_from(length).map_or(MAX_CHARS, |n| n.min(MAX_CHARS));
        let text = String::from_utf16_lossy(&buffer[..written]);
        // Return, Tab, Escape and the arrows type control or private-use
        // characters; none of them is plain typing.
        let typed = !text.is_empty() && text.chars().all(|c| !c.is_control() && !is_private_use(c));
        if typed {
            for c in text.chars() {
                self.emit(KeyEvent::Char(c));
            }
        } else {
            self.emit(KeyEvent::Reset);
        }
    }
}

/// The private-use area macOS uses for function and arrow keys (`U+F700...`).
fn is_private_use(c: char) -> bool {
    ('\u{e000}'..='\u{f8ff}').contains(&c)
}

unsafe extern "C-unwind" fn tap_callback(
    _proxy: CGEventTapProxy,
    kind: CGEventType,
    event: NonNull<CGEvent>,
    user_info: *mut c_void,
) -> *mut CGEvent {
    if !user_info.is_null() {
        // SAFETY: `user_info` is the TapState the tap thread allocated; only
        // that thread's run loop calls this, and the state outlives the tap.
        let state = unsafe { &mut *user_info.cast::<TapState>() };
        // SAFETY: the event is valid for the duration of the callback.
        let event_ref = unsafe { event.as_ref() };
        // A panic must not unwind into the system.
        let _ = catch_unwind(AssertUnwindSafe(|| state.handle(kind, event_ref)));
    }
    event.as_ptr()
}

fn tap_thread(sink: KeySink, started: &mpsc::Sender<std::result::Result<SendRunLoop, String>>) {
    let state = Box::into_raw(Box::new(TapState {
        sink,
        frontmost: paste::frontmost_pid().unwrap_or(0),
        port: None,
    }));
    let mask: u64 = [
        CGEventType::KeyDown,
        CGEventType::LeftMouseDown,
        CGEventType::RightMouseDown,
        CGEventType::OtherMouseDown,
    ]
    .iter()
    .fold(0, |mask, kind| mask | (1u64 << kind.0));

    // SAFETY: the callback only touches the state behind `user_info`, which stays
    // allocated until after the tap is switched off below.
    let tap = unsafe {
        CGEvent::tap_create(
            CGEventTapLocation::SessionEventTap,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::ListenOnly,
            mask,
            Some(tap_callback),
            state.cast(),
        )
    };
    let Some(tap) = tap else {
        // SAFETY: the tap does not exist, so nothing else refers to the state.
        drop(unsafe { Box::from_raw(state) });
        let _ = started.send(Err(NEEDS_INPUT_MONITORING.to_owned()));
        return;
    };
    // SAFETY: the tap is not enabled yet, so the callback cannot be running.
    unsafe { (*state).port = Some(tap.clone()) };

    let run_loop = CFRunLoop::current();
    let source = CFMachPort::new_run_loop_source(None, Some(&tap), 0);
    let (Some(run_loop), Some(source)) = (run_loop, source) else {
        // SAFETY: as above.
        drop(unsafe { Box::from_raw(state) });
        let _ = started.send(Err(
            "could not attach the event tap to a run loop".to_owned()
        ));
        return;
    };
    // SAFETY: reading an extern static that Core Foundation initialises.
    let modes = unsafe { kCFRunLoopCommonModes };
    run_loop.add_source(Some(&source), modes);
    CGEvent::tap_enable(&tap, true);
    let _ = started.send(Ok(SendRunLoop(run_loop.clone())));

    CFRunLoop::run();

    CGEvent::tap_enable(&tap, false);
    run_loop.remove_source(Some(&source), modes);
    // SAFETY: the tap is off and its source removed, so no callback can run.
    drop(unsafe { Box::from_raw(state) });
}

pub(crate) fn typing_target() -> TypingTarget {
    TypingTarget {
        app: paste::foreground_app(),
        own_window: paste::frontmost_pid() == i32::try_from(std::process::id()).ok(),
        // SAFETY: a plain C call without arguments.
        private: unsafe { IsSecureEventInputEnabled() } != 0,
    }
}

pub(crate) fn replace_typed_text(
    delete: usize,
    text: &str,
    still_current: &dyn Fn() -> bool,
) -> Result<bool> {
    if !paste::accessibility_granted() {
        return Err(PlatformError::Os {
            operation: "CGEventPost",
            message: NEEDS_ACCESSIBILITY.to_owned(),
        });
    }
    expand::replace_typed_text(
        delete,
        text,
        still_current,
        &SystemExpandClipboard,
        &MacExpand,
        true,
    )
}

struct MacExpand;

/// Posts one key press (down then up) tagged as Sevak's own.
fn post_key(keycode: u16, flags: CGEventFlags) -> Result<()> {
    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState);
    for key_down in [true, false] {
        let event =
            CGEvent::new_keyboard_event(source.as_deref(), keycode, key_down).ok_or_else(|| {
                PlatformError::Os {
                    operation: "CGEvent",
                    message: "could not create the key event".to_owned(),
                }
            })?;
        CGEvent::set_flags(Some(&event), flags);
        CGEvent::set_integer_value_field(
            Some(&event),
            CGEventField::EventSourceUserData,
            INJECTED_TAG,
        );
        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
    }
    Ok(())
}

impl ExpandDriver for MacExpand {
    fn foreground_token(&self) -> Option<u64> {
        paste::frontmost_pid().and_then(|pid| u64::try_from(pid).ok())
    }

    fn release_modifiers(&self) {
        wait_for_modifier_release();
    }

    fn press_backspaces(&self, count: usize) -> Result<()> {
        for _ in 0..count {
            post_key(KEYCODE_DELETE, CGEventFlags(0))?;
        }
        Ok(())
    }

    fn press_paste(&self) -> Result<()> {
        post_key(KEYCODE_V, CGEventFlags::MaskCommand)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_and_arrow_keys_are_private_use() {
        assert!(is_private_use('\u{f700}')); // up arrow
        assert!(is_private_use('\u{f728}')); // forward delete
        assert!(!is_private_use('a'));
        assert!(!is_private_use('é'));
        assert!(!is_private_use('€'));
    }

    #[test]
    fn the_listener_reports_a_permission_state_without_panicking() {
        // Whatever this machine has granted, asking must be safe.
        let _ = key_listener_support();
    }
}
