//! Backup and restore of Sevak's settings.
//!
//! One action writes a single portable archive (`.sevakbackup`, a zip with a
//! versioned `manifest.json` and the user's configuration files); another reads
//! one back, shows what would change, and applies it all-or-nothing after
//! taking a safety copy so the restore can be undone.
//!
//! This crate has no Tauri or UI code, so everything here is tested with
//! temporary folders. The shell (`src-tauri/src/backup.rs`) adds the file
//! dialogs, the Settings page and the scheduler.
//!
//! | Module | Concern |
//! |---|---|
//! | [`category`] | the categories and the **allowlist** of what can be in a backup |
//! | [`collect`] | reading the current configuration into backup items |
//! | [`archive`] | writing and (distrustfully) reading the zip |
//! | [`plan`], [`restore`], `apply` | preview, merge or replace, snapshot, atomic swap, undo |
//! | [`auto`] | `backup.toml`, the schedule, retention |
//! | [`state`] | when the last backup was made |
//!
//! Never in a backup, by construction (the allowlist does not name them): AI
//! and other API keys, clipboard history, the history database, tokens,
//! anything of the credential store or 1Password, usage statistics and logs,
//! and the record of which scripts the user allowed to run.

pub mod archive;
pub mod auto;
pub mod backup;
pub mod category;
pub mod collect;
pub mod error;
pub mod item;
pub mod limits;
pub mod manifest;
pub mod plan;
pub mod restore;
pub mod state;
pub mod util;

mod apply;

#[cfg(test)]
mod tests;

pub use archive::{read, read_file, ArchiveInfo, Backup};
pub use auto::{AutoConfig, Schedule};
pub use backup::{
    contents, create, default_file_name, BackupReport, CategoryContents, Contents, Destination,
};
pub use category::{Category, Mode};
pub use error::{Error, Result};
pub use manifest::{Kind, Manifest, EXTENSION, FORMAT_VERSION};
pub use plan::{CategoryPreview, Change, ItemPreview};
pub use restore::{
    latest_snapshot, preview, restore, undo_restore, Preview, RestoreOptions, RestoreReport,
    SnapshotInfo,
};
pub use state::State;
pub use util::{Roots, Stamp};
