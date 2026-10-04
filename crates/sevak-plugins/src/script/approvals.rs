//! Which script plugins the user has allowed to run.
//!
//! A script plugin runs arbitrary code with the user's privileges, so a plugin
//! folder that appears in the config directory is not started until the user
//! says yes once. The answer is remembered per plugin id **and** the exact
//! command it runs: change the manifest's command (or mode) and Sevak asks
//! again. The record is a small JSON file in the data directory, so it is
//! separate from the hand-edited config.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
struct Record {
    /// Plugin id to the approved command (see `Manifest::approval_key`).
    #[serde(default)]
    approved: BTreeMap<String, String>,
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
        match fs::read_to_string(&self.path) {
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
        self.read()
            .approved
            .get(id)
            .is_some_and(|approved| approved == key)
    }

    /// Records that plugin `id` may run exactly `key`.
    pub fn approve(&self, id: &str, key: &str) -> io::Result<()> {
        let mut record = self.read();
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
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(record).map_err(io::Error::other)?;
        let mut temp = self.path.as_os_str().to_owned();
        temp.push(".tmp");
        let temp = PathBuf::from(temp);
        let written = fs::write(&temp, text).and_then(|()| fs::rename(&temp, &self.path));
        if written.is_err() {
            let _ = fs::remove_file(&temp);
        }
        written
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_is_per_plugin_and_per_command() {
        let dir = tempfile::tempdir().unwrap();
        let store = ApprovalStore::new(dir.path().join("sub").join("approvals.json"));
        assert!(!store.is_approved("script:a", "python main.py"));

        store.approve("script:a", "python main.py").unwrap();
        assert!(store.is_approved("script:a", "python main.py"));
        assert!(
            !store.is_approved("script:a", "python evil.py"),
            "a changed command asks again"
        );
        assert!(!store.is_approved("script:b", "python main.py"));

        store.approve("script:b", "node b.js").unwrap();
        assert!(
            store.is_approved("script:a", "python main.py"),
            "approvals accumulate"
        );
        assert!(store.is_approved("script:b", "node b.js"));
        assert!(!dir.path().join("sub").join("approvals.json.tmp").exists());
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
}
