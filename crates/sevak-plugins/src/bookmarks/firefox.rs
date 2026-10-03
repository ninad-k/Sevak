//! Bookmarks of the Firefox family (Firefox, LibreWolf, Zen): `places.sqlite`
//! in every profile listed in `profiles.ini`.
//!
//! The database is locked while the browser runs, and recent changes live in
//! the write-ahead log next to it. So the database and its `-wal` file are
//! copied into a private temporary folder, opened there and deleted again; the
//! browser's own files are only ever read.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use super::{RawBookmark, FOLDER_SEPARATOR, MAX_SOURCE_BYTES};

pub const PLACES: &str = "places.sqlite";

/// The `places.sqlite` of every profile below `root` (a folder holding
/// `profiles.ini`).
pub fn places_files(root: &Path) -> Vec<PathBuf> {
    let mut profiles: Vec<PathBuf> = Vec::new();
    if let Ok(ini) = fs::read_to_string(root.join("profiles.ini")) {
        profiles.extend(parse_profiles_ini(&ini, root));
    }
    // A profile can exist without being listed (copied in, or a stale ini).
    if let Ok(entries) = fs::read_dir(root.join("Profiles")) {
        let mut unlisted: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .collect();
        unlisted.sort();
        profiles.extend(unlisted);
    }

    let mut seen = HashSet::new();
    profiles
        .into_iter()
        .map(|dir| dir.join(PLACES))
        .filter(|file| file.is_file() && seen.insert(file.clone()))
        .collect()
}

/// Profile folders named by the `[ProfileN]` sections of a `profiles.ini`.
fn parse_profiles_ini(text: &str, root: &Path) -> Vec<PathBuf> {
    let mut profiles = Vec::new();
    let mut in_profile = false;
    let mut path: Option<String> = None;
    let mut relative = true;

    let mut finish = |path: &mut Option<String>, relative: &mut bool| {
        if let Some(path) = path.take() {
            profiles.push(if *relative {
                root.join(path)
            } else {
                PathBuf::from(path)
            });
        }
        *relative = true;
    };

    for line in text.lines() {
        let line = line.trim();
        if let Some(section) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            finish(&mut path, &mut relative);
            in_profile = section.starts_with("Profile");
        } else if in_profile {
            if let Some((key, value)) = line.split_once('=') {
                match key.trim() {
                    "Path" => path = Some(value.trim().to_owned()).filter(|p| !p.is_empty()),
                    "IsRelative" => relative = value.trim() != "0",
                    _ => {}
                }
            }
        }
    }
    finish(&mut path, &mut relative);
    profiles
}

/// `places.sqlite-wal` next to `places`.
pub fn wal_file(places: &Path) -> PathBuf {
    let mut name = places.as_os_str().to_owned();
    name.push("-wal");
    PathBuf::from(name)
}

pub fn read(places: &Path) -> Result<Vec<RawBookmark>, String> {
    read_in(places, &std::env::temp_dir())
}

/// Like [`read`], with the private copy made below `scratch_parent`.
fn read_in(places: &Path, scratch_parent: &Path) -> Result<Vec<RawBookmark>, String> {
    let length = fs::metadata(places).map_err(|err| err.to_string())?.len();
    if length > MAX_SOURCE_BYTES {
        return Err(format!("{length} bytes is too large to index"));
    }

    let scratch = tempfile::Builder::new()
        .prefix("sevak-places-")
        .tempdir_in(scratch_parent)
        .map_err(|err| format!("cannot create a temporary folder: {err}"))?;
    let copy = scratch.path().join(PLACES);
    fs::copy(places, &copy).map_err(|err| format!("cannot copy {}: {err}", places.display()))?;
    let wal = wal_file(places);
    if wal.is_file() {
        // Best effort: without it the copy is merely a little out of date.
        if let Err(err) = fs::copy(&wal, wal_file(&copy)) {
            tracing::debug!(%err, "could not copy the places write-ahead log");
        }
    }

    // The connection must be closed before `scratch` deletes the folder.
    let bookmarks = query(&copy);
    drop(scratch);
    bookmarks
}

/// Reads a database file (never the browser's own).
fn query(db: &Path) -> Result<Vec<RawBookmark>, String> {
    // Read-write on our private copy so SQLite can replay the WAL; nothing is
    // written back to the browser's files.
    let conn = Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|err| err.to_string())?;
    read_bookmarks(&conn).map_err(|err| err.to_string())
}

const BOOKMARK: i64 = 1;
const FOLDER: i64 = 2;

const ROOT_GUID: &str = "root________";
const TAGS_GUID: &str = "tags________";

struct Row {
    parent: i64,
    guid: String,
    title: String,
    url: Option<String>,
    kind: i64,
}

/// Firefox names its top-level folders `menu`, `toolbar`, ... internally.
fn root_folder_name(guid: &str) -> Option<&'static str> {
    match guid {
        "menu________" => Some("Bookmarks Menu"),
        "toolbar_____" => Some("Bookmarks Toolbar"),
        "unfiled_____" => Some("Other Bookmarks"),
        "mobile______" => Some("Mobile Bookmarks"),
        _ => None,
    }
}

fn read_bookmarks(conn: &Connection) -> rusqlite::Result<Vec<RawBookmark>> {
    let mut statement = conn.prepare(
        "SELECT b.id, b.type, b.parent, b.guid, b.title, p.url \
         FROM moz_bookmarks b LEFT JOIN moz_places p ON p.id = b.fk \
         WHERE b.type IN (1, 2) \
         ORDER BY b.parent, b.position, b.id",
    )?;
    let mut rows: HashMap<i64, Row> = HashMap::new();
    let mut order: Vec<i64> = Vec::new();
    let mut cursor = statement.query([])?;
    while let Some(row) = cursor.next()? {
        let id: i64 = row.get(0)?;
        rows.insert(
            id,
            Row {
                kind: row.get(1)?,
                parent: row.get(2)?,
                guid: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                title: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                url: row.get(5)?,
            },
        );
        order.push(id);
    }

    let mut out = Vec::new();
    for id in order {
        let row = &rows[&id];
        if row.kind != BOOKMARK {
            continue;
        }
        let Some(url) = row.url.as_deref() else {
            continue;
        };
        let Some(folder) = folder_path(&rows, row.parent) else {
            continue; // lives under the tags folder
        };
        out.push(RawBookmark {
            title: row.title.clone(),
            url: url.to_owned(),
            folder,
        });
    }
    Ok(out)
}

/// The folder names from the top-level folder down to `parent`, or `None` when
/// the bookmark is only a tag entry (tags are folders under `tags________`
/// that hold a second copy of every tagged bookmark).
fn folder_path(rows: &HashMap<i64, Row>, mut parent: i64) -> Option<String> {
    let mut names: Vec<String> = Vec::new();
    // Real trees are shallow; the limit only guards against a corrupt cycle.
    for _ in 0..64 {
        let Some(folder) = rows.get(&parent) else {
            break;
        };
        if folder.kind != FOLDER {
            break;
        }
        if folder.guid == TAGS_GUID {
            return None;
        }
        if folder.guid == ROOT_GUID {
            break;
        }
        let name = root_folder_name(&folder.guid).unwrap_or(folder.title.trim());
        if !name.is_empty() {
            names.push(name.to_owned());
        }
        parent = folder.parent;
    }
    names.reverse();
    Some(names.join(FOLDER_SEPARATOR))
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// Creates a `places.sqlite`-shaped database with the columns Sevak reads.
    pub fn create_places(path: &Path) -> Connection {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE moz_places (id INTEGER PRIMARY KEY, url LONGVARCHAR, title LONGVARCHAR);
             CREATE TABLE moz_bookmarks (
                 id INTEGER PRIMARY KEY, type INTEGER, fk INTEGER DEFAULT NULL,
                 parent INTEGER, position INTEGER, title LONGVARCHAR, guid TEXT);
             INSERT INTO moz_bookmarks (id, type, parent, position, title, guid) VALUES
                 (1, 2, 0, 0, '', 'root________'),
                 (2, 2, 1, 0, 'menu', 'menu________'),
                 (3, 2, 1, 1, 'toolbar', 'toolbar_____'),
                 (4, 2, 1, 2, 'tags', 'tags________'),
                 (5, 2, 1, 3, 'unfiled', 'unfiled_____'),
                 (6, 2, 1, 4, 'mobile', 'mobile______');",
        )
        .unwrap();
        conn
    }

    pub fn add_bookmark(conn: &Connection, id: i64, parent: i64, title: Option<&str>, url: &str) {
        conn.execute(
            "INSERT INTO moz_places (id, url) VALUES (?1, ?2)",
            (id, url),
        )
        .unwrap();
        conn.execute(
            "INSERT INTO moz_bookmarks (id, type, fk, parent, position, title) \
             VALUES (?1, 1, ?1, ?2, ?1, ?3)",
            (id, parent, title),
        )
        .unwrap();
    }

    pub fn add_folder(conn: &Connection, id: i64, parent: i64, title: &str) {
        conn.execute(
            "INSERT INTO moz_bookmarks (id, type, parent, position, title) VALUES (?1, 2, ?2, ?1, ?3)",
            (id, parent, title),
        )
        .unwrap();
    }

    fn fixture(path: &Path) -> Connection {
        let conn = create_places(path);
        add_bookmark(
            &conn,
            100,
            3,
            Some("Rust Lang"),
            "https://www.rust-lang.org/",
        );
        add_folder(&conn, 20, 2, "Dev");
        add_folder(&conn, 21, 20, "Tools");
        add_bookmark(&conn, 101, 20, Some("GitHub"), "https://github.com/");
        add_bookmark(&conn, 102, 21, None, "https://crates.io/");
        add_bookmark(&conn, 103, 5, Some("Unfiled"), "http://example.com/");
        // Tag entries duplicate a bookmark under tags________/<tag>.
        add_folder(&conn, 30, 4, "rust");
        add_bookmark(&conn, 104, 30, None, "https://www.rust-lang.org/");
        // A folder without a URL row, and a place: query bookmark.
        conn.execute(
            "INSERT INTO moz_bookmarks (id, type, parent, position, title) VALUES (105, 1, 3, 9, 'Orphan')",
            (),
        )
        .unwrap();
        add_bookmark(&conn, 106, 3, Some("Recent"), "place:sort=14&type=6");
        conn
    }

    fn summary(found: &[RawBookmark]) -> Vec<(String, String, String)> {
        found
            .iter()
            .map(|b| (b.title.clone(), b.url.clone(), b.folder.clone()))
            .collect()
    }

    #[test]
    fn reads_folders_titles_and_skips_tags() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join(PLACES);
        let _open = fixture(&db);
        let found = summary(&read(&db).unwrap());

        let expect = |title: &str, url: &str, folder: &str| {
            assert!(
                found.contains(&(title.into(), url.into(), folder.into())),
                "missing {title:?} in {found:#?}"
            );
        };
        expect(
            "Rust Lang",
            "https://www.rust-lang.org/",
            "Bookmarks Toolbar",
        );
        expect("GitHub", "https://github.com/", "Bookmarks Menu / Dev");
        expect("", "https://crates.io/", "Bookmarks Menu / Dev / Tools");
        expect("Unfiled", "http://example.com/", "Other Bookmarks");
        // The tag copy is not a second bookmark.
        assert_eq!(
            found
                .iter()
                .filter(|b| b.1 == "https://www.rust-lang.org/")
                .count(),
            1
        );
        // Scheme filtering is the plugin's job; the orphan has no URL at all.
        assert!(found.iter().any(|b| b.1.starts_with("place:")));
        assert!(!found.iter().any(|b| b.0 == "Orphan"));
    }

    #[test]
    fn replays_the_write_ahead_log_and_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join(PLACES);
        let conn = create_places(&db);
        // Like a running Firefox: WAL mode, connection (and lock) still open.
        let mode: String = conn
            .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
        conn.execute_batch("PRAGMA wal_autocheckpoint = 0").unwrap();
        add_bookmark(&conn, 200, 3, Some("Only in the WAL"), "https://wal.test/");
        assert!(wal_file(&db).is_file(), "fixture should have a -wal file");

        let found = read(&db).unwrap();
        assert!(
            found.iter().any(|b| b.url == "https://wal.test/"),
            "{found:?}"
        );

        // The original is untouched and the only extra files are Firefox's own.
        let mut names: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert!(names.iter().all(|n| n.starts_with(PLACES)), "{names:?}");
        drop(conn);
    }

    #[test]
    fn leaves_no_temporary_copy_behind() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join(PLACES);
        drop(fixture(&db));
        let scratch = tempfile::tempdir().unwrap();

        assert!(!read_in(&db, scratch.path()).unwrap().is_empty());
        assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);

        // Also when the database turns out to be unreadable.
        fs::write(&db, b"garbage").unwrap();
        assert!(read_in(&db, scratch.path()).is_err());
        assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
    }

    #[test]
    fn broken_databases_are_errors() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join(PLACES);
        fs::write(&db, b"this is not sqlite").unwrap();
        assert!(read(&db).is_err());
        // A valid SQLite file without the places tables.
        let other = dir.path().join("other.sqlite");
        Connection::open(&other)
            .unwrap()
            .execute_batch("CREATE TABLE t (x)")
            .unwrap();
        assert!(read(&other).is_err());
        assert!(read(&dir.path().join("missing.sqlite")).is_err());
    }

    #[test]
    fn profiles_ini_relative_absolute_and_install_sections() {
        let root = Path::new("ffroot");
        let ini = "\
[Install4F96D1932A9F858E]
Default=Profiles/abcd.default-release
Locked=1

[Profile1]
Name=work
IsRelative=0
Path=/data/work-profile

[Profile0]
Name=default-release
IsRelative=1
Path=Profiles/abcd.default-release
Default=1

[General]
StartWithLastProfile=1
Version=2
";
        assert_eq!(
            parse_profiles_ini(ini, root),
            [
                PathBuf::from("/data/work-profile"),
                root.join("Profiles/abcd.default-release"),
            ]
        );
    }

    #[test]
    fn finds_listed_and_unlisted_profiles_once() {
        let root = tempfile::tempdir().unwrap();
        let listed = root.path().join("Profiles").join("a.default");
        let unlisted = root.path().join("Profiles").join("b.extra");
        let elsewhere = tempfile::tempdir().unwrap();
        for dir in [&listed, &unlisted, elsewhere.path()] {
            fs::create_dir_all(dir).unwrap();
            fs::write(dir.join(PLACES), b"x").unwrap();
        }
        // Listed but without a database: ignored.
        fs::create_dir_all(root.path().join("Profiles").join("c.empty")).unwrap();
        let ini = format!(
            "[Profile0]\nName=a\nIsRelative=1\nPath=Profiles/a.default\n\
             [Profile1]\nName=c\nIsRelative=1\nPath=Profiles/c.empty\n\
             [Profile2]\nName=e\nIsRelative=0\nPath={}\n",
            elsewhere.path().display()
        );
        fs::write(root.path().join("profiles.ini"), ini).unwrap();

        let found = places_files(root.path());
        assert_eq!(
            found,
            [
                listed.join(PLACES),
                elsewhere.path().join(PLACES),
                unlisted.join(PLACES)
            ]
        );
    }

    #[test]
    fn a_missing_root_has_no_profiles() {
        assert!(places_files(Path::new("definitely-not-here")).is_empty());
    }
}
