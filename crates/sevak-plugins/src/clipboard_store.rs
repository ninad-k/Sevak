//! The image files behind the clipboard history.
//!
//! An image the history keeps lives in `<data dir>/clipboard/` as two PNG files
//! named after the hash of its pixels: `<hash>.png` (the picture) and
//! `<hash>.thumb.png` (a small copy for the result row). The history file only
//! stores the hash, so a path is never read back from it.
//!
//! The folder is private to the user (0700, files 0600 on Unix; on Windows it
//! inherits the per-user access of the profile folder). Where the system can
//! encrypt files for the user (Windows) the files are encrypted too: the
//! callers pass the [`Sealer`], and readers that only have a path (the preview
//! pane, the icon loader, pasting) open them through
//! [`sevak_core::sealed::global`]. Nothing outside the two name patterns is ever
//! deleted from it.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::PathBuf;

use sevak_core::sealed::{self, Sealer};
use sevak_platform::private_file::{
    is_sealed_file, read_capped, write_atomic, write_atomic_sealed,
};

/// The folder inside Sevak's data folder.
pub const DIR_NAME: &str = "clipboard";

/// The largest image file Sevak reads or moves: what `max_image_bytes` allows
/// at most (64 MiB), with room for the thumbnail's format overhead.
pub const MAX_IMAGE_FILE_BYTES: u64 = 65 * 1024 * 1024;

const FULL_SUFFIX: &str = ".png";
const THUMB_SUFFIX: &str = ".thumb.png";

/// What [`MediaStore::adopt_from`] did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Adopted {
    /// Files now in this store (and gone from the old one).
    pub moved: usize,
    /// Files that could not be moved this time (disk trouble); the old copy is
    /// left, so the next start tries again.
    pub retry: usize,
    /// Files that were not moved and were deleted instead (larger than any
    /// image Sevak records).
    pub dropped: usize,
}

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
    /// picture on disk always has its thumbnail), encrypted when there is a
    /// `sealer`.
    pub fn write(
        &self,
        hash: u64,
        png: &[u8],
        thumb_png: &[u8],
        sealer: Option<&dyn Sealer>,
    ) -> io::Result<()> {
        self.ensure_dir()?;
        write_atomic_sealed(&self.thumb_path(hash), thumb_png, sealer)?;
        write_atomic_sealed(&self.png_path(hash), png, sealer)
    }

    /// Encrypts every image file that is still plain (from before encryption
    /// was on). Returns how many files were rewritten; one that cannot be is
    /// left as it is.
    pub fn seal_existing(&self, sealer: &dyn Sealer) -> usize {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return 0;
        };
        let mut sealed_now = 0;
        for entry in entries.flatten() {
            let name = entry.file_name();
            if !name.to_str().is_some_and(|name| image_hash(name).is_some()) {
                continue;
            }
            let path = entry.path();
            if is_sealed_file(&path) {
                continue;
            }
            let done = read_capped(&path, MAX_IMAGE_FILE_BYTES)
                .and_then(|bytes| write_atomic(&path, &sealed::seal(sealer, &bytes)?));
            match done {
                Ok(()) => sealed_now += 1,
                Err(err) => tracing::warn!("could not encrypt a clipboard image: {}", err.kind()),
            }
        }
        sealed_now
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

    /// Moves Sevak's image files out of `old` (another folder, where an older
    /// version kept them) into this store, one at a time: a file is deleted
    /// from `old` only after its copy is written here. Files not named like
    /// Sevak's are left alone, and the old folder is removed if that empties it.
    pub fn adopt_from(&self, old: &MediaStore, sealer: Option<&dyn Sealer>) -> Adopted {
        let mut report = Adopted::default();
        let Ok(entries) = fs::read_dir(&old.dir) else {
            return report;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let from = entry.path();
            if is_leftover_temp(name) {
                let _ = fs::remove_file(&from);
                continue;
            }
            if image_hash(name).is_none() {
                continue;
            }
            let moved = read_capped(&from, MAX_IMAGE_FILE_BYTES).and_then(|bytes| {
                self.ensure_dir()?;
                // Files that are already sealed (by a sealer of this user) are
                // kept as they are; plain ones are sealed on the way.
                match sealer {
                    Some(sealer) if !sealed::is_sealed(&bytes) => {
                        write_atomic(&self.dir.join(name), &sealed::seal(sealer, &bytes)?)
                    }
                    _ => write_atomic(&self.dir.join(name), &bytes),
                }
            });
            match moved {
                Ok(()) => {
                    if fs::remove_file(&from).is_ok() {
                        report.moved += 1;
                    } else {
                        report.retry += 1;
                    }
                }
                Err(err) if err.kind() == io::ErrorKind::FileTooLarge => {
                    report.dropped += 1;
                    let _ = fs::remove_file(&from);
                }
                Err(err) => {
                    tracing::warn!("could not move a clipboard image: {}", err.kind());
                    report.retry += 1;
                }
            }
        }
        // Succeeds only when nothing is left in it.
        let _ = fs::remove_dir(&old.dir);
        report
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
        store.write(7, b"full", b"thumb", None).unwrap();
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
        store.write(1, b"a", b"a", None).unwrap();
        store.write(2, b"b", b"b", None).unwrap();
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

    #[test]
    fn images_move_between_folders_and_the_old_ones_go() {
        let (_dir, new) = store();
        let old_dir = tempfile::tempdir().unwrap();
        let old = MediaStore::new(old_dir.path().join(DIR_NAME));
        old.write(1, b"full", b"thumb", None).unwrap();
        fs::write(old.dir().join("holiday.png"), b"mine").unwrap();
        fs::write(old.dir().join("0000000000000009.png.tmp"), b"half").unwrap();

        let report = new.adopt_from(&old, None);
        assert_eq!(report.moved, 2);
        assert_eq!((report.retry, report.dropped), (0, 0));
        assert_eq!(fs::read(new.png_path(1)).unwrap(), b"full");
        assert_eq!(fs::read(new.thumb_path(1)).unwrap(), b"thumb");
        // Only the user's own file is left behind, and so is its folder.
        assert_eq!(names(&old), ["holiday.png"]);
        // Nothing to move from a folder that does not exist.
        let none = MediaStore::new(old_dir.path().join("nowhere"));
        assert_eq!(new.adopt_from(&none, None), Adopted::default());
    }

    #[test]
    fn an_image_that_cannot_be_written_stays_in_the_old_folder() {
        let (dir, _) = store();
        // The new "folder" is a file, so nothing can be written into it.
        let blocked = dir.path().join("blocked");
        fs::write(&blocked, b"file").unwrap();
        let new = MediaStore::new(blocked);
        let old_dir = tempfile::tempdir().unwrap();
        let old = MediaStore::new(old_dir.path().join(DIR_NAME));
        old.write(5, b"full", b"thumb", None).unwrap();

        let report = new.adopt_from(&old, None);
        assert_eq!(report.moved, 0);
        assert_eq!(report.retry, 2);
        assert!(old.contains(5), "the old copy is kept for the next try");
    }

    #[test]
    fn sealed_images_are_unreadable_on_disk_and_open_with_the_key() {
        use sevak_core::sealed::fake::XorSealer;
        let (_dir, store) = store();
        let sealer = XorSealer(3);
        store
            .write(4, b"PNGDATA-secret", b"THUMB-secret", Some(&sealer))
            .unwrap();
        for path in [store.png_path(4), store.thumb_path(4)] {
            let on_disk = fs::read(&path).unwrap();
            assert!(!on_disk.windows(6).any(|w| w == b"secret"));
            let opened = sealed::open(Some(&sealer), on_disk).unwrap();
            assert!(opened.was_sealed);
            assert!(opened.bytes.ends_with(b"secret"));
        }
    }

    #[test]
    fn plain_images_are_sealed_once_and_sealed_ones_are_left() {
        use sevak_core::sealed::fake::XorSealer;
        let (_dir, store) = store();
        let sealer = XorSealer(3);
        store.write(1, b"plain-full", b"plain-thumb", None).unwrap();
        store
            .write(2, b"sealed-full", b"sealed-thumb", Some(&sealer))
            .unwrap();
        let before = fs::read(store.png_path(2)).unwrap();
        fs::write(store.dir().join("holiday.png"), b"mine").unwrap();

        assert_eq!(store.seal_existing(&sealer), 2);
        assert_eq!(store.seal_existing(&sealer), 0, "nothing left to do");
        assert!(is_sealed_file(&store.png_path(1)));
        assert!(is_sealed_file(&store.thumb_path(1)));
        assert_eq!(fs::read(store.png_path(2)).unwrap(), before);
        assert_eq!(fs::read(store.dir().join("holiday.png")).unwrap(), b"mine");
    }

    #[test]
    fn moved_images_are_sealed_on_the_way() {
        use sevak_core::sealed::fake::XorSealer;
        let (_dir, new) = store();
        let old_dir = tempfile::tempdir().unwrap();
        let old = MediaStore::new(old_dir.path().join(DIR_NAME));
        old.write(1, b"full-secret", b"thumb-secret", None).unwrap();
        let sealer = XorSealer(8);

        assert_eq!(new.adopt_from(&old, Some(&sealer)).moved, 2);
        assert!(is_sealed_file(&new.png_path(1)));
        assert!(is_sealed_file(&new.thumb_path(1)));
    }

    #[cfg(unix)]
    #[test]
    fn the_folder_and_files_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir, store) = store();
        store.write(9, b"x", b"y", None).unwrap();
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(store.dir()), 0o700);
        assert_eq!(mode(&store.png_path(9)), 0o600);
        assert_eq!(mode(&store.thumb_path(9)), 0o600);
    }
}
