//! The OS abstraction the rest of Sevak programs against.

use std::path::Path;

use sevak_core::{AppEntry, IconData, IconSource, LaunchTarget, ShellConfig};

use crate::browsers::BrowserRoot;
use crate::error::Result;
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
}
