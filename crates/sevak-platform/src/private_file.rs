//! Writing files that hold the user's private data (the clipboard history).

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;

/// Reads a whole file, but never one larger than `cap` bytes: the size is
/// checked first, and the read itself stops one byte past the cap, so a file
/// that grows while it is read cannot get past it either. A file over the cap
/// is an [`io::ErrorKind::FileTooLarge`] error and is not read.
pub fn read_capped(path: &Path, cap: u64) -> io::Result<Vec<u8>> {
    let file = File::open(path)?;
    let too_large = || {
        io::Error::new(
            io::ErrorKind::FileTooLarge,
            format!("the file is larger than {cap} bytes"),
        )
    };
    let len = file.metadata()?.len();
    if len > cap {
        return Err(too_large());
    }
    let mut bytes = Vec::with_capacity(usize::try_from(len).unwrap_or(0));
    file.take(cap.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > cap {
        return Err(too_large());
    }
    Ok(bytes)
}

/// Replaces `path` with `contents` atomically (temporary file + rename), so a
/// crash never leaves half a file. Parent directories are created. On Unix the
/// file is readable by its owner only; on Windows it inherits the per-user
/// access of the profile folder it lives in.
pub fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
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
