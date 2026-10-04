//! Which script plugins the user has allowed to run.
//!
//! A script plugin runs arbitrary code with the user's privileges, so a plugin
//! folder that appears in the config directory is not started until the user
//! says yes once. The answer is remembered per plugin id **and** a SHA-256 over
//! what would run (see [`script_approval_key`]): the manifest, the command, the
//! bytes of the script files the command names and the folder's location.
//! Change any of them and Sevak asks again. The record is a small JSON file in
//! the data directory, so it is separate from the hand-edited config.
//!
//! The file is shared with workflows, which store their own keys under
//! `workflow:<folder>` ids.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use sevak_core::bounded_read::{read_capped, read_to_string_capped, MAX_STATE_BYTES};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::manifest::{relative_inside, Manifest};

/// The approvals file format this Sevak writes.
///
/// * 1 (no `version` field): script plugin keys were the command line only.
///   Those entries never match a current key, so every script plugin asks once
///   more under the new rules.
/// * 2: script plugin keys are `v2:sha256:...` over the plugin's contents.
const FORMAT: u32 = 2;

/// Prefix of the keys script plugins store.
const KEY_PREFIX: &str = "v2:";

/// The largest file whose bytes are hashed. A bigger one is bound by its size
/// only (nobody ships a multi-hundred-megabyte script).
pub(super) const MAX_HASHED_FILE: u64 = 256 * 1024 * 1024;

fn format_one() -> u32 {
    1
}

#[derive(Debug, Serialize, Deserialize)]
struct Record {
    #[serde(default = "format_one")]
    version: u32,
    /// Plugin id to the approved key (see [`script_approval_key`]).
    #[serde(default)]
    approved: BTreeMap<String, String>,
}

impl Default for Record {
    fn default() -> Self {
        Self {
            version: FORMAT,
            approved: BTreeMap::new(),
        }
    }
}

/// One read of the approvals file; see [`ApprovalStore::snapshot`].
#[derive(Debug)]
pub struct Approvals(Record);

impl Approvals {
    pub fn is_approved(&self, id: &str, key: &str) -> bool {
        self.0
            .approved
            .get(id)
            .is_some_and(|approved| approved == key)
    }

    pub fn has_record(&self, id: &str) -> bool {
        self.0.approved.contains_key(id)
    }
}

#[derive(Debug, Clone)]
pub struct ApprovalStore {
    path: PathBuf,
}

impl ApprovalStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn read(&self) -> Record {
        match read_to_string_capped(&self.path, MAX_STATE_BYTES) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|err| {
                // Treat a damaged file as "nothing approved yet": plugins ask again.
                tracing::warn!(path = %self.path.display(), %err, "script plugin approvals are unreadable");
                Record::default()
            }),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Record::default(),
            Err(err) => {
                tracing::warn!(path = %self.path.display(), %err, "cannot read script plugin approvals");
                Record::default()
            }
        }
    }

    pub fn is_approved(&self, id: &str, key: &str) -> bool {
        self.snapshot().is_approved(id, key)
    }

    /// Whether anything is recorded for `id`, current or not. A plugin that
    /// has a record but is not approved was allowed before and has changed
    /// since (or was allowed under the older rules).
    pub fn has_record(&self, id: &str) -> bool {
        self.snapshot().has_record(id)
    }

    /// The file as it is now, for checking many plugins with one read.
    pub fn snapshot(&self) -> Approvals {
        Approvals(self.read())
    }

    /// Drops what is recorded for `id` (the plugin or workflow was removed), so
    /// a later install under the same name asks again and the file does not
    /// keep names of things that are gone. Nothing happens if there is no record.
    pub fn forget(&self, id: &str) -> io::Result<()> {
        let mut record = self.read();
        if record.approved.remove(id).is_none() {
            return Ok(());
        }
        record.version = FORMAT;
        let text = serde_json::to_string_pretty(&record).map_err(io::Error::other)?;
        sevak_platform::private_file::write_atomic(&self.path, text.as_bytes())
    }

    /// Records that plugin `id` may run exactly `key`.
    pub fn approve(&self, id: &str, key: &str) -> io::Result<()> {
        let mut record = self.read();
        record.version = FORMAT;
        record.approved.insert(id.to_owned(), key.to_owned());
        self.write(&record)
    }

    /// Forgets the approval of each of `ids`, so they ask again. A missing
    /// record is left missing. Used when a restore brings back scripts that
    /// must not inherit an old yes.
    pub fn revoke(&self, ids: &[String]) -> io::Result<()> {
        if !self.path.exists() {
            return Ok(());
        }
        let mut record = self.read();
        let before = record.approved.len();
        for id in ids {
            record.approved.remove(id);
        }
        if record.approved.len() == before {
            return Ok(());
        }
        self.write(&record)
    }

    fn write(&self, record: &Record) -> io::Result<()> {
        let text = serde_json::to_string_pretty(record).map_err(io::Error::other)?;
        // Preserve owner-only, atomic storage for both approvals and revocations.
        sevak_platform::private_file::write_atomic(&self.path, text.as_bytes())
    }
}

/// Builds the SHA-256 an approval is bound to. Every piece is labelled and
/// length-prefixed, so no two different inputs feed the same bytes.
pub struct ContentHasher {
    hasher: Sha256,
}

impl Default for ContentHasher {
    fn default() -> Self {
        Self::new()
    }
}

impl ContentHasher {
    pub fn new() -> Self {
        Self {
            hasher: Sha256::new(),
        }
    }

    /// Adds `bytes` under `label`.
    pub fn feed(&mut self, label: &str, bytes: &[u8]) {
        self.hasher.update(label.as_bytes());
        self.hasher.update((bytes.len() as u64).to_le_bytes());
        self.hasher.update(bytes);
    }

    /// Adds the file `relative` inside `dir` (a plain relative path, see
    /// [`relative_inside`]); a file that is missing or unreadable is recorded
    /// as missing, so creating it later changes the hash.
    pub fn file(&mut self, dir: &Path, relative: &str) {
        let Some(path) = relative_inside(dir, relative) else {
            self.feed("missing", relative.as_bytes());
            return;
        };
        // Read through the same size cap as every other file Sevak did not
        // write; a bigger one is bound by its size only.
        match fs::metadata(&path) {
            Ok(meta) if meta.is_file() => match read_capped(&path, MAX_HASHED_FILE) {
                Ok(bytes) => self.feed("file", &bytes),
                Err(err) if err.kind() == io::ErrorKind::FileTooLarge => {
                    self.feed("large", &meta.len().to_le_bytes());
                }
                Err(_) => self.feed("missing", relative.as_bytes()),
            },
            _ => self.feed("missing", relative.as_bytes()),
        }
    }

    /// The finished hash as `sha256:<hex>`.
    pub fn finish(self) -> String {
        let hex: String = self
            .hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        format!("sha256:{hex}")
    }
}

/// What the user approves for a script plugin: a SHA-256 over the folder's
/// canonical location, the bytes of `plugin.toml`, the id, how the script is
/// run (command line, mode, format) and the bytes of every file the command
/// names inside the folder (plus the manifest's `files`). The folder's
/// location is in it so a second folder that merely claims the same id and
/// command is a different plugin, and so a moved folder is reviewed again.
///
/// Reads `plugin.toml` itself and returns the parsed manifest with the key, so
/// what is approved and what runs come from the same bytes.
pub fn script_approval_key(dir: &Path) -> Result<(Manifest, String), String> {
    let (manifest, manifest_bytes) = Manifest::read(dir)?;
    let mut hasher = ContentHasher::new();
    hasher.feed("sevak-script-approval", &FORMAT.to_le_bytes());
    let canonical = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    hasher.feed("dir", canonical.to_string_lossy().as_bytes());
    hasher.feed("manifest", &manifest_bytes);
    hasher.feed("id", manifest.id.as_bytes());
    hasher.feed(
        "run",
        format!(
            "{:?}/{:?}: {}",
            manifest.mode,
            manifest.format,
            manifest.command_line()
        )
        .as_bytes(),
    );
    for file in manifest.support_files() {
        hasher.feed("path", file.as_bytes());
        hasher.file(dir, &file);
    }
    Ok((manifest, format!("{KEY_PREFIX}{}", hasher.finish())))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn approval_is_per_plugin_and_per_key() {
        let dir = tempfile::tempdir().unwrap();
        let store = ApprovalStore::new(dir.path().join("sub").join("approvals.json"));
        assert!(!store.is_approved("script:a", "k1"));
        assert!(!store.has_record("script:a"));

        store.approve("script:a", "k1").unwrap();
        assert!(store.is_approved("script:a", "k1"));
        assert!(store.has_record("script:a"));
        assert!(
            !store.is_approved("script:a", "k2"),
            "a changed key asks again"
        );
        assert!(!store.is_approved("script:b", "k1"));

        store.approve("script:b", "k3").unwrap();
        assert!(store.is_approved("script:a", "k1"), "approvals accumulate");
        assert!(store.is_approved("script:b", "k3"));
        assert!(!dir.path().join("sub").join("approvals.json.tmp").exists());
    }

    #[cfg(unix)]
    #[test]
    fn the_approvals_file_is_private_to_the_owner() {
        use std::os::unix::fs::PermissionsExt;
        let mode =
            |path: &std::path::Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state").join("approvals.json");
        let store = ApprovalStore::new(path.clone());
        store.approve("script:a", "python main.py").unwrap();
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        // Rewriting a file that was left wider by an older version tightens it.
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        store.approve("script:b", "node b.js").unwrap();
        assert_eq!(mode(&path), 0o600);
    }

    #[test]
    fn an_oversized_file_means_nothing_is_approved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("approvals.json");
        let mut text = String::from("{\"approved\": {\"script:a\": \"x\"}}");
        text.push_str(&" ".repeat(MAX_STATE_BYTES as usize));
        fs::write(&path, text).unwrap();
        let store = ApprovalStore::new(path);
        assert!(!store.is_approved("script:a", "x"));
    }

    #[test]
    fn revoking_forgets_only_the_named_plugins() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("approvals.json");
        let store = ApprovalStore::new(path.clone());
        // Nothing recorded yet: revoking is a no-op and creates no file.
        store.revoke(&["script:a".to_owned()]).unwrap();
        assert!(!path.exists());

        store.approve("script:a", "python a.py").unwrap();
        store.approve("script:b", "python b.py").unwrap();
        store
            .revoke(&["script:a".to_owned(), "script:unknown".to_owned()])
            .unwrap();
        assert!(!store.is_approved("script:a", "python a.py"));
        assert!(store.is_approved("script:b", "python b.py"));
    }

    #[test]
    fn a_corrupt_file_means_nothing_is_approved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("approvals.json");
        fs::write(&path, "{ not json").unwrap();
        let store = ApprovalStore::new(path);
        assert!(!store.is_approved("script:a", "x"));
        store.approve("script:a", "x").unwrap();
        assert!(store.is_approved("script:a", "x"));
    }

    #[test]
    fn an_old_format_file_is_read_but_never_matches_and_is_upgraded_on_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("approvals.json");
        // Format 1: no version, script keys were the command line.
        fs::write(
            &path,
            r#"{"approved":{"script:a":"Persistent/Sevak: python main.py","workflow:w":"sha256:abc"}}"#,
        )
        .unwrap();
        let store = ApprovalStore::new(path.clone());
        assert!(store.has_record("script:a"), "so the dialog can say why");
        assert!(!store.is_approved("script:a", "v2:sha256:abc"));
        assert!(store.is_approved("workflow:w", "sha256:abc"), "kept as is");

        store.approve("script:a", "v2:sha256:def").unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"version\": 2"), "{text}");
        assert!(store.is_approved("script:a", "v2:sha256:def"));
        assert!(store.is_approved("workflow:w", "sha256:abc"));
    }

    #[test]
    fn the_hasher_separates_its_pieces() {
        let hash = |pieces: &[(&str, &str)]| {
            let mut hasher = ContentHasher::new();
            for (label, bytes) in pieces {
                hasher.feed(label, bytes.as_bytes());
            }
            hasher.finish()
        };
        let a = hash(&[("x", "ab"), ("y", "c")]);
        assert_eq!(a, hash(&[("x", "ab"), ("y", "c")]));
        assert_ne!(
            a,
            hash(&[("x", "a"), ("y", "bc")]),
            "moving a byte changes it"
        );
        assert_ne!(a, hash(&[("y", "ab"), ("x", "c")]));
        assert!(a.starts_with("sha256:") && a.len() == "sha256:".len() + 64);
    }

    #[test]
    fn a_file_is_hashed_by_content_and_a_missing_one_differs_from_an_empty_one() {
        let dir = tempfile::tempdir().unwrap();
        let hash = |name: &str| {
            let mut hasher = ContentHasher::new();
            hasher.file(dir.path(), name);
            hasher.finish()
        };
        let missing = hash("main.py");
        fs::write(dir.path().join("main.py"), "").unwrap();
        let empty = hash("main.py");
        assert_ne!(missing, empty);
        fs::write(dir.path().join("main.py"), "print(1)").unwrap();
        let one = hash("main.py");
        assert_ne!(empty, one);
        fs::write(dir.path().join("main.py"), "print(2)").unwrap();
        assert_ne!(one, hash("main.py"));
        assert_ne!(
            hash("../escape.py"),
            hash("main.py"),
            "a path that leaves the folder is never read"
        );
    }
}
