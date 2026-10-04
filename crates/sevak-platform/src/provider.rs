//! The OS abstraction the rest of Sevak programs against.

use std::path::{Path, PathBuf};

use sevak_core::preview::{RenderKind, Rendered};
use sevak_core::{AppEntry, ClipContent, IconData, IconSource, LaunchTarget, ShellConfig};

use crate::browsers::BrowserRoot;
use crate::capture::{CaptureOptions, SelectionCapture};
use crate::clipboard::{ClipboardMedia, MediaRequest};
use crate::contacts::{Contact, ContactsAccess};
use crate::deep_link::DeepLink;
use crate::dictionary::Spelling;
use crate::error::Result;
use crate::keyboard::{KeyListener, KeyListenerSupport, KeySink, TypingTarget};
use crate::media::{MediaCommand, NowPlaying};
use crate::os_search::{OsHit, OsSearchError, OsSearchRequest};
use crate::paste::{ClipboardRead, ForegroundApp, PasteOutcome, PasteSupport, UNSUPPORTED_REASON};
use crate::system::{SettingsPage, SystemCommand};
use crate::tasks::{Drive, ProcessInfo, RunningApp, Task, TaskKind};

/// Everything Sevak needs from the operating system to find and start things.
///
/// Implementations: `WindowsProvider` (Start Menu shortcuts + packaged apps),
/// `MacProvider` (`.app` bundles) and `LinuxProvider` (freedesktop `.desktop`
/// entries). Obtain the one for the
/// current OS with [`crate::native_provider`].
pub trait PlatformProvider: Send + Sync {
    /// What the OS calls itself (name, version, build), for the diagnostics
    /// report. Cheap; reads the registry, `/etc/os-release` or runs `sw_vers`.
    fn os_info(&self) -> sevak_core::diagnostics::OsInfo {
        crate::os_info::detect()
    }

    /// Enumerates installed, user-visible applications. Slow (file system and
    /// shell enumeration); call it from a background thread.
    fn list_applications(&self) -> Result<Vec<AppEntry>>;

    /// Starts an application, fully detached from Sevak.
    fn launch(&self, target: &LaunchTarget) -> Result<()>;

    /// Opens a file or folder with its default handler.
    ///
    /// A network path is refused unless `[files] allow_network_paths` is on
    /// ([`crate::netpath`]).
    fn open_path(&self, path: &Path) -> Result<()> {
        crate::netpath::guard(path)?;
        crate::open::open_path(path)
    }

    /// Shows `path` selected in the system file manager (its parent folder
    /// opens with the item highlighted where the file manager supports it).
    fn reveal_path(&self, path: &Path) -> Result<()> {
        crate::netpath::guard(path)?;
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

    /// How a value must be quoted to be one literal word for the shell
    /// [`PlatformProvider::run_in_terminal`] would start with `config`.
    fn shell_quoting(&self, config: &ShellConfig) -> crate::terminal::ShellQuoting {
        crate::terminal::shell_quoting(config)
    }

    /// Opens a terminal window in `dir`, at a shell prompt (it stays open even
    /// if `[shell] keep_open` is off). `dir` must be an absolute path.
    fn open_terminal_in(&self, dir: &Path, config: &ShellConfig) -> Result<()> {
        crate::netpath::guard(dir)?;
        crate::terminal::open_terminal_in(dir, config)
    }

    /// Replaces the clipboard's contents with `text`.
    fn set_clipboard_text(&self, text: &str) -> Result<()> {
        crate::clipboard::set_text(text)
    }

    /// Puts the files and folders on the clipboard as a file list, so pasting
    /// in Explorer, Finder or a Linux file manager copies them.
    fn set_clipboard_files(&self, paths: &[PathBuf]) -> Result<()> {
        crate::clipboard::set_files(paths)
    }

    /// Moves a file or folder to the system trash (never deletes it for good;
    /// it fails where the item cannot be trashed). `path` must be absolute.
    fn move_to_trash(&self, path: &Path) -> Result<()> {
        crate::netpath::guard(path)?;
        crate::trash::move_to_trash(path)
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

    /// The automation tasks (dark mode, volume, quit an app, ...) that can work
    /// on this system right now. Probes the system (`PATH`, radios); call it
    /// from a background thread.
    fn supported_tasks(&self) -> Vec<TaskKind> {
        crate::tasks::supported_tasks()
    }

    /// Runs a [`Task`] of one of the [`PlatformProvider::supported_tasks`]
    /// kinds. A closed vocabulary, like system commands.
    fn run_task(&self, task: &Task) -> Result<()> {
        crate::tasks::run_task(task)
    }

    /// Every process with its CPU and memory use, for the `kill` task. Takes a
    /// fraction of a second; call it from a background thread.
    fn list_processes(&self) -> Result<Vec<ProcessInfo>> {
        crate::tasks::list_processes()
    }

    /// The apps that have a window, for the `quit` task. Call it from a
    /// background thread.
    fn list_running_apps(&self) -> Result<Vec<RunningApp>> {
        crate::tasks::list_running_apps()
    }

    /// The removable drives that can be ejected. Call it from a background
    /// thread.
    fn list_removable_drives(&self) -> Result<Vec<Drive>> {
        crate::tasks::list_drives()
    }

    /// The media buttons that can work here (none on Linux without `playerctl`).
    /// Probes the system; call it from a background thread.
    fn supported_media_commands(&self) -> Vec<MediaCommand> {
        crate::media::available_commands()
    }

    /// Presses a media button on the player the user is listening to.
    fn media_control(&self, command: MediaCommand) -> Result<()> {
        crate::media::control(command)
    }

    /// Whether [`PlatformProvider::now_playing`] can work here.
    fn now_playing_available(&self) -> bool {
        crate::media::now_playing_available()
    }

    /// The track playing now, if any (`None` when nothing is). Talks to the
    /// player and can take a moment: call it from a background thread, never
    /// while the user types.
    fn now_playing(&self) -> Result<Option<NowPlaying>> {
        crate::media::now_playing()
    }

    /// The user-data folders of the web browsers installed for this user (only
    /// those that exist on disk), for the bookmarks plugin. Cheap: no file is
    /// read.
    fn browser_roots(&self) -> Vec<BrowserRoot> {
        crate::browsers::detect_roots()
    }

    /// A picture of the first page of `path` (a PDF), or a thumbnail (an Office
    /// document, a video), drawn by the operating system, for the preview pane.
    /// Blocks until it is drawn or gives up (every helper has a timeout): call it
    /// from a background thread. The caller has already limited the file size.
    fn render_thumbnail(&self, path: &Path, kind: RenderKind) -> Rendered {
        crate::thumbnail::render(path, kind)
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

    /// Whether [`PlatformProvider::foreground_app`] works on this system at all
    /// (not on Wayland, which tells apps nothing about other windows). When it
    /// does and still answers `None` for a window, the app could not be told
    /// (Windows: a process that cannot be opened; Linux: a window without
    /// `WM_CLASS`; macOS: an app without a bundle id), and everything that
    /// excludes apps by name treats that as "excluded".
    fn identifies_apps(&self) -> bool {
        false
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

    /// Whether [`PlatformProvider::start_key_listener`] can work right now (it
    /// can change at runtime: macOS needs a permission the user may grant
    /// later). Cheap.
    fn key_listener_support(&self) -> KeyListenerSupport {
        KeyListenerSupport::Unavailable(crate::keyboard::UNSUPPORTED_REASON.to_owned())
    }

    /// Asks the OS to show its permission prompt for watching the keyboard
    /// (macOS Input Monitoring), so the user can grant it. Called when
    /// [`PlatformProvider::key_listener_support`] says a permission is missing.
    fn request_key_listener_permission(&self) {}

    /// Starts reporting what is typed in other apps (snippet expansion; see
    /// [`crate::keyboard`]). Observes keystrokes until the returned
    /// [`KeyListener`] is dropped, so only call it when the user has turned
    /// the feature on.
    fn start_key_listener(&self, _sink: KeySink) -> Result<KeyListener> {
        Err(crate::error::PlatformError::Unsupported(
            "watching the keyboard",
        ))
    }

    /// Where the next typed character would land: the app, whether it is one of
    /// Sevak's own windows, whether the focused control hides what is typed.
    /// Called on keystrokes of an active key listener, so it must be cheap.
    fn typing_target(&self) -> TypingTarget {
        TypingTarget::default()
    }

    /// Removes the last `delete` characters typed into the app in front and
    /// pastes `text` instead, restoring the clipboard afterwards (snippet
    /// expansion). Blocks for a few hundred milliseconds; call it from a
    /// background thread.
    ///
    /// `still_current` is asked just before any key is pressed: if it says
    /// `false` (the user typed more since the keyword was seen) nothing is
    /// deleted and the result is `Ok(false)`; `Ok(true)` means the text was
    /// replaced.
    fn replace_typed_text(
        &self,
        _delete: usize,
        _text: &str,
        _still_current: &dyn Fn() -> bool,
    ) -> Result<bool> {
        Err(crate::error::PlatformError::Unsupported(
            "replacing typed text",
        ))
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

    /// Searches the whole disk through the OS's own file index (Windows Search,
    /// Spotlight, locate / Tracker / Baloo): file names or the text inside
    /// files. Queries go only to that local index. Slow and blocking; call it
    /// from a background thread. See [`crate::os_search`].
    fn os_search(
        &self,
        request: &OsSearchRequest,
    ) -> std::result::Result<Vec<OsHit>, OsSearchError> {
        crate::os_search::search(request)
    }

    /// What encrypts files for the current user on this system (Windows:
    /// DPAPI), used for the clipboard history. `None` where there is nothing
    /// of the kind: the files are then plain, readable by their owner only.
    fn history_sealer(&self) -> Option<std::sync::Arc<dyn sevak_core::sealed::Sealer>> {
        None
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

    /// Opens a link from the closed [`DeepLink`] list (`tel:`, 1Password,
    /// macOS Contacts). Separate from [`PlatformProvider::open_url`], which
    /// stays limited to web and mail links.
    fn open_link(&self, link: &DeepLink) -> Result<()> {
        crate::open::open_deep_link(link)
    }

    /// Whether Sevak may read the OS address book (macOS Contacts). Cheap and
    /// never asks the user anything. Systems without a readable address book
    /// say [`ContactsAccess::Unsupported`]; the contacts plugin then relies on
    /// vCard files.
    fn contacts_access(&self) -> ContactsAccess {
        ContactsAccess::Unsupported
    }

    /// Shows the OS question "allow Sevak to access your contacts?" and waits
    /// for the answer (macOS). Call it from a background thread, and only
    /// because the user asked for it.
    fn request_contacts_access(&self) -> Result<ContactsAccess> {
        Ok(self.contacts_access())
    }

    /// Reads the OS address book (macOS Contacts, Windows People). Slow; call it
    /// from a background thread, and only when
    /// [`PlatformProvider::contacts_access`] is [`ContactsAccess::Granted`].
    /// Systems without one return an empty list.
    fn system_contacts(&self) -> Result<Vec<Contact>> {
        Ok(Vec::new())
    }

    /// Evolution Data Server address books (`contacts.db` files; Linux). Only
    /// paths, nothing is read.
    fn evolution_address_books(&self) -> Vec<PathBuf> {
        crate::contacts::evolution_databases()
    }

    /// The definition of `word` from the OS dictionary (macOS Dictionary
    /// Services), as plain text, or `None` if the OS has no dictionary or no
    /// entry. Offline and quick.
    fn system_definition(&self, _word: &str) -> Option<String> {
        None
    }

    /// Spell-checks one word with the OS spell checker (Windows `ISpellChecker`).
    /// `None` where the OS has none, or when it could not answer in time; the
    /// dictionary plugin then uses its own word list.
    fn system_spelling(&self, _word: &str) -> Option<Spelling> {
        None
    }
}
