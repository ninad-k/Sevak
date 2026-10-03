//! macOS: wait for the hotkey's modifiers to come up, then post Cmd+C with a
//! `CGEvent`. Like pasting, this needs the Accessibility permission.

use std::thread::sleep;
use std::time::{Duration, Instant};

use objc2_core_graphics::{
    CGEvent, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation,
};

use crate::capture::{
    self, CaptureDriver, CaptureOptions, SelectionCapture, SystemClipboardCapture, MODIFIER_TIMEOUT,
};
use crate::error::{PlatformError, Result};
use crate::paste::ForegroundApp;

use super::paste;

/// `kVK_ANSI_C`. Like the paste key, this assumes a layout where that key types
/// `c`.
const KEYCODE_C: u16 = 8;

const NEEDS_ACCESSIBILITY: &str =
    "Allow Sevak in System Settings > Privacy & Security > Accessibility to read your selection";

/// Control, Option, Shift and Command: the keys a hotkey is made of.
fn modifier_bits() -> u64 {
    (CGEventFlags::MaskControl
        | CGEventFlags::MaskAlternate
        | CGEventFlags::MaskShift
        | CGEventFlags::MaskCommand)
        .0
}

fn modifiers_down() -> bool {
    CGEventSource::flags_state(CGEventSourceStateID::CombinedSessionState).0 & modifier_bits() != 0
}

pub(crate) fn capture_selection(_options: &CaptureOptions) -> SelectionCapture {
    if !paste::accessibility_granted() {
        return SelectionCapture::Unavailable(NEEDS_ACCESSIBILITY.to_owned());
    }
    let clipboard = SystemClipboardCapture {
        sequence: paste::clipboard_sequence,
        read: paste::read_clipboard,
    };
    capture::capture_by_copy(&clipboard, &MacCapture, true)
}

struct MacCapture;

impl CaptureDriver for MacCapture {
    fn foreground_app(&self) -> Option<ForegroundApp> {
        paste::foreground_app()
    }

    fn copy_can_interrupt_terminals(&self) -> bool {
        // Copy is Cmd+C here; Ctrl+C is the interrupt.
        false
    }

    fn release_modifiers(&self) {
        // The key event below carries exactly the Command flag, so a modifier
        // still down cannot leak into it; waiting is only for the apps that
        // read the physical modifier state as well.
        let deadline = Instant::now() + MODIFIER_TIMEOUT;
        while modifiers_down() && Instant::now() < deadline {
            sleep(Duration::from_millis(10));
        }
    }

    fn press_copy(&self) -> Result<()> {
        let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState);
        for key_down in [true, false] {
            let event = CGEvent::new_keyboard_event(source.as_deref(), KEYCODE_C, key_down)
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
