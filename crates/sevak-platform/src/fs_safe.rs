//! File moves that never replace what is already there.
//!
//! [`std::fs::rename`] silently replaces an existing file at the destination
//! (on Unix with `rename(2)`, on Windows with `MOVEFILE_REPLACE_EXISTING`).
//! A caller that has just checked that a name is free still races with every
//! other process: whatever appears at that name in between is destroyed.
//! [`rename_no_replace`] asks the operating system to refuse instead, in one
//! step: `renameat2(RENAME_NOREPLACE)` on Linux, `renamex_np(RENAME_EXCL)` on
//! macOS and `MoveFileExW` without the replace flag on Windows.

use std::io;
use std::path::Path;

/// Moves `from` to `to`, which must not exist. If something is there (a file,
/// a folder, a link, even a dangling one) the error is
/// [`io::ErrorKind::AlreadyExists`] and nothing is changed. Both paths must be
/// on one file system, as with `rename`: otherwise the error is
/// [`io::ErrorKind::CrossesDevices`].
///
/// Where the operating system or the file system has no such call (an old
/// kernel, a network file system) the name is checked first and then `rename`
/// is used: the window is small but not closed there.
pub fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    match imp::rename_no_replace(from, to) {
        Err(err) if imp::is_unsupported(&err) => {
            if to.symlink_metadata().is_ok() {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("{} already exists", to.display()),
                ));
            }
            std::fs::rename(from, to)
        }
        other => other,
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::ffi::CString;
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    fn c_path(path: &Path) -> io::Result<CString> {
        CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a path contains a NUL"))
    }

    pub(super) fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
        let (from, to) = (c_path(from)?, c_path(to)?);
        // SAFETY: both strings are NUL-terminated and outlive the call; the
        // syscall only reads them.
        let result = unsafe {
            libc::syscall(
                libc::SYS_renameat2,
                libc::AT_FDCWD as libc::c_long,
                from.as_ptr(),
                libc::AT_FDCWD as libc::c_long,
                to.as_ptr(),
                libc::RENAME_NOREPLACE as libc::c_long,
            )
        };
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// The kernel or the file system does not know `RENAME_NOREPLACE`.
    pub(super) fn is_unsupported(err: &io::Error) -> bool {
        matches!(
            err.raw_os_error(),
            Some(libc::ENOSYS | libc::EINVAL | libc::ENOTSUP)
        )
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::CString;
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    fn c_path(path: &Path) -> io::Result<CString> {
        CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a path contains a NUL"))
    }

    pub(super) fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
        let (from, to) = (c_path(from)?, c_path(to)?);
        // SAFETY: both strings are NUL-terminated and outlive the call.
        let result = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub(super) fn is_unsupported(err: &io::Error) -> bool {
        matches!(err.raw_os_error(), Some(libc::ENOTSUP | libc::EINVAL))
    }
}

#[cfg(windows)]
mod imp {
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{MoveFileExW, MOVE_FILE_FLAGS};

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    pub(super) fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
        const ERROR_NOT_SAME_DEVICE: i32 = 17;
        let (from, to) = (wide(from), wide(to));
        // SAFETY: both buffers are NUL-terminated and outlive the call. No
        // flags: no MOVEFILE_REPLACE_EXISTING (an existing target is an error)
        // and no MOVEFILE_COPY_ALLOWED (a move across volumes is an error).
        let moved = unsafe {
            MoveFileExW(
                PCWSTR(from.as_ptr()),
                PCWSTR(to.as_ptr()),
                MOVE_FILE_FLAGS(0),
            )
        };
        match moved {
            Ok(()) => Ok(()),
            Err(err) => {
                let os = io::Error::from_raw_os_error(win32_code(&err));
                if os.raw_os_error() == Some(ERROR_NOT_SAME_DEVICE) {
                    Err(io::Error::from(io::ErrorKind::CrossesDevices))
                } else {
                    Err(os)
                }
            }
        }
    }

    /// The Win32 error code inside a `windows` crate error (an HRESULT such as
    /// `0x80070050` carries it in the low 16 bits).
    fn win32_code(err: &windows::core::Error) -> i32 {
        err.code().0 & 0xFFFF
    }

    /// Windows always has the call.
    pub(super) fn is_unsupported(_err: &io::Error) -> bool {
        false
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod imp {
    use std::io;
    use std::path::Path;

    pub(super) fn rename_no_replace(_from: &Path, _to: &Path) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    pub(super) fn is_unsupported(_err: &io::Error) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn a_free_name_is_taken() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("a.txt");
        let to = dir.path().join("b.txt");
        fs::write(&from, "payload").unwrap();
        rename_no_replace(&from, &to).unwrap();
        assert!(!from.exists());
        assert_eq!(fs::read_to_string(&to).unwrap(), "payload");
    }

    #[test]
    fn a_file_is_never_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("a.txt");
        let to = dir.path().join("b.txt");
        fs::write(&from, "mine").unwrap();
        fs::write(&to, "theirs").unwrap();
        let err = rename_no_replace(&from, &to).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists, "{err}");
        assert_eq!(fs::read_to_string(&from).unwrap(), "mine");
        assert_eq!(fs::read_to_string(&to).unwrap(), "theirs");
    }

    #[test]
    fn a_folder_is_never_replaced_not_even_an_empty_one() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("moving");
        let to = dir.path().join("target");
        fs::create_dir(&from).unwrap();
        fs::write(from.join("inside.txt"), "x").unwrap();
        // `rename(2)` would replace an empty folder.
        fs::create_dir(&to).unwrap();
        let err = rename_no_replace(&from, &to).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists, "{err}");
        assert!(from.join("inside.txt").is_file());
        assert!(to.is_dir() && fs::read_dir(&to).unwrap().next().is_none());
        // A folder moves onto a free name.
        let free = dir.path().join("free");
        rename_no_replace(&from, &free).unwrap();
        assert!(free.join("inside.txt").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_the_way_is_not_followed_or_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let victim = dir.path().join("victim.txt");
        fs::write(&victim, "precious").unwrap();
        let from = dir.path().join("a.txt");
        fs::write(&from, "mine").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&victim, &link).unwrap();
        assert_eq!(
            rename_no_replace(&from, &link).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        // Dangling links occupy their name too.
        let dangling = dir.path().join("dangling");
        std::os::unix::fs::symlink(dir.path().join("nowhere"), &dangling).unwrap();
        assert_eq!(
            rename_no_replace(&from, &dangling).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read_to_string(&victim).unwrap(), "precious");
        assert_eq!(fs::read_to_string(&from).unwrap(), "mine");
    }

    #[test]
    fn a_missing_source_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let err = rename_no_replace(&dir.path().join("none"), &dir.path().join("x")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound, "{err}");
    }
}
