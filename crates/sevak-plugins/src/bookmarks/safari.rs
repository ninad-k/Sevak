//! Bookmarks of Safari (macOS): `~/Library/Safari/Bookmarks.plist`.
//!
//! A binary property list (Safari may also be given an XML one; the `plist`
//! crate reads both). The root is a folder (`WebBookmarkTypeList`) whose
//! `Children` are folders, leaves (`WebBookmarkTypeLeaf`, with `URLString` and
//! `URIDictionary.title`) and proxies (`WebBookmarkTypeProxy`, the History
//! entry). The Reading List is a folder too, and its items carry a
//! `ReadingList` dictionary; neither is a bookmark, so both are skipped.
//!
//! macOS keeps the file behind Full Disk Access: opening it fails with
//! "Operation not permitted" until the user allows Sevak in System Settings.
//! [`read`] reports that as [`ReadError::PermissionDenied`] so the plugin can
//! explain it once, instead of treating it like a corrupt file.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use plist::{Dictionary, Value};

use super::{RawBookmark, ReadError, FOLDER_SEPARATOR};

/// The file inside Safari's folder.
pub const BOOKMARKS_FILE: &str = "Bookmarks.plist";
/// A bookmarks file larger than this is not Safari's.
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// Folders nested deeper than this are ignored (a malformed or hostile file
/// must not overflow the stack).
const MAX_DEPTH: usize = 32;

/// The bookmarks file of the Safari folder `root`.
///
/// It is returned whether or not it can be seen: without Full Disk Access even
/// `stat` on it may fail, which must not look like "Safari has no bookmarks".
pub fn bookmark_files(root: &Path) -> Vec<PathBuf> {
    vec![root.join(BOOKMARKS_FILE)]
}

fn classify(err: std::io::Error) -> ReadError {
    match err.kind() {
        // Safari never ran for this user: nothing to index.
        std::io::ErrorKind::NotFound => ReadError::Missing,
        // macOS answers "Operation not permitted" (EPERM) when Full Disk
        // Access is missing; Rust reports it, like EACCES, as this kind.
        std::io::ErrorKind::PermissionDenied => ReadError::PermissionDenied,
        _ => ReadError::Failed(err.to_string()),
    }
}

pub fn read(path: &Path) -> Result<Vec<RawBookmark>, ReadError> {
    // Open first: that is the call macOS refuses, and it is what we want to
    // tell apart from every other failure.
    let file = fs::File::open(path).map_err(classify)?;
    let length = file.metadata().map_err(classify)?.len();
    if length > MAX_FILE_BYTES {
        return Err(ReadError::Failed(format!(
            "{length} bytes is too large to index"
        )));
    }
    let bytes = {
        use std::io::Read;
        let mut bytes = Vec::with_capacity(length as usize);
        file.take(MAX_FILE_BYTES)
            .read_to_end(&mut bytes)
            .map_err(classify)?;
        bytes
    };
    parse(&bytes).map_err(ReadError::Failed)
}

pub fn parse(bytes: &[u8]) -> Result<Vec<RawBookmark>, String> {
    let root = Value::from_reader(Cursor::new(bytes)).map_err(|err| err.to_string())?;
    let root = root
        .as_dictionary()
        .ok_or_else(|| "the root of the property list is not a dictionary".to_owned())?;
    let mut out = Vec::new();
    // The root is unnamed; its children are Safari's top-level folders.
    visit_children(root, &mut Vec::new(), &mut out, 0);
    Ok(out)
}

fn text<'a>(dict: &'a Dictionary, key: &str) -> &'a str {
    dict.get(key).and_then(Value::as_string).unwrap_or("")
}

fn visit_children(
    folder: &Dictionary,
    path: &mut Vec<String>,
    out: &mut Vec<RawBookmark>,
    depth: usize,
) {
    if depth > MAX_DEPTH {
        return;
    }
    let Some(children) = folder.get("Children").and_then(Value::as_array) else {
        return;
    };
    for child in children.iter().filter_map(Value::as_dictionary) {
        match text(child, "WebBookmarkType") {
            "WebBookmarkTypeLeaf" => {
                // Reading List items are leaves with a `ReadingList` dictionary.
                if child.contains_key("ReadingList") {
                    continue;
                }
                let title = child
                    .get("URIDictionary")
                    .and_then(Value::as_dictionary)
                    .map_or("", |uri| text(uri, "title"));
                out.push(RawBookmark {
                    title: title.to_owned(),
                    url: text(child, "URLString").to_owned(),
                    folder: path.join(FOLDER_SEPARATOR),
                });
            }
            "WebBookmarkTypeList" => {
                let name = folder_name(child, depth);
                // The Reading List folder and its items are not bookmarks.
                let Some(name) = name else { continue };
                let named = !name.is_empty();
                if named {
                    path.push(name);
                }
                visit_children(child, path, out, depth + 1);
                if named {
                    path.pop();
                }
            }
            // History and other proxies.
            _ => {}
        }
    }
}

/// The folder's display name: `Some("")` for a folder without a name, `None`
/// for the Reading List. Safari's two top-level folders carry internal names.
fn folder_name(folder: &Dictionary, depth: usize) -> Option<String> {
    let title = text(folder, "Title").trim();
    if title == "com.apple.ReadingList" {
        return None;
    }
    if depth == 0 {
        match title {
            "BookmarksBar" => return Some("Favorites".to_owned()),
            "BookmarksMenu" => return Some("Bookmarks Menu".to_owned()),
            _ => {}
        }
    }
    Some(title.to_owned())
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    fn dict(entries: Vec<(&str, Value)>) -> Value {
        let mut dict = Dictionary::new();
        for (key, value) in entries {
            dict.insert(key.to_owned(), value);
        }
        Value::Dictionary(dict)
    }

    pub(crate) fn leaf(title: &str, url: &str) -> Value {
        dict(vec![
            ("WebBookmarkType", "WebBookmarkTypeLeaf".into()),
            ("URLString", url.into()),
            ("URIDictionary", dict(vec![("title", title.into())])),
            (
                "WebBookmarkUUID",
                "11111111-2222-3333-4444-555555555555".into(),
            ),
        ])
    }

    pub(crate) fn folder(title: &str, children: Vec<Value>) -> Value {
        dict(vec![
            ("WebBookmarkType", "WebBookmarkTypeList".into()),
            ("Title", title.into()),
            ("Children", Value::Array(children)),
        ])
    }

    /// A reading list item: a leaf with a `ReadingList` dictionary.
    fn reading_item(title: &str, url: &str) -> Value {
        dict(vec![
            ("WebBookmarkType", "WebBookmarkTypeLeaf".into()),
            ("URLString", url.into()),
            ("URIDictionary", dict(vec![("title", title.into())])),
            (
                "ReadingList",
                dict(vec![("DateAdded", "2024-01-01".into())]),
            ),
        ])
    }

    fn proxy(title: &str) -> Value {
        dict(vec![
            ("WebBookmarkType", "WebBookmarkTypeProxy".into()),
            ("Title", title.into()),
            ("WebBookmarkIdentifier", "History".into()),
        ])
    }

    /// The root list of a plist as Safari writes it.
    pub(crate) fn root(children: Vec<Value>) -> Value {
        dict(vec![
            ("WebBookmarkType", "WebBookmarkTypeList".into()),
            ("Title", "".into()),
            ("WebBookmarkFileVersion", 1_u64.into()),
            ("Children", Value::Array(children)),
        ])
    }

    /// `root` as a binary property list, the way Safari stores it.
    pub(crate) fn binary(root: &Value) -> Vec<u8> {
        let mut bytes = Vec::new();
        root.to_writer_binary(&mut bytes).unwrap();
        bytes
    }

    pub(crate) fn sample() -> Value {
        root(vec![
            proxy("History"),
            folder(
                "BookmarksBar",
                vec![
                    leaf("Rust", "https://www.rust-lang.org/"),
                    folder(
                        "Dev",
                        vec![
                            leaf("Docs.rs", "https://docs.rs/"),
                            folder("Deep", vec![leaf("Crates", "https://crates.io/")]),
                        ],
                    ),
                    leaf("Mail link", "mailto:a@b.test"),
                ],
            ),
            folder(
                "BookmarksMenu",
                vec![
                    leaf("Menu item", "http://menu.test/"),
                    leaf("Script", "javascript:alert(1)"),
                    leaf("Local", "file:///etc/hosts"),
                ],
            ),
            folder(
                "com.apple.ReadingList",
                vec![reading_item("Read later", "https://later.test/")],
            ),
        ])
    }

    fn found(bytes: &[u8]) -> Vec<(String, String, String)> {
        parse(bytes)
            .unwrap()
            .into_iter()
            .map(|b| (b.title, b.url, b.folder))
            .collect()
    }

    #[test]
    fn reads_nested_folders_with_their_paths() {
        let found = found(&binary(&sample()));
        let has = |title: &str, url: &str, folder: &str| {
            found.contains(&(title.to_owned(), url.to_owned(), folder.to_owned()))
        };
        assert!(has("Rust", "https://www.rust-lang.org/", "Favorites"));
        assert!(has("Docs.rs", "https://docs.rs/", "Favorites / Dev"));
        assert!(has(
            "Crates",
            "https://crates.io/",
            "Favorites / Dev / Deep"
        ));
        assert!(has("Menu item", "http://menu.test/", "Bookmarks Menu"));
    }

    #[test]
    fn the_reading_list_and_history_are_not_bookmarks() {
        let found = found(&binary(&sample()));
        assert!(!found.iter().any(|(_, url, _)| url.contains("later.test")));
        assert!(!found.iter().any(|(title, _, _)| title == "History"));
        // Non-web URLs are kept here and dropped by the merge step.
        assert!(found.iter().any(|(_, url, _)| url.starts_with("mailto:")));
    }

    #[test]
    fn a_reading_list_item_outside_the_reading_list_folder_is_skipped_too() {
        let tree = root(vec![folder(
            "BookmarksBar",
            vec![reading_item("Odd", "https://odd.test/")],
        )]);
        assert!(found(&binary(&tree)).is_empty());
    }

    #[test]
    fn xml_property_lists_work_too() {
        let mut xml = Vec::new();
        sample().to_writer_xml(&mut xml).unwrap();
        assert_eq!(found(&xml).len(), found(&binary(&sample())).len());
    }

    #[test]
    fn rejects_files_that_are_not_property_lists() {
        assert!(parse(b"{ this is not a plist").is_err());
        assert!(parse(b"").is_err());
        // A property list whose root is not a dictionary.
        let array = Value::Array(vec!["x".into()]);
        assert!(parse(&binary(&array)).is_err());
    }

    #[test]
    fn missing_keys_do_not_panic() {
        let tree = root(vec![
            dict(vec![("WebBookmarkType", "WebBookmarkTypeLeaf".into())]),
            dict(vec![("WebBookmarkType", "WebBookmarkTypeList".into())]),
            Value::String("stray".into()),
            folder("BookmarksBar", vec![]),
        ]);
        let found = found(&binary(&tree));
        // The leaf without a URL is read (and skipped later as not openable).
        assert_eq!(found, [("".into(), "".into(), "".into())]);
    }

    #[test]
    fn absurdly_deep_folders_are_cut_off() {
        let mut tree = leaf("Bottom", "https://bottom.test/");
        for i in 0..(MAX_DEPTH * 2) {
            tree = folder(&format!("f{i}"), vec![tree]);
        }
        let found = found(&binary(&root(vec![tree])));
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn read_tells_missing_from_unreadable_files() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(BOOKMARKS_FILE);
        assert!(matches!(read(&file), Err(ReadError::Missing)));

        fs::write(&file, binary(&sample())).unwrap();
        assert_eq!(read(&file).unwrap().len(), 7);

        fs::write(&file, b"not a plist").unwrap();
        assert!(matches!(read(&file), Err(ReadError::Failed(_))));
    }

    #[test]
    fn permission_errors_are_told_apart_from_other_failures() {
        use std::io::{Error, ErrorKind};
        assert!(matches!(
            classify(Error::from(ErrorKind::PermissionDenied)),
            ReadError::PermissionDenied
        ));
        assert!(matches!(
            classify(Error::from(ErrorKind::NotFound)),
            ReadError::Missing
        ));
        assert!(matches!(
            classify(Error::from(ErrorKind::UnexpectedEof)),
            ReadError::Failed(_)
        ));
        // macOS's EPERM (1) and EACCES (13) are both permission errors.
        #[cfg(unix)]
        for code in [1, 13] {
            assert!(matches!(
                classify(Error::from_raw_os_error(code)),
                ReadError::PermissionDenied
            ));
        }
    }

    #[test]
    fn the_bookmarks_file_is_always_listed() {
        let dir = Path::new("some").join("Safari");
        assert_eq!(bookmark_files(&dir), [dir.join("Bookmarks.plist")]);
    }
}
