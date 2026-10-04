//! Bookkeeping about backups: when the last one was made and by what. Kept in
//! the data folder, never inside a backup.

use serde::{Deserialize, Serialize};

use crate::category::Category;
use crate::error::{Error, Result};
use crate::manifest::Kind;
use crate::util::Roots;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastBackup {
    pub path: String,
    pub unix: i64,
    pub created: String,
    pub kind: Kind,
    pub categories: Vec<Category>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    /// The last backup the user (or the schedule) made; restore snapshots do
    /// not count.
    pub last: Option<LastBackup>,
    /// When the last automatic backup ran.
    pub last_auto_unix: Option<i64>,
    /// The Sevak version that last looked at the schedule; a different one
    /// means Sevak was updated.
    pub last_seen_version: Option<String>,
}

impl State {
    /// Reads the file; a missing or damaged one is an empty state.
    pub fn load(roots: &Roots) -> Self {
        std::fs::read_to_string(roots.state_file())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, roots: &Roots) -> Result<()> {
        let text = serde_json::to_string_pretty(self)
            .map_err(|err| Error::Current(format!("cannot write the backup state: {err}")))?;
        sevak_platform::private_file::write_atomic(&roots.state_file(), text.as_bytes())
            .map_err(|err| Error::io("cannot save the backup state", err))
    }
}
