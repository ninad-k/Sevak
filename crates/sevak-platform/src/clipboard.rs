//! Clipboard access via `arboard`.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
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
    read_with(|clipboard| clipboard.get_text())
}

/// Runs a read on the reading handle. "The clipboard holds something else"
/// is `None`; any other failure drops the handle (it is made again next time).
fn read_with<T>(
    read: impl FnOnce(&mut Clipboard) -> std::result::Result<T, arboard::Error>,
) -> Result<Option<T>> {
    let slot = READER.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.is_none() {
        *guard = Some(Clipboard::new().map_err(clipboard_error)?);
    }
    let clipboard = guard.as_mut().expect("initialized above");
    match read(clipboard) {
        Ok(value) => Ok(Some(value)),
        Err(arboard::Error::ContentNotAvailable) => Ok(None),
        Err(err) => {
            *guard = None;
            Err(clipboard_error(err))
        }
    }
}

/// The files and folders on the clipboard (a file manager's copy); empty if it
/// holds something else.
pub fn get_files() -> Result<Vec<PathBuf>> {
    Ok(read_with(|clipboard| clipboard.get().file_list())?.unwrap_or_default())
}

/// Replaces the clipboard's contents with a list of files and folders (what a
/// file manager's Copy puts there), so a paste in Explorer or Finder copies them.
pub fn set_files(paths: &[PathBuf]) -> Result<()> {
    if paths.is_empty() {
        return Err(PlatformError::Os {
            operation: "clipboard",
            message: "there are no files to copy".to_owned(),
        });
    }
    with_handle(&CLIPBOARD, |clipboard| clipboard.set().file_list(paths))
}

/// The X11 PRIMARY selection: the text highlighted in the app that owns it,
/// without anything having been copied. `None` if there is none.
#[cfg(target_os = "linux")]
pub fn get_primary_text() -> Result<Option<String>> {
    use arboard::{GetExtLinux, LinuxClipboardKind};
    read_with(|clipboard| {
        clipboard
            .get()
            .clipboard(LinuxClipboardKind::Primary)
            .text()
    })
}

/// Empties the clipboard.
pub fn clear() -> Result<()> {
    with_handle(&CLIPBOARD, |clipboard| clipboard.clear())
}

/// What was on the clipboard, as far as Sevak can put it back: plain text,
/// HTML (with its plain-text form) and files. Images and other formats are not
/// kept.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClipboardSnapshot {
    pub text: Option<String>,
    pub html: Option<String>,
    pub files: Vec<PathBuf>,
}

impl ClipboardSnapshot {
    pub fn is_empty(&self) -> bool {
        self.text.is_none() && self.html.is_none() && self.files.is_empty()
    }
}

/// Reads the clipboard for [`restore`]. Best effort: a format that cannot be
/// read is simply missing from the snapshot.
pub fn snapshot() -> ClipboardSnapshot {
    ClipboardSnapshot {
        files: get_files().unwrap_or_default(),
        text: read_text().ok().flatten(),
        html: read_with(|clipboard| clipboard.get().html()).ok().flatten(),
    }
}

/// Puts a [`snapshot`] back (or empties the clipboard if it was empty). Written
/// so the OS history and cloud sync skip it, and so Sevak's clipboard history
/// does not record it again: it was on the clipboard before.
pub fn restore(snapshot: &ClipboardSnapshot) -> Result<()> {
    if let Some(text) = &snapshot.text {
        note_own_write(text);
    }
    if snapshot.is_empty() {
        return clear();
    }
    with_handle(&CLIPBOARD, |clipboard| {
        if !snapshot.files.is_empty() {
            private_setter(clipboard).file_list(&snapshot.files)
        } else if let Some(html) = &snapshot.html {
            private_setter(clipboard).html(html.clone(), snapshot.text.clone())
        } else {
            private_setter(clipboard).text(snapshot.text.clone().unwrap_or_default())
        }
    })
}

/// A setter that asks the OS to keep what it writes out of its own history.
fn private_setter(clipboard: &mut Clipboard) -> arboard::Set<'_> {
    let setter = clipboard.set();
    #[cfg(windows)]
    let setter = arboard::SetExtWindows::exclude_from_monitoring(setter);
    #[cfg(target_os = "macos")]
    let setter = arboard::SetExtApple::exclude_from_history(setter);
    #[cfg(target_os = "linux")]
    let setter = arboard::SetExtLinux::exclude_from_history(setter);
    setter
}

/// A count of things in flight.
struct InFlight(AtomicUsize);

impl InFlight {
    const fn new() -> Self {
        Self(AtomicUsize::new(0))
    }

    fn enter(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }

    fn leave(&self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }

    fn any(&self) -> bool {
        self.0.load(Ordering::SeqCst) > 0
    }
}

/// The synthetic copies (Universal Actions) in flight.
static SYNTHETIC_COPIES: InFlight = InFlight::new();

/// While a value of this type exists, Sevak is borrowing the clipboard for a
/// synthetic copy of the user's selection: the clipboard history must not
/// record what is on it. Counted, so overlapping captures cannot end each
/// other's protection early.
pub struct SyntheticCopy(());

impl SyntheticCopy {
    pub fn begin() -> Self {
        SYNTHETIC_COPIES.enter();
        Self(())
    }
}

impl Drop for SyntheticCopy {
    fn drop(&mut self) {
        SYNTHETIC_COPIES.leave();
    }
}

/// True while a [`SyntheticCopy`] is alive.
pub fn synthetic_copy_in_progress() -> bool {
    SYNTHETIC_COPIES.any()
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
    fn an_empty_file_list_is_refused_before_the_clipboard_is_touched() {
        let err = set_files(&[]).unwrap_err();
        assert!(err.to_string().contains("no files"), "{err}");
    }

    #[test]
    fn overlapping_borrows_are_counted() {
        let count = InFlight::new();
        assert!(!count.any());
        count.enter();
        count.enter();
        assert!(count.any());
        count.leave();
        assert!(count.any());
        count.leave();
        assert!(!count.any());
    }

    #[test]
    fn empty_snapshots_are_empty() {
        assert!(ClipboardSnapshot::default().is_empty());
        let text = ClipboardSnapshot {
            text: Some("x".into()),
            ..ClipboardSnapshot::default()
        };
        assert!(!text.is_empty());
    }

    #[test]
    fn own_writes_are_recognised_once() {
        note_own_write("secret pasted text 1");
        assert!(take_own_write("secret pasted text 1"));
        assert!(!take_own_write("secret pasted text 1"));
        assert!(!take_own_write("something the user copied"));
    }
}
