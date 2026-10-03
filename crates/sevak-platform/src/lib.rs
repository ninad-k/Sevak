//! Operating-system integration for Sevak.
//!
//! Everything that touches the OS lives here, behind small cross-platform
//! functions. Platform-specific implementations are selected with
//! `#[cfg(target_os = ...)]` inside each module so callers never need cfg gates.

pub mod error;
pub mod gnome;
pub mod open;
pub mod paths;
pub mod process;
pub mod session;

#[cfg(windows)]
mod windows;

pub use error::{PlatformError, Result};
pub use paths::AppPaths;
pub use session::{DisplayServer, HotkeyStrategy};
