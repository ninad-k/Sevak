//! Operating-system integration for Sevak.
//!
//! Everything that touches the OS lives here, behind small cross-platform
//! functions and the [`PlatformProvider`] trait. Platform-specific
//! implementations are selected with `#[cfg(target_os = ...)]` so callers never
//! need cfg gates.

pub mod browsers;
pub mod capture;
pub mod clipboard;
pub mod desktop_entry;
pub mod error;
pub mod gnome;
pub mod icon_file;
pub mod icon_theme;
pub mod media;
pub mod open;
pub mod os_search;
pub mod paste;
pub mod paths;
pub mod private_file;
pub mod process;
pub mod provider;
pub mod session;
pub mod system;
pub mod terminal;
pub mod trash;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
compile_error!("Sevak supports Windows, macOS and Linux only");

pub use browsers::{BrowserFamily, BrowserRoot};
pub use capture::{CaptureOptions, SelectionCapture};
pub use clipboard::{ClipboardMedia, MediaRequest};
pub use error::{PlatformError, Result};
pub use media::ClipboardImage;
pub use os_search::{OsHit, OsSearchError, OsSearchKind, OsSearchRequest};
pub use paste::{ClipboardRead, ForegroundApp, PasteOutcome, PasteSupport};
pub use paths::AppPaths;
pub use provider::PlatformProvider;
pub use session::{DisplayServer, HotkeyStrategy};
pub use system::{SettingsPage, SystemCommand};

/// The [`PlatformProvider`] for the operating system Sevak was built for.
pub fn native_provider() -> Box<dyn PlatformProvider> {
    #[cfg(windows)]
    {
        Box::new(windows::WindowsProvider::new())
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::LinuxProvider::new())
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacProvider::new())
    }
}
