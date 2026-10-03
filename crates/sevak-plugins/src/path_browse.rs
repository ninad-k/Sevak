//! Path browsing for the files plugin: typing a path (`~/Doc`, `C:\Users\`,
//! `/etc/`) lists that one directory instead of searching the index.
//!
//! Nothing here recurses. A directory is read once per keystroke burst (the last
//! listing is cached for a moment) on a helper thread that the caller waits on
//! for at most [`LIST_TIMEOUT`], so a stalled network share cannot stall typing.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a query waits for a directory listing before giving up.
pub const LIST_TIMEOUT: Duration = Duration::from_millis(150);
/// Entries read from one directory; a larger directory is filtered over the
/// first ones only.
pub const MAX_DIR_ENTRIES: usize = 5000;
/// Listings that may still be running (e.g. blocked on a dead share) at once;
/// further path queries answer nothing until one finishes.
const MAX_IN_FLIGHT: usize = 4;
/// How long the last listing is reused (typing the filter re-queries the same
/// directory on every keystroke).
const CACHE_TTL: Duration = Duration::from_millis(1500);

/// A typed path split into the directory to list and the name being typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathQuery {
    /// The directory to list, with `~` expanded.
    pub dir: PathBuf,
    /// The input up to and including the last separator, as typed
    /// (`~/Documents/`); completions are built on it.
    pub typed_dir: String,
    /// Text after the last separator: what entry names are filtered by.
    pub filter: String,
    /// The separator that ends `typed_dir`.
    pub sep: char,
}

/// Recognizes path-like input: `~/` or `~\` (home), `/`, a drive (`C:\`, `C:/`)
/// or a UNC share (`\\server\share\`). Backslashes and drives only count when
/// `windows` is set, as on other systems they are ordinary file-name characters.
/// Returns `None` for anything else, and for a UNC path without both a server
/// and a share (listing `\\server` would go to the network and cannot work).
pub fn parse(input: &str, home: Option<&Path>, windows: bool) -> Option<PathQuery> {
    let is_sep = |c: char| c == '/' || (windows && c == '\\');
    let mut chars = input.chars();
    let first = chars.next()?;
    let second = chars.next();
    let third = chars.next();

    let home_relative = first == '~' && second.is_some_and(is_sep);
    let unc = windows && first == '\\' && second == Some('\\');
    let drive =
        windows && first.is_ascii_alphabetic() && second == Some(':') && third.is_some_and(is_sep);
    if !(home_relative || unc || drive || is_sep(first)) {
        return None;
    }

    let last = input.rfind(is_sep)?;
    let (typed_dir, filter) = input.split_at(last + 1);
    let sep = input[last..].chars().next()?;

    if unc {
        let parts = typed_dir[2..].split(is_sep).filter(|part| !part.is_empty());
        if parts.count() < 2 {
            return None;
        }
    }
    let dir = if home_relative {
        home?.join(&typed_dir[2..])
    } else {
        PathBuf::from(typed_dir)
    };
    Some(PathQuery {
        dir,
        typed_dir: typed_dir.to_owned(),
        filter: filter.to_owned(),
        sep,
    })
}

/// One entry of a listed directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirItem {
    pub name: String,
    pub is_dir: bool,
}

struct Cached {
    dir: PathBuf,
    at: Instant,
    /// `None`: the listing failed or timed out (also remembered briefly).
    items: Option<Arc<Vec<DirItem>>>,
}

/// Lists directories within a time and concurrency budget.
#[derive(Default)]
pub struct DirReader {
    cache: Mutex<Option<Cached>>,
    in_flight: Arc<AtomicUsize>,
}

/// Decrements the in-flight counter when the listing thread ends, however it ends.
struct InFlight(Arc<AtomicUsize>);

impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl DirReader {
    /// The entries of `dir` (unfiltered, unsorted), or `None` if it cannot be
    /// read within [`LIST_TIMEOUT`].
    pub fn list(&self, dir: &Path) -> Option<Arc<Vec<DirItem>>> {
        self.list_within(dir, LIST_TIMEOUT)
    }

    fn list_within(&self, dir: &Path, timeout: Duration) -> Option<Arc<Vec<DirItem>>> {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cached) = cache.as_ref() {
            if cached.dir == dir && cached.at.elapsed() < CACHE_TTL {
                return cached.items.clone();
            }
        }
        let items = self.read_bounded(dir, timeout).map(Arc::new);
        *cache = Some(Cached {
            dir: dir.to_path_buf(),
            at: Instant::now(),
            items: items.clone(),
        });
        items
    }

    fn read_bounded(&self, dir: &Path, timeout: Duration) -> Option<Vec<DirItem>> {
        if self.in_flight.fetch_add(1, Ordering::SeqCst) >= MAX_IN_FLIGHT {
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        let guard = InFlight(Arc::clone(&self.in_flight));
        let (tx, rx) = mpsc::channel();
        let path = dir.to_path_buf();
        let spawned = std::thread::Builder::new()
            .name("sevak-dir".to_owned())
            .spawn(move || {
                let _guard = guard;
                let _ = tx.send(read_entries(&path, MAX_DIR_ENTRIES));
            });
        if spawned.is_err() {
            // The closure (and with it the guard) is dropped when spawn fails.
            return None;
        }
        match rx.recv_timeout(timeout) {
            Ok(items) => items,
            Err(_) => {
                tracing::debug!(dir = %dir.display(), "directory listing timed out");
                None
            }
        }
    }
}

fn read_entries(dir: &Path, cap: usize) -> Option<Vec<DirItem>> {
    let mut items = Vec::new();
    for entry in fs::read_dir(dir).ok()? {
        let Ok(entry) = entry else { continue };
        if items.len() >= cap {
            break;
        }
        let is_dir = match entry.file_type() {
            Ok(kind) if kind.is_symlink() => entry.path().is_dir(),
            Ok(kind) => kind.is_dir(),
            Err(_) => false,
        };
        items.push(DirItem {
            name: entry.file_name().to_string_lossy().into_owned(),
            is_dir,
        });
    }
    Some(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = if cfg!(windows) {
        "C:\\Users\\me"
    } else {
        "/home/me"
    };

    fn parsed(input: &str, windows: bool) -> Option<PathQuery> {
        parse(input, Some(Path::new(HOME)), windows)
    }

    #[test]
    fn home_relative_paths() {
        let q = parsed("~/Docs/rep", false).unwrap();
        assert_eq!(q.dir, Path::new(HOME).join("Docs/"));
        assert_eq!(q.typed_dir, "~/Docs/");
        assert_eq!(q.filter, "rep");
        assert_eq!(q.sep, '/');

        let q = parsed("~/", false).unwrap();
        assert_eq!(q.dir, Path::new(HOME).join(""));
        assert_eq!(q.filter, "");

        let q = parsed("~\\Docs\\x", true).unwrap();
        assert_eq!(q.typed_dir, "~\\Docs\\");
        assert_eq!(q.sep, '\\');
        assert_eq!(q.dir, Path::new(HOME).join("Docs\\"));
    }

    #[test]
    fn needs_a_home_directory_and_a_separator() {
        assert_eq!(parse("~/x", None, false), None);
        assert_eq!(parsed("~", false), None);
        assert_eq!(parsed("~user/x", false), None);
        assert_eq!(parsed("~\\x", false), None, "backslash is a name char here");
    }

    #[test]
    fn rooted_paths() {
        let q = parsed("/etc/ho", false).unwrap();
        assert_eq!(q.dir, Path::new("/etc/"));
        assert_eq!(q.filter, "ho");
        let q = parsed("/", false).unwrap();
        assert_eq!(q.dir, Path::new("/"));
        assert_eq!(q.filter, "");
        // Not paths: plain words, calculator input, relative paths.
        for input in ["etc/hosts", "", "1/2", "g /x", "./x/", "x:"] {
            assert_eq!(parsed(input, false), None, "{input:?}");
            assert_eq!(parsed(input, true), None, "{input:?}");
        }
    }

    #[test]
    fn drive_paths_are_windows_only() {
        let q = parsed("C:\\Users\\me\\Down", true).unwrap();
        assert_eq!(q.dir, Path::new("C:\\Users\\me\\"));
        assert_eq!(q.typed_dir, "C:\\Users\\me\\");
        assert_eq!(q.filter, "Down");
        assert_eq!(q.sep, '\\');

        let q = parsed("d:/proj/", true).unwrap();
        assert_eq!(q.typed_dir, "d:/proj/");
        assert_eq!(q.sep, '/');
        assert_eq!(q.filter, "");

        assert_eq!(parsed("C:\\Users", false), None);
        assert_eq!(parsed("C:", true), None, "no separator yet");
        assert_eq!(parsed("C:foo", true), None);
    }

    #[test]
    fn unc_paths_need_a_server_and_a_share() {
        let q = parsed("\\\\srv\\share\\dir\\f", true).unwrap();
        assert_eq!(q.dir, Path::new("\\\\srv\\share\\dir\\"));
        assert_eq!(q.filter, "f");
        assert!(parsed("\\\\srv\\share\\", true).is_some());
        assert_eq!(parsed("\\\\srv\\share", true), None);
        assert_eq!(parsed("\\\\srv\\", true), None);
        assert_eq!(parsed("\\\\", true), None);
        assert_eq!(parsed("\\\\srv\\share\\", false), None);
    }

    #[test]
    fn lists_one_directory_with_kinds() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("a.txt"), b"x").unwrap();
        fs::write(dir.path().join("sub").join("deep.txt"), b"x").unwrap();

        let reader = DirReader::default();
        let mut items = reader.list(dir.path()).unwrap().to_vec();
        items.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(
            items,
            vec![
                DirItem {
                    name: "a.txt".into(),
                    is_dir: false
                },
                DirItem {
                    name: "sub".into(),
                    is_dir: true
                },
            ]
        );
    }

    #[test]
    fn missing_directories_list_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(DirReader::default()
            .list(&dir.path().join("nope"))
            .is_none());
    }

    #[test]
    fn the_last_listing_is_reused_briefly() {
        let dir = tempfile::tempdir().unwrap();
        let reader = DirReader::default();
        let first = reader.list(dir.path()).unwrap();
        fs::write(dir.path().join("new.txt"), b"x").unwrap();
        let second = reader.list(dir.path()).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        // Another directory replaces the cache.
        let other = tempfile::tempdir().unwrap();
        assert!(reader.list(other.path()).is_some());
        let third = reader.list(dir.path()).unwrap();
        assert!(!Arc::ptr_eq(&first, &third));
        assert_eq!(third.len(), 1);
    }

    #[test]
    fn stuck_listings_are_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let reader = DirReader::default();
        // Pretend four listings are blocked on a dead share.
        reader.in_flight.store(MAX_IN_FLIGHT, Ordering::SeqCst);
        assert!(reader.list(dir.path()).is_none());
        assert_eq!(reader.in_flight.load(Ordering::SeqCst), MAX_IN_FLIGHT);
        reader.in_flight.store(0, Ordering::SeqCst);
        // The failure was cached for a moment; a fresh reader lists fine and
        // releases its slot.
        let reader = DirReader::default();
        assert!(reader.list(dir.path()).is_some());
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(reader.in_flight.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn reading_stops_at_the_entry_cap() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..10 {
            fs::write(dir.path().join(format!("f{i}")), b"x").unwrap();
        }
        assert_eq!(read_entries(dir.path(), 4).unwrap().len(), 4);
    }
}
