//! Moving a file or folder to the system trash (the Recycle Bin, the Finder
//! Trash, the freedesktop trash). Never deletes for good: where an item cannot
//! be trashed the call fails and the item stays where it is.

use std::path::Path;

use crate::error::{PlatformError, Result};

/// How long `gio trash` may take before it counts as started (a large folder).
#[cfg(not(any(windows, target_os = "macos")))]
const GIO_GRACE: std::time::Duration = std::time::Duration::from_secs(30);

/// Moves `path` (absolute, existing) to the trash.
pub fn move_to_trash(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return Err(PlatformError::Os {
            operation: "move to trash",
            message: format!("{} is not an absolute path", path.display()),
        });
    }
    // `symlink_metadata`: a link is trashed itself, and a dangling one exists.
    if std::fs::symlink_metadata(path).is_err() {
        return Err(PlatformError::Os {
            operation: "move to trash",
            message: format!("{} no longer exists", path.display()),
        });
    }
    imp(path)
}

#[cfg(windows)]
fn imp(path: &Path) -> Result<()> {
    crate::windows::trash::move_to_trash(path)
}

#[cfg(target_os = "macos")]
fn imp(path: &Path) -> Result<()> {
    crate::macos::trash::move_to_trash(path)
}

/// `gio trash` (GLib's, the same one file managers use), which follows the
/// freedesktop trash specification and refuses rather than deleting when a
/// volume has no trash.
#[cfg(not(any(windows, target_os = "macos")))]
fn imp(path: &Path) -> Result<()> {
    let line = gio_trash_args(path);
    crate::process::run_checked("gio", &line, GIO_GRACE)
}

/// Arguments for `gio`: `--` so a name starting with `-` is not an option.
#[cfg(not(any(windows, target_os = "macos")))]
fn gio_trash_args(path: &Path) -> Vec<std::ffi::OsString> {
    vec!["trash".into(), "--".into(), path.as_os_str().to_owned()]
}

#[cfg(all(test, not(any(windows, target_os = "macos"))))]
mod linux_tests {
    use super::*;

    #[test]
    fn gio_gets_the_path_after_a_separator() {
        let args = gio_trash_args(Path::new("/tmp/-rf"));
        assert_eq!(args, ["trash", "--", "/tmp/-rf"]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_and_missing_paths_are_refused_without_touching_anything() {
        assert!(move_to_trash(Path::new("relative.txt")).is_err());
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.txt");
        let err = move_to_trash(&missing).unwrap_err();
        assert!(err.to_string().contains("no longer exists"), "{err}");
    }

    /// Puts a real file into the real trash; run on purpose with `--ignored`.
    #[test]
    #[ignore = "uses the system trash"]
    fn a_file_in_a_temp_dir_goes_to_the_trash() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sevak-trash-test.txt");
        std::fs::write(&file, b"x").unwrap();
        move_to_trash(&file).unwrap();
        assert!(!file.exists());
    }
}
