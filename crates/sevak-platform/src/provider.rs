//! The OS abstraction the rest of Sevak programs against.

use std::path::Path;

use sevak_core::{AppEntry, IconData, IconSource, LaunchTarget, ShellConfig};

use crate::error::Result;

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
}
