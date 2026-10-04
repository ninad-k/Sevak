//! What is in Sevak's folders (names and sizes, never contents) and what the
//! log files say (the last lines, and how many errors and panics).

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Most entries listed for one folder.
const MAX_ENTRIES: usize = 200;
/// Most files counted below one sub-folder.
const MAX_COUNTED: usize = 5_000;
/// How much of the end of each log file is read.
const LOG_READ_BYTES: u64 = 1024 * 1024;
/// What the panic hook writes (`thread panicked at file:line`); the plugin
/// guard's own line says "plugin panicked" and is not counted again.
pub const PANIC_MARKER: &str = "thread panicked at";
/// Log files looked at (one per day, newest first).
const MAX_LOG_FILES: usize = 7;

/// One entry of a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    /// Size of a file; the total size of the files below a folder.
    pub bytes: u64,
    /// `Some(n)` for a folder: the number of files in it (counted up to a limit).
    pub files: Option<usize>,
}

/// A folder's top level.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inventory {
    pub exists: bool,
    pub entries: Vec<FileEntry>,
    /// There were more entries than are listed.
    pub truncated: bool,
    pub error: Option<String>,
}

/// Lists `dir`'s entries with sizes. A sub-folder shows how many files it holds
/// and their total size, not their names.
pub fn inventory(dir: &Path) -> Inventory {
    let read = match fs::read_dir(dir) {
        Ok(read) => read,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Inventory::default(),
        Err(err) => {
            return Inventory {
                exists: true,
                error: Some(err.to_string()),
                ..Inventory::default()
            }
        }
    };
    let mut entries: Vec<FileEntry> = Vec::new();
    for entry in read.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            let (files, bytes) = count_below(&entry.path());
            entries.push(FileEntry {
                name,
                bytes,
                files: Some(files),
            });
        } else {
            entries.push(FileEntry {
                name,
                bytes: meta.len(),
                files: None,
            });
        }
    }
    entries.sort_by_key(|entry| entry.name.to_lowercase());
    let truncated = entries.len() > MAX_ENTRIES;
    entries.truncate(MAX_ENTRIES);
    Inventory {
        exists: true,
        entries,
        truncated,
        error: None,
    }
}

/// `(files, bytes)` below `dir`, without following links, stopping at a limit.
fn count_below(dir: &Path) -> (usize, u64) {
    let mut files = 0;
    let mut bytes = 0;
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        let Ok(read) = fs::read_dir(&next) else {
            continue;
        };
        for entry in read.filter_map(Result::ok) {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() {
                files += 1;
                bytes += entry.metadata().map_or(0, |meta| meta.len());
                if files >= MAX_COUNTED {
                    return (files, bytes);
                }
            }
        }
    }
    (files, bytes)
}

/// The log folder as the report shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogScan {
    /// The log files, newest first (names and sizes).
    pub files: Vec<FileEntry>,
    /// The last lines, oldest first, as written (not yet redacted).
    pub tail: Vec<String>,
    /// Lines at `ERROR` level in the files read.
    pub errors: usize,
    pub warnings: usize,
    /// Lines that report a panic.
    pub panics: usize,
    pub error: Option<String>,
}

/// Whether `name` is one of Sevak's daily log files (`sevak.2026-10-04.log`).
fn is_log_file(name: &str) -> bool {
    name.starts_with("sevak") && name.ends_with(".log")
}

/// Reads the last `tail` lines of the logs in `dir`, and counts the errors,
/// warnings and panics in the recent days' files (the end of each).
pub fn scan_logs(dir: &Path, tail: usize) -> LogScan {
    let read = match fs::read_dir(dir) {
        Ok(read) => read,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return LogScan::default(),
        Err(err) => {
            return LogScan {
                error: Some(err.to_string()),
                ..LogScan::default()
            }
        }
    };
    let mut names: Vec<(String, u64)> = read
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let meta = entry.metadata().ok()?;
            (meta.is_file() && is_log_file(&name)).then_some((name, meta.len()))
        })
        .collect();
    // The date is in the name, so the names sort by age.
    names.sort_by(|a, b| b.0.cmp(&a.0));

    let mut scan = LogScan {
        files: names
            .iter()
            .map(|(name, bytes)| FileEntry {
                name: name.clone(),
                bytes: *bytes,
                files: None,
            })
            .collect(),
        ..LogScan::default()
    };
    // Newest file first; older ones only top the tail up.
    let mut older_first: Vec<Vec<String>> = Vec::new();
    let mut have = 0;
    for (name, _) in names.iter().take(MAX_LOG_FILES) {
        let Some(lines) = read_tail_lines(&dir.join(name)) else {
            continue;
        };
        for line in &lines {
            match level_of(line) {
                Some("ERROR") => scan.errors += 1,
                Some("WARN") => scan.warnings += 1,
                _ => {}
            }
            if line.contains(PANIC_MARKER) {
                scan.panics += 1;
            }
        }
        if have < tail {
            have += lines.len();
            older_first.push(lines);
        }
    }
    let mut all: Vec<String> = older_first.into_iter().rev().flatten().collect();
    let skip = all.len().saturating_sub(tail);
    scan.tail = all.split_off(skip);
    scan
}

/// The level word of a log line: the second field, after the timestamp.
fn level_of(line: &str) -> Option<&str> {
    line.split_whitespace().nth(1)
}

/// The lines in the last [`LOG_READ_BYTES`] of a file (a cut first line is
/// dropped). `None` when it cannot be read.
fn read_tail_lines(path: &Path) -> Option<Vec<String>> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(LOG_READ_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.take(LOG_READ_BYTES).read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    lines.retain(|line| !line.trim().is_empty());
    Some(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, text: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(name), text).unwrap();
    }

    #[test]
    fn a_missing_folder_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope");
        assert_eq!(inventory(&missing), Inventory::default());
        assert_eq!(scan_logs(&missing, 10), LogScan::default());
    }

    #[test]
    fn the_inventory_lists_names_and_sizes_not_contents() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "config.toml", "0123456789");
        write(dir.path(), "usage.json", "SECRET-USAGE");
        write(&dir.path().join("clipboard"), "a.png", "12345");
        write(&dir.path().join("clipboard"), "b.png", "123");
        write(&dir.path().join("plugins").join("mine"), "data.txt", "xx");

        let found = inventory(dir.path());
        assert!(found.exists && !found.truncated && found.error.is_none());
        let summary: Vec<(String, u64, Option<usize>)> = found
            .entries
            .iter()
            .map(|e| (e.name.clone(), e.bytes, e.files))
            .collect();
        assert_eq!(
            summary,
            [
                ("clipboard".to_owned(), 8, Some(2)),
                ("config.toml".to_owned(), 10, None),
                ("plugins".to_owned(), 2, Some(1)),
                ("usage.json".to_owned(), 12, None),
            ]
        );
        // The file names inside a sub-folder are not reported.
        let dump = format!("{found:?}");
        assert!(!dump.contains("a.png") && !dump.contains("SECRET"));
    }

    #[test]
    fn a_long_folder_is_cut() {
        let dir = tempfile::tempdir().unwrap();
        for n in 0..MAX_ENTRIES + 5 {
            write(dir.path(), &format!("f{n:04}"), "x");
        }
        let found = inventory(dir.path());
        assert_eq!(found.entries.len(), MAX_ENTRIES);
        assert!(found.truncated);
    }

    fn line(level: &str, text: &str) -> String {
        format!("2026-10-04T12:00:00.000000Z {level:>5} sevak::x: {text}\n")
    }

    #[test]
    fn the_tail_spans_files_and_levels_are_counted() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "sevak.2026-10-02.log",
            &(line("INFO", "old one") + &line("ERROR", "old error")),
        );
        write(
            dir.path(),
            "sevak.2026-10-03.log",
            &(line("WARN", "w") + &line("INFO", "yesterday")),
        );
        write(
            dir.path(),
            "sevak.2026-10-04.log",
            &(line("INFO", "start")
                + &line("ERROR", "thread panicked at src/x.rs:1:1: boom")
                + &line("WARN", "careful")
                + &line("INFO", "last")),
        );
        write(dir.path(), "notes.txt", "not a log");

        let scan = scan_logs(dir.path(), 5);
        assert_eq!(
            scan.files
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>(),
            [
                "sevak.2026-10-04.log",
                "sevak.2026-10-03.log",
                "sevak.2026-10-02.log"
            ]
        );
        assert_eq!(scan.errors, 2);
        assert_eq!(scan.warnings, 2);
        assert_eq!(scan.panics, 1);
        assert_eq!(scan.tail.len(), 5);
        assert!(scan.tail[0].contains("yesterday"));
        assert!(scan.tail[4].contains("last"));
    }

    #[test]
    fn a_big_log_is_read_from_its_end_without_its_cut_first_line() {
        let dir = tempfile::tempdir().unwrap();
        let mut text = String::new();
        let filler = line("INFO", &"x".repeat(200));
        while (text.len() as u64) < LOG_READ_BYTES + 5_000 {
            text.push_str(&filler);
        }
        text.push_str(&line("ERROR", "the end"));
        write(dir.path(), "sevak.2026-10-04.log", &text);
        let scan = scan_logs(dir.path(), 3);
        assert_eq!(scan.tail.len(), 3);
        assert!(scan.tail[2].contains("the end"));
        // Every line read is whole.
        assert!(scan.tail[0].starts_with("2026-"));
        assert_eq!(scan.errors, 1);
    }

    #[test]
    fn non_utf8_logs_are_read_lossily() {
        let dir = tempfile::tempdir().unwrap();
        let mut bytes = line("INFO", "caf").into_bytes();
        bytes.insert(bytes.len() - 1, 0xE9);
        fs::write(dir.path().join("sevak.2026-10-04.log"), bytes).unwrap();
        let scan = scan_logs(dir.path(), 5);
        assert_eq!(scan.tail.len(), 1);
    }
}
