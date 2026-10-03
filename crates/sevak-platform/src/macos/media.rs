//! macOS: media keys. Play/pause, next and previous are special system-defined
//! key events (`NX_SYSDEFINED`, subtype 8) that the system routes to whichever
//! app currently owns the media keys, the same events the keyboard's F7 to F9
//! keys produce. Posting them needs no permission beyond what any synthetic
//! event needs; the first use may ask for Accessibility access.

use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType};
use objc2_core_graphics::{CGEvent, CGEventTapLocation};
use objc2_foundation::NSPoint;

use crate::error::{PlatformError, Result};
use crate::media::mac_key_event_data;

/// `NX_SUBTYPE_AUX_CONTROL_BUTTONS`.
const SUBTYPE_AUX_CONTROL_BUTTONS: i16 = 8;

fn failure(message: &str) -> PlatformError {
    PlatformError::Os {
        operation: "media key",
        message: message.to_owned(),
    }
}

/// Presses and releases the media key `key` (see
/// [`crate::media::mac_media_key`]).
pub(crate) fn post_media_key(key: i64) -> Result<()> {
    for down in [true, false] {
        let flags = NSEventModifierFlags(if down { 0xa00 } else { 0xb00 });
        let data1 = mac_key_event_data(key, down) as isize;
        let event = NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
            NSEventType::SystemDefined,
            NSPoint::new(0.0, 0.0),
            flags,
            0.0,
            0,
            None,
            SUBTYPE_AUX_CONTROL_BUTTONS,
            data1,
            -1,
        )
        .ok_or_else(|| failure("could not create the key event"))?;
        let cg_event = event
            .CGEvent()
            .ok_or_else(|| failure("could not convert the key event"))?;
        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&cg_event));
    }
    Ok(())
}
