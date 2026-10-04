//! The file operations behind the file buffer: copy, move, trash and zip.
//!
//! Nothing here overwrites or deletes for good. A name that is taken becomes
//! `name (2).ext`; a move across drives is a copy that removes the original only
//! after it succeeded; "trash" goes through the platform. Every batch runs item
//! by item and reports how many worked rather than stopping at the first failure.

use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, ErrorKind, Write};
use std::path::{Component, Path, PathBuf};

use sevak_platform::fs_safe::rename_no_replace;
use sevak_platform::PlatformProvider;
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

/// Names tried before giving up on finding a free `name (n)`.
const MAX_NAME_ATTEMPTS: u32 = 10_000;
/// Files from this size on need the zip64 extension.
const ZIP64_THRESHOLD: u64 = 0xFFFF_FFFE;

/// Reports progress as `(items finished, items in total, the one starting)`.
/// Called before each item and once more, with an empty name, at the end.
pub type Progress<'a> = &'a mut dyn FnMut(usize, usize, &str);

/// One item that could not be handled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub path: PathBuf,
    pub reason: String,
}

/// What a batch did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// Items the batch looked at (nested duplicates are not counted).
    pub total: usize,
    /// Items fully handled, as given: copied, moved or trashed.
    pub done: Vec<PathBuf>,
    pub failed: Vec<Failure>,
}

/// What [`zip_items`] made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZipReport {
    pub archive: PathBuf,
    /// Files written into the archive.
    pub files: usize,
    /// Files that could not be read, and links and special files, left out.
    pub skipped: usize,
}

/// `items` without repeats and without anything inside another item of the
/// list: moving a folder together with one of its own files would otherwise
/// handle the file twice. Order is kept.
pub fn prune_nested(items: &[PathBuf]) -> Vec<PathBuf> {
    let mut kept: Vec<PathBuf> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let repeated = items[..i].contains(item);
        let nested = items
            .iter()
            .any(|other| other != item && item.starts_with(other));
        if !repeated && !nested {
            kept.push(item.clone());
        }
    }
    kept
}

/// The folder every item is in: the deepest folder above all of them. `None`
/// when there is none (items on different drives) or the list is empty.
pub fn common_parent(items: &[PathBuf]) -> Option<PathBuf> {
    let mut common: Option<Vec<Component<'_>>> = None;
    for item in items {
        let parent: Vec<Component<'_>> = item.parent()?.components().collect();
        common = Some(match common {
            None => parent,
            Some(so_far) => so_far
                .iter()
                .zip(&parent)
                .take_while(|(a, b)| a == b)
                .map(|(a, _)| *a)
                .collect(),
        });
    }
    let common = common?;
    // A bare drive prefix (`C:`) is not a folder; there is no shared one.
    if common.iter().all(|c| matches!(c, Component::Prefix(_))) {
        return None;
    }
    Some(common.iter().collect())
}

/// `name`, or `stem (n).ext` for attempt `n >= 2`. Folders keep dots in their
/// names, `.tar.gz` stays together and a leading dot is not an extension.
fn numbered(name: &OsStr, is_dir: bool, n: u32) -> OsString {
    if n <= 1 {
        return name.to_owned();
    }
    let Some(text) = name.to_str() else {
        let mut taken = name.to_owned();
        taken.push(format!(" ({n})"));
        return taken;
    };
    let split = if is_dir {
        text.len()
    } else {
        match text.rfind('.') {
            Some(dot) if dot > 0 && dot + 1 < text.len() => {
                let stem = &text[..dot];
                match stem.rfind('.') {
                    Some(inner) if stem[inner..].eq_ignore_ascii_case(".tar") && inner > 0 => inner,
                    _ => dot,
                }
            }
            _ => text.len(),
        }
    };
    format!("{} ({n}){}", &text[..split], &text[split..]).into()
}

/// A path in `dir` that nothing uses yet, for an item called `name`.
pub fn unique_path(dir: &Path, name: &OsStr, is_dir: bool) -> io::Result<PathBuf> {
    for n in 1..=MAX_NAME_ATTEMPTS {
        let candidate = dir.join(numbered(name, is_dir, n));
        // `symlink_metadata`: a dangling link occupies its name too.
        if fs::symlink_metadata(&candidate).is_err() {
            return Ok(candidate);
        }
    }
    Err(no_free_name(name))
}

/// Creates a new, empty file in `dir` under a free name, so two copies running
/// at once cannot pick the same one.
fn claim_file(dir: &Path, name: &OsStr) -> io::Result<(File, PathBuf)> {
    for n in 1..=MAX_NAME_ATTEMPTS {
        let candidate = dir.join(numbered(name, false, n));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => return Ok((file, candidate)),
            Err(err) if err.kind() == ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err),
        }
    }
    Err(no_free_name(name))
}

/// Creates a new, empty folder in `dir` under a free name.
fn claim_dir(dir: &Path, name: &OsStr) -> io::Result<PathBuf> {
    for n in 1..=MAX_NAME_ATTEMPTS {
        let candidate = dir.join(numbered(name, true, n));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(err) if err.kind() == ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err),
        }
    }
    Err(no_free_name(name))
}

fn no_free_name(name: &OsStr) -> io::Error {
    io::Error::other(format!(
        "no free name for {} in the destination",
        name.to_string_lossy()
    ))
}

/// Whether `inner` is `outer` or below it, comparing real locations where they
/// can be resolved (so `..` and links cannot hide a folder inside itself).
fn is_inside(inner: &Path, outer: &Path) -> bool {
    let real = |path: &Path| fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    real(inner).starts_with(real(outer))
}

fn same_place(a: &Path, b: &Path) -> bool {
    let real = |path: &Path| fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    real(a) == real(b)
}

fn file_name_of(path: &Path) -> io::Result<&OsStr> {
    path.file_name().ok_or_else(|| {
        io::Error::new(
            ErrorKind::InvalidInput,
            format!("{} has no name (a drive or root folder)", path.display()),
        )
    })
}

fn copy_symlink(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(fs::read_link(from)?, to)
    }
    #[cfg(windows)]
    {
        // Windows links need privileges to create; a link to a file is copied
        // as the file, a link to a folder is not followed (it could loop).
        if fs::metadata(from)?.is_file() {
            // `create_new`: a name that has been taken meanwhile is an error,
            // never overwritten (`fs::copy` would).
            let mut out = OpenOptions::new().write(true).create_new(true).open(to)?;
            io::copy(&mut File::open(from)?, &mut out).map(drop)
        } else {
            Err(io::Error::new(
                ErrorKind::Unsupported,
                "links to folders are not copied",
            ))
        }
    }
}

/// Writes `from`'s content into the new file `out` and gives it the original's
/// permissions and modification time (both best effort).
fn copy_file_into(from: &Path, out: &mut File, meta: &fs::Metadata) -> io::Result<()> {
    io::copy(&mut File::open(from)?, out)?;
    out.flush()?;
    let _ = out.set_permissions(meta.permissions());
    if let Ok(modified) = meta.modified() {
        let _ = out.set_modified(modified);
    }
    Ok(())
}

fn copy_dir_contents(from: &Path, to: &Path) -> io::Result<()> {
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            copy_symlink(&source, &target)?;
        } else if kind.is_dir() {
            fs::create_dir(&target)?;
            copy_dir_contents(&source, &target)?;
        } else if kind.is_file() {
            let meta = entry.metadata()?;
            let mut out = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)?;
            copy_file_into(&source, &mut out, &meta)?;
        } else {
            return Err(not_regular(&source));
        }
    }
    Ok(())
}

fn not_regular(path: &Path) -> io::Error {
    io::Error::new(
        ErrorKind::InvalidInput,
        format!("{} is not a regular file or folder", path.display()),
    )
}

/// Copies `source` into the folder `dest_dir` under a free name; returns where.
/// A copy that fails halfway is removed again.
fn copy_item(source: &Path, dest_dir: &Path) -> io::Result<PathBuf> {
    let meta = fs::symlink_metadata(source)?;
    let name = file_name_of(source)?;
    let kind = meta.file_type();
    if kind.is_symlink() {
        let target = unique_path(dest_dir, name, false)?;
        copy_symlink(source, &target)?;
        Ok(target)
    } else if kind.is_dir() {
        if is_inside(dest_dir, source) {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "a folder cannot be copied into itself",
            ));
        }
        let target = claim_dir(dest_dir, name)?;
        match copy_dir_contents(source, &target) {
            Ok(()) => Ok(target),
            Err(err) => {
                // The folder is brand new and ours alone.
                let _ = fs::remove_dir_all(&target);
                Err(err)
            }
        }
    } else if kind.is_file() {
        let (mut out, target) = claim_file(dest_dir, name)?;
        match copy_file_into(source, &mut out, &meta) {
            Ok(()) => Ok(target),
            Err(err) => {
                drop(out);
                let _ = fs::remove_file(&target);
                Err(err)
            }
        }
    } else {
        Err(not_regular(source))
    }
}

/// Removes `path` (a file, a link or a folder with everything in it).
fn remove_item(path: &Path) -> io::Result<()> {
    if fs::symlink_metadata(path)?.is_dir() {
        fs::remove_dir_all(path)
    } else {
        // A folder link on Windows is removed like a folder, not like a file.
        fs::remove_file(path).or_else(|err| fs::remove_dir(path).map_err(|_| err))
    }
}

/// The folder a batch puts its items in, as it was when the batch started.
///
/// The folder the user typed is resolved once (links followed, as they meant)
/// and every item first checks that the path still leads to that same folder,
/// so a link swapped in meanwhile cannot redirect the rest of the batch.
struct Destination {
    path: PathBuf,
    real: PathBuf,
}

impl Destination {
    /// Fails unless `path` is a folder, and remembers where it really is.
    fn open(path: &Path) -> Result<Self, String> {
        match fs::metadata(path) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return Err(format!("{} is not a folder", path.display())),
            Err(err) => return Err(format!("{}: {err}", path.display())),
        }
        let real = fs::canonicalize(path).map_err(|err| format!("{}: {err}", path.display()))?;
        Ok(Self {
            path: path.to_path_buf(),
            real,
        })
    }

    /// Fails unless the destination still leads to the folder it did at the
    /// start. Called right before each item is placed.
    fn verify(&self) -> io::Result<()> {
        let changed = || {
            io::Error::other(format!(
                "{} is not the folder it was when the operation started",
                self.path.display()
            ))
        };
        let now = fs::canonicalize(&self.path).map_err(|_| changed())?;
        if now != self.real || !fs::symlink_metadata(&now).is_ok_and(|meta| meta.is_dir()) {
            return Err(changed());
        }
        Ok(())
    }
}

/// Moves `source` into `dest` under a free name. Across drives it copies,
/// then removes the original; if that removal fails the copy stays and the
/// error says so. Moving an item to the folder it is in does nothing.
///
/// The move never replaces anything: the name is chosen, and the move itself
/// is refused by the operating system if something took that name since (the
/// next free name is then tried).
fn move_item(source: &Path, dest: &Destination) -> io::Result<PathBuf> {
    move_item_hooked(source, dest, &mut |_| {})
}

/// [`move_item`], calling `before_move(target)` after a name was chosen and
/// before the move: tests change the file system there.
fn move_item_hooked(
    source: &Path,
    dest: &Destination,
    before_move: &mut dyn FnMut(&Path),
) -> io::Result<PathBuf> {
    let dest_dir = dest.path.as_path();
    let name = file_name_of(source)?;
    if source
        .parent()
        .is_some_and(|parent| same_place(parent, dest_dir))
    {
        return Ok(source.to_path_buf());
    }
    let meta = fs::symlink_metadata(source)?;
    if meta.is_dir() && is_inside(dest_dir, source) {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "a folder cannot be moved into itself",
        ));
    }
    for n in 1..=MAX_NAME_ATTEMPTS {
        let target = dest_dir.join(numbered(name, meta.is_dir(), n));
        // `symlink_metadata`: a dangling link occupies its name too.
        if fs::symlink_metadata(&target).is_ok() {
            continue;
        }
        before_move(&target);
        // Right before the move: still the folder the user meant?
        dest.verify()?;
        match rename_no_replace(source, &target) {
            Ok(()) => return Ok(target),
            // Taken since it was looked at: try the next name.
            Err(err) if err.kind() == ErrorKind::AlreadyExists => {}
            Err(err) if err.kind() == ErrorKind::CrossesDevices => {
                dest.verify()?;
                let copied = copy_item(source, dest_dir)?;
                remove_item(source).map_err(|err| {
                    io::Error::other(format!(
                        "copied to {} but could not remove the original: {err}",
                        copied.display()
                    ))
                })?;
                return Ok(copied);
            }
            Err(err) => return Err(err),
        }
    }
    Err(no_free_name(name))
}

/// Runs `step` on every item, in order, and collects how it went. Progress is
/// reported before each item. Callers that must not touch an item twice
/// [`prune_nested`] first.
pub fn run_each(
    items: &[PathBuf],
    progress: Progress<'_>,
    mut step: impl FnMut(&Path) -> Result<(), String>,
) -> Report {
    let mut report = Report {
        total: items.len(),
        ..Report::default()
    };
    for (i, item) in items.iter().enumerate() {
        let name = item.file_name().unwrap_or_default().to_string_lossy();
        progress(i, items.len(), &name);
        match step(item) {
            Ok(()) => report.done.push(item.clone()),
            Err(reason) => report.failed.push(Failure {
                path: item.clone(),
                reason,
            }),
        }
    }
    progress(items.len(), items.len(), "");
    report
}

/// Every item failed with `reason` (a destination that cannot be used).
fn all_failed(items: &[PathBuf], reason: &str) -> Report {
    let items = prune_nested(items);
    Report {
        total: items.len(),
        done: Vec::new(),
        failed: items
            .into_iter()
            .map(|path| Failure {
                path,
                reason: reason.to_owned(),
            })
            .collect(),
    }
}

/// Copies every item into the folder `dest`.
pub fn copy_items(items: &[PathBuf], dest: &Path, progress: Progress<'_>) -> Report {
    let dest = match Destination::open(dest) {
        Ok(dest) => dest,
        Err(reason) => return all_failed(items, &reason),
    };
    run_each(&prune_nested(items), progress, |item| {
        dest.verify()
            .and_then(|()| copy_item(item, &dest.path))
            .map(drop)
            .map_err(|err| err.to_string())
    })
}

/// Moves every item into the folder `dest`.
pub fn move_items(items: &[PathBuf], dest: &Path, progress: Progress<'_>) -> Report {
    let dest = match Destination::open(dest) {
        Ok(dest) => dest,
        Err(reason) => return all_failed(items, &reason),
    };
    run_each(&prune_nested(items), progress, |item| {
        move_item(item, &dest)
            .map(drop)
            .map_err(|err| err.to_string())
    })
}

/// Sends every item to the system trash.
pub fn trash_items(
    platform: &dyn PlatformProvider,
    items: &[PathBuf],
    progress: Progress<'_>,
) -> Report {
    run_each(&prune_nested(items), progress, |item| {
        platform.move_to_trash(item).map_err(|err| err.to_string())
    })
}

/// The name of the archive for `items`: the item's own name plus `.zip`, or
/// `Archive.zip` for several.
fn archive_name(items: &[PathBuf]) -> OsString {
    match items {
        [only] => {
            let mut name = only.file_name().unwrap_or_default().to_owned();
            name.push(".zip");
            name
        }
        _ => OsString::from("Archive.zip"),
    }
}

/// The modification time as a zip (DOS, local time) timestamp.
fn zip_time(time: std::time::SystemTime) -> Option<zip::DateTime> {
    use chrono::{Datelike, Local, Timelike};
    let local = chrono::DateTime::<Local>::from(time).naive_local();
    zip::DateTime::from_date_and_time(
        u16::try_from(local.year()).ok()?,
        u8::try_from(local.month()).ok()?,
        u8::try_from(local.day()).ok()?,
        u8::try_from(local.hour()).ok()?,
        u8::try_from(local.minute()).ok()?,
        u8::try_from(local.second()).ok()?,
    )
    .ok()
}

fn entry_options(meta: &fs::Metadata) -> SimpleFileOptions {
    let mut options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .large_file(meta.len() >= ZIP64_THRESHOLD);
    if let Some(time) = meta.modified().ok().and_then(zip_time) {
        options = options.last_modified_time(time);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        options = options.unix_permissions(meta.permissions().mode() & 0o7777);
    }
    options
}

/// The entry name for `path` below `base`: relative, with `/` separators.
fn entry_name(base: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(base).ok()?;
    let parts: Vec<String> = relative
        .components()
        .filter_map(|c| match c {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// Compresses `items` (folders with everything in them) into one `.zip` placed
/// in their common parent folder under a free name. Files that cannot be read,
/// and links and special files, are left out and counted. Progress counts
/// files. A failure while writing removes the half-written archive.
pub fn zip_items(items: &[PathBuf], progress: Progress<'_>) -> Result<ZipReport, String> {
    let items = prune_nested(items);
    if items.is_empty() {
        return Err("there is nothing to compress".to_owned());
    }
    let base = common_parent(&items).ok_or_else(|| {
        "the items are on different drives, so there is no folder to put the archive in".to_owned()
    })?;

    let total = items
        .iter()
        .flat_map(|item| WalkDir::new(item).follow_links(false))
        .filter(|entry| entry.as_ref().is_ok_and(|e| e.file_type().is_file()))
        .count();

    let (file, archive) = claim_file(&base, &archive_name(&items))
        .map_err(|err| format!("{}: {err}", base.display()))?;
    let outcome = write_archive(file, &base, &items, total, progress);
    match outcome {
        Ok((files, skipped)) => Ok(ZipReport {
            archive,
            files,
            skipped,
        }),
        Err(err) => {
            let _ = fs::remove_file(&archive);
            Err(err)
        }
    }
}

fn write_archive(
    file: File,
    base: &Path,
    items: &[PathBuf],
    total: usize,
    progress: Progress<'_>,
) -> Result<(usize, usize), String> {
    let mut zip = ZipWriter::new(BufWriter::new(file));
    let mut written = 0usize;
    let mut skipped = 0usize;
    let zip_error = |err: zip::result::ZipError| err.to_string();

    for item in items {
        for entry in WalkDir::new(item).follow_links(false).sort_by_file_name() {
            let Ok(entry) = entry else {
                skipped += 1;
                continue;
            };
            let Some(name) = entry_name(base, entry.path()) else {
                continue;
            };
            let Ok(meta) = entry.metadata() else {
                skipped += 1;
                continue;
            };
            if entry.file_type().is_dir() {
                zip.add_directory(format!("{name}/"), entry_options(&meta))
                    .map_err(zip_error)?;
            } else if entry.file_type().is_file() {
                progress(written, total, &entry.file_name().to_string_lossy());
                // Opened first: a file that cannot be read leaves no empty entry.
                let Ok(mut source) = File::open(entry.path()) else {
                    skipped += 1;
                    continue;
                };
                zip.start_file(name, entry_options(&meta))
                    .map_err(zip_error)?;
                io::copy(&mut source, &mut zip).map_err(|err| err.to_string())?;
                written += 1;
            } else {
                skipped += 1;
            }
        }
    }
    progress(written, total, "");
    let mut inner = zip.finish().map_err(zip_error)?;
    inner.flush().map_err(|err| err.to_string())?;
    Ok((written, skipped))
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;
    use crate::test_util::MockPlatform;

    fn quiet() -> impl FnMut(usize, usize, &str) {
        |_, _, _| {}
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn names_are_numbered_before_the_extension() {
        let name = |text: &str, dir: bool, n| numbered(OsStr::new(text), dir, n);
        assert_eq!(name("file.txt", false, 1), "file.txt");
        assert_eq!(name("file.txt", false, 2), "file (2).txt");
        assert_eq!(name("file.txt", false, 13), "file (13).txt");
        assert_eq!(name("noext", false, 2), "noext (2)");
        assert_eq!(name(".bashrc", false, 2), ".bashrc (2)");
        assert_eq!(name("trailing.", false, 2), "trailing. (2)");
        assert_eq!(name("a.tar.gz", false, 2), "a (2).tar.gz");
        assert_eq!(name("photos.2024", true, 2), "photos.2024 (2)");
        assert_eq!(name("v1.2.docx", false, 3), "v1.2 (3).docx");
    }

    #[test]
    fn unique_path_skips_taken_names() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("a.txt"), "1");
        write(&dir.path().join("a (2).txt"), "2");
        let free = unique_path(dir.path(), OsStr::new("a.txt"), false).unwrap();
        assert_eq!(free, dir.path().join("a (3).txt"));
        let untouched = unique_path(dir.path(), OsStr::new("b.txt"), false).unwrap();
        assert_eq!(untouched, dir.path().join("b.txt"));
    }

    #[test]
    fn nested_and_repeated_items_are_dropped() {
        let items: Vec<PathBuf> = ["/a/dir", "/a/dir/file", "/a/other", "/a/dir", "/a/dir2"]
            .iter()
            .map(PathBuf::from)
            .collect();
        assert_eq!(
            prune_nested(&items),
            ["/a/dir", "/a/other", "/a/dir2"].map(PathBuf::from)
        );
    }

    #[test]
    fn the_common_parent_is_the_deepest_shared_folder() {
        let parent = |items: &[&str]| {
            let items: Vec<PathBuf> = items.iter().map(PathBuf::from).collect();
            common_parent(&items)
        };
        assert_eq!(parent(&["/a/b/c.txt"]), Some(PathBuf::from("/a/b")));
        assert_eq!(
            parent(&["/a/b/c.txt", "/a/b/d.txt"]),
            Some(PathBuf::from("/a/b"))
        );
        assert_eq!(
            parent(&["/a/b/c.txt", "/a/x/y/d.txt"]),
            Some(PathBuf::from("/a"))
        );
        assert_eq!(parent(&[]), None);
    }

    #[cfg(windows)]
    #[test]
    fn items_on_different_drives_have_no_common_parent() {
        let items = [PathBuf::from(r"C:\a\b.txt"), PathBuf::from(r"D:\a\c.txt")];
        assert_eq!(common_parent(&items), None);
    }

    #[test]
    fn copying_files_and_folders_keeps_the_originals() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        let dest = dir.path().join("dest");
        write(&src.join("note.txt"), "hello");
        write(&src.join("tree/inner.txt"), "inner");
        write(&src.join("tree/deep/leaf.txt"), "leaf");
        fs::create_dir_all(src.join("tree/empty")).unwrap();
        fs::create_dir_all(&dest).unwrap();

        let items = [src.join("note.txt"), src.join("tree")];
        let mut seen = Vec::new();
        let report = copy_items(&items, &dest, &mut |done, total, name| {
            seen.push((done, total, name.to_owned()));
        });

        assert_eq!(report.total, 2);
        assert_eq!(report.done, items);
        assert!(report.failed.is_empty());
        assert_eq!(read(&dest.join("note.txt")), "hello");
        assert_eq!(read(&dest.join("tree/inner.txt")), "inner");
        assert_eq!(read(&dest.join("tree/deep/leaf.txt")), "leaf");
        assert!(dest.join("tree/empty").is_dir());
        // The originals are still there.
        assert_eq!(read(&src.join("note.txt")), "hello");
        assert!(src.join("tree/deep/leaf.txt").is_file());
        assert_eq!(
            seen,
            [
                (0, 2, "note.txt".to_owned()),
                (1, 2, "tree".to_owned()),
                (2, 2, String::new())
            ]
        );
    }

    #[test]
    fn copying_onto_an_existing_name_numbers_the_copy() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("dest");
        write(&dest.join("a.txt"), "old");
        write(&dest.join("pics/x.png"), "old pic");
        write(&dir.path().join("src/a.txt"), "new");
        write(&dir.path().join("src/pics/y.png"), "new pic");

        let items = [dir.path().join("src/a.txt"), dir.path().join("src/pics")];
        let report = copy_items(&items, &dest, &mut quiet());
        assert!(report.failed.is_empty(), "{:?}", report.failed);

        // Nothing was overwritten.
        assert_eq!(read(&dest.join("a.txt")), "old");
        assert_eq!(read(&dest.join("a (2).txt")), "new");
        assert_eq!(read(&dest.join("pics/x.png")), "old pic");
        assert_eq!(read(&dest.join("pics (2)/y.png")), "new pic");

        // And again: the next free numbers are used.
        copy_items(&items, &dest, &mut quiet());
        assert_eq!(read(&dest.join("a (3).txt")), "new");
        assert!(dest.join("pics (3)/y.png").is_file());
    }

    #[test]
    fn copying_a_file_into_its_own_folder_makes_a_numbered_duplicate() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("a.txt"), "x");
        let report = copy_items(&[dir.path().join("a.txt")], dir.path(), &mut quiet());
        assert!(report.failed.is_empty());
        assert_eq!(names(dir.path()), ["a (2).txt", "a.txt"]);
    }

    #[test]
    fn a_folder_cannot_be_copied_or_moved_into_itself() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("f");
        write(&folder.join("sub/x.txt"), "x");
        for report in [
            copy_items(
                std::slice::from_ref(&folder),
                &folder.join("sub"),
                &mut quiet(),
            ),
            move_items(
                std::slice::from_ref(&folder),
                &folder.join("sub"),
                &mut quiet(),
            ),
            copy_items(std::slice::from_ref(&folder), &folder, &mut quiet()),
        ] {
            assert!(report.done.is_empty());
            assert_eq!(report.failed.len(), 1);
            assert!(report.failed[0].reason.contains("into itself"));
        }
        // Nothing was created inside.
        assert_eq!(names(&folder), ["sub"]);
        assert_eq!(names(&folder.join("sub")), ["x.txt"]);
    }

    #[test]
    fn moving_removes_the_originals() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        let dest = dir.path().join("dest");
        write(&src.join("a.txt"), "a");
        write(&src.join("tree/b.txt"), "b");
        fs::create_dir_all(&dest).unwrap();

        let items = [src.join("a.txt"), src.join("tree")];
        let report = move_items(&items, &dest, &mut quiet());
        assert_eq!(report.done, items);
        assert!(report.failed.is_empty());
        assert!(!src.join("a.txt").exists());
        assert!(!src.join("tree").exists());
        assert_eq!(read(&dest.join("a.txt")), "a");
        assert_eq!(read(&dest.join("tree/b.txt")), "b");
    }

    #[test]
    fn moving_onto_an_existing_name_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("dest");
        write(&dest.join("a.txt"), "keep me");
        write(&dir.path().join("src/a.txt"), "incoming");
        let report = move_items(&[dir.path().join("src/a.txt")], &dest, &mut quiet());
        assert!(report.failed.is_empty());
        assert_eq!(read(&dest.join("a.txt")), "keep me");
        assert_eq!(read(&dest.join("a (2).txt")), "incoming");
        assert!(!dir.path().join("src/a.txt").exists());
    }

    #[test]
    fn moving_to_the_folder_an_item_is_in_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("a.txt"), "x");
        let report = move_items(&[dir.path().join("a.txt")], dir.path(), &mut quiet());
        assert_eq!(report.done.len(), 1);
        assert_eq!(names(dir.path()), ["a.txt"]);
    }

    #[test]
    fn a_bad_destination_fails_every_item_and_touches_nothing() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("a.txt"), "x");
        write(&dir.path().join("plain"), "i am a file");
        let items = [dir.path().join("a.txt")];

        let missing = move_items(&items, &dir.path().join("nope"), &mut quiet());
        assert!(missing.done.is_empty());
        assert_eq!(missing.failed.len(), 1);

        let not_a_folder = copy_items(&items, &dir.path().join("plain"), &mut quiet());
        assert_eq!(not_a_folder.failed.len(), 1);
        assert!(not_a_folder.failed[0].reason.contains("not a folder"));
        assert_eq!(names(dir.path()), ["a.txt", "plain"]);
    }

    #[test]
    fn a_partial_failure_reports_what_worked() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("dest");
        fs::create_dir_all(&dest).unwrap();
        write(&dir.path().join("one.txt"), "1");
        write(&dir.path().join("three.txt"), "3");
        let items = [
            dir.path().join("one.txt"),
            dir.path().join("gone.txt"),
            dir.path().join("three.txt"),
        ];
        let report = move_items(&items, &dest, &mut quiet());
        assert_eq!(report.total, 3);
        assert_eq!(report.done, [items[0].clone(), items[2].clone()]);
        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.failed[0].path, items[1]);
        assert!(dest.join("one.txt").is_file() && dest.join("three.txt").is_file());
    }

    #[test]
    fn nested_selections_are_handled_once() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("dest");
        fs::create_dir_all(&dest).unwrap();
        write(&dir.path().join("tree/b.txt"), "b");
        let items = [dir.path().join("tree"), dir.path().join("tree/b.txt")];
        let report = copy_items(&items, &dest, &mut quiet());
        assert_eq!(report.total, 1);
        assert_eq!(names(&dest), ["tree"]);
        assert_eq!(names(&dest.join("tree")), ["b.txt"]);
    }

    #[cfg(unix)]
    #[test]
    fn links_are_copied_as_links_and_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("dest");
        fs::create_dir_all(&dest).unwrap();
        write(&dir.path().join("target.txt"), "t");
        std::os::unix::fs::symlink("target.txt", dir.path().join("link")).unwrap();
        // A link loop must not hang a folder copy.
        let folder = dir.path().join("folder");
        fs::create_dir_all(&folder).unwrap();
        std::os::unix::fs::symlink(&folder, folder.join("loop")).unwrap();

        let report = copy_items(
            &[dir.path().join("link"), folder.clone()],
            &dest,
            &mut quiet(),
        );
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        assert_eq!(
            fs::read_link(dest.join("link")).unwrap(),
            Path::new("target.txt")
        );
        assert!(fs::symlink_metadata(dest.join("folder/loop"))
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[test]
    fn trashing_asks_the_platform_for_each_item_and_counts_refusals() {
        let platform = MockPlatform::empty();
        let items: Vec<PathBuf> = ["/t/a.txt", "/t/b.txt", "/t/c.txt"]
            .iter()
            .map(PathBuf::from)
            .collect();
        platform
            .trash_refuses
            .lock()
            .unwrap()
            .push(PathBuf::from("/t/b.txt"));

        let report = trash_items(&*platform, &items, &mut quiet());
        assert_eq!(report.total, 3);
        assert_eq!(report.done, [items[0].clone(), items[2].clone()]);
        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.failed[0].path, items[1]);
        assert_eq!(
            *platform.trashed.lock().unwrap(),
            [items[0].clone(), items[2].clone()]
        );
    }

    fn zip_entries(archive: &Path) -> Vec<(String, String)> {
        let mut zip = zip::ZipArchive::new(File::open(archive).unwrap()).unwrap();
        let mut entries = Vec::new();
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).unwrap();
            let mut text = String::new();
            if entry.is_file() {
                entry.read_to_string(&mut text).unwrap();
            }
            entries.push((entry.name().to_owned(), text));
        }
        entries.sort();
        entries
    }

    #[test]
    fn zipping_several_items_makes_archive_zip_in_their_common_parent() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("docs/a.txt"), "alpha");
        write(&dir.path().join("docs/sub/b.txt"), "beta");
        fs::create_dir_all(dir.path().join("docs/empty")).unwrap();
        write(&dir.path().join("loose.txt"), "gamma");
        write(&dir.path().join("elsewhere/c.txt"), "delta");

        let items = [
            dir.path().join("docs"),
            dir.path().join("loose.txt"),
            // Nested in "docs": must not be stored twice.
            dir.path().join("docs/a.txt"),
        ];
        let mut last = (0, 0);
        let report = zip_items(&items, &mut |done, total, _| last = (done, total)).unwrap();

        assert_eq!(report.archive, dir.path().join("Archive.zip"));
        assert_eq!(report.files, 3);
        assert_eq!(report.skipped, 0);
        assert_eq!(last, (3, 3));
        let entries = zip_entries(&report.archive);
        let listed: Vec<&str> = entries.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            listed,
            [
                "docs/",
                "docs/a.txt",
                "docs/empty/",
                "docs/sub/",
                "docs/sub/b.txt",
                "loose.txt"
            ]
        );
        let content = |name: &str| entries.iter().find(|(n, _)| n == name).unwrap().1.clone();
        assert_eq!(content("docs/a.txt"), "alpha");
        assert_eq!(content("docs/sub/b.txt"), "beta");
        assert_eq!(content("loose.txt"), "gamma");
        // The sources are untouched.
        assert_eq!(read(&dir.path().join("docs/a.txt")), "alpha");
    }

    #[test]
    fn zipping_one_item_names_the_archive_after_it_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("report.docx"), "text");
        let item = [dir.path().join("report.docx")];

        let first = zip_items(&item, &mut quiet()).unwrap();
        assert_eq!(first.archive, dir.path().join("report.docx.zip"));
        let second = zip_items(&item, &mut quiet()).unwrap();
        assert_eq!(second.archive, dir.path().join("report.docx (2).zip"));
        assert!(first.archive.is_file());
        assert_eq!(
            zip_entries(&second.archive),
            [("report.docx".to_owned(), "text".to_owned())]
        );
    }

    #[test]
    fn zipping_items_from_different_folders_goes_in_their_shared_parent() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("one/a.txt"), "a");
        write(&dir.path().join("two/b.txt"), "b");
        let items = [dir.path().join("one/a.txt"), dir.path().join("two/b.txt")];
        let report = zip_items(&items, &mut quiet()).unwrap();
        assert_eq!(report.archive, dir.path().join("Archive.zip"));
        let listed: Vec<String> = zip_entries(&report.archive)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(listed, ["one/a.txt", "two/b.txt"]);
    }

    /// A name that is free when it is chosen but taken before the move: the
    /// newcomer is left alone and the item takes the next name.
    #[test]
    fn a_move_never_replaces_a_name_that_appears_after_it_was_chosen() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("from/report.txt");
        write(&src, "mine");
        let dest_dir = dir.path().join("to");
        fs::create_dir_all(&dest_dir).unwrap();
        let dest = Destination::open(&dest_dir).unwrap();
        let mut raced = false;
        let moved = move_item_hooked(&src, &dest, &mut |target| {
            if !raced {
                raced = true;
                write(target, "theirs, created in the gap");
            }
        })
        .unwrap();
        assert_eq!(
            read(&dest_dir.join("report.txt")),
            "theirs, created in the gap"
        );
        assert_eq!(moved, dest_dir.join("report (2).txt"));
        assert_eq!(read(&moved), "mine");
        assert!(!src.exists());
    }

    /// `rename(2)` would replace an empty folder that took the name meanwhile.
    #[test]
    fn a_moved_folder_does_not_replace_a_folder_that_appeared_in_the_gap() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("from/photos/a.jpg"), "a");
        let dest_dir = dir.path().join("to");
        fs::create_dir_all(&dest_dir).unwrap();
        let dest = Destination::open(&dest_dir).unwrap();
        let mut raced = false;
        let moved = move_item_hooked(&dir.path().join("from/photos"), &dest, &mut |target| {
            if !raced {
                raced = true;
                fs::create_dir(target).unwrap();
            }
        })
        .unwrap();
        assert_eq!(moved, dest_dir.join("photos (2)"));
        assert_eq!(read(&moved.join("a.jpg")), "a");
        assert!(fs::read_dir(dest_dir.join("photos"))
            .unwrap()
            .next()
            .is_none());
    }

    /// The destination is replaced by a link to somewhere else after the batch
    /// started: the item that has not been placed yet is not.
    #[cfg(unix)]
    #[test]
    fn a_destination_swapped_for_a_link_is_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("from/a.txt");
        write(&src, "a");
        let dest_dir = dir.path().join("to");
        let elsewhere = dir.path().join("elsewhere");
        fs::create_dir_all(&dest_dir).unwrap();
        fs::create_dir_all(&elsewhere).unwrap();
        let dest = Destination::open(&dest_dir).unwrap();
        let err = move_item_hooked(&src, &dest, &mut |_| {
            fs::remove_dir(&dest_dir).unwrap();
            std::os::unix::fs::symlink(&elsewhere, &dest_dir).unwrap();
        })
        .unwrap_err();
        assert!(err.to_string().contains("not the folder it was"), "{err}");
        assert_eq!(read(&src), "a");
        assert!(names(&elsewhere).is_empty());
    }

    /// The same in a whole batch: the first item is placed, then the folder
    /// becomes a link, and the rest are refused.
    #[cfg(unix)]
    #[test]
    fn a_batch_stops_following_a_destination_that_became_a_link() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("from/one.txt");
        let second = dir.path().join("from/two.txt");
        write(&first, "1");
        write(&second, "2");
        let dest_dir = dir.path().join("to");
        let elsewhere = dir.path().join("elsewhere");
        fs::create_dir_all(&dest_dir).unwrap();
        fs::create_dir_all(&elsewhere).unwrap();
        let swap_dir = dest_dir.clone();
        let swap_to = elsewhere.clone();
        let mut progress = move |done: usize, _total: usize, _name: &str| {
            // Before the second item starts.
            if done == 1 {
                fs::rename(&swap_dir, swap_dir.with_extension("moved")).unwrap();
                std::os::unix::fs::symlink(&swap_to, &swap_dir).unwrap();
            }
        };
        let report = move_items(&[first.clone(), second.clone()], &dest_dir, &mut progress);
        assert_eq!(report.done, [first]);
        assert_eq!(report.failed.len(), 1);
        assert!(report.failed[0].reason.contains("not the folder it was"));
        assert_eq!(read(&second), "2", "the refused item stays where it was");
        assert!(names(&elsewhere).is_empty());
        assert_eq!(read(&dest_dir.with_extension("moved").join("one.txt")), "1");
    }

    /// A link already sitting at the name a copy would use is never written
    /// through: the copy takes the next name.
    #[cfg(unix)]
    #[test]
    fn a_planted_link_at_the_destination_name_is_not_written_through() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("from/a.txt");
        write(&src, "new");
        let victim = dir.path().join("victim.txt");
        write(&victim, "precious");
        let dest_dir = dir.path().join("to");
        fs::create_dir_all(&dest_dir).unwrap();
        std::os::unix::fs::symlink(&victim, dest_dir.join("a.txt")).unwrap();
        let report = copy_items(&[src], &dest_dir, &mut quiet());
        assert!(report.failed.is_empty(), "{report:?}");
        assert_eq!(read(&victim), "precious");
        assert_eq!(read(&dest_dir.join("a (2).txt")), "new");
        // Dangling links count as taken names too.
        let src2 = dir.path().join("from/b.txt");
        write(&src2, "b");
        std::os::unix::fs::symlink(dir.path().join("nowhere"), dest_dir.join("b.txt")).unwrap();
        let report = move_items(&[src2], &dest_dir, &mut quiet());
        assert!(report.failed.is_empty(), "{report:?}");
        assert!(!dir.path().join("nowhere").exists());
        assert_eq!(read(&dest_dir.join("b (2).txt")), "b");
    }

    #[test]
    fn zipping_nothing_is_an_error_and_leaves_no_archive() {
        assert!(zip_items(&[], &mut quiet()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn zipping_skips_links_and_counts_them() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("f/real.txt"), "r");
        std::os::unix::fs::symlink("real.txt", dir.path().join("f/link")).unwrap();
        let report = zip_items(&[dir.path().join("f")], &mut quiet()).unwrap();
        assert_eq!(report.files, 1);
        assert_eq!(report.skipped, 1);
        let listed: Vec<String> = zip_entries(&report.archive)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(listed, ["f/", "f/real.txt"]);
    }
}
