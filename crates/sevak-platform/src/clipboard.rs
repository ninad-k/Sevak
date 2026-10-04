//! Clipboard access via `arboard`.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use arboard::Clipboard;
use sevak_core::ClipContent;

use crate::clip_media::{files_hash, ClipboardImage};
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
/// Like every write Sevak makes, the clipboard history does not record it (see
/// [`set_clip`], which this is).
pub fn set_files(paths: &[PathBuf]) -> Result<()> {
    if paths.is_empty() {
        return Err(PlatformError::Os {
            operation: "clipboard",
            message: "there are no files to copy".to_owned(),
        });
    }
    set_clip(
        &ClipContent::Files {
            paths: paths.to_vec(),
        },
        false,
    )
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
/// HTML (with its plain-text form), files and an image. Other formats are not
/// kept. The clipboard holds one of these at a time when it is put back:
/// files, else an image, else HTML, else text.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClipboardSnapshot {
    pub text: Option<String>,
    pub html: Option<String>,
    pub files: Vec<PathBuf>,
    /// Read only when the clipboard holds no text and no files: an app that
    /// copies text *and* a picture (a spreadsheet range) is restored as text.
    pub image: Option<ClipboardImage>,
}

impl ClipboardSnapshot {
    pub fn is_empty(&self) -> bool {
        self.text.is_none() && self.html.is_none() && self.files.is_empty() && self.image.is_none()
    }
}

/// Reads the clipboard for [`restore`]. Best effort: a format that cannot be
/// read is simply missing from the snapshot.
pub fn snapshot() -> ClipboardSnapshot {
    let files = get_files().unwrap_or_default();
    let text = read_text().ok().flatten();
    // Reading a picture is the costly part; skip it when text or files already
    // describe the clipboard.
    let image = if files.is_empty() && text.is_none() {
        get_image().ok().flatten()
    } else {
        None
    };
    ClipboardSnapshot {
        files,
        text,
        html: read_with(|clipboard| clipboard.get().html()).ok().flatten(),
        image,
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
    if !snapshot.files.is_empty() {
        note_own_files(&snapshot.files);
    } else if let Some(image) = &snapshot.image {
        note_own_image(image);
    }
    let restored = with_handle(&CLIPBOARD, |clipboard| {
        if !snapshot.files.is_empty() {
            private_setter(clipboard).file_list(&snapshot.files)
        } else if let Some(image) = &snapshot.image {
            private_setter(clipboard).image(image_data(image))
        } else if let Some(html) = &snapshot.html {
            private_setter(clipboard).html(html.clone(), snapshot.text.clone())
        } else {
            private_setter(clipboard).text(snapshot.text.clone().unwrap_or_default())
        }
    });
    if restored.is_ok() && !snapshot.files.is_empty() {
        note_files_read_back();
    }
    restored
}

/// The pixels of `image` in the form `arboard` takes.
fn image_data(image: &ClipboardImage) -> arboard::ImageData<'_> {
    arboard::ImageData {
        width: image.width as usize,
        height: image.height as usize,
        bytes: std::borrow::Cow::Borrowed(&image.rgba),
    }
}

/// The image on the clipboard; `None` if it holds something else, or an image
/// too large to hold (see [`crate::clip_media::MAX_IMAGE_RAW_BYTES`]).
pub fn get_image() -> Result<Option<ClipboardImage>> {
    // The clipboard library converts the whole picture before Sevak sees it;
    // an enormous one is turned away on its header (Windows) first.
    if crate::clip_media::clipboard_image_is_oversized() {
        return Ok(None);
    }
    let data = read_with(|clipboard| clipboard.get_image())?;
    Ok(data.and_then(|data| {
        ClipboardImage::new(
            u32::try_from(data.width).ok()?,
            u32::try_from(data.height).ok()?,
            data.bytes.into_owned(),
        )
    }))
}

/// What the clipboard history wants to know besides the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaRequest {
    /// Read the file list (a file manager's copy).
    pub files: bool,
    /// Read the image, if there are no files.
    pub image: bool,
}

/// Files and an image from the clipboard (see [`read_media`]).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClipboardMedia {
    pub files: Vec<PathBuf>,
    pub image: Option<ClipboardImage>,
}

/// Reads what [`MediaRequest`] asks for. Files win over an image, as a file
/// manager's copy can carry a preview picture too. A format that cannot be read
/// is simply absent: the history would only retry it forever.
pub fn read_media(request: MediaRequest) -> ClipboardMedia {
    let mut media = ClipboardMedia::default();
    if request.files {
        media.files = get_files().unwrap_or_default();
    }
    if request.image && media.files.is_empty() {
        media.image = get_image().unwrap_or_else(|err| {
            tracing::debug!("clipboard image not readable: {err}");
            None
        });
    }
    media
}

/// Puts an image or files on the clipboard. `private` asks the OS to keep
/// them out of its own history and cloud sync (used when pasting).
pub fn set_clip(content: &ClipContent, private: bool) -> Result<()> {
    match content {
        ClipContent::Image { path } => {
            let image = load_png_file(path)?;
            note_own_image(&image);
            with_handle(&CLIPBOARD, |clipboard| {
                if private {
                    private_setter(clipboard).image(image_data(&image))
                } else {
                    clipboard.set().image(image_data(&image))
                }
            })
        }
        ClipContent::Files { paths } => {
            note_own_files(paths);
            with_handle(&CLIPBOARD, |clipboard| {
                if private {
                    private_setter(clipboard).file_list(paths)
                } else {
                    clipboard.set().file_list(paths)
                }
            })?;
            note_files_read_back();
            Ok(())
        }
    }
}

/// The most bytes of a PNG file Sevak reads back.
const MAX_PNG_FILE_BYTES: u64 = 64 * 1024 * 1024;

fn load_png_file(path: &Path) -> Result<ClipboardImage> {
    let size = std::fs::metadata(path)?.len();
    if size > MAX_PNG_FILE_BYTES {
        return Err(PlatformError::Os {
            operation: "clipboard",
            message: format!("{} is too large to copy", path.display()),
        });
    }
    ClipboardImage::decode_png(&sevak_core::bounded_read::read_capped(
        path,
        MAX_PNG_FILE_BYTES,
    )?)
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

/// Texts, images and file lists Sevak itself put on the clipboard recently, as
/// hashes. The clipboard history skips them: a paste or copy Sevak performs is
/// not something the user copied from another app.
static OWN_WRITES: Mutex<Vec<(u64, Instant)>> = Mutex::new(Vec::new());

/// How long a write counts as ours. The history polls a few times a second.
const OWN_WRITE_WINDOW: Duration = Duration::from_secs(10);

fn hash_text(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

// Images and file lists are told apart from texts (and from each other) by
// mixing a different constant into their hashes.
fn own_image_key(image: &ClipboardImage) -> u64 {
    image.content_hash() ^ 0x1a2b_3c4d_5e6f_7081
}

fn own_files_key(paths: &[PathBuf]) -> u64 {
    files_hash(paths) ^ 0x8170_6f5e_4d3c_2b1a
}

fn note_own(key: u64) {
    let mut writes = OWN_WRITES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    writes.retain(|(_, at)| at.elapsed() < OWN_WRITE_WINDOW);
    writes.push((key, Instant::now()));
}

fn take_own(key: u64) -> bool {
    let mut writes = OWN_WRITES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    writes.retain(|(_, at)| at.elapsed() < OWN_WRITE_WINDOW);
    match writes.iter().position(|(k, _)| *k == key) {
        Some(index) => {
            writes.remove(index);
            true
        }
        None => false,
    }
}

/// Records that Sevak is about to put `text` on the clipboard (the writers in
/// this module do it themselves; this is for code that writes another way).
pub fn note_own_write(text: &str) {
    note_own(hash_text(text));
}

/// True once if `text` is what Sevak itself recently put on the clipboard.
pub fn take_own_write(text: &str) -> bool {
    take_own(hash_text(text))
}

/// [`note_own_write`] for an image.
pub fn note_own_image(image: &ClipboardImage) {
    note_own(own_image_key(image));
}

/// [`take_own_write`] for an image.
pub fn take_own_image(image: &ClipboardImage) -> bool {
    take_own(own_image_key(image))
}

/// [`note_own_write`] for a list of files.
pub fn note_own_files(paths: &[PathBuf]) {
    note_own(own_files_key(paths));
}

/// [`take_own_write`] for a list of files.
pub fn take_own_files(paths: &[PathBuf]) -> bool {
    take_own(own_files_key(paths))
}

/// The OS may spell the paths it was given differently (case, `\\?\` prefix),
/// so what the clipboard now holds is noted as well as what was written.
fn note_files_read_back() {
    if let Ok(paths) = get_files() {
        if !paths.is_empty() {
            note_own_files(&paths);
        }
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

    fn picture(seed: u8, alpha: u8) -> ClipboardImage {
        let rgba = (0..4 * 3)
            .flat_map(|i: u32| [seed, i as u8 * 20, 255 - i as u8, alpha])
            .collect();
        ClipboardImage::new(4, 3, rgba).unwrap()
    }

    #[test]
    fn own_images_and_files_are_recognised_once_and_apart_from_text() {
        let image = picture(201, 255);
        note_own_image(&image);
        assert!(!take_own_image(&picture(202, 255)));
        assert!(take_own_image(&image));
        assert!(!take_own_image(&image));

        let files = vec![PathBuf::from("/own/one.txt"), PathBuf::from("/own/two.txt")];
        note_own_files(&files);
        assert!(!take_own_files(&files[..1]));
        assert!(take_own_files(&files));
        assert!(!take_own_files(&files));

        // A text that happens to be a path list is not the file list.
        note_own_files(&files);
        assert!(!take_own_write(
            "/own/one.txt
/own/two.txt"
        ));
        assert!(take_own_files(&files));
    }

    #[test]
    fn a_snapshot_with_only_an_image_is_not_empty() {
        let snapshot = ClipboardSnapshot {
            image: Some(picture(1, 255)),
            ..ClipboardSnapshot::default()
        };
        assert!(!snapshot.is_empty());
    }

    /// Images, files and snapshots through the real clipboard: writes it (and
    /// puts the user's text back), so it is run by hand:
    /// `cargo test -p sevak-platform real_clipboard_round_trip -- --ignored`
    #[test]
    #[ignore = "writes to the real clipboard"]
    fn real_clipboard_round_trip() {
        let before = snapshot();
        let dir = tempfile::tempdir().unwrap();

        // An opaque and a translucent picture, from PNG files, come back as written.
        for image in [picture(10, 255), picture(20, 128)] {
            let png = dir.path().join("p.png");
            std::fs::write(&png, image.encode_png().unwrap()).unwrap();
            set_clip(&ClipContent::Image { path: png }, true).unwrap();
            assert_eq!(get_image().unwrap().as_ref(), Some(&image));
            // It is the image alone: no text, no files.
            assert_eq!(read_text().unwrap(), None);
            assert!(get_files().unwrap().is_empty());
            // Sevak's own write is recognised, once.
            assert!(take_own_image(&image));

            // A Universal Actions style borrow: save, copy text over it, restore.
            let saved = snapshot();
            assert_eq!(saved.image.as_ref(), Some(&image));
            assert!(saved.text.is_none() && saved.files.is_empty());
            set_text("the selection").unwrap();
            assert_eq!(get_text().unwrap().as_deref(), Some("the selection"));
            restore(&saved).unwrap();
            assert_eq!(get_image().unwrap().as_ref(), Some(&image));
            assert_eq!(read_text().unwrap(), None);
            assert!(take_own_image(&image), "the restore is not a new copy");
        }

        // Files.
        let one = dir.path().join("one.txt");
        let two = dir.path().join("two.txt");
        std::fs::write(&one, "1").unwrap();
        std::fs::write(&two, "2").unwrap();
        let names = |paths: Vec<PathBuf>| -> Vec<String> {
            paths
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect()
        };
        set_clip(
            &ClipContent::Files {
                paths: vec![one, two],
            },
            true,
        )
        .unwrap();
        assert_eq!(names(get_files().unwrap()), ["one.txt", "two.txt"]);
        let saved = snapshot();
        set_text("the selection").unwrap();
        restore(&saved).unwrap();
        assert_eq!(names(get_files().unwrap()), ["one.txt", "two.txt"]);
        // What the OS reports back (its spelling of the paths) is what is noted.
        assert!(take_own_files(&get_files().unwrap()));

        // Leave the user's clipboard as it was.
        restore(&before).unwrap();
    }
}
