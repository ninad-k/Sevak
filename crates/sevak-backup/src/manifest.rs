//! The manifest: the first thing in every backup, and the only thing read
//! before the rest of the archive is trusted.

use serde::{Deserialize, Serialize};

use crate::category::Category;
use crate::error::{Error, Result};

/// The `format` value of every manifest.
pub const FORMAT_NAME: &str = "sevak-backup";
/// The manifest version this build writes and understands. A backup with a
/// higher number is refused with a message to update Sevak; one with a lower
/// number would be migrated here (there is none yet).
pub const FORMAT_VERSION: u32 = 1;
/// Where the manifest sits in the archive.
pub const MANIFEST_PATH: &str = "manifest.json";
/// The file extension of a backup.
pub const EXTENSION: &str = "sevakbackup";

/// Why a backup was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// The user asked for it.
    #[default]
    Manual,
    /// The scheduled or before-an-update backup.
    Auto,
    /// The safety copy taken just before a restore (what "Undo restore" uses).
    Snapshot,
}

/// One file in the archive, with the checksum a restore verifies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub category: Category,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Always [`FORMAT_NAME`].
    pub format: String,
    pub version: u32,
    /// The Sevak that made it.
    pub app_version: String,
    pub created_unix: i64,
    pub created: String,
    /// `windows`, `macos` or `linux`.
    pub platform: String,
    #[serde(default)]
    pub kind: Kind,
    /// The categories this backup covers. A category listed here with no files
    /// means "there was nothing": restoring it in Replace mode clears it.
    pub categories: Vec<Category>,
    pub files: Vec<FileEntry>,
    /// For a safety copy: the categories of the restore it protects.
    #[serde(default)]
    pub restore_of: Vec<Category>,
    /// What this backup never contains, in words (informational).
    #[serde(default)]
    pub never_included: Vec<String>,
}

impl Manifest {
    /// Parses `manifest.json`, refusing what is not ours and what is newer.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let value: serde_json::Value = serde_json::from_slice(bytes)
            .map_err(|err| Error::invalid(format!("the backup's manifest is not valid: {err}")))?;
        if value.get("format").and_then(serde_json::Value::as_str) != Some(FORMAT_NAME) {
            return Err(Error::invalid("this is not a Sevak backup"));
        }
        let version = value
            .get("version")
            .and_then(serde_json::Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| Error::invalid("the backup has no format version"))?;
        if version > FORMAT_VERSION {
            return Err(Error::TooNew {
                found: version,
                supported: FORMAT_VERSION,
            });
        }
        if version == 0 {
            return Err(Error::invalid("the backup has an invalid format version"));
        }
        serde_json::from_value(value)
            .map_err(|err| Error::invalid(format!("the backup's manifest is not valid: {err}")))
    }

    /// The categories as ids, for display.
    pub fn category_ids(&self) -> Vec<&'static str> {
        self.categories.iter().map(|c| c.id()).collect()
    }
}
