//! The opt-in gallery: a curated list of workflows and script plugins that can
//! be installed with one click.
//!
//! Nothing here runs by itself. The settings window asks for the index when the
//! user presses "Load gallery" and for a package when they press "Install";
//! those two requests are the only network use (see the README's "Privacy and
//! updates"). Each package is checked against the SHA-256 the index lists
//! before anything is written, unpacked with strict path rules, and installed
//! into the workflows or plugins folder as a *new, unapproved* folder: the
//! normal Allow dialog still decides whether it may run.
//!
//! The index is a JSON file, `gallery/index.json` in the Sevak repository:
//!
//! ```json
//! {"format": 1, "name": "Sevak gallery", "entries": [
//!   {"id": "search-docs", "kind": "workflow", "name": "Search docs",
//!    "description": "...", "author": "...", "version": "1.0",
//!    "source": "https://raw.githubusercontent.com/.../search-docs.zip",
//!    "sha256": "<64 hex digits>"}
//! ]}
//! ```
//!
//! The download and the checksum are the generic [`crate::net::fetch_https`]
//! and [`sevak_core::checksum`], shared with the theme gallery; only [`Kind`]
//! is specific to workflows and script plugins, and entries of kinds this Sevak
//! does not know are skipped rather than failing the index.

use std::collections::HashSet;
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::model::{valid_folder_name, Workflow, FILE as WORKFLOW_FILE};
use super::validate::error_summary;
use crate::net::{fetch_https, verify_sha256};
use crate::script::{Manifest, MANIFEST_FILE as PLUGIN_FILE};

/// Where the index lives. The settings window fetches it only on request.
pub const INDEX_URL: &str =
    "https://raw.githubusercontent.com/ninad-k/Sevak/main/gallery/index.json";
/// The index format this Sevak reads.
pub const INDEX_FORMAT: u32 = 1;

pub const MAX_INDEX_BYTES: usize = 512 * 1024;
pub const MAX_PACKAGE_BYTES: usize = 5 * 1024 * 1024;
const MAX_FILES: usize = 200;
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_UNPACKED_BYTES: u64 = 10 * 1024 * 1024;
const MAX_PATH_BYTES: usize = 200;

/// What an entry installs as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A folder for `<config dir>/workflows`.
    Workflow,
    /// A script plugin folder for `<config dir>/plugins`.
    Plugin,
}

impl Kind {
    /// The file that makes a folder one of these.
    pub fn manifest_file(self) -> &'static str {
        match self {
            Self::Workflow => WORKFLOW_FILE,
            Self::Plugin => PLUGIN_FILE,
        }
    }
}

/// One installable thing in the index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Unique, lower case letters, digits and dashes.
    pub id: String,
    pub kind: Kind,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub version: String,
    /// The zip package: an `https://` URL.
    pub source: String,
    /// SHA-256 of the zip, as 64 hex digits.
    pub sha256: String,
    /// Where to read more (shown as text, never opened automatically).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// The folder the package installs to; the `id` when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
}

impl Entry {
    /// The folder name this entry installs to.
    pub fn folder_name(&self) -> &str {
        self.folder.as_deref().unwrap_or(&self.id)
    }
}

/// A parsed index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Index {
    pub name: String,
    pub entries: Vec<Entry>,
    /// Entries that were left out (unknown kind, invalid), with the reason.
    pub skipped: Vec<String>,
}

#[derive(Deserialize)]
struct RawIndex {
    format: u32,
    #[serde(default)]
    name: String,
    #[serde(default)]
    entries: Vec<serde_json::Value>,
}

/// Parses the index file. A bad entry is skipped, not fatal: one typo should
/// not take the whole gallery away.
pub fn parse_index(text: &str) -> Result<Index, String> {
    let raw: RawIndex = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|err| format!("the gallery index is not valid: {err}"))?;
    if raw.format != INDEX_FORMAT {
        return Err(format!(
            "the gallery uses index format {}, but this Sevak reads format {INDEX_FORMAT}; \
             update Sevak",
            raw.format
        ));
    }
    let mut entries: Vec<Entry> = Vec::new();
    let mut skipped = Vec::new();
    let mut ids: HashSet<String> = HashSet::new();
    for (position, value) in raw.entries.into_iter().enumerate() {
        let label = value
            .get("id")
            .and_then(|id| id.as_str())
            .map_or_else(|| format!("entry {}", position + 1), str::to_owned);
        let mut entry: Entry = match serde_json::from_value(value) {
            Ok(entry) => entry,
            Err(err) => {
                skipped.push(format!("{label}: {err}"));
                continue;
            }
        };
        entry.sha256 = entry.sha256.trim().to_ascii_lowercase();
        if let Err(reason) = check_entry(&entry) {
            skipped.push(format!("{label}: {reason}"));
            continue;
        }
        if !ids.insert(entry.id.clone()) {
            skipped.push(format!("{label}: the id is used twice"));
            continue;
        }
        entries.push(entry);
    }
    Ok(Index {
        name: raw.name,
        entries,
        skipped,
    })
}

fn check_entry(entry: &Entry) -> Result<(), String> {
    let valid_id = |id: &str| {
        !id.is_empty()
            && id.len() <= 48
            && id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    };
    if !valid_id(&entry.id) {
        return Err("the id may only use a-z, 0-9 and -".to_owned());
    }
    if let Some(folder) = &entry.folder {
        if !valid_folder_name(folder) {
            return Err("the folder name is not valid".to_owned());
        }
    }
    if entry.name.trim().is_empty() {
        return Err("the name is empty".to_owned());
    }
    if !entry.source.to_ascii_lowercase().starts_with("https://") {
        return Err("the source must be an https:// address".to_owned());
    }
    if entry.sha256.len() != 64 || !entry.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("sha256 must be 64 hex digits".to_owned());
    }
    Ok(())
}

/// Fetches and parses the index at [`INDEX_URL`].
pub fn fetch_index() -> Result<Index, String> {
    let body = fetch_https(INDEX_URL, MAX_INDEX_BYTES)?;
    let text = String::from_utf8(body).map_err(|_| "the gallery index is not text".to_owned())?;
    parse_index(&text)
}

/// Where installs go.
#[derive(Debug, Clone, Copy)]
pub struct Dirs<'a> {
    pub workflows: &'a Path,
    pub plugins: &'a Path,
}

impl Dirs<'_> {
    fn root(&self, kind: Kind) -> &Path {
        match kind {
            Kind::Workflow => self.workflows,
            Kind::Plugin => self.plugins,
        }
    }

    /// Whether the folder for `entry` already exists.
    pub fn is_installed(&self, entry: &Entry) -> bool {
        self.root(entry.kind).join(entry.folder_name()).exists()
    }
}

/// What an install put on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub kind: Kind,
    pub folder: String,
    pub path: PathBuf,
    pub files: usize,
}

/// Downloads `entry` and installs it (see [`install_bytes`]).
pub fn install(entry: &Entry, dirs: &Dirs<'_>) -> Result<Installed, String> {
    let bytes = fetch_https(&entry.source, MAX_PACKAGE_BYTES)?;
    install_bytes(entry, &bytes, dirs)
}

/// Verifies `bytes` against the entry's checksum, checks the package and
/// unpacks it into a new folder below the workflows or plugins folder. An
/// existing folder of that name is never touched.
pub fn install_bytes(entry: &Entry, bytes: &[u8], dirs: &Dirs<'_>) -> Result<Installed, String> {
    verify_sha256(bytes, &entry.sha256)?;
    let folder = entry.folder_name();
    let root = dirs.root(entry.kind);
    let target = root.join(folder);
    if target.exists() {
        return Err(format!(
            "\"{folder}\" is already installed. To reinstall it, remove that folder first: {}",
            target.display()
        ));
    }
    let package = Package::read(bytes, folder, entry.kind)?;
    check_content(&package, entry.kind, folder)?;

    fs::create_dir_all(root).map_err(|err| format!("could not create the folder: {err}"))?;
    let staging = root.join(format!(".installing-{folder}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&staging);
    let result = write_package(&package, &staging).and_then(|()| {
        fs::rename(&staging, &target).map_err(|err| format!("could not finish the install: {err}"))
    });
    if let Err(err) = result {
        let _ = fs::remove_dir_all(&staging);
        return Err(err);
    }
    Ok(Installed {
        kind: entry.kind,
        folder: folder.to_owned(),
        path: target,
        files: package.files.len(),
    })
}

/// A package's files, checked and held in memory.
struct Package {
    /// `(path relative to the folder, contents, executable)`.
    files: Vec<(String, Vec<u8>, bool)>,
}

impl Package {
    fn read(bytes: &[u8], folder: &str, kind: Kind) -> Result<Self, String> {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|err| format!("the package is not a valid zip file: {err}"))?;
        if archive.len() > MAX_FILES * 2 {
            return Err("the package has too many entries".to_owned());
        }
        let mut files = Vec::new();
        let mut total: u64 = 0;
        // The package's single top-level folder, when it has one.
        let mut top: Option<String> = None;
        let mut rooted = 0usize;
        for index in 0..archive.len() {
            let mut entry = archive
                .by_index(index)
                .map_err(|err| format!("the package is damaged: {err}"))?;
            let name = clean_path(entry.name())?;
            if entry.is_dir() {
                continue;
            }
            if let Some(mode) = entry.unix_mode() {
                // 0o120000: a symbolic link, which could point anywhere.
                if mode & 0o170000 == 0o120000 {
                    return Err("the package contains a link, which is not allowed".to_owned());
                }
            }
            let executable = entry.unix_mode().is_some_and(|mode| mode & 0o111 != 0);
            if files.len() >= MAX_FILES {
                return Err("the package has too many files".to_owned());
            }
            if entry.size() > MAX_FILE_BYTES {
                return Err("a file in the package is too large".to_owned());
            }
            match name.split_once('/') {
                Some((first, _)) => match &top {
                    None => top = Some(first.to_owned()),
                    Some(seen) if seen == first => {}
                    Some(_) => {
                        return Err(
                            "the package must contain a single folder, or only files".to_owned()
                        )
                    }
                },
                None => rooted += 1,
            }
            let mut data = Vec::new();
            (&mut entry)
                .take(MAX_FILE_BYTES + 1)
                .read_to_end(&mut data)
                .map_err(|err| format!("the package is damaged: {err}"))?;
            if data.len() as u64 > MAX_FILE_BYTES {
                return Err("a file in the package is too large".to_owned());
            }
            total += data.len() as u64;
            if total > MAX_UNPACKED_BYTES {
                return Err("the package is too large when unpacked".to_owned());
            }
            files.push((name, data, executable));
        }
        if files.is_empty() {
            return Err("the package is empty".to_owned());
        }
        if top.is_some() && rooted > 0 {
            return Err("the package must contain a single folder, or only files".to_owned());
        }
        if let Some(top) = &top {
            if top != folder {
                return Err(format!(
                    "the package's folder is called \"{top}\", but the gallery says \"{folder}\""
                ));
            }
            for (name, _, _) in &mut files {
                *name = name[top.len() + 1..].to_owned();
            }
        }
        let manifest = kind.manifest_file();
        if !files.iter().any(|(name, _, _)| name == manifest) {
            return Err(format!("the package has no {manifest}"));
        }
        let mut seen = HashSet::new();
        for (name, _, _) in &files {
            if !seen.insert(name.to_lowercase()) {
                return Err(format!("the package lists {name} twice"));
            }
        }
        Ok(Self { files })
    }

    fn file(&self, name: &str) -> Option<&[u8]> {
        self.files
            .iter()
            .find(|(path, _, _)| path == name)
            .map(|(_, data, _)| data.as_slice())
    }
}

/// A path from a zip entry as forward-slash components, or why it is refused:
/// no leading `/`, drive letter, `..`, empty or hidden-looking tricks.
fn clean_path(raw: &str) -> Result<String, String> {
    let name = raw.replace('\\', "/");
    if name.len() > MAX_PATH_BYTES {
        return Err("a path in the package is too long".to_owned());
    }
    if name.starts_with('/') || name.contains(':') || name.contains('\0') {
        return Err("a path in the package is not allowed".to_owned());
    }
    let mut parts = Vec::new();
    for part in name.split('/') {
        match part {
            "" | "." => continue,
            ".." => return Err("a path in the package leaves its folder".to_owned()),
            part if part.chars().any(char::is_control) => {
                return Err("a path in the package is not allowed".to_owned())
            }
            // Windows trims a trailing dot or space, so `a.` and `a` collide.
            part if part.ends_with('.') || part.ends_with(' ') => {
                return Err("a path in the package is not allowed".to_owned())
            }
            part => parts.push(part),
        }
    }
    Ok(parts.join("/"))
}

/// Checks that the package's manifest is one Sevak can load.
fn check_content(package: &Package, kind: Kind, folder: &str) -> Result<(), String> {
    let manifest = kind.manifest_file();
    let text = package
        .file(manifest)
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .ok_or_else(|| format!("{manifest} is not text"))?;
    match kind {
        Kind::Workflow => {
            let workflow = Workflow::from_toml(text)
                .map_err(|err| format!("the package's workflow is not valid: {err}"))?;
            if let Some(summary) = error_summary(&workflow.validate()) {
                return Err(format!("the package's workflow is not valid: {summary}"));
            }
        }
        Kind::Plugin => {
            Manifest::parse(text, folder)
                .map_err(|err| format!("the package's plugin is not valid: {err}"))?;
        }
    }
    Ok(())
}

fn write_package(package: &Package, dir: &Path) -> Result<(), String> {
    for (name, data, executable) in &package.files {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| format!("could not write a file: {err}"))?;
        }
        let mut file =
            fs::File::create(&path).map_err(|err| format!("could not write a file: {err}"))?;
        file.write_all(data)
            .map_err(|err| format!("could not write a file: {err}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // Only the owner's execute bit survives: never setuid and friends.
            let mode = if *executable { 0o755 } else { 0o644 };
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(mode));
        }
        #[cfg(not(unix))]
        let _ = executable;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use zip::write::SimpleFileOptions;

    use super::*;
    use crate::net::sha256_hex;

    fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(&mut out);
        for (name, data) in files {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap();
        out.into_inner()
    }

    const WORKFLOW: &str = r#"
        name = "Docs"
        [[node]]
        id = "k"
        type = "keyword"
        keyword = "docs"
        [[node]]
        id = "o"
        type = "open_url"
        url = "https://example.com/?q={query}"
        [[connection]]
        from = "k"
        to = "o"
    "#;

    fn entry_for(bytes: &[u8], kind: Kind, id: &str) -> Entry {
        Entry {
            id: id.to_owned(),
            kind,
            name: id.to_owned(),
            description: String::new(),
            author: String::new(),
            version: String::new(),
            source: "https://example.com/p.zip".to_owned(),
            sha256: sha256_hex(bytes),
            homepage: None,
            folder: None,
        }
    }

    struct Roots {
        _tmp: tempfile::TempDir,
        workflows: PathBuf,
        plugins: PathBuf,
    }

    fn roots() -> Roots {
        let tmp = tempfile::tempdir().unwrap();
        Roots {
            workflows: tmp.path().join("workflows"),
            plugins: tmp.path().join("plugins"),
            _tmp: tmp,
        }
    }

    impl Roots {
        fn dirs(&self) -> Dirs<'_> {
            Dirs {
                workflows: &self.workflows,
                plugins: &self.plugins,
            }
        }
    }

    // ---- the index ----------------------------------------------------

    fn good_entry(id: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id, "kind": "workflow", "name": id, "description": "d", "author": "a",
            "version": "1", "source": "https://example.com/x.zip",
            "sha256": "A".repeat(64)
        })
    }

    #[test]
    fn parses_an_index_and_skips_bad_entries() {
        let text = serde_json::json!({
            "format": 1, "name": "Test gallery",
            "entries": [
                good_entry("one"),
                {"id": "future", "kind": "theme", "name": "T", "source": "https://x.test/t.zip", "sha256": "0".repeat(64)},
                {"id": "http", "kind": "plugin", "name": "T", "source": "http://x.test/t.zip", "sha256": "0".repeat(64)},
                {"id": "short-hash", "kind": "plugin", "name": "T", "source": "https://x.test/t.zip", "sha256": "abc"},
                {"id": "Bad Id", "kind": "plugin", "name": "T", "source": "https://x.test/t.zip", "sha256": "0".repeat(64)},
                good_entry("one"),
                {"kind": "plugin"},
                "not an object",
                {"id": "plug", "kind": "plugin", "name": "P", "source": "HTTPS://x.test/p.zip", "sha256": "f".repeat(64), "folder": "plug-dir"}
            ]
        })
        .to_string();
        let index = parse_index(&text).unwrap();
        assert_eq!(index.name, "Test gallery");
        let ids: Vec<_> = index.entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["one", "plug"]);
        // Hashes are normalized to lower case.
        assert_eq!(index.entries[0].sha256, "a".repeat(64));
        assert_eq!(index.entries[1].folder_name(), "plug-dir");
        assert_eq!(index.entries[0].folder_name(), "one");
        assert_eq!(index.skipped.len(), 7, "{:?}", index.skipped);
        assert!(index.skipped.iter().any(|s| s.starts_with("future")));
    }

    #[test]
    fn the_index_format_and_json_are_checked() {
        let err = parse_index(r#"{"format": 2, "entries": []}"#).unwrap_err();
        assert!(err.contains("format 2"), "{err}");
        assert!(parse_index("not json").is_err());
        assert!(parse_index(r#"{"entries": []}"#).is_err());
        let empty = parse_index("\u{feff}{\"format\": 1}").unwrap();
        assert!(empty.entries.is_empty());
    }

    // ---- installing ---------------------------------------------------

    #[test]
    fn installs_a_workflow_package_into_a_new_folder() {
        let r = roots();
        let bytes = zip_of(&[
            ("docs/workflow.toml", WORKFLOW.as_bytes()),
            ("docs/scripts/helper.py", b"print('hi')"),
        ]);
        let entry = entry_for(&bytes, Kind::Workflow, "docs");
        assert!(!r.dirs().is_installed(&entry));
        let done = install_bytes(&entry, &bytes, &r.dirs()).unwrap();
        assert_eq!(done.folder, "docs");
        assert_eq!(done.files, 2);
        assert!(r.workflows.join("docs/workflow.toml").is_file());
        assert!(r.workflows.join("docs/scripts/helper.py").is_file());
        assert!(r.dirs().is_installed(&entry));
        // No staging folder is left behind.
        let left: Vec<_> = fs::read_dir(&r.workflows)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, ["docs"]);
        // The folder is a loadable, unapproved workflow.
        let text = fs::read_to_string(r.workflows.join("docs/workflow.toml")).unwrap();
        assert!(Workflow::from_toml(&text).unwrap().is_valid());
    }

    #[test]
    fn a_package_may_hold_its_files_at_the_top() {
        let r = roots();
        let bytes = zip_of(&[("workflow.toml", WORKFLOW.as_bytes())]);
        let entry = entry_for(&bytes, Kind::Workflow, "flat");
        install_bytes(&entry, &bytes, &r.dirs()).unwrap();
        assert!(r.workflows.join("flat/workflow.toml").is_file());
    }

    #[test]
    fn installs_a_script_plugin_into_the_plugins_folder() {
        let r = roots();
        let manifest = "protocol = 1\nkeyword = \"hi\"\ncommand = [\"python3\", \"main.py\"]\n";
        let bytes = zip_of(&[
            ("hello/plugin.toml", manifest.as_bytes()),
            ("hello/main.py", b"print(1)"),
        ]);
        let entry = entry_for(&bytes, Kind::Plugin, "hello");
        let done = install_bytes(&entry, &bytes, &r.dirs()).unwrap();
        assert_eq!(done.path, r.plugins.join("hello"));
        assert!(!r.workflows.exists(), "a plugin is not a workflow");
    }

    #[test]
    fn a_tampered_download_is_discarded_before_anything_is_written() {
        let r = roots();
        let good = zip_of(&[("docs/workflow.toml", WORKFLOW.as_bytes())]);
        let entry = entry_for(&good, Kind::Workflow, "docs");
        let evil = zip_of(&[("docs/workflow.toml", b"name = \"evil\"")]);
        let err = install_bytes(&entry, &evil, &r.dirs()).unwrap_err();
        assert!(err.contains("checksum"), "{err}");
        assert!(!r.workflows.exists());
    }

    #[test]
    fn an_existing_folder_is_never_overwritten() {
        let r = roots();
        let bytes = zip_of(&[("docs/workflow.toml", WORKFLOW.as_bytes())]);
        let entry = entry_for(&bytes, Kind::Workflow, "docs");
        fs::create_dir_all(r.workflows.join("docs")).unwrap();
        fs::write(r.workflows.join("docs/mine.txt"), "my edits").unwrap();
        let err = install_bytes(&entry, &bytes, &r.dirs()).unwrap_err();
        assert!(err.contains("already installed"), "{err}");
        assert_eq!(
            fs::read_to_string(r.workflows.join("docs/mine.txt")).unwrap(),
            "my edits"
        );
        assert!(r.dirs().is_installed(&entry));
    }

    fn refused(files: &[(&str, &[u8])], needle: &str) {
        let r = roots();
        let bytes = zip_of(files);
        let entry = entry_for(&bytes, Kind::Workflow, "docs");
        let err = install_bytes(&entry, &bytes, &r.dirs()).unwrap_err();
        assert!(err.contains(needle), "{files:?}: {err}");
        // Nothing - not even a staging folder - may be left behind.
        let leftovers = fs::read_dir(&r.workflows).map_or(0, Iterator::count);
        assert_eq!(leftovers, 0, "{files:?}");
    }

    #[test]
    fn paths_that_escape_the_folder_are_refused() {
        let w = WORKFLOW.as_bytes();
        refused(
            &[("docs/workflow.toml", w), ("docs/../../evil.txt", b"x")],
            "leaves its folder",
        );
        refused(
            &[("docs/workflow.toml", w), ("/etc/passwd", b"x")],
            "not allowed",
        );
        refused(
            &[("docs/workflow.toml", w), ("C:/Windows/evil.dll", b"x")],
            "not allowed",
        );
        refused(
            &[("docs/workflow.toml", w), ("docs\\..\\..\\evil.txt", b"x")],
            "leaves its folder",
        );
        refused(
            &[("docs/workflow.toml", w), ("docs/file.", b"x")],
            "not allowed",
        );
        refused(
            &[("docs/workflow.toml", w), ("docs/a\u{7}b", b"x")],
            "not allowed",
        );
    }

    #[test]
    fn the_layout_is_checked() {
        let w = WORKFLOW.as_bytes();
        refused(&[("other/workflow.toml", w)], "gallery says");
        refused(
            &[("docs/workflow.toml", w), ("elsewhere/x.txt", b"x")],
            "single folder",
        );
        refused(
            &[("docs/workflow.toml", w), ("loose.txt", b"x")],
            "single folder",
        );
        refused(&[("docs/readme.txt", b"x")], "no workflow.toml");
        refused(&[], "empty");
        refused(
            &[
                ("docs/workflow.toml", w),
                ("docs/A.txt", b"1"),
                ("docs/a.txt", b"2"),
            ],
            "twice",
        );
    }

    #[test]
    fn the_content_is_validated_before_it_is_installed() {
        refused(&[("docs/workflow.toml", b"not toml [")], "not valid");
        // A loop is an error even though the file parses.
        let looped = r#"
            name = "Loop"
            [[node]]
            id = "k"
            type = "keyword"
            keyword = "k"
            [[node]]
            id = "a"
            type = "copy"
            [[node]]
            id = "b"
            type = "copy"
            [[connection]]
            from = "k"
            to = "a"
            [[connection]]
            from = "a"
            to = "b"
            [[connection]]
            from = "b"
            to = "a"
        "#;
        refused(&[("docs/workflow.toml", looped.as_bytes())], "loop");
        // A link in a plugin package cannot be written: zip stores the type in
        // the Unix mode.
        let r = roots();
        let mut out = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(&mut out);
        writer
            .start_file("docs/workflow.toml", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(WORKFLOW.as_bytes()).unwrap();
        writer
            .add_symlink("docs/link", "/etc/passwd", SimpleFileOptions::default())
            .unwrap();
        writer.finish().unwrap();
        let bytes = out.into_inner();
        let entry = entry_for(&bytes, Kind::Workflow, "docs");
        let err = install_bytes(&entry, &bytes, &r.dirs()).unwrap_err();
        assert!(err.contains("link"), "{err}");
    }

    #[test]
    fn a_plugin_manifest_must_load() {
        let r = roots();
        let bytes = zip_of(&[("p/plugin.toml", b"protocol = 1\n")]);
        let entry = entry_for(&bytes, Kind::Plugin, "p");
        let err = install_bytes(&entry, &bytes, &r.dirs()).unwrap_err();
        assert!(err.contains("plugin is not valid"), "{err}");
    }

    #[test]
    fn size_limits_hold_even_when_the_header_lies() {
        let r = roots();
        let big = vec![b'a'; MAX_FILE_BYTES as usize + 1];
        let bytes = zip_of(&[
            ("docs/workflow.toml", WORKFLOW.as_bytes()),
            ("docs/big.bin", &big),
        ]);
        let entry = entry_for(&bytes, Kind::Workflow, "docs");
        let err = install_bytes(&entry, &bytes, &r.dirs()).unwrap_err();
        assert!(err.contains("too large"), "{err}");

        let many: Vec<(String, Vec<u8>)> = (0..MAX_FILES + 1)
            .map(|i| (format!("docs/f{i}.txt"), b"x".to_vec()))
            .collect();
        let mut files: Vec<(&str, &[u8])> = many
            .iter()
            .map(|(n, d)| (n.as_str(), d.as_slice()))
            .collect();
        files.push(("docs/workflow.toml", WORKFLOW.as_bytes()));
        let bytes = zip_of(&files);
        let entry = entry_for(&bytes, Kind::Workflow, "docs");
        let err = install_bytes(&entry, &bytes, &r.dirs()).unwrap_err();
        assert!(err.contains("too many"), "{err}");
    }

    #[test]
    fn a_zip_bomb_is_refused_by_total_size() {
        let r = roots();
        // Each file is within its own limit; together they are not.
        let chunk = vec![b'0'; MAX_FILE_BYTES as usize];
        let names: Vec<String> = (0..7).map(|i| format!("docs/part{i}.bin")).collect();
        let mut files: Vec<(&str, &[u8])> = names
            .iter()
            .map(|name| (name.as_str(), chunk.as_slice()))
            .collect();
        files.push(("docs/workflow.toml", WORKFLOW.as_bytes()));
        let mut out = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(&mut out);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in &files {
            writer.start_file(*name, options).unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap();
        let bytes = out.into_inner();
        assert!(bytes.len() < MAX_PACKAGE_BYTES, "the bomb compresses well");
        let entry = entry_for(&bytes, Kind::Workflow, "docs");
        let err = install_bytes(&entry, &bytes, &r.dirs()).unwrap_err();
        assert!(err.contains("too large when unpacked"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn only_the_owners_execute_bit_is_restored() {
        use std::os::unix::fs::PermissionsExt;

        let r = roots();
        let mut out = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(&mut out);
        writer
            .start_file("docs/workflow.toml", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(WORKFLOW.as_bytes()).unwrap();
        writer
            .start_file(
                "docs/run.sh",
                SimpleFileOptions::default().unix_permissions(0o4777),
            )
            .unwrap();
        writer.write_all(b"#!/bin/sh\n").unwrap();
        writer.finish().unwrap();
        let bytes = out.into_inner();
        let entry = entry_for(&bytes, Kind::Workflow, "docs");
        install_bytes(&entry, &bytes, &r.dirs()).unwrap();
        let mode = fs::metadata(r.workflows.join("docs/run.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o7777, 0o755);
        let plain = fs::metadata(r.workflows.join("docs/workflow.toml"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(plain & 0o7777, 0o644);
    }

    // ---- the repository's own gallery ----------------------------------

    fn repo_gallery() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../gallery")
    }

    #[test]
    fn the_shipped_index_matches_the_shipped_packages() {
        let text = fs::read_to_string(repo_gallery().join("index.json")).unwrap();
        let index = parse_index(&text).unwrap();
        assert!(index.skipped.is_empty(), "{:?}", index.skipped);
        assert!(index.entries.len() >= 3);
        let kinds: HashSet<_> = index.entries.iter().map(|e| e.kind).collect();
        assert_eq!(kinds.len(), 2, "both workflows and plugins are shown");
        for entry in &index.entries {
            // The source is the raw GitHub URL of the committed package.
            let file = entry.source.rsplit('/').next().unwrap();
            assert!(
                entry.source.starts_with(
                    "https://raw.githubusercontent.com/ninad-k/Sevak/main/gallery/packages/"
                ),
                "{}",
                entry.source
            );
            let bytes = fs::read(repo_gallery().join("packages").join(file))
                .unwrap_or_else(|err| panic!("{file}: {err}"));
            verify_sha256(&bytes, &entry.sha256)
                .unwrap_or_else(|err| panic!("{}: {err}", entry.id));
            // And it installs.
            let r = roots();
            let done = install_bytes(entry, &bytes, &r.dirs())
                .unwrap_or_else(|err| panic!("{}: {err}", entry.id));
            assert_eq!(done.folder, entry.folder_name());
        }
    }

    #[test]
    fn the_packages_match_the_examples_they_were_built_from() {
        let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        let text = fs::read_to_string(repo_gallery().join("index.json")).unwrap();
        let index = parse_index(&text).unwrap();
        for entry in &index.entries {
            let file = entry.source.rsplit('/').next().unwrap();
            let bytes = fs::read(repo_gallery().join("packages").join(file)).unwrap();
            let r = roots();
            install_bytes(entry, &bytes, &r.dirs()).unwrap();
            let (installed, source) = match entry.kind {
                Kind::Workflow => (
                    r.workflows.join(entry.folder_name()),
                    examples.join("workflows").join(entry.folder_name()),
                ),
                Kind::Plugin => (
                    r.plugins.join(entry.folder_name()),
                    examples.join("plugins").join(entry.folder_name()),
                ),
            };
            assert_same_files(&installed, &source);
        }
    }

    /// Both folders hold the same files with the same text (line endings
    /// aside, which a Windows checkout may change).
    fn assert_same_files(installed: &Path, source: &Path) {
        fn list(root: &Path, dir: &Path, out: &mut Vec<String>) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    list(root, &path, out);
                } else {
                    out.push(
                        path.strip_prefix(root)
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/"),
                    );
                }
            }
        }
        let (mut a, mut b) = (Vec::new(), Vec::new());
        list(installed, installed, &mut a);
        list(source, source, &mut b);
        a.sort();
        b.sort();
        assert_eq!(a, b, "{}", source.display());
        for name in a {
            let normalize = |path: PathBuf| {
                String::from_utf8_lossy(&fs::read(path).unwrap()).replace("\r\n", "\n")
            };
            assert_eq!(
                normalize(installed.join(&name)),
                normalize(source.join(&name)),
                "{name} in {}",
                source.display()
            );
        }
    }
}
