//! Refusing network paths before any file system call.
//!
//! On Windows, touching `\\server\share\x` (listing it, asking whether it
//! exists, opening it) makes the system connect to `server` and authenticate
//! to it. A path that was typed, pasted or handed to Sevak by another program
//! can therefore leak credentials just by being looked at. `[files]
//! allow_network_paths` (off by default) decides whether such paths are used;
//! this module is the one place that says whether a path is one:
//!
//! * UNC forms (`\\server\share`, `//server/share`, `\\?\UNC\...`,
//!   `\\.\UNC\...`) and Windows **mapped network drives** (`Z:\`, found with
//!   `GetDriveTypeW`, which answers from the local drive table without
//!   contacting the server) are *network paths*: refused unless allowed.
//! * Other device paths (`\\.\pipe\...`, `\\?\GLOBALROOT\...`) are never a
//!   file or folder Sevak should open: refused whatever the setting says.
//!
//! On other systems these spellings are ordinary file-name characters and
//! nothing is refused.
//!
//! Callers that have the setting (the files plugin) pass it to [`refusal`].
//! Everything that opens or reveals a path through [`PlatformProvider`] is
//! also checked against the process-wide setting ([`set_allow_network_paths`],
//! applied from the configuration at startup and on reload), so a caller
//! that forgot the check still cannot reach the network by accident.
//!
//! [`PlatformProvider`]: crate::PlatformProvider

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use sevak_core::netpath::{
    is_device_path, is_network_path, DEVICE_PATH_REFUSED, NETWORK_PATHS_OFF,
};

use crate::error::{PlatformError, Result};

static ALLOW_NETWORK: AtomicBool = AtomicBool::new(false);

/// Sets the process-wide `[files] allow_network_paths`. Off until the
/// configuration says otherwise.
pub fn set_allow_network_paths(allow: bool) {
    ALLOW_NETWORK.store(allow, Ordering::Relaxed);
}

/// The process-wide setting.
pub fn allow_network_paths() -> bool {
    ALLOW_NETWORK.load(Ordering::Relaxed)
}

/// Why a path must not be touched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// A network path while `[files] allow_network_paths` is off.
    Network,
    /// A Windows device path.
    Device,
}

impl Refusal {
    /// One line for the user.
    pub fn message(self) -> &'static str {
        match self {
            Self::Network => NETWORK_PATHS_OFF,
            Self::Device => DEVICE_PATH_REFUSED,
        }
    }
}

/// Whether `path` (text, as typed) must not be touched, given the setting.
/// Looks at the text and, for a drive letter, at the drive table: no file or
/// network access.
pub fn refusal(path: &str, allow_network: bool) -> Option<Refusal> {
    refusal_with(path, allow_network, cfg!(windows), &is_network_drive)
}

/// [`refusal`] without the drive table: only the spelling of `text` counts
/// (`\\server\share`, device paths). For input that is probably not a path
/// at all, such as a search query, where `z:foo` must not be taken for a
/// drive.
pub fn text_refusal(text: &str, allow_network: bool) -> Option<Refusal> {
    refusal_with(text, allow_network, cfg!(windows), &|_| false)
}

/// [`refusal`] with the machine-dependent parts passed in, for tests.
pub(crate) fn refusal_with(
    path: &str,
    allow_network: bool,
    windows: bool,
    remote_drive: &dyn Fn(&str) -> bool,
) -> Option<Refusal> {
    if !windows {
        return None;
    }
    let path = path.trim();
    if is_device_path(path) {
        return Some(Refusal::Device);
    }
    if !allow_network && (is_network_path(path) || remote_drive(path)) {
        return Some(Refusal::Network);
    }
    None
}

/// [`refusal`] for a [`Path`], as a result: the check every call that is about
/// to touch a path makes with the process-wide setting.
pub fn guard(path: &Path) -> Result<()> {
    match refusal(&path.to_string_lossy(), allow_network_paths()) {
        None => Ok(()),
        Some(why) => Err(PlatformError::Os {
            operation: "open",
            message: why.message().to_owned(),
        }),
    }
}

/// Whether `path` is on a mapped network drive (`Z:\...` where `Z:` is a
/// network share). Always `false` off Windows and for paths without a drive
/// letter.
pub fn is_network_drive(path: &str) -> bool {
    #[cfg(windows)]
    {
        windows_drive_is_remote(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        false
    }
}

#[cfg(windows)]
fn windows_drive_is_remote(path: &str) -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDriveTypeW;

    const DRIVE_REMOTE: u32 = 4;
    let Some(letter) = drive_letter(path) else {
        return false;
    };
    let root: Vec<u16> = format!("{letter}:\\")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    // SAFETY: `root` is NUL-terminated and outlives the call. The call reads
    // the local drive table; it does not contact the server.
    let kind = unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) };
    kind == DRIVE_REMOTE
}

/// The drive letter a path starts with (`C:\x`, `c:`, `\\?\C:\x`), if any.
fn drive_letter(path: &str) -> Option<char> {
    let unified = path.trim().replace('/', "\\");
    let rest = unified
        .strip_prefix("\\\\?\\")
        .or_else(|| unified.strip_prefix("\\\\.\\"))
        .unwrap_or(&unified);
    let mut chars = rest.chars();
    let letter = chars.next().filter(char::is_ascii_alphabetic)?;
    (chars.next() == Some(':')).then_some(letter.to_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn never(_: &str) -> bool {
        false
    }

    fn z_is_remote(path: &str) -> bool {
        drive_letter(path) == Some('Z')
    }

    #[test]
    fn network_paths_are_refused_on_windows_unless_allowed() {
        for path in [
            r"\\server\share\x",
            "//server/share/x",
            r"\\?\UNC\server\share",
            r"\\.\UNC\server\share",
            r"  \\server\share  ",
        ] {
            assert_eq!(
                refusal_with(path, false, true, &never),
                Some(Refusal::Network),
                "{path}"
            );
            assert_eq!(refusal_with(path, true, true, &never), None, "{path}");
        }
    }

    #[test]
    fn mapped_network_drives_count_as_network_paths() {
        assert_eq!(
            refusal_with(r"Z:\docs", false, true, &z_is_remote),
            Some(Refusal::Network)
        );
        assert_eq!(
            refusal_with(r"\\?\Z:\docs", false, true, &z_is_remote),
            Some(Refusal::Network)
        );
        assert_eq!(refusal_with(r"Z:\docs", true, true, &z_is_remote), None);
        assert_eq!(refusal_with(r"C:\docs", false, true, &z_is_remote), None);
    }

    #[test]
    fn device_paths_are_refused_whatever_the_setting() {
        for path in [r"\\.\pipe\x", r"\\?\GLOBALROOT\Device\X", r"\\.\COM1"] {
            for allow in [false, true] {
                assert_eq!(
                    refusal_with(path, allow, true, &never),
                    Some(Refusal::Device),
                    "{path}"
                );
            }
        }
        // A drive in long form is just a local path.
        assert_eq!(refusal_with(r"\\?\C:\Users", false, true, &never), None);
    }

    #[test]
    fn nothing_is_refused_off_windows() {
        for path in [r"\\server\share", "//server/share", r"\\.\pipe\x", "/etc"] {
            assert_eq!(refusal_with(path, false, false, &|_| true), None, "{path}");
        }
    }

    #[test]
    fn local_paths_pass() {
        for path in [r"C:\Users\me", "~/x", "/home/me", "relative", ""] {
            assert_eq!(refusal_with(path, false, true, &never), None, "{path}");
        }
    }

    #[test]
    fn drive_letters_are_read_from_the_text() {
        assert_eq!(drive_letter(r"c:\x"), Some('C'));
        assert_eq!(drive_letter("D:"), Some('D'));
        assert_eq!(drive_letter("e:/x"), Some('E'));
        assert_eq!(drive_letter(r"\\?\F:\x"), Some('F'));
        assert_eq!(drive_letter(r"\\server\share"), None);
        assert_eq!(drive_letter("/x"), None);
        assert_eq!(drive_letter("1:\\x"), None);
        assert_eq!(drive_letter(""), None);
    }

    #[test]
    fn the_messages_say_what_to_change() {
        assert!(Refusal::Network.message().contains("Settings"));
        assert!(Refusal::Network
            .message()
            .contains("Network paths are turned off"));
        assert!(!Refusal::Device.message().is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn the_system_drive_is_not_remote() {
        assert!(!is_network_drive(r"C:\"));
        assert!(!is_network_drive("not a drive"));
    }

    /// The process-wide setting is off until something turns it on. (This test
    /// never turns it on: the flag is shared by every test in the process.)
    #[test]
    fn the_process_wide_setting_starts_off() {
        assert!(!allow_network_paths());
        assert!(guard(Path::new("/tmp")).is_ok());
    }
}
