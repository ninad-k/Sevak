//! Writing files that hold the user's private data (the clipboard history).

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;

use sevak_core::sealed::{self, OpenError, Opened, Sealer};

/// Reads a whole file, but never one larger than `cap` bytes. This is the one
/// implementation, [`sevak_core::bounded_read::read_capped`]: the size is
/// checked first and the read stops one byte past the cap, so a file that
/// grows while it is read cannot get past it either.
pub use sevak_core::bounded_read::read_capped;

/// [`write_atomic`] of `contents` encrypted by `sealer` (see
/// [`sevak_core::sealed`]); plain when there is no sealer. The sealed bytes are
/// what is written: the plain contents never reach the disk.
pub fn write_atomic_sealed(
    path: &Path,
    contents: &[u8],
    sealer: Option<&dyn Sealer>,
) -> io::Result<()> {
    match sealer {
        Some(sealer) => write_atomic(path, &sealed::seal(sealer, contents)?),
        None => write_atomic(path, contents),
    }
}

/// What reading a file that may be sealed came to.
#[derive(Debug)]
pub enum SealedRead {
    /// The contents, decrypted if the file was sealed.
    Contents(Opened),
    /// The file is sealed and cannot be opened by this user on this system.
    Unopenable(OpenError),
}

/// [`read_capped`] and then opens the file if it is sealed.
pub fn read_sealed(path: &Path, cap: u64, sealer: Option<&dyn Sealer>) -> io::Result<SealedRead> {
    let bytes = read_capped(path, cap)?;
    Ok(match sealed::open(sealer, bytes) {
        Ok(opened) => SealedRead::Contents(opened),
        Err(err) => SealedRead::Unopenable(err),
    })
}

/// True if the file starts like a sealed one (only its first bytes are read).
pub fn is_sealed_file(path: &Path) -> bool {
    let mut head = vec![0u8; sealed::MAGIC.len()];
    File::open(path)
        .and_then(|mut file| file.read_exact(&mut head))
        .is_ok()
        && sealed::is_sealed(&head)
}

/// Replaces `path` with `contents` atomically (temporary file + rename), so a
/// crash never leaves half a file. Parent directories are created. On Unix the
/// file is readable by its owner only (0600) and folders created for it are
/// too (0700); on Windows it inherits the per-user access of the profile
/// folder it lives in.
pub fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        create_private_dir_all(parent)?;
    }
    let mut temp_name = path.file_name().unwrap_or_default().to_owned();
    temp_name.push(".tmp");
    let temp = path.with_file_name(temp_name);

    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let written = options
        .open(&temp)
        .and_then(|mut file| file.write_all(contents).and_then(|()| file.sync_all()))
        .and_then(|()| fs::rename(&temp, path));
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written
}

/// Creates `dir` and any missing parents. On Unix the folders it creates are
/// readable by their owner only (0700); folders that already exist are left as
/// they are. Elsewhere this is `create_dir_all`.
pub fn create_private_dir_all(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(dir)
    }
}

/// Makes an existing folder readable by its owner only (0700) on Unix. A
/// no-op elsewhere, where the folder keeps the access of the profile it is in.
pub fn restrict_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_the_file_and_leaves_no_temporary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("history.json");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"two");
        assert!(!dir.path().join("nested").join("history.json.tmp").exists());
    }

    #[test]
    fn sealed_files_hold_no_plain_text_and_read_back() {
        use sevak_core::sealed::fake::XorSealer;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.json");
        let sealer = XorSealer(9);
        write_atomic_sealed(&path, b"password hunter2", Some(&sealer)).unwrap();

        let on_disk = fs::read(&path).unwrap();
        assert!(!on_disk.windows(6).any(|w| w == b"hunter"));
        assert!(is_sealed_file(&path));
        assert!(matches!(
            read_sealed(&path, 1024, Some(&sealer)).unwrap(),
            SealedRead::Contents(opened) if opened.was_sealed && opened.bytes == b"password hunter2"
        ));
        // Another user's key, and a system without one, get nothing.
        assert!(matches!(
            read_sealed(&path, 1024, Some(&XorSealer(1))).unwrap(),
            SealedRead::Unopenable(OpenError::Undecryptable)
        ));
        assert!(matches!(
            read_sealed(&path, 1024, None).unwrap(),
            SealedRead::Unopenable(OpenError::NoSealer)
        ));

        // Without a sealer the file is plain, and says so.
        write_atomic_sealed(&path, b"plain", None).unwrap();
        assert!(!is_sealed_file(&path));
        assert!(matches!(
            read_sealed(&path, 1024, Some(&sealer)).unwrap(),
            SealedRead::Contents(opened) if !opened.was_sealed && opened.bytes == b"plain"
        ));
    }

    #[test]
    fn a_file_over_the_cap_is_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.json");
        fs::write(&path, b"12345").unwrap();
        assert_eq!(read_capped(&path, 5).unwrap(), b"12345");
        let err = read_capped(&path, 4).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::FileTooLarge);
        let missing = read_capped(&dir.path().join("none"), 4).unwrap_err();
        assert_eq!(missing.kind(), io::ErrorKind::NotFound);
    }

    #[cfg(unix)]
    #[test]
    fn folders_created_for_a_file_are_private_and_existing_ones_are_left_alone() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755)).unwrap();
        let file = root.path().join("a").join("b").join("state.json");
        write_atomic(&file, b"x").unwrap();
        assert_eq!(mode(&root.path().join("a")), 0o700);
        assert_eq!(mode(&root.path().join("a").join("b")), 0o700);
        assert_eq!(mode(&file), 0o600);
        assert_eq!(
            mode(root.path()),
            0o755,
            "an existing folder is not touched"
        );
    }

    #[cfg(unix)]
    #[test]
    fn rewriting_a_file_that_had_wider_permissions_makes_it_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("usage.json");
        fs::write(&path, b"old").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        write_atomic(&path, b"new").unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(fs::read(&path).unwrap(), b"new");
    }

    #[cfg(unix)]
    #[test]
    fn restrict_dir_makes_a_folder_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
        restrict_dir(dir.path()).unwrap();
        assert_eq!(
            fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }

    #[test]
    fn private_folders_can_be_created_anywhere() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("x").join("y");
        create_private_dir_all(&nested).unwrap();
        assert!(nested.is_dir());
        // Again: no error for a folder that exists.
        create_private_dir_all(&nested).unwrap();
        restrict_dir(&nested).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn is_private_to_the_owner() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.json");
        write_atomic(&path, b"x").unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
