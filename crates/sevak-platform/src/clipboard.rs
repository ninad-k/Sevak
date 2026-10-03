//! Clipboard access via `arboard`.

use std::sync::{Mutex, OnceLock};

use arboard::Clipboard;

use crate::error::{PlatformError, Result};

/// One clipboard handle for the life of the process. On X11 (and XWayland) the
/// copying application must keep serving the selection until another app takes
/// ownership; dropping the handle right after `set_text` would make the copied
/// text vanish. Sevak is resident, so keeping the handle alive solves that.
static CLIPBOARD: OnceLock<Mutex<Option<Clipboard>>> = OnceLock::new();

pub fn set_text(text: &str) -> Result<()> {
    let slot = CLIPBOARD.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.is_none() {
        *guard = Some(Clipboard::new().map_err(clipboard_error)?);
    }
    let clipboard = guard.as_mut().expect("initialized above");
    clipboard.set_text(text.to_owned()).map_err(|err| {
        // A broken connection (e.g. the X server restarted) is retried with a
        // fresh handle next time.
        *guard = None;
        clipboard_error(err)
    })
}

fn clipboard_error(err: arboard::Error) -> PlatformError {
    PlatformError::Os {
        operation: "clipboard",
        message: err.to_string(),
    }
}
