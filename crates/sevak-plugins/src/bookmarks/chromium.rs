//! Bookmarks of the Chromium family (Chrome, Edge, Brave, Vivaldi, Chromium,
//! Opera): one `Bookmarks` JSON file per profile folder.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sevak_core::bounded_read::{read_to_string_capped, MAX_BOOKMARKS_BYTES};

use super::{RawBookmark, FOLDER_SEPARATOR};

/// The `Bookmarks` file of every profile below `root`.
///
/// Profiles are the sub-folders that contain such a file (`Default`,
/// `Profile 1`, ...). Opera keeps its file directly in the root folder.
pub fn bookmark_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let direct = root.join("Bookmarks");
    if direct.is_file() {
        files.push(direct);
    }
    if let Ok(entries) = fs::read_dir(root) {
        let mut profiles: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .map(|dir| dir.join("Bookmarks"))
            .filter(|file| file.is_file())
            .collect();
        profiles.sort();
        files.extend(profiles);
    }
    files
}

pub fn read(path: &Path) -> Result<Vec<RawBookmark>, String> {
    // The JSON is read whole; the limit is far above a real bookmarks file.
    let text = read_to_string_capped(path, MAX_BOOKMARKS_BYTES)
        .map_err(|err| format!("cannot read the bookmarks: {err}"))?;
    parse(&text)
}

#[derive(Deserialize)]
struct BookmarkFile {
    #[serde(default)]
    roots: HashMap<String, serde_json::Value>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Node {
    #[serde(rename = "type")]
    kind: String,
    name: String,
    url: String,
    children: Vec<Node>,
}

/// The roots Chromium writes, in the order people expect to see them.
const ROOT_ORDER: [(&str, &str); 3] = [
    ("bookmark_bar", "Bookmarks bar"),
    ("other", "Other bookmarks"),
    ("synced", "Mobile bookmarks"),
];

pub fn parse(json: &str) -> Result<Vec<RawBookmark>, String> {
    let file: BookmarkFile = serde_json::from_str(json).map_err(|err| err.to_string())?;
    let mut roots = file.roots;
    let mut out = Vec::new();

    let mut visit_root = |value: serde_json::Value, fallback_name: &str| {
        // Entries such as `sync_transaction_version` are plain strings.
        let Ok(root) = serde_json::from_value::<Node>(value) else {
            return;
        };
        let name = if root.name.trim().is_empty() {
            fallback_name
        } else {
            root.name.trim()
        };
        let mut path = vec![name.to_owned()];
        collect(&root.children, &mut path, &mut out);
    };

    for (key, fallback) in ROOT_ORDER {
        if let Some(value) = roots.remove(key) {
            visit_root(value, fallback);
        }
    }
    let mut rest: Vec<_> = roots.into_iter().collect();
    rest.sort_by(|a, b| a.0.cmp(&b.0));
    for (key, value) in rest {
        visit_root(value, &key);
    }
    Ok(out)
}

fn collect(nodes: &[Node], path: &mut Vec<String>, out: &mut Vec<RawBookmark>) {
    for node in nodes {
        match node.kind.as_str() {
            "url" => out.push(RawBookmark {
                title: node.name.clone(),
                url: node.url.clone(),
                folder: path.join(FOLDER_SEPARATOR),
            }),
            "folder" => {
                path.push(node.name.trim().to_owned());
                collect(&node.children, path, out);
                path.pop();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    pub const FIXTURE: &str = r#"{
      "checksum": "abc",
      "roots": {
        "bookmark_bar": {
          "type": "folder", "name": "Bookmarks bar", "id": "1",
          "children": [
            {"type": "url", "name": "Rust Lang", "url": "https://www.rust-lang.org/", "id": "5"},
            {"type": "folder", "name": "Dev", "children": [
              {"type": "url", "name": "GitHub", "url": "https://github.com/", "id": "6"},
              {"type": "folder", "name": "Tools", "children": [
                {"type": "url", "name": "crates.io", "url": "https://crates.io/"}
              ]}
            ]},
            {"type": "url", "name": "Bookmarklet", "url": "javascript:alert(1)"}
          ]
        },
        "other": {
          "type": "folder", "name": "Other bookmarks",
          "children": [{"type": "url", "name": "Example", "url": "http://example.com/a?b=1#c"}]
        },
        "synced": {"type": "folder", "name": "Mobile bookmarks", "children": []},
        "sync_transaction_version": "2"
      },
      "version": 1
    }"#;

    #[test]
    fn parses_nested_folders_in_root_order() {
        let found = parse(FIXTURE).unwrap();
        let summary: Vec<(&str, &str)> = found
            .iter()
            .map(|b| (b.title.as_str(), b.folder.as_str()))
            .collect();
        assert_eq!(
            summary,
            [
                ("Rust Lang", "Bookmarks bar"),
                ("GitHub", "Bookmarks bar / Dev"),
                ("crates.io", "Bookmarks bar / Dev / Tools"),
                // Scheme filtering is the plugin's job, not the parser's.
                ("Bookmarklet", "Bookmarks bar"),
                ("Example", "Other bookmarks"),
            ]
        );
    }

    #[test]
    fn edge_style_root_names_are_kept() {
        let json = r#"{"roots":{"bookmark_bar":{"type":"folder","name":"Favorites bar","children":[
            {"type":"url","name":"A","url":"https://a.test/"}]}}}"#;
        assert_eq!(parse(json).unwrap()[0].folder, "Favorites bar");
    }

    #[test]
    fn nameless_roots_fall_back() {
        let json = r#"{"roots":{"other":{"type":"folder","children":[
            {"type":"url","name":"A","url":"https://a.test/"}]}}}"#;
        assert_eq!(parse(json).unwrap()[0].folder, "Other bookmarks");
    }

    #[test]
    fn malformed_or_empty_files_are_errors_or_empty() {
        assert!(parse("not json").is_err());
        assert!(parse("{}").unwrap().is_empty());
        assert!(parse(r#"{"roots": {}}"#).unwrap().is_empty());
        // Unknown node kinds and missing fields are tolerated.
        let odd = r#"{"roots":{"other":{"type":"folder","children":[
            {"type":"mystery","name":"x"}, {"type":"url"}]}}}"#;
        let found = parse(odd).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].url, "");
    }

    #[test]
    fn finds_every_profile_with_a_bookmarks_file() {
        let root = tempfile::tempdir().unwrap();
        for profile in ["Default", "Profile 2", "Guest Profile"] {
            fs::create_dir_all(root.path().join(profile)).unwrap();
        }
        fs::write(root.path().join("Default").join("Bookmarks"), FIXTURE).unwrap();
        fs::write(root.path().join("Profile 2").join("Bookmarks"), FIXTURE).unwrap();
        // Not a profile: a plain file with the name, and a folder without one.
        fs::write(root.path().join("Local State"), "{}").unwrap();

        let files = bookmark_files(root.path());
        assert_eq!(
            files,
            [
                root.path().join("Default").join("Bookmarks"),
                root.path().join("Profile 2").join("Bookmarks"),
            ]
        );
    }

    #[test]
    fn opera_keeps_the_file_in_the_root() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("Bookmarks"), FIXTURE).unwrap();
        assert_eq!(bookmark_files(root.path()), [root.path().join("Bookmarks")]);
        assert_eq!(read(&root.path().join("Bookmarks")).unwrap().len(), 5);
    }
}
