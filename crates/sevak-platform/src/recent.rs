//! Recently used files, read from what the operating system already keeps.
//!
//! | OS | Source |
//! |---|---|
//! | Linux | `~/.local/share/recently-used.xbel` (freedesktop bookmarks) |
//! | Windows | the `.lnk` files in `%APPDATA%\Microsoft\Windows\Recent` |
//! | macOS | Spotlight (`mdfind`, files used in the last 14 days) |
//!
//! Everything is read locally; nothing is sent anywhere. Only files that still
//! exist on this computer are returned (never network paths, folders, app
//! bundles or, on macOS, anything under `~/Library`).
//!
//! macOS approximation: `mdls` per file is too slow, so the files `mdfind`
//! returns are ordered by the later of their access and modification times
//! rather than by `kMDItemLastUsedDate`.

#[cfg(target_os = "macos")]
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;
#[cfg(any(target_os = "linux", test))]
use std::time::{Duration, UNIX_EPOCH};

/// A file the user opened recently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentFile {
    pub path: PathBuf,
    /// The file name, for display and matching.
    pub name: String,
    /// When it was last used, as far as the OS says.
    pub last_used: Option<SystemTime>,
}

/// The most recently used files, newest first, at most `limit`. Reads files
/// (and on macOS runs `mdfind`); call it from a background thread.
pub fn recent_files(limit: usize) -> Vec<RecentFile> {
    if limit == 0 {
        return Vec::new();
    }
    finish(candidates(limit), limit)
}

#[cfg(target_os = "linux")]
fn candidates(_limit: usize) -> Vec<(PathBuf, Option<SystemTime>)> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let file = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".local/share"))
        .join("recently-used.xbel");
    match sevak_core::bounded_read::read_capped(&file, XBEL_MAX_BYTES) {
        Ok(bytes) => parse_xbel(&String::from_utf8_lossy(&bytes)),
        Err(_) => Vec::new(),
    }
}

#[cfg(target_os = "macos")]
fn candidates(limit: usize) -> Vec<(PathBuf, Option<SystemTime>)> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let output = std::process::Command::new("mdfind")
        .arg("-0")
        .arg("-onlyin")
        .arg(&home)
        .arg(MDFIND_QUERY)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let cap = limit.saturating_mul(10).max(MDFIND_MIN_CANDIDATES);
    parse_mdfind(&text, &home, cap)
        .into_iter()
        .map(|path| {
            let used = used_time(&path);
            (path, used)
        })
        .collect()
}

#[cfg(windows)]
fn candidates(limit: usize) -> Vec<(PathBuf, Option<SystemTime>)> {
    crate::windows::recent::candidates(limit)
}

/// Files Spotlight saw used in the last two weeks: not folders, not apps.
#[cfg(target_os = "macos")]
const MDFIND_QUERY: &str = "kMDItemLastUsedDate >= $time.today(-14) \
     && kMDItemContentTypeTree != 'public.folder' \
     && kMDItemContentTypeTree != 'com.apple.application'";

/// Files looked at (for their times) before the newest `limit` are chosen.
#[cfg(target_os = "macos")]
const MDFIND_MIN_CANDIDATES: usize = 500;

/// Largest `recently-used.xbel` read (the desktop trims it to a few hundred
/// entries; a huge file is not one).
#[cfg(target_os = "linux")]
const XBEL_MAX_BYTES: u64 = 8 * 1024 * 1024;

/// Later of the access and modification times: the closest cheap stand-in for
/// "last used".
#[cfg(target_os = "macos")]
fn used_time(path: &Path) -> Option<SystemTime> {
    let meta = std::fs::metadata(path).ok()?;
    match (meta.accessed().ok(), meta.modified().ok()) {
        (Some(a), Some(m)) => Some(a.max(m)),
        (a, m) => a.or(m),
    }
}

/// Keeps what still is a regular local file, newest first, one row per path.
pub(crate) fn finish(
    candidates: Vec<(PathBuf, Option<SystemTime>)>,
    limit: usize,
) -> Vec<RecentFile> {
    let mut seen = std::collections::HashSet::new();
    let mut files: Vec<RecentFile> = candidates
        .into_iter()
        .filter(|(path, _)| path.is_absolute() && seen.insert(path.clone()))
        .filter(|(path, _)| crate::netpath::refusal(&path.to_string_lossy(), false).is_none())
        .filter(|(path, _)| path.is_file())
        .filter_map(|(path, last_used)| {
            let name = path.file_name()?.to_string_lossy().into_owned();
            Some(RecentFile {
                path,
                name,
                last_used,
            })
        })
        .collect();
    // Stable: files without a time keep the OS's own order, after dated ones.
    files.sort_by_key(|f| std::cmp::Reverse(f.last_used));
    files.truncate(limit);
    files
}

/// Paths from `mdfind -0` output: absolute, outside `~/Library`, not hidden
/// and not inside an app bundle. At most `cap` of them.
#[cfg(any(target_os = "macos", all(test, unix)))]
pub(crate) fn parse_mdfind(output: &str, home: &std::path::Path, cap: usize) -> Vec<PathBuf> {
    let library = home.join("Library");
    output
        .split('\0')
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && !path.starts_with(&library))
        .filter(|path| {
            path.components().all(|c| {
                let part = c.as_os_str().to_string_lossy();
                !part.starts_with('.') && !part.to_ascii_lowercase().ends_with(".app")
            })
        })
        .take(cap)
        .collect()
}

/// The files of an XBEL document (`<bookmark href="file://..." modified=".."
/// visited="..">`) with the time each was last used. Only `file://` links on
/// this computer count; the rest are skipped. Never fails: a damaged document
/// yields what could be read.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn parse_xbel(xml: &str) -> Vec<(PathBuf, Option<SystemTime>)> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find("<bookmark") {
        rest = &rest[at + "<bookmark".len()..];
        // `<bookmarks` and `<bookmark:x` are other elements.
        if !rest.starts_with(|c: char| c.is_ascii_whitespace()) {
            continue;
        }
        let (tag, after) = split_tag(rest);
        rest = after;
        let attrs = attributes(tag);
        let get = |name: &str| {
            attrs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.as_str())
        };
        let Some(path) = get("href").and_then(file_uri_path) else {
            continue;
        };
        let used = ["visited", "modified", "added"]
            .iter()
            .filter_map(|name| get(name).and_then(parse_iso8601))
            .max();
        out.push((path, used));
    }
    out
}

/// The text up to the `>` that ends the tag (quotes respected) and the rest.
#[cfg(any(target_os = "linux", test))]
fn split_tag(text: &str) -> (&str, &str) {
    let mut quote: Option<char> = None;
    for (i, c) in text.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '"' | '\'') => quote = Some(c),
            (None, '>') => return (&text[..i], &text[i + 1..]),
            _ => {}
        }
    }
    (text, "")
}

/// `name="value"` pairs of a tag body, values with XML entities decoded.
#[cfg(any(target_os = "linux", test))]
fn attributes(tag: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = tag.trim_start();
    while let Some(eq) = rest.find('=') {
        let name = rest[..eq].trim().to_owned();
        let after = rest[eq + 1..].trim_start();
        let Some(quote) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            break;
        };
        let body = &after[1..];
        let Some(end) = body.find(quote) else {
            break;
        };
        out.push((name, xml_unescape(&body[..end])));
        rest = body[end + 1..].trim_start();
    }
    out
}

#[cfg(any(target_os = "linux", test))]
fn xml_unescape(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let decoded = after.find(';').filter(|end| *end <= 8).and_then(|end| {
            let entity = &after[..end];
            let c = match entity {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => entity
                    .strip_prefix("#x")
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                    .and_then(char::from_u32),
            };
            c.map(|c| (c, end))
        });
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The local path of a `file://` URI (`file:///home/a%20b` becomes
/// `/home/a b`); `None` for other schemes and for other computers.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn file_uri_path(uri: &str) -> Option<PathBuf> {
    let scheme = uri.get(..7).filter(|p| p.eq_ignore_ascii_case("file://"))?;
    let rest = &uri[scheme.len()..];
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') {
        return None;
    }
    Some(PathBuf::from(percent_decode(rest)?))
}

/// Decodes `%XX` escapes; `None` when the result is not valid UTF-8 or holds
/// a NUL.
#[cfg(any(target_os = "linux", test))]
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let text = String::from_utf8(out).ok()?;
    (!text.contains('\0')).then_some(text)
}

/// `2024-05-01T10:00:00Z` (fractions and `+hh:mm` offsets allowed) as a time.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn parse_iso8601(text: &str) -> Option<SystemTime> {
    let (date, time) = text.trim().split_once(['T', 't'])?;
    let mut d = date.splitn(3, '-');
    let year: i64 = d.next()?.parse().ok()?;
    let month: i64 = d.next()?.parse().ok()?;
    let day: i64 = d.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let (clock, offset) = match time.find(['Z', 'z', '+', '-']) {
        Some(at) => (&time[..at], &time[at..]),
        None => (time, "Z"),
    };
    let mut t = clock.splitn(3, ':');
    let hour: i64 = t.next()?.parse().ok()?;
    let minute: i64 = t.next()?.parse().ok()?;
    let second: i64 = t.next().unwrap_or("0").split('.').next()?.parse().ok()?;
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let offset_secs = match offset.chars().next()? {
        'Z' | 'z' => 0,
        sign @ ('+' | '-') => {
            let (h, m) = offset[1..].split_once(':').unwrap_or((&offset[1..], "0"));
            let value = h.parse::<i64>().ok()? * 3600 + m.parse::<i64>().ok()? * 60;
            if sign == '+' {
                value
            } else {
                -value
            }
        }
        _ => return None,
    };
    let secs = days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second
        - offset_secs;
    if secs >= 0 {
        UNIX_EPOCH.checked_add(Duration::from_secs(secs.unsigned_abs()))
    } else {
        UNIX_EPOCH.checked_sub(Duration::from_secs(secs.unsigned_abs()))
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date.
#[cfg(any(target_os = "linux", test))]
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::path::Path;

    use super::*;

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn parses_timestamps() {
        assert_eq!(parse_iso8601("1970-01-01T00:00:00Z"), Some(at(0)));
        assert_eq!(
            parse_iso8601("2024-05-01T10:00:00Z"),
            Some(at(1_714_557_600))
        );
        assert_eq!(
            parse_iso8601("2024-05-01T10:00:00.123456Z"),
            Some(at(1_714_557_600))
        );
        assert_eq!(
            parse_iso8601("2024-05-01T12:00:00+02:00"),
            Some(at(1_714_557_600))
        );
        assert_eq!(parse_iso8601("yesterday"), None);
        assert_eq!(parse_iso8601("2024-13-01T00:00:00Z"), None);
    }

    #[test]
    fn decodes_file_uris() {
        assert_eq!(
            file_uri_path("file:///home/a/My%20Doc%C3%A9.txt"),
            Some(PathBuf::from("/home/a/My Docé.txt"))
        );
        assert_eq!(
            file_uri_path("file://localhost/tmp/x"),
            Some(PathBuf::from("/tmp/x"))
        );
        assert_eq!(file_uri_path("https://example.com/a"), None);
        assert_eq!(file_uri_path("file://otherhost/share/x"), None);
        assert_eq!(file_uri_path("file:///bad%zz"), None);
        assert_eq!(file_uri_path("file:///bad%00name"), None);
    }

    const XBEL: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<xbel version="1.0" xmlns:bookmark="http://www.freedesktop.org/standards/desktop-bookmarks">
  <bookmark href="file:///home/a/one.txt" added="2024-05-01T10:00:00Z" modified="2024-05-02T10:00:00Z" visited="2024-05-03T10:00:00Z">
    <info><metadata owner="http://freedesktop.org"><bookmark:applications>
      <bookmark:application name="gedit" exec="&apos;gedit %u&apos;" modified="2024-05-03T10:00:00Z" count="1"/>
    </bookmark:applications></metadata></info>
  </bookmark>
  <bookmark href='file:///home/a/a%20%26%20b.pdf' modified="2024-04-01T00:00:00Z"/>
  <bookmark href="https://example.com/page" modified="2024-06-01T00:00:00Z"/>
  <bookmark href="file:///home/a/entity&amp;.txt"/>
</xbel>"#;

    #[test]
    fn reads_bookmarks_from_xbel() {
        assert_eq!(
            parse_xbel(XBEL),
            vec![
                (
                    PathBuf::from("/home/a/one.txt"),
                    parse_iso8601("2024-05-03T10:00:00Z")
                ),
                (
                    PathBuf::from("/home/a/a & b.pdf"),
                    parse_iso8601("2024-04-01T00:00:00Z")
                ),
                (PathBuf::from("/home/a/entity&.txt"), None),
            ]
        );
    }

    #[test]
    fn a_damaged_xbel_gives_what_is_readable() {
        let found = parse_xbel(r#"<bookmark href="file:///a/b.txt"/><bookmark href="file:///c"#);
        assert_eq!(found, vec![(PathBuf::from("/a/b.txt"), None)]);
        assert!(parse_xbel("").is_empty());
        assert!(parse_xbel("<bookmarks href=\"file:///x\"/>").is_empty());
    }

    // `mdfind` runs on macOS only, and `/Users/me/...` is not an absolute path
    // on Windows.
    #[cfg(unix)]
    #[test]
    fn mdfind_output_drops_library_apps_and_hidden_paths() {
        let home = Path::new("/Users/me");
        let out = "/Users/me/Documents/a.txt\0/Users/me/Library/Mail/x.emlx\0\
                   /Users/me/Applications/Foo.app/Contents/x\0/Users/me/.config/y\0\
                   relative/z\0/Users/me/Desktop/b.png\0";
        assert_eq!(
            parse_mdfind(out, home, 10),
            vec![
                PathBuf::from("/Users/me/Documents/a.txt"),
                PathBuf::from("/Users/me/Desktop/b.png")
            ]
        );
        assert_eq!(parse_mdfind(out, home, 1).len(), 1);
    }

    #[test]
    fn finish_keeps_existing_files_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old.txt");
        let new = dir.path().join("new.txt");
        let undated = dir.path().join("undated.txt");
        for p in [&old, &new, &undated] {
            std::fs::write(p, "x").unwrap();
        }
        let sub = dir.path().join("folder");
        std::fs::create_dir(&sub).unwrap();
        let found = finish(
            vec![
                (undated, None),
                (old.clone(), Some(at(100))),
                (dir.path().join("gone.txt"), Some(at(900))),
                (sub, Some(at(800))),
                (new.clone(), Some(at(200))),
                (old.clone(), Some(at(300))),
                (PathBuf::from("relative.txt"), Some(at(1))),
            ],
            10,
        );
        let names: Vec<_> = found.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["new.txt", "old.txt", "undated.txt"]);
        assert_eq!(found[1].last_used, Some(at(100)));
        let limited = finish(vec![(new, Some(at(1))), (old, Some(at(2)))], 1);
        assert_eq!(limited.len(), 1);
        assert_eq!(limited[0].name, "old.txt");
    }
}
