//! Linux (X11): read the PRIMARY selection when there is one (no key needed),
//! else wait for the hotkey's modifiers to come up and press Ctrl+C with XTest.
//!
//! Wayland offers no way to read another app's selection or to inject keys, so
//! there the caller gets [`SelectionCapture::Unavailable`] (and may fall back
//! to the clipboard when the user allows it).

use std::thread::sleep;
use std::time::{Duration, Instant};

use sevak_core::Selection;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt as _, KeyButMask};

use crate::capture::{
    self, CaptureDriver, CaptureOptions, SelectionCapture, SystemClipboardCapture, MODIFIER_TIMEOUT,
};
use crate::error::{PlatformError, Result};
use crate::paste::{ClipboardRead, ForegroundApp, PasteSupport};
use crate::session::DisplayServer;

use super::paste::{self, X};

const KEYSYM_C: u32 = 0x63;
const KEYSYM_CONTROL_L: u32 = 0xffe3;
const FALLBACK_KEYCODE_C: u8 = 54;
const FALLBACK_KEYCODE_CONTROL_L: u8 = 37;

/// Modifier bits a hotkey is made of, with the keysyms that set each one.
const MODIFIERS: [(KeyButMask, [u32; 2]); 4] = [
    (KeyButMask::SHIFT, [0xffe1, 0xffe2]),
    (KeyButMask::CONTROL, [0xffe3, 0xffe4]),
    (KeyButMask::MOD1, [0xffe9, 0xffea]),
    (KeyButMask::MOD4, [0xffeb, 0xffec]),
];

pub(crate) fn capture_selection(options: &CaptureOptions) -> SelectionCapture {
    match DisplayServer::detect() {
        DisplayServer::X11 => {}
        DisplayServer::Wayland => {
            return SelectionCapture::Unavailable(
                "Wayland does not let apps read the selection in another app".to_owned(),
            )
        }
        _ => {
            return SelectionCapture::Unavailable(
                "Reading the selection needs an X11 session".to_owned(),
            )
        }
    }

    // Highlighted text is already on the PRIMARY selection: no key, no
    // clipboard borrowed.
    if options.use_primary_selection {
        if let Some(selection) = crate::clipboard::get_primary_text()
            .ok()
            .flatten()
            .and_then(Selection::from_text)
        {
            return SelectionCapture::Selected(selection);
        }
    }

    if let PasteSupport::CopyOnly(reason) = paste::paste_support() {
        return SelectionCapture::Unavailable(reason);
    }
    let clipboard = SystemClipboardCapture {
        sequence: || None,
        read: || {
            Ok(ClipboardRead {
                text: crate::clipboard::read_text()?,
                sensitive: false,
            })
        },
    };
    capture::capture_by_copy(&clipboard, &X11Capture, true)
}

struct X11Capture;

/// The modifier bits that are down right now.
fn held(x: &X) -> Vec<(KeyButMask, [u32; 2])> {
    let Ok(cookie) = x.conn.query_pointer(x.root) else {
        return Vec::new();
    };
    let Ok(reply) = cookie.reply() else {
        return Vec::new();
    };
    let mask = u16::from(reply.mask);
    MODIFIERS
        .into_iter()
        .filter(|(bit, _)| mask & u16::from(*bit) != 0)
        .collect()
}

impl CaptureDriver for X11Capture {
    fn foreground_app(&self) -> Option<ForegroundApp> {
        paste::foreground_app()
    }

    fn release_modifiers(&self) {
        let Ok(x) = X::connect() else { return };
        let deadline = Instant::now() + MODIFIER_TIMEOUT;
        loop {
            let down = held(&x);
            if down.is_empty() {
                return;
            }
            if Instant::now() >= deadline {
                // Still held: let go of them for the app, or it would see
                // Ctrl+Alt+C.
                for (_, keysyms) in down {
                    for keysym in keysyms {
                        if let Some(keycode) = x.keycode_for(keysym) {
                            let _ = x.fake_key(keycode, false);
                        }
                    }
                }
                let _ = x.conn.flush();
                return;
            }
            sleep(Duration::from_millis(10));
        }
    }

    fn press_copy(&self) -> Result<()> {
        let x = X::connect()?;
        let control = x
            .keycode_for(KEYSYM_CONTROL_L)
            .unwrap_or(FALLBACK_KEYCODE_CONTROL_L);
        let c = x.keycode_for(KEYSYM_C).unwrap_or(FALLBACK_KEYCODE_C);
        x.fake_key(control, true)?;
        x.fake_key(c, true)?;
        x.fake_key(c, false)?;
        x.fake_key(control, false)?;
        // A round trip, so every event above has been processed before we return.
        x.conn
            .get_input_focus()
            .map_err(|e| PlatformError::Os {
                operation: "X11",
                message: e.to_string(),
            })?
            .reply()
            .map_err(|e| PlatformError::Os {
                operation: "X11",
                message: e.to_string(),
            })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_an_x_session_nothing_is_captured() {
        // On an X11 session this would press Ctrl+C in whatever has focus, so
        // only sessions that cannot capture are exercised.
        if DisplayServer::detect() != DisplayServer::X11 {
            let outcome = capture_selection(&CaptureOptions {
                use_primary_selection: false,
            });
            assert!(matches!(outcome, SelectionCapture::Unavailable(_)));
        }
    }
}
