//! Clipboard access via `arboard`.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use arboard::Clipboard;

use crate::error::{PlatformError, Result};

/// One clipboard handle for the life of the process. On X11 (and XWayland) the
/// copying application must keep serving the selection until another app takes
/// ownership; dropping the handle right after `set_text` would make the copied
/// text vanish. Sevak is resident, so keeping the handle alive solves that.
static CLIPBOARD: OnceLock<Mutex<Option<Clipboard>>> = OnceLock::new();

/// A second handle for reading, so a slow read (X11 asks the owning app for
/// the text) never holds up a copy.
static READER: OnceLock<Mutex<Option<Clipboard>>> = OnceLock::new();

pub fn set_text(text: &str) -> Result<()> {
    note_own_write(text);
    with_handle(&CLIPBOARD, |clipboard| clipboard.set_text(text.to_owned()))
}

/// Like [`set_text`], but asks the OS to keep the text out of its own clipboard
/// history and cloud sync (Windows Win+V, macOS clipboard managers that honour
/// `org.nspasteboard.ConcealedType`, the KDE password-manager hint). Used when
/// pasting, since pasted text can be a password or a private snippet.
pub fn set_text_private(text: &str) -> Result<()> {
    note_own_write(text);
    with_handle(&CLIPBOARD, |clipboard| {
        #[cfg(windows)]
        {
            use arboard::SetExtWindows;
            clipboard
                .set()
                .exclude_from_monitoring()
                .text(text.to_owned())
        }
        #[cfg(target_os = "macos")]
        {
            use arboard::SetExtApple;
            clipboard.set().exclude_from_history().text(text.to_owned())
        }
        #[cfg(target_os = "linux")]
        {
            use arboard::SetExtLinux;
            clipboard.set().exclude_from_history().text(text.to_owned())
        }
    })
}

/// The clipboard's text; `None` if it holds something else or is empty.
pub fn get_text() -> Result<Option<String>> {
    read_text()
}

pub(crate) fn read_text() -> Result<Option<String>> {
    let slot = READER.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.is_none() {
        *guard = Some(Clipboard::new().map_err(clipboard_error)?);
    }
    let clipboard = guard.as_mut().expect("initialized above");
    match clipboard.get_text() {
        Ok(text) => Ok(Some(text)),
        Err(arboard::Error::ContentNotAvailable) => Ok(None),
        Err(err) => {
            *guard = None;
            Err(clipboard_error(err))
        }
    }
}

fn with_handle(
    slot: &'static OnceLock<Mutex<Option<Clipboard>>>,
    write: impl FnOnce(&mut Clipboard) -> std::result::Result<(), arboard::Error>,
) -> Result<()> {
    let slot = slot.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.is_none() {
        *guard = Some(Clipboard::new().map_err(clipboard_error)?);
    }
    let clipboard = guard.as_mut().expect("initialized above");
    write(clipboard).map_err(|err| {
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

/// Texts Sevak itself put on the clipboard recently, as hashes. The clipboard
/// history skips them: a paste or copy Sevak performs is not something the user
/// copied from another app.
static OWN_WRITES: Mutex<Vec<(u64, Instant)>> = Mutex::new(Vec::new());

/// How long a write counts as ours. The history polls a few times a second.
const OWN_WRITE_WINDOW: Duration = Duration::from_secs(10);

fn hash_text(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// Records that Sevak is about to put `text` on the clipboard (the writers in
/// this module do it themselves; this is for code that writes another way).
pub fn note_own_write(text: &str) {
    let mut writes = OWN_WRITES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    writes.retain(|(_, at)| at.elapsed() < OWN_WRITE_WINDOW);
    writes.push((hash_text(text), Instant::now()));
}

/// True once if `text` is what Sevak itself recently put on the clipboard.
pub fn take_own_write(text: &str) -> bool {
    let hash = hash_text(text);
    let mut writes = OWN_WRITES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    writes.retain(|(_, at)| at.elapsed() < OWN_WRITE_WINDOW);
    match writes.iter().position(|(h, _)| *h == hash) {
        Some(index) => {
            writes.remove(index);
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_writes_are_recognised_once() {
        note_own_write("secret pasted text 1");
        assert!(take_own_write("secret pasted text 1"));
        assert!(!take_own_write("secret pasted text 1"));
        assert!(!take_own_write("something the user copied"));
    }
}
