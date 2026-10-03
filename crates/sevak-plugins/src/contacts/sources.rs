//! Where contacts come from, besides the OS address book.
//!
//! - **vCard files**: the files and folders listed in `[contacts] vcard_files`.
//!   A folder contributes the `.vcf` files directly inside it.
//! - **Evolution Data Server** (Linux): the local address books' `contacts.db`.
//!   They are SQLite databases with the contacts as vCard text. Like the
//!   bookmarks plugin does for Firefox, the database and its write-ahead log are
//!   copied to a private temporary folder and read there, so Evolution's own
//!   files are never opened for writing or locked.
//!
//! Nothing here keeps the contacts: the caller owns them and holds them in
//! memory only.

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use sevak_platform::contacts::{parse_vcards, read_vcard_file};
use sevak_platform::Contact;

/// At most this many `.vcf` files are read from one folder.
const MAX_FILES_PER_FOLDER: usize = 500;
/// Evolution databases larger than this are not read.
const MAX_DATABASE_BYTES: u64 = 512 * 1024 * 1024;

/// The contacts in `path`: a `.vcf` file, or the `.vcf` files in a folder.
/// Anything unreadable contributes nothing.
pub fn read_vcard_path(path: &Path) -> Vec<Contact> {
    if path.is_dir() {
        let mut files: Vec<PathBuf> = fs::read_dir(path)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|file| {
                file.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("vcf"))
            })
            .collect();
        files.sort();
        files.truncate(MAX_FILES_PER_FOLDER);
        files
            .iter()
            .filter_map(|file| read_vcard_file(file))
            .flatten()
            .collect()
    } else {
        read_vcard_file(path).unwrap_or_default()
    }
}

/// The contacts in an Evolution Data Server `contacts.db`.
pub fn read_evolution(db: &Path) -> Result<Vec<Contact>, String> {
    read_evolution_in(db, &std::env::temp_dir())
}

/// Like [`read_evolution`], with the private copy made below `scratch_parent`.
fn read_evolution_in(db: &Path, scratch_parent: &Path) -> Result<Vec<Contact>, String> {
    let length = fs::metadata(db).map_err(|err| err.to_string())?.len();
    if length > MAX_DATABASE_BYTES {
        return Err(format!("{length} bytes is too large to read"));
    }
    let scratch = tempfile::Builder::new()
        .prefix("sevak-contacts-")
        .tempdir_in(scratch_parent)
        .map_err(|err| format!("cannot create a temporary folder: {err}"))?;
    let copy = scratch.path().join("contacts.db");
    fs::copy(db, &copy).map_err(|err| format!("cannot copy the address book: {err}"))?;
    let wal = sibling(db, "-wal");
    if wal.is_file() {
        // Best effort: without it the copy is merely a little out of date.
        let _ = fs::copy(&wal, sibling(&copy, "-wal"));
    }
    // The connection must be closed before `scratch` deletes the folder.
    let contacts = query_vcards(&copy);
    drop(scratch);
    contacts
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Reads the `vcard` column of every table that has one. Evolution names its
/// per-folder table after the folder, so the tables are found rather than
/// assumed.
fn query_vcards(db: &Path) -> Result<Vec<Contact>, String> {
    // Read-write on the private copy so SQLite can replay the write-ahead log.
    let conn = Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|err| err.to_string())?;
    let tables = names(&conn, "SELECT name FROM sqlite_master WHERE type = 'table'")
        .map_err(|err| err.to_string())?;

    let mut contacts = Vec::new();
    for table in tables {
        let quoted = format!("\"{}\"", table.replace('"', "\"\""));
        let columns = names(
            &conn,
            &format!("SELECT name FROM pragma_table_info({})", sql_text(&table)),
        )
        .unwrap_or_default();
        if !columns.iter().any(|column| column == "vcard") {
            continue;
        }
        let mut statement = conn
            .prepare(&format!("SELECT vcard FROM {quoted}"))
            .map_err(|err| err.to_string())?;
        let cards = statement
            .query_map([], |row| row.get::<_, Option<String>>(0))
            .map_err(|err| err.to_string())?;
        for card in cards.flatten().flatten() {
            contacts.extend(parse_vcards(&card));
        }
    }
    Ok(contacts)
}

fn names(conn: &Connection, sql: &str) -> rusqlite::Result<Vec<String>> {
    let mut statement = conn.prepare(sql)?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    Ok(rows.flatten().collect())
}

/// `text` as an SQL string literal.
fn sql_text(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARD: &str =
        "BEGIN:VCARD\nVERSION:3.0\nFN:Ada Lovelace\nEMAIL:ada@example.org\nUID:ada-1\nEND:VCARD\n";

    #[test]
    fn a_folder_contributes_its_vcf_files_only() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.vcf"), CARD).unwrap();
        fs::write(dir.path().join("B.VCF"), CARD.replace("ada-1", "ada-2")).unwrap();
        fs::write(dir.path().join("notes.txt"), CARD).unwrap();
        fs::create_dir(dir.path().join("nested.vcf")).unwrap();
        let contacts = read_vcard_path(dir.path());
        assert_eq!(contacts.len(), 2);
        assert_eq!(read_vcard_path(&dir.path().join("a.vcf")).len(), 1);
        assert!(read_vcard_path(&dir.path().join("missing.vcf")).is_empty());
    }

    #[test]
    fn reads_vcards_from_any_table_with_a_vcard_column() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("contacts.db");
        {
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE folders (folder_id TEXT PRIMARY KEY, folder_name TEXT);
                 CREATE TABLE \"it's-a-folder\" (uid TEXT PRIMARY KEY, vcard TEXT, bdata TEXT);",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO \"it's-a-folder\" (uid, vcard) VALUES ('1', ?1), ('2', NULL)",
                [CARD],
            )
            .unwrap();
        }
        let contacts = read_evolution_in(&db, dir.path()).unwrap();
        assert_eq!(contacts.len(), 1);
        assert_eq!(contacts[0].name, "Ada Lovelace");
        // The private copy is gone again.
        let leftovers = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("sevak-contacts-")
            })
            .count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn a_missing_or_foreign_database_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_evolution_in(&dir.path().join("none.db"), dir.path()).is_err());
        let junk = dir.path().join("junk.db");
        fs::write(&junk, b"this is not sqlite").unwrap();
        assert!(read_evolution_in(&junk, dir.path()).is_err());
    }
}
