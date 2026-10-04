//! Small shared helpers: where things live, timestamps, hashing.

use std::fmt::Write as _;
use std::path::PathBuf;

use chrono::{DateTime, FixedOffset, Local, SecondsFormat};
use serde::Serialize;
use sevak_platform::AppPaths;
use sha2::{Digest, Sha256};

/// The folders a backup reads from and a restore writes to. Plain paths, so
/// tests can point it at a temporary directory.
#[derive(Debug, Clone)]
pub struct Roots {
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub data_dir: PathBuf,
}

/// Where the user's decision about which scripts may run is kept (in the data
/// folder, so it is never part of the configuration or of a backup). The same
/// file `sevak_plugins` reads; the shell passes this path to both hosts.
pub const APPROVALS_FILE: &str = "script-plugin-approvals.json";

impl Roots {
    pub fn from_paths(paths: &AppPaths) -> Self {
        Self {
            config_dir: paths.config_dir.clone(),
            config_file: paths.config_file.clone(),
            data_dir: paths.data_dir.clone(),
        }
    }

    pub fn approvals_file(&self) -> PathBuf {
        self.data_dir.join(APPROVALS_FILE)
    }

    /// Where the safety copies made before a restore are kept.
    pub fn snapshots_dir(&self) -> PathBuf {
        self.data_dir.join("backup-snapshots")
    }

    /// When the last backup was made, and similar bookkeeping.
    pub fn state_file(&self) -> PathBuf {
        self.data_dir.join("backup-state.json")
    }

    /// The options of automatic backups (hand-editable).
    pub fn schedule_file(&self) -> PathBuf {
        self.config_dir.join(crate::auto::SCHEDULE_FILE)
    }

    pub fn themes_dir(&self) -> PathBuf {
        self.config_dir.join(crate::category::THEMES_DIR)
    }

    pub fn plugins_dir(&self) -> PathBuf {
        self.config_dir.join(crate::category::PLUGINS_DIR)
    }

    pub fn workflows_dir(&self) -> PathBuf {
        self.config_dir.join(crate::category::WORKFLOWS_DIR)
    }
}

/// A moment, in the forms a backup needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Stamp {
    pub unix: i64,
    /// `20261004-153045`, for file names.
    pub compact: String,
    /// `2026-10-04T15:30:45+05:30`.
    pub iso: String,
}

impl Stamp {
    pub fn now() -> Self {
        Self::at(Local::now().fixed_offset())
    }

    pub fn at(moment: DateTime<FixedOffset>) -> Self {
        Self {
            unix: moment.timestamp(),
            compact: moment.format("%Y%m%d-%H%M%S").to_string(),
            iso: moment.to_rfc3339_opts(SecondsFormat::Secs, false),
        }
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// `windows`, `macos` or `linux`: where the backup was made.
pub fn platform_name() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// Writes `bytes` to a new file `<dir>/<stem>.<ext>` (`<stem>-2.<ext>`, ... if
/// that exists), readable by the owner only on Unix. Returns the path.
pub fn write_unique(
    dir: &std::path::Path,
    stem: &str,
    ext: &str,
    bytes: &[u8],
) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let mut n = 1u32;
    loop {
        let name = if n == 1 {
            format!("{stem}.{ext}")
        } else {
            format!("{stem}-{n}.{ext}")
        };
        let path = dir.join(name);
        if std::fs::symlink_metadata(&path).is_err() {
            sevak_platform::private_file::write_atomic(&path, bytes)?;
            return Ok(path);
        }
        n += 1;
        if n > 1000 {
            return Err(std::io::Error::other("too many backups with the same name"));
        }
    }
}
