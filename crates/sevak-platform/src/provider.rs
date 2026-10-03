//! The OS abstraction the rest of Sevak programs against.

use std::path::Path;

use sevak_core::{AppEntry, ClipContent, IconData, IconSource, LaunchTarget, ShellConfig};

use crate::browsers::BrowserRoot;
use crate::capture::{CaptureOptions, SelectionCapture};
use crate::clipboard::{ClipboardMedia, MediaRequest};
use crate::error::Result;
use crate::paste::{ClipboardRead, ForegroundApp, PasteOutcome, PasteSupport, UNSUPPORTED_REASON};
use crate::system::{SettingsPage, SystemCommand};

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

    /// Shows `path` selected in the system file manager (its parent folder
    /// opens with the item highlighted where the file manager supports it).
    fn reveal_path(&self, path: &Path) -> Result<()> {
        crate::open::reveal_path(path)
    }

    /// Whether [`PlatformProvider::launch_as_admin`] can work here. Plugins
    /// offer "Run as administrator" only when this is true.
    fn can_run_as_admin(&self) -> bool {
        false
    }

    /// Starts an application elevated (Windows: the `runas` verb, which shows
    /// the UAC prompt). Unsupported where `can_run_as_admin` is false.
    fn launch_as_admin(&self, _target: &LaunchTarget) -> Result<()> {
        Err(crate::error::PlatformError::Unsupported(
            "running as administrator",
        ))
    }

    /// Opens an `http(s)://` or `mailto:` URL with the default handler.
    fn open_url(&self, url: &str) -> Result<()> {
        crate::open::open_url(url)
    }

    /// Loads an icon as encoded image bytes, rendered at roughly `size`
    /// physical pixels where the source is not already an image file.
    /// [`IconSource::Builtin`] icons are drawn by the UI and are rejected here.
    fn load_icon(&self, source: &IconSource, size: u32) -> Result<IconData>;

    /// Opens a terminal window and runs `command` in it (an empty command just
    /// opens the terminal), using the terminal and shell chosen by `config`.
    /// The terminal is detached from Sevak. See [`crate::terminal`].
    fn run_in_terminal(&self, command: &str, config: &ShellConfig) -> Result<()> {
        crate::terminal::run_in_terminal(command, config)
    }

    /// Opens a terminal window in `dir`, at a shell prompt (it stays open even
    /// if `[shell] keep_open` is off). `dir` must be an absolute path.
    fn open_terminal_in(&self, dir: &Path, config: &ShellConfig) -> Result<()> {
        crate::terminal::open_terminal_in(dir, config)
    }

    /// Replaces the clipboard's contents with `text`.
    fn set_clipboard_text(&self, text: &str) -> Result<()> {
        crate::clipboard::set_text(text)
    }

    /// The power and session commands that can work on this system right now
    /// (for example no hibernate without swap, no logout the desktop cannot
    /// do). Probes the system; call it from a background thread.
    fn supported_system_commands(&self) -> Vec<SystemCommand> {
        crate::system::supported_commands()
    }

    /// Runs one of the [`PlatformProvider::supported_system_commands`].
    fn run_system_command(&self, command: SystemCommand) -> Result<()> {
        crate::system::run_command(command)
    }

    /// The pages of the system settings app that exist on this system.
    fn supported_settings_pages(&self) -> Vec<SettingsPage> {
        crate::system::supported_settings_pages()
    }

    /// Opens one page of the system settings app. A closed set of pages rather
    /// than a URI, so [`PlatformProvider::open_url`] can stay limited to web
    /// and mail links.
    fn open_settings_page(&self, page: SettingsPage) -> Result<()> {
        crate::system::open_settings_page(page)
    }

    /// The user-data folders of the web browsers installed for this user (only
    /// those that exist on disk), for the bookmarks plugin. Cheap: no file is
    /// read.
    fn browser_roots(&self) -> Vec<BrowserRoot> {
        crate::browsers::detect_roots()
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

    /// Puts an image or files on the clipboard and pastes them into the app
    /// remembered by [`PlatformProvider::remember_foreground_app`], like
    /// [`PlatformProvider::paste_text`] does for text.
    fn paste_clip(&self, content: &ClipContent, _restore_clipboard: bool) -> Result<PasteOutcome> {
        crate::clipboard::set_clip(content, false)?;
        Ok(PasteOutcome::CopiedOnly(UNSUPPORTED_REASON.to_owned()))
    }

    /// Replaces the clipboard's contents with an image or files.
    fn set_clipboard_clip(&self, content: &ClipContent) -> Result<()> {
        crate::clipboard::set_clip(content, false)
    }

    /// Reads what is selected in the app that has focus (Universal Actions):
    /// the app is asked to copy it, and the clipboard is put back as it was.
    /// Call it from a background thread, before Sevak's window takes focus.
    /// See [`crate::capture`].
    fn capture_selection(&self, _options: &CaptureOptions) -> SelectionCapture {
        SelectionCapture::Unavailable(
            "Reading the selection is not supported on this system".to_owned(),
        )
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

    /// Reads the files and the image on the clipboard, as far as `request`
    /// asks (they are only worth reading when the history records them).
    /// Called after [`PlatformProvider::read_clipboard`] found the content not
    /// secret. Something unreadable is simply absent.
    fn read_clipboard_media(&self, request: MediaRequest) -> ClipboardMedia {
        crate::clipboard::read_media(request)
    }
}
