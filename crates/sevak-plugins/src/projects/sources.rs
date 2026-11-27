//! Reading the editors' recent lists.
//!
//! - **VS Code and its forks** keep `history.recentlyOpenedPathsList` in
//!   `User/globalStorage/state.vscdb`, an SQLite database. It is opened
//!   read-only; if the editor holds it locked, a private copy (with its
//!   write-ahead log) is read instead, as the bookmarks plugin does for
//!   Firefox. Older versions kept the same list in `storage.json`.
//! - **JetBrains IDEs** keep `options/recentProjects.xml` (Rider:
//!   `recentSolutions.xml`) in a settings folder per product and version.
//!
//! Every file is read only from inside the editor's own settings folder
//! (symbolic links that lead elsewhere are not followed) and with a size cap.
//! Only local `file://` locations are kept: remote (SSH, WSL, containers)
//! and network-share entries are skipped.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::Value;

use super::editors::Os;
use crate::local_source::{confined, read_confined_text};

/// `storage.json` and `recentProjects.xml` larger than this are not read.
const MAX_TEXT_BYTES: u64 = 16 * 1024 * 1024;
/// `state.vscdb` larger than this is not read.
const MAX_DATABASE_BYTES: u64 = 512 * 1024 * 1024;
/// Entries kept from one editor's list.
const MAX_ENTRIES: usize = 500;
const RECENT_KEY: &str = "history.recentlyOpenedPathsList";

/// What an entry opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Folder,
    /// A `.code-workspace` file.
    Workspace,
}

/// One recent project as an editor lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recent {
    pub path: PathBuf,
    pub kind: Kind,
    /// When it was last opened, in milliseconds since 1970, where known.
    pub opened_ms: Option<i64>,
    /// The name the editor shows (JetBrains' frame title), if any.
    pub title: Option<String>,
}

/// Milliseconds since 1970 of `path`'s last change.
pub fn modified_ms(path: &Path) -> Option<i64> {
    let time = fs::metadata(path).ok()?.modified().ok()?;
    let ms = time.duration_since(UNIX_EPOCH).ok()?.as_millis();
    i64::try_from(ms).ok()
}

/// Now, in milliseconds since 1970.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

// ---- VS Code ---------------------------------------------------------------

/// The recent folders and workspaces of a VS Code build whose settings
/// folder is `data_root`, most recent first, with the time the list was saved.
pub fn read_vscode(data_root: &Path, os: Os) -> (Vec<Recent>, Option<i64>) {
    let global = data_root.join("User").join("globalStorage");
    let database = global.join("state.vscdb");
    if let Some(json) = read_state_database(data_root, &database) {
        if let Ok(value) = serde_json::from_str::<Value>(&json) {
            let recents = parse_recent_list(&value, os);
            if !recents.is_empty() {
                return (recents, modified_ms(&database));
            }
        }
    }
    for storage in [global.join("storage.json"), data_root.join("storage.json")] {
        let Some(text) = read_confined_text(data_root, &storage, MAX_TEXT_BYTES) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if let Some(list) = value.get("openedPathsList") {
            let recents = parse_recent_list(list, os);
            if !recents.is_empty() {
                return (recents, modified_ms(&storage));
            }
        }
    }
    (Vec::new(), None)
}

/// The JSON value of [`RECENT_KEY`] in `state.vscdb`.
fn read_state_database(root: &Path, database: &Path) -> Option<String> {
    let real = confined(root, database)?;
    let length = fs::metadata(&real).ok()?.len();
    if !real.is_file() || length > MAX_DATABASE_BYTES {
        return None;
    }
    let read_only = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    match query_recent(&real, read_only) {
        Ok(found) => found,
        Err(err) => {
            tracing::debug!(%err, "state.vscdb is busy; reading a private copy");
            read_copy(&real)
        }
    }
}

fn query_recent(database: &Path, flags: OpenFlags) -> rusqlite::Result<Option<String>> {
    let conn = Connection::open_with_flags(database, flags)?;
    conn.busy_timeout(std::time::Duration::from_millis(200))?;
    conn.query_row(
        "SELECT value FROM ItemTable WHERE key = ?1",
        [RECENT_KEY],
        |row| {
            Ok(match row.get_ref(0)? {
                ValueRef::Text(bytes) | ValueRef::Blob(bytes) => {
                    String::from_utf8_lossy(bytes).into_owned()
                }
                _ => String::new(),
            })
        },
    )
    .optional()
}

/// Copies the database (and its write-ahead log) to a private temporary
/// folder and reads it there, so the editor's own files are never locked.
fn read_copy(database: &Path) -> Option<String> {
    let scratch = tempfile::Builder::new()
        .prefix("sevak-projects-")
        .tempdir()
        .ok()?;
    let copy = scratch.path().join("state.vscdb");
    fs::copy(database, &copy).ok()?;
    let mut wal = database.as_os_str().to_owned();
    wal.push("-wal");
    let wal = PathBuf::from(wal);
    if wal.is_file() {
        let mut target = copy.as_os_str().to_owned();
        target.push("-wal");
        let _ = fs::copy(&wal, PathBuf::from(target));
    }
    // Read-write on the copy so SQLite can replay the log. The connection is
    // closed before `scratch` deletes the folder.
    let found = query_recent(
        &copy,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()
    .flatten();
    drop(scratch);
    found
}

/// The folders and workspaces of a recent list: `{"entries": [...]}` in
/// current versions, `{"workspaces3": [...]}` in old ones. Files are skipped.
pub fn parse_recent_list(list: &Value, os: Os) -> Vec<Recent> {
    let mut recents = Vec::new();
    let mut push = |uri: Option<&str>, kind: Kind| {
        if let Some(path) = uri.and_then(|uri| file_uri_to_path(uri, os)) {
            if !recents.iter().any(|r: &Recent| r.path == path) && recents.len() < MAX_ENTRIES {
                recents.push(Recent {
                    path,
                    kind,
                    opened_ms: None,
                    title: None,
                });
            }
        }
    };
    let entries = ["entries", "workspaces3", "workspaces2", "folders2"]
        .iter()
        .filter_map(|key| list.get(key).and_then(Value::as_array))
        .flatten();
    for entry in entries {
        match entry {
            Value::String(uri) => push(Some(uri), Kind::Folder),
            Value::Object(object) => {
                if let Some(folder) = object.get("folderUri").and_then(Value::as_str) {
                    push(Some(folder), Kind::Folder);
                } else if let Some(workspace) = object.get("workspace") {
                    let config = workspace
                        .get("configPath")
                        .or_else(|| workspace.get("configURIPath"))
                        .and_then(Value::as_str);
                    push(config, Kind::Workspace);
                } else if let Some(config) = object.get("configURIPath").and_then(Value::as_str) {
                    push(Some(config), Kind::Workspace);
                }
            }
            _ => {}
        }
    }
    recents
}

/// The local path of a `file://` URI, or `None` for anything else (remote
/// schemes, a network host, characters that cannot be in a path).
pub fn file_uri_to_path(uri: &str, os: Os) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // `file://server/share` names another computer.
    if !rest.starts_with('/') {
        return None;
    }
    let decoded = percent_decode(rest)?;
    if decoded.chars().any(|c| c.is_control()) {
        return None;
    }
    match os {
        Os::Windows => {
            // `/c:/Users/x` is `C:\Users\x`.
            let bytes = decoded.as_bytes();
            if bytes.len() >= 3
                && bytes[0] == b'/'
                && bytes[1].is_ascii_alphabetic()
                && bytes[2] == b':'
            {
                let drive = (bytes[1] as char).to_ascii_uppercase();
                let tail = decoded[3..].replace('/', "\\");
                let tail = if tail.is_empty() {
                    "\\".to_owned()
                } else {
                    tail
                };
                Some(PathBuf::from(format!("{drive}:{tail}")))
            } else {
                None
            }
        }
        _ => Some(PathBuf::from(decoded)),
    }
}

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
    String::from_utf8(out).ok()
}

// ---- JetBrains -------------------------------------------------------------

/// The settings folders of JetBrains products under `root`
/// (`<config>/JetBrains`), with their recent-projects files.
pub fn jetbrains_files(root: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut found: Vec<(String, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            Some((name, entry.path()))
        })
        .flat_map(|(name, dir)| {
            ["recentProjects.xml", "recentSolutions.xml"]
                .into_iter()
                .map(move |file| (name.clone(), dir.join("options").join(file)))
        })
        .filter(|(_, file)| file.is_file())
        .collect();
    found.sort();
    found
}

/// The projects in one `recentProjects.xml`.
pub fn read_jetbrains(root: &Path, file: &Path, home: Option<&Path>, os: Os) -> Vec<Recent> {
    let Some(xml) = read_confined_text(root, file, MAX_TEXT_BYTES) else {
        return Vec::new();
    };
    let mut recents: Vec<Recent> = Vec::new();
    for (raw, opened_ms, title) in parse_recent_projects(&xml) {
        let Some(path) = jetbrains_path(&raw, home, os) else {
            continue;
        };
        if recents.iter().any(|r| r.path == path) || recents.len() >= MAX_ENTRIES {
            continue;
        }
        recents.push(Recent {
            path,
            kind: Kind::Folder,
            opened_ms,
            title,
        });
    }
    recents
}

/// `$USER_HOME$/x` with the home folder put in; other macros are not known.
fn jetbrains_path(raw: &str, home: Option<&Path>, os: Os) -> Option<PathBuf> {
    let text = match raw.strip_prefix("$USER_HOME$") {
        Some(rest) => format!("{}{rest}", home?.to_string_lossy()),
        None => raw.to_owned(),
    };
    if text.contains('$') || text.chars().any(char::is_control) {
        return None;
    }
    let text = match os {
        Os::Windows => {
            let text = text.replace('/', "\\");
            // `\\server\share` is another computer.
            if text.starts_with("\\\\") {
                return None;
            }
            text
        }
        _ => text,
    };
    let path = PathBuf::from(text);
    let absolute = match os {
        Os::Windows => {
            let bytes = path.to_string_lossy();
            let b = bytes.as_bytes();
            b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\'
        }
        _ => path.to_string_lossy().starts_with('/'),
    };
    absolute.then_some(path)
}

/// `(path, opened, title)` for each project in the XML: the `additionalInfo`
/// map (with each project's `activationTimestamp` / `projectOpenTimestamp`
/// and `frameTitle`) and the older `recentPaths` list.
fn parse_recent_projects(xml: &str) -> Vec<(String, Option<i64>, Option<String>)> {
    let mut projects: Vec<(String, Option<i64>, Option<String>)> = Vec::new();
    // Open elements, as `(name, the `name` attribute)`.
    let mut stack: Vec<(String, Option<String>)> = Vec::new();
    // The `additionalInfo` entry being read, and the depth it was opened at.
    let mut current: Option<(usize, String, Option<i64>, Option<String>)> = None;

    for tag in tags(xml) {
        if tag.closing {
            if let Some(pos) = stack.iter().rposition(|(name, _)| *name == tag.name) {
                stack.truncate(pos);
            }
            if current
                .as_ref()
                .is_some_and(|(depth, ..)| stack.len() <= *depth)
            {
                if let Some((_, path, opened, title)) = current.take() {
                    projects.push((path, opened, title));
                }
            }
            continue;
        }
        let in_recent = stack.iter().any(|(name, attr)| {
            name == "component" && attr.as_deref().is_some_and(|a| a.contains("Recent"))
        });
        let under = |option: &str| {
            stack
                .iter()
                .any(|(name, attr)| name == "option" && attr.as_deref() == Some(option))
        };
        if in_recent {
            match tag.name.as_str() {
                "entry" if under("additionalInfo") && current.is_none() => {
                    if let Some(key) = tag.attr("key") {
                        if tag.self_closing {
                            projects.push((key.to_owned(), None, None));
                        } else {
                            current = Some((stack.len(), key.to_owned(), None, None));
                        }
                    }
                }
                "option" if current.is_some() => {
                    let name = tag.attr("name");
                    if matches!(name, Some("activationTimestamp" | "projectOpenTimestamp")) {
                        if let (Some(value), Some((_, _, opened, _))) = (
                            tag.attr("value").and_then(|v| v.parse::<i64>().ok()),
                            current.as_mut(),
                        ) {
                            *opened = Some(opened.map_or(value, |o| o.max(value)));
                        }
                    }
                }
                "RecentProjectMetaInfo" => {
                    if let (Some(title), Some((_, _, _, slot))) =
                        (tag.attr("frameTitle"), current.as_mut())
                    {
                        slot.get_or_insert_with(|| title.to_owned());
                    }
                }
                "option" if under("recentPaths") => {
                    if let Some(value) = tag.attr("value") {
                        if !projects.iter().any(|(p, ..)| p == value) {
                            projects.push((value.to_owned(), None, None));
                        }
                    }
                }
                _ => {}
            }
        }
        if !tag.self_closing {
            stack.push((tag.name.clone(), tag.attr("name").map(str::to_owned)));
        }
    }
    // Newest first; entries without a time keep their order after them.
    let mut indexed: Vec<_> = projects.into_iter().enumerate().collect();
    indexed.sort_by(|(ia, a), (ib, b)| b.1.cmp(&a.1).then(ia.cmp(ib)));
    let mut seen = std::collections::HashSet::new();
    indexed
        .into_iter()
        .map(|(_, p)| p)
        .filter(|(path, ..)| seen.insert(path.clone()))
        .collect()
}

/// One XML start or end tag.
struct Tag {
    name: String,
    attrs: Vec<(String, String)>,
    closing: bool,
    self_closing: bool,
}

impl Tag {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// The tags of an XML document, skipping text, comments, CDATA, processing
/// instructions and declarations. Enough for the settings files IDEs write;
/// malformed input ends the list early instead of failing.
fn tags(xml: &str) -> Vec<Tag> {
    let mut tags = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find('<') {
        rest = &rest[start..];
        let skip_to = |rest: &str, end: &str| rest.find(end).map(|i| i + end.len());
        let skipped = if rest.starts_with("<!--") {
            skip_to(rest, "-->")
        } else if rest.starts_with("<![CDATA[") {
            skip_to(rest, "]]>")
        } else if rest.starts_with("<?") {
            skip_to(rest, "?>")
        } else if rest.starts_with("<!") {
            skip_to(rest, ">")
        } else {
            None
        };
        if rest.starts_with("<!") || rest.starts_with("<?") {
            match skipped {
                Some(n) => {
                    rest = &rest[n..];
                    continue;
                }
                None => break,
            }
        }
        // The end of the tag, outside quotes.
        let mut quote = None;
        let mut end = None;
        for (i, c) in rest.char_indices().skip(1) {
            match (quote, c) {
                (None, '"' | '\'') => quote = Some(c),
                (Some(q), c) if c == q => quote = None,
                (None, '>') => {
                    end = Some(i);
                    break;
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            break;
        };
        let inner = &rest[1..end];
        rest = &rest[end + 1..];
        let closing = inner.starts_with('/');
        let self_closing = inner.ends_with('/');
        let inner = inner.trim_start_matches('/').trim_end_matches('/');
        let name_end = inner
            .find(|c: char| c.is_whitespace())
            .unwrap_or(inner.len());
        let name = inner[..name_end].to_owned();
        if name.is_empty() {
            continue;
        }
        tags.push(Tag {
            attrs: attributes(&inner[name_end..]),
            name,
            closing,
            self_closing,
        });
    }
    tags
}

fn attributes(text: &str) -> Vec<(String, String)> {
    let mut attrs = Vec::new();
    let mut rest = text;
    loop {
        rest = rest.trim_start();
        let Some(eq) = rest.find('=') else {
            break;
        };
        let key = rest[..eq].trim().to_owned();
        rest = rest[eq + 1..].trim_start();
        let Some(quote) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            break;
        };
        let Some(close) = rest[1..].find(quote) else {
            break;
        };
        attrs.push((key, unescape(&rest[1..1 + close])));
        rest = &rest[close + 2..];
    }
    attrs
}

/// Replaces the XML entities (`&amp;`, `&#38;`, `&#x26;` ...).
fn unescape(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let Some(semi) = rest.find(';').filter(|&i| i <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..semi];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uris_become_local_paths() {
        assert_eq!(
            file_uri_to_path("file:///c%3A/Users/Ada/My%20Repo", Os::Windows),
            Some(PathBuf::from(r"C:\Users\Ada\My Repo"))
        );
        assert_eq!(
            file_uri_to_path("file:///home/ada/caf%C3%A9", Os::Linux),
            Some(PathBuf::from("/home/ada/café"))
        );
        assert_eq!(file_uri_to_path("file://server/share/x", Os::Windows), None);
        assert_eq!(
            file_uri_to_path("vscode-remote://ssh-remote%2Bbox/home/x", Os::Linux),
            None
        );
        assert_eq!(file_uri_to_path("file:///home/a%0Ab", Os::Linux), None);
        assert_eq!(file_uri_to_path("file:///bad%zz", Os::Linux), None);
        assert_eq!(file_uri_to_path("file:///home/x", Os::Windows), None);
    }

    #[test]
    fn recent_lists_of_old_and_new_versions() {
        let new: Value = serde_json::from_str(
            r#"{"entries":[
                {"folderUri":"file:///home/ada/sevak"},
                {"workspace":{"id":"abc","configPath":"file:///home/ada/all.code-workspace"}},
                {"fileUri":"file:///home/ada/notes.txt"},
                {"folderUri":"vscode-remote://wsl%2Bubuntu/home/ada/x"},
                {"folderUri":"file:///home/ada/sevak"},
                {"folderUri":"file:///home/ada/other","label":"x"}
            ]}"#,
        )
        .unwrap();
        let recents = parse_recent_list(&new, Os::Linux);
        let summary: Vec<_> = recents
            .iter()
            .map(|r| (r.path.to_string_lossy().into_owned(), r.kind))
            .collect();
        assert_eq!(
            summary,
            [
                ("/home/ada/sevak".to_owned(), Kind::Folder),
                ("/home/ada/all.code-workspace".to_owned(), Kind::Workspace),
                ("/home/ada/other".to_owned(), Kind::Folder),
            ]
        );

        let old: Value = serde_json::from_str(
            r#"{"workspaces3":["file:///home/ada/old",{"id":"1","configURIPath":"file:///home/ada/w.code-workspace"}],"files2":[]}"#,
        )
        .unwrap();
        let recents = parse_recent_list(&old, Os::Linux);
        assert_eq!(recents.len(), 2);
        assert_eq!(recents[1].kind, Kind::Workspace);
    }

    fn write_state_db(path: &Path, json: &str) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
            [RECENT_KEY, json],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO ItemTable (key, value) VALUES ('other', 'x')",
            [],
        )
        .unwrap();
    }

    #[test]
    fn reads_the_state_database_then_falls_back_to_storage_json() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Code");
        let global = root.join("User").join("globalStorage");
        fs::create_dir_all(&global).unwrap();
        fs::write(
            global.join("storage.json"),
            r#"{"openedPathsList":{"entries":[{"folderUri":"file:///from/json"}]}}"#,
        )
        .unwrap();
        let (recents, saved) = read_vscode(&root, Os::Linux);
        assert_eq!(recents[0].path, PathBuf::from("/from/json"));
        assert!(saved.is_some());

        write_state_db(
            &global.join("state.vscdb"),
            r#"{"entries":[{"folderUri":"file:///from/db"}]}"#,
        );
        let (recents, _) = read_vscode(&root, Os::Linux);
        assert_eq!(recents.len(), 1);
        assert_eq!(recents[0].path, PathBuf::from("/from/db"));

        // A copy reads the same.
        let real = global.join("state.vscdb").canonicalize().unwrap();
        assert!(read_copy(&real).unwrap().contains("/from/db"));
    }

    #[test]
    fn a_broken_database_or_json_reads_as_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Code");
        let global = root.join("User").join("globalStorage");
        fs::create_dir_all(&global).unwrap();
        fs::write(global.join("state.vscdb"), b"not sqlite at all").unwrap();
        fs::write(global.join("storage.json"), b"{ nope").unwrap();
        assert_eq!(read_vscode(&root, Os::Linux), (Vec::new(), None));
        assert_eq!(read_vscode(&dir.path().join("missing"), Os::Linux).0, []);
    }

    const RECENT_XML: &str = r#"<application>
  <component name="RecentProjectsManager">
    <option name="additionalInfo">
      <map>
        <entry key="$USER_HOME$/IdeaProjects/old">
          <value>
            <RecentProjectMetaInfo frameTitle="old – README.md" opened="false">
              <option name="activationTimestamp" value="1700000000000" />
            </RecentProjectMetaInfo>
          </value>
        </entry>
        <entry key="/work/new &amp; shiny">
          <value>
            <RecentProjectMetaInfo>
              <option name="projectOpenTimestamp" value="1710000000000" />
              <option name="binFolder" value="$APPLICATION_HOME_DIR$/bin" />
            </RecentProjectMetaInfo>
          </value>
        </entry>
        <entry key="$APPLICATION_CONFIG_DIR$/scratch" />
      </map>
    </option>
    <option name="recentPaths">
      <list>
        <option value="/legacy/path" />
        <option value="/work/new &amp; shiny" />
      </list>
    </option>
    <!-- <entry key="/in/a/comment" /> -->
  </component>
  <component name="Other"><option name="additionalInfo"><map><entry key="/not/recent"/></map></option></component>
</application>"#;

    #[test]
    fn parses_jetbrains_recent_projects_newest_first() {
        let projects = parse_recent_projects(RECENT_XML);
        let paths: Vec<_> = projects.iter().map(|(p, ..)| p.as_str()).collect();
        assert_eq!(
            paths,
            [
                "/work/new & shiny",
                "$USER_HOME$/IdeaProjects/old",
                "$APPLICATION_CONFIG_DIR$/scratch",
                "/legacy/path",
            ]
        );
        assert_eq!(projects[0].1, Some(1_710_000_000_000));
        assert_eq!(projects[1].2.as_deref(), Some("old – README.md"));
    }

    #[test]
    fn jetbrains_files_are_found_and_macros_resolved() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("JetBrains");
        let options = root.join("GoLand2024.1").join("options");
        fs::create_dir_all(&options).unwrap();
        fs::write(options.join("recentProjects.xml"), RECENT_XML).unwrap();
        fs::create_dir_all(root.join("Toolbox")).unwrap();
        let files = jetbrains_files(&root);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].0, "GoLand2024.1");

        let home = PathBuf::from("/home/ada");
        let recents = read_jetbrains(&root, &files[0].1, Some(&home), Os::Linux);
        let paths: Vec<_> = recents.iter().map(|r| r.path.clone()).collect();
        assert_eq!(
            paths,
            [
                PathBuf::from("/work/new & shiny"),
                PathBuf::from("/home/ada/IdeaProjects/old"),
                PathBuf::from("/legacy/path"),
            ]
        );
        assert_eq!(
            jetbrains_path("C:/Users/Ada/proj", None, Os::Windows),
            Some(PathBuf::from(r"C:\Users\Ada\proj"))
        );
        assert_eq!(jetbrains_path("//server/share/p", None, Os::Windows), None);
        assert_eq!(jetbrains_path("relative/p", None, Os::Linux), None);
        assert_eq!(jetbrains_path("$USER_HOME$/x", None, Os::Linux), None);
    }

    #[test]
    fn entities_and_odd_markup() {
        assert_eq!(
            unescape("a &lt;b&gt; &#38; &#x41; &bogus; &"),
            "a <b> & A &bogus; &"
        );
        let tags = tags("<?xml version='1.0'?><!DOCTYPE x><a k='1 > 2'><b/></a><unterminated");
        let names: Vec<_> = tags.iter().map(|t| (t.name.as_str(), t.closing)).collect();
        assert_eq!(names, [("a", false), ("b", false), ("a", true)]);
        assert_eq!(tags[0].attr("k"), Some("1 > 2"));
        assert!(tags[1].self_closing);
    }
}
