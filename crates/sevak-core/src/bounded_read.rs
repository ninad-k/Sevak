//! Reading local files without trusting their size.
//!
//! Sevak reads files that other programs (or other people's packages) can
//! write: configuration, plugin and workflow manifests, state files, caches.
//! A file far bigger than any real one would otherwise be read whole into
//! memory. [`read_capped`] checks the size first and also stops reading one
//! byte past the cap, so a file that grows while it is read cannot get past
//! it either. A file over the cap is a [`std::io::ErrorKind::FileTooLarge`]
//! error whose message names the limit.
//!
//! The limits below are per kind of file; each is far above what Sevak itself
//! writes.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

/// `config.toml`.
pub const MAX_CONFIG_BYTES: u64 = 4 * 1024 * 1024;
/// `usage.json`, the launch counts and last queries.
pub const MAX_USAGE_BYTES: u64 = 8 * 1024 * 1024;
/// Small JSON state files (script approvals, the shortcut takeover record).
pub const MAX_STATE_BYTES: u64 = 1024 * 1024;
/// A script plugin's `plugin.toml`.
pub const MAX_PLUGIN_MANIFEST_BYTES: u64 = 256 * 1024;
/// A workflow's `workflow.toml`.
pub const MAX_WORKFLOW_BYTES: u64 = 2 * 1024 * 1024;
/// Cached data fetched earlier (exchange rates).
pub const MAX_CACHE_BYTES: u64 = 256 * 1024;
/// A browser's bookmarks file (JSON or property list).
pub const MAX_BOOKMARKS_BYTES: u64 = 64 * 1024 * 1024;
/// A `.desktop`, `index.theme` or similar small description file.
pub const MAX_DESCRIPTION_BYTES: u64 = 1024 * 1024;
/// An icon image.
pub const MAX_ICON_BYTES: u64 = 8 * 1024 * 1024;

fn too_large(cap: u64) -> io::Error {
    io::Error::new(
        io::ErrorKind::FileTooLarge,
        format!("the file is larger than the limit of {}", describe(cap)),
    )
}

/// `4 MiB`, `256 KiB` or `12 bytes`.
fn describe(bytes: u64) -> String {
    if bytes >= 1024 * 1024 && bytes.is_multiple_of(1024 * 1024) {
        format!("{} MiB", bytes / (1024 * 1024))
    } else if bytes >= 1024 && bytes.is_multiple_of(1024) {
        format!("{} KiB", bytes / 1024)
    } else {
        format!("{bytes} bytes")
    }
}

/// The contents of `path`, if it is at most `cap` bytes.
pub fn read_capped(path: &Path, cap: u64) -> io::Result<Vec<u8>> {
    let file = File::open(path)?;
    let len = file.metadata()?.len();
    if len > cap {
        return Err(too_large(cap));
    }
    let mut bytes = Vec::with_capacity(usize::try_from(len).unwrap_or(0));
    file.take(cap.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > cap {
        return Err(too_large(cap));
    }
    Ok(bytes)
}

/// [`read_capped`] as text, with the error `fs::read_to_string` gives for
/// bytes that are not UTF-8.
pub fn read_to_string_capped(path: &Path, cap: u64) -> io::Result<String> {
    String::from_utf8(read_capped(path, cap)?).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "stream did not contain valid UTF-8",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_within_the_cap_is_read_and_one_over_it_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.txt");
        std::fs::write(&path, b"12345").unwrap();
        assert_eq!(read_capped(&path, 5).unwrap(), b"12345");
        assert_eq!(read_to_string_capped(&path, 6).unwrap(), "12345");
        let err = read_capped(&path, 4).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::FileTooLarge);
        assert!(err.to_string().contains("limit of 4 bytes"), "{err}");
        assert_eq!(
            read_to_string_capped(&path, 4).unwrap_err().kind(),
            io::ErrorKind::FileTooLarge
        );
    }

    #[test]
    fn missing_files_and_bad_text_keep_their_usual_errors() {
        let dir = tempfile::tempdir().unwrap();
        let missing = read_capped(&dir.path().join("none"), 10).unwrap_err();
        assert_eq!(missing.kind(), io::ErrorKind::NotFound);
        let path = dir.path().join("binary");
        std::fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
        assert_eq!(
            read_to_string_capped(&path, 10).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(read_capped(&path, 10).unwrap(), [0xff, 0xfe, 0x00]);
    }

    #[test]
    fn an_empty_file_and_a_zero_cap() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty");
        std::fs::write(&path, b"").unwrap();
        assert_eq!(read_capped(&path, 0).unwrap(), b"");
        std::fs::write(&path, b"x").unwrap();
        assert!(read_capped(&path, 0).is_err());
    }

    #[test]
    fn limits_are_described_in_words() {
        assert_eq!(describe(4 * 1024 * 1024), "4 MiB");
        assert_eq!(describe(256 * 1024), "256 KiB");
        assert_eq!(describe(100), "100 bytes");
        assert_eq!(describe(1500), "1500 bytes");
    }
}
