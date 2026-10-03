//! The image files behind the clipboard history.
//!
//! An image the history keeps lives in `<data dir>/clipboard/` as two PNG files
//! named after the hash of its pixels: `<hash>.png` (the picture) and
//! `<hash>.thumb.png` (a small copy for the result row). The history file only
//! stores the hash, so a path is never read back from it.
//!
//! The folder is private to the user (0700, files 0600 on Unix; on Windows it
//! inherits the per-user access of the profile folder). Nothing outside the two
//! name patterns is ever deleted from it.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::PathBuf;

use sevak_platform::private_file::write_atomic;

/// The folder inside Sevak's data folder.
pub const DIR_NAME: &str = "clipboard";

const FULL_SUFFIX: &str = ".png";
const THUMB_SUFFIX: &str = ".thumb.png";

/// The files of the images in one history.
#[derive(Debug, Clone)]
pub struct MediaStore {
    dir: PathBuf,
}

impl MediaStore {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn png_path(&self, hash: u64) -> PathBuf {
        self.dir.join(format!("{hash:016x}{FULL_SUFFIX}"))
    }

    pub fn thumb_path(&self, hash: u64) -> PathBuf {
        self.dir.join(format!("{hash:016x}{THUMB_SUFFIX}"))
    }

    /// True if the picture is on disk.
    pub fn contains(&self, hash: u64) -> bool {
        self.png_path(hash).is_file()
    }

    /// Writes an image's files (atomically each; the picture last, so a
    /// picture on disk always has its thumbnail).
    pub fn write(&self, hash: u64, png: &[u8], thumb_png: &[u8]) -> io::Result<()> {
        self.ensure_dir()?;
        write_atomic(&self.thumb_path(hash), thumb_png)?;
        write_atomic(&self.png_path(hash), png)
    }

    /// Deletes an image's files. A file that is already gone is fine.
    pub fn remove(&self, hash: u64) {
        for path in [self.png_path(hash), self.thumb_path(hash)] {
            if let Err(err) = fs::remove_file(&path) {
                if err.kind() != io::ErrorKind::NotFound {
                    tracing::warn!("could not delete {}: {err}", path.display());
                }
            }
        }
    }

    /// Deletes every image file whose hash is not in `keep`, and leftover
    /// temporary files of interrupted writes. Files that are not named like
    /// Sevak's are left alone. Returns how many files were deleted.
    pub fn prune(&self, keep: &HashSet<u64>) -> usize {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return 0;
        };
        let mut deleted = 0;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let orphan = match (image_hash(name), is_leftover_temp(name)) {
                (Some(hash), _) => !keep.contains(&hash),
                (None, leftover) => leftover,
            };
            if orphan && fs::remove_file(entry.path()).is_ok() {
                deleted += 1;
            }
        }
        deleted
    }

    fn ensure_dir(&self) -> io::Result<()> {
        if self.dir.is_dir() {
            return Ok(());
        }
        fs::create_dir_all(&self.dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn dir(&self) -> &std::path::Path {
        &self.dir
    }
}

/// The hash in a file name of the form `<16 hex digits>.png` or
/// `<16 hex digits>.thumb.png`.
fn image_hash(name: &str) -> Option<u64> {
    let digits = name
        .strip_suffix(THUMB_SUFFIX)
        .or_else(|| name.strip_suffix(FULL_SUFFIX))?;
    (digits.len() == 16 && digits.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| u64::from_str_radix(digits, 16).ok())
        .flatten()
}

/// `<name>.png.tmp` / `<name>.thumb.png.tmp`: what `write_atomic` leaves behind
/// if the process dies between writing and renaming.
fn is_leftover_temp(name: &str) -> bool {
    name.strip_suffix(".tmp")
        .is_some_and(|rest| image_hash(rest).is_some())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn store() -> (tempfile::TempDir, MediaStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = MediaStore::new(dir.path().join(DIR_NAME));
        (dir, store)
    }

    fn names(store: &MediaStore) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(store.dir())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn files_are_named_after_the_hash() {
        let store = MediaStore::new(PathBuf::from("/data/clipboard"));
        assert_eq!(
            store.png_path(0xAB),
            Path::new("/data/clipboard/00000000000000ab.png")
        );
        assert_eq!(
            store.thumb_path(0xAB),
            Path::new("/data/clipboard/00000000000000ab.thumb.png")
        );
    }

    #[test]
    fn only_sevaks_own_names_are_recognised() {
        assert_eq!(image_hash("00000000000000ab.png"), Some(0xab));
        assert_eq!(image_hash("00000000000000AB.thumb.png"), Some(0xab));
        assert_eq!(image_hash("ab.png"), None);
        assert_eq!(image_hash("00000000000000zz.png"), None);
        assert_eq!(image_hash("holiday.png"), None);
        assert_eq!(image_hash("00000000000000ab.jpg"), None);
        assert!(is_leftover_temp("00000000000000ab.png.tmp"));
        assert!(!is_leftover_temp("notes.tmp"));
    }

    #[test]
    fn written_images_can_be_found_and_removed() {
        let (_dir, store) = store();
        assert!(!store.contains(7));
        store.write(7, b"full", b"thumb").unwrap();
        assert!(store.contains(7));
        assert_eq!(fs::read(store.png_path(7)).unwrap(), b"full");
        assert_eq!(fs::read(store.thumb_path(7)).unwrap(), b"thumb");
        store.remove(7);
        assert!(!store.contains(7));
        assert!(names(&store).is_empty());
        // Removing what is not there is not an error.
        store.remove(7);
    }

    #[test]
    fn pruning_deletes_orphans_and_nothing_else() {
        let (_dir, store) = store();
        store.write(1, b"a", b"a").unwrap();
        store.write(2, b"b", b"b").unwrap();
        // A user's own file, a stray temp file of ours, and a lone thumbnail.
        fs::write(store.dir().join("holiday.png"), b"mine").unwrap();
        fs::write(store.dir().join("0000000000000003.png.tmp"), b"half").unwrap();
        fs::write(store.thumb_path(4), b"orphan").unwrap();

        let deleted = store.prune(&HashSet::from([1]));
        assert_eq!(deleted, 4); // image 2 (two files), the temp file, thumbnail 4
        assert_eq!(
            names(&store),
            [
                "0000000000000001.png",
                "0000000000000001.thumb.png",
                "holiday.png"
            ]
        );
        // A folder that does not exist yet is nothing to prune.
        assert_eq!(
            MediaStore::new(PathBuf::from("/definitely/not/here")).prune(&HashSet::new()),
            0
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_folder_and_files_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir, store) = store();
        store.write(9, b"x", b"y").unwrap();
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(store.dir()), 0o700);
        assert_eq!(mode(&store.png_path(9)), 0o600);
        assert_eq!(mode(&store.thumb_path(9)), 0o600);
    }
}
