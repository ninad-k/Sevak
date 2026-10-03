//! macOS: remember the frontmost app with `NSWorkspace`, reactivate it, and
//! press Cmd+V with a `CGEvent`. Synthetic key events only reach other apps
//! once the user has allowed Sevak under System Settings > Privacy & Security >
//! Accessibility; without that the text is copied and nothing is pressed.

use std::sync::Mutex;
use std::thread::sleep;
use std::time::{Duration, Instant};

use objc2_app_kit::{
    NSApplicationActivationOptions, NSPasteboard, NSRunningApplication, NSWorkspace,
};
use objc2_core_graphics::{
    CGEvent, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation,
};

use crate::error::{PlatformError, Result};
use crate::paste::{
    self, pasteboard_marks_secret, ClipboardRead, ForegroundApp, PasteDriver, PasteOutcome,
    PasteSupport, SystemClipboard,
};

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    /// Whether this process may control the computer through the accessibility
    /// API, which includes posting key events to other apps.
    fn AXIsProcessTrusted() -> u8;
}

/// `kVK_ANSI_V`. Like most paste helpers this assumes a layout where that key
/// types `v` (QWERTY, AZERTY, QWERTZ); Dvorak and similar would need a lookup.
const KEYCODE_V: u16 = 9;

/// Process id of the app that was frontmost when Sevak was shown.
static REMEMBERED: Mutex<Option<i32>> = Mutex::new(None);

const FOCUS_TIMEOUT: Duration = Duration::from_millis(500);

const NEEDS_ACCESSIBILITY: &str =
    "Allow Sevak in System Settings > Privacy & Security > Accessibility to paste";

fn accessibility_granted() -> bool {
    // SAFETY: a plain C call without arguments.
    unsafe { AXIsProcessTrusted() != 0 }
}

fn frontmost() -> Option<objc2::rc::Retained<NSRunningApplication>> {
    NSWorkspace::sharedWorkspace().frontmostApplication()
}

fn app_of(app: &NSRunningApplication) -> Option<ForegroundApp> {
    let bundle = app.bundleIdentifier().map(|id| id.to_string());
    let name = app
        .localizedName()
        .map(|name| name.to_string())
        .or_else(|| bundle.clone())?;
    let mut described = ForegroundApp::new(name);
    if let Some(bundle) = bundle {
        described = described.with_identifier(bundle);
    }
    Some(described)
}

pub(crate) fn remember_foreground_app() {
    let current = frontmost();
    let pid = current.as_ref().map(|app| app.processIdentifier());
    if pid == Some(std::process::id() as i32) {
        // Sevak is already in front (shown twice); the earlier app stands.
        return;
    }
    *REMEMBERED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = pid;
}

pub(crate) fn foreground_app() -> Option<ForegroundApp> {
    frontmost().and_then(|app| app_of(&app))
}

pub(crate) fn paste_support() -> PasteSupport {
    if accessibility_granted() {
        PasteSupport::Available
    } else {
        PasteSupport::CopyOnly(NEEDS_ACCESSIBILITY.to_owned())
    }
}

pub(crate) fn paste_text(text: &str, restore_clipboard: bool) -> Result<PasteOutcome> {
    if let PasteSupport::CopyOnly(reason) = paste_support() {
        crate::clipboard::set_text(text)?;
        return Ok(PasteOutcome::CopiedOnly(reason));
    }
    paste::paste(text, restore_clipboard, &SystemClipboard, &MacDriver, true)
}

struct MacDriver;

impl PasteDriver for MacDriver {
    fn focus_previous(&self) -> std::result::Result<(), String> {
        let Some(pid) = *REMEMBERED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
        else {
            return Err("There is no previous app to paste into".to_owned());
        };
        let is_frontmost = || frontmost().is_some_and(|app| app.processIdentifier() == pid);
        if !is_frontmost() {
            let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
            else {
                return Err("The previous app has quit".to_owned());
            };
            #[allow(deprecated)]
            let options = NSApplicationActivationOptions::ActivateIgnoringOtherApps;
            app.activateWithOptions(options);
        }

        let deadline = Instant::now() + FOCUS_TIMEOUT;
        while !is_frontmost() {
            if Instant::now() >= deadline {
                return Err("Could not return to the previous app".to_owned());
            }
            sleep(Duration::from_millis(10));
        }
        Ok(())
    }

    fn press_paste(&self) -> Result<()> {
        let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState);
        for key_down in [true, false] {
            let event = CGEvent::new_keyboard_event(source.as_deref(), KEYCODE_V, key_down)
                .ok_or_else(|| PlatformError::Os {
                    operation: "CGEvent",
                    message: "could not create the key event".to_owned(),
                })?;
            CGEvent::set_flags(Some(&event), CGEventFlags::MaskCommand);
            CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
        }
        Ok(())
    }
}

pub(crate) fn clipboard_sequence() -> Option<u64> {
    let count = NSPasteboard::generalPasteboard().changeCount();
    u64::try_from(count).ok()
}

pub(crate) fn read_clipboard() -> Result<ClipboardRead> {
    let types: Vec<String> = NSPasteboard::generalPasteboard()
        .types()
        .map(|types| types.to_vec().iter().map(|ty| ty.to_string()).collect())
        .unwrap_or_default();
    if pasteboard_marks_secret(&types) {
        return Ok(ClipboardRead {
            text: None,
            sensitive: true,
        });
    }
    Ok(ClipboardRead {
        text: crate::clipboard::read_text()?,
        sensitive: false,
    })
}
