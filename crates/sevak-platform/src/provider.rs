//! The OS abstraction the rest of Sevak programs against.

use std::path::Path;

use sevak_core::{AppEntry, IconData, IconSource, LaunchTarget};

use crate::error::Result;
use crate::paste::{ClipboardRead, ForegroundApp, PasteOutcome, PasteSupport, UNSUPPORTED_REASON};

/// Everything Sevak needs from the operating system to find and start things.
///
/// Implementations: `WindowsProvider` (Start Menu shortcuts + packaged apps),
/// `MacProvider` (`.app` bundles) and `LinuxProvider` (freedesktop `.desktop`
/// entries). Obtain the one for the
/// current OS with [`crate::native_provider`].
pub trait PlatformProvider: Send + Sync {
    /// Enumerates installed, user-visible applications. Slow (file system and
    /// shell enumeration); call it from a background thread.
    fn list_applications(&self) -> Result<Vec<AppEntry>>;

    /// Starts an application, fully detached from Sevak.
    fn launch(&self, target: &LaunchTarget) -> Result<()>;

    /// Opens a file or folder with its default handler.
    fn open_path(&self, path: &Path) -> Result<()> {
        crate::open::open_path(path)
    }

    /// Opens an `http(s)://` or `mailto:` URL with the default handler.
    fn open_url(&self, url: &str) -> Result<()> {
        crate::open::open_url(url)
    }

    /// Loads an icon as encoded image bytes, rendered at roughly `size`
    /// physical pixels where the source is not already an image file.
    /// [`IconSource::Builtin`] icons are drawn by the UI and are rejected here.
    fn load_icon(&self, source: &IconSource, size: u32) -> Result<IconData>;

    /// Replaces the clipboard's contents with `text`.
    fn set_clipboard_text(&self, text: &str) -> Result<()> {
        crate::clipboard::set_text(text)
    }

    /// The clipboard's text, or `None` if it holds something else.
    fn clipboard_text(&self) -> Result<Option<String>> {
        crate::clipboard::get_text()
    }

    /// Notes which window has focus, so [`PlatformProvider::paste_text`] can
    /// return to it. Called as Sevak's window is about to be shown, while the
    /// user's app still has focus. Must be fast; does nothing if the focused
    /// window is Sevak's own.
    fn remember_foreground_app(&self) {}

    /// The app with focus right now (what a copy came from), if it can be told.
    fn foreground_app(&self) -> Option<ForegroundApp> {
        None
    }

    /// Whether [`PlatformProvider::paste_text`] can paste on this system right
    /// now (it can change at runtime: macOS needs a permission the user may
    /// grant later). Cheap enough to call on every keystroke.
    fn paste_support(&self) -> PasteSupport {
        PasteSupport::CopyOnly(UNSUPPORTED_REASON.to_owned())
    }

    /// Puts `text` on the clipboard and pastes it into the app remembered by
    /// [`PlatformProvider::remember_foreground_app`]. Sevak's window should
    /// already be hidden. Where pasting is not possible the text is only
    /// copied, and the outcome says why.
    fn paste_text(&self, text: &str, _restore_clipboard: bool) -> Result<PasteOutcome> {
        crate::clipboard::set_text(text)?;
        Ok(PasteOutcome::CopiedOnly(UNSUPPORTED_REASON.to_owned()))
    }

    /// A counter that changes whenever the clipboard does, where the OS has one
    /// (Windows sequence number, macOS change count). `None` means the history
    /// must compare the text itself.
    fn clipboard_sequence(&self) -> Option<u64> {
        None
    }

    /// Reads the clipboard for the history: the text, unless the app that
    /// copied it marked it secret. An error means "try again later".
    fn read_clipboard(&self) -> Result<ClipboardRead> {
        Ok(ClipboardRead {
            text: crate::clipboard::read_text()?,
            sensitive: false,
        })
    }
}
