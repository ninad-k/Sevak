//! Automatic backups: off by default, a few lines in `backup.toml` when on.
//!
//! The schedule is deliberately plain: a backup is due when the last automatic
//! one is a day (or a week) old, or when Sevak has been updated since it last
//! looked. The shell calls [`run_due`] a minute after start and then every
//! half hour; it does nothing unless something is due. Only files this module
//! made (`sevak-auto-*.sevakbackup`) are ever pruned.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::backup::{create, BackupReport, Destination};
use crate::category::Category;
use crate::error::{Error, Result};
use crate::manifest::{Kind, EXTENSION};
use crate::state::State;
use crate::util::{Roots, Stamp};

/// The options file, next to `config.toml`.
pub const SCHEDULE_FILE: &str = "backup.toml";
/// What automatic backups are called.
pub const AUTO_PREFIX: &str = "sevak-auto";
/// What a manual backup in the default folder is called.
pub const MANUAL_PREFIX: &str = "sevak-backup";
pub const DEFAULT_KEEP: usize = 5;
pub const MAX_KEEP: usize = 50;

const DAY: i64 = 24 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Schedule {
    Daily,
    Weekly,
    /// Any other value in the file means off, the safe choice.
    #[default]
    #[serde(other)]
    Off,
}

impl Schedule {
    pub fn id(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Daily => "daily",
            Self::Weekly => "weekly",
        }
    }

    fn interval(self) -> Option<i64> {
        match self {
            Self::Off => None,
            Self::Daily => Some(DAY),
            Self::Weekly => Some(7 * DAY),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoConfig {
    pub schedule: Schedule,
    /// Also make a backup the first time a new version of Sevak starts.
    pub on_update: bool,
    /// How many automatic backups to keep.
    pub keep: usize,
    /// Where they go; empty means the default folder.
    pub folder: String,
}

impl Default for AutoConfig {
    fn default() -> Self {
        Self {
            schedule: Schedule::Off,
            on_update: false,
            keep: DEFAULT_KEEP,
            folder: String::new(),
        }
    }
}

impl AutoConfig {
    /// Whether any automatic backup is switched on.
    pub fn enabled(&self) -> bool {
        self.schedule != Schedule::Off || self.on_update
    }

    pub fn normalized(mut self) -> Self {
        self.keep = self.keep.clamp(1, MAX_KEEP);
        self.folder = self.folder.trim().to_owned();
        self
    }

    /// Reads `backup.toml`. A missing file means everything off; a damaged one
    /// too, with the reason returned so it can be shown.
    pub fn load(roots: &Roots) -> (Self, Option<String>) {
        match fs::read_to_string(roots.schedule_file()) {
            Ok(text) => match toml::from_str::<Self>(&text) {
                Ok(config) => (config.normalized(), None),
                Err(err) => (
                    Self::default(),
                    Some(format!(
                        "backup.toml has a mistake ({}); automatic backups are off until it is fixed",
                        err.message()
                    )),
                ),
            },
            Err(_) => (Self::default(), None),
        }
    }

    /// The file's text, with its comments.
    pub fn render(&self) -> String {
        let folder = toml::Value::String(self.folder.clone());
        format!(
            "# Sevak automatic backups. Edit here or in Settings, Backup & restore.\n\
             # Nothing is backed up automatically unless you turn it on.\n\
             \n\
             # \"off\", \"daily\" or \"weekly\".\n\
             schedule = \"{}\"\n\
             \n\
             # Also back up the first time a new version of Sevak starts.\n\
             on_update = {}\n\
             \n\
             # How many automatic backups to keep (1-{MAX_KEEP}); older ones are deleted.\n\
             keep = {}\n\
             \n\
             # Where they go. \"\" means the \"Sevak backups\" folder in your Documents folder.\n\
             # \"~\" is your home folder.\n\
             folder = {folder}\n",
            self.schedule.id(),
            self.on_update,
            self.keep,
        )
    }

    pub fn save(&self, roots: &Roots) -> Result<()> {
        let config = self.clone().normalized();
        // Parse back what we are about to write, so a bad folder never lands on disk.
        let text = config.render();
        toml::from_str::<Self>(&text)
            .map_err(|err| Error::Current(format!("cannot write backup.toml: {err}")))?;
        sevak_platform::private_file::write_atomic(&roots.schedule_file(), text.as_bytes())
            .map_err(|err| Error::io("cannot save backup.toml", err))
    }

    /// The folder automatic backups go to.
    pub fn resolve_folder(&self) -> Result<PathBuf> {
        resolve_folder(&self.folder)
    }
}

/// The folder "Back up now" and the automatic backups use unless told otherwise:
/// `Sevak backups` inside the Documents folder (or the home folder when there is
/// none).
pub fn default_folder() -> PathBuf {
    dirs::document_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Sevak backups")
}

/// Expands `~`; empty means [`default_folder`]; anything else must be absolute.
pub fn resolve_folder(text: &str) -> Result<PathBuf> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(default_folder());
    }
    let path = match text.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => dirs::home_dir()
            .ok_or_else(|| Error::Current("your home folder could not be found".to_owned()))?
            .join(rest.trim_start_matches(['/', '\\'])),
        _ => PathBuf::from(text),
    };
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(Error::Current(format!(
            "the backup folder \"{text}\" must be a full path (or start with ~)"
        )))
    }
}

/// Why a backup is due.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Schedule,
    Update,
}

/// Whether an automatic backup is due.
pub fn due(config: &AutoConfig, state: &State, now_unix: i64, app_version: &str) -> Option<Reason> {
    if config.on_update
        && state
            .last_seen_version
            .as_deref()
            .is_some_and(|seen| seen != app_version)
    {
        return Some(Reason::Update);
    }
    let interval = config.schedule.interval()?;
    match state.last_auto_unix {
        None => Some(Reason::Schedule),
        Some(last) if now_unix.saturating_sub(last) >= interval => Some(Reason::Schedule),
        Some(_) => None,
    }
}

/// Deletes all but the newest `keep` automatic backups in `folder`. Returns how
/// many were deleted. Other files are never touched.
pub fn prune(folder: &Path, keep: usize) -> usize {
    let Ok(entries) = fs::read_dir(folder) else {
        return 0;
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with(&format!("{AUTO_PREFIX}-"))
                        && name.ends_with(&format!(".{EXTENSION}"))
                })
        })
        .collect();
    files.sort();
    let excess = files.len().saturating_sub(keep.max(1));
    files
        .iter()
        .take(excess)
        .filter(|path| fs::remove_file(path).is_ok())
        .count()
}

/// Makes the automatic backup if one is due, prunes, and remembers. `None` when
/// nothing was due.
pub fn run_due(roots: &Roots, stamp: &Stamp, app_version: &str) -> Result<Option<BackupReport>> {
    let (config, _) = AutoConfig::load(roots);
    let mut state = State::load(roots);
    let reason = due(&config, &state, stamp.unix, app_version);

    // The version is recorded whether or not anything was due, so switching
    // "on update" on later does not back up for an update that is long past.
    let remember_version = |roots: &Roots| {
        let mut latest = State::load(roots);
        latest.last_seen_version = Some(app_version.to_owned());
        if let Err(err) = latest.save(roots) {
            tracing::warn!("could not record the Sevak version: {err}");
        }
    };

    if reason.is_none() {
        if state.last_seen_version.as_deref() != Some(app_version) {
            state.last_seen_version = Some(app_version.to_owned());
            remember_version(roots);
        }
        return Ok(None);
    }

    let folder = config.resolve_folder()?;
    let report = create(
        roots,
        &Category::ALL,
        Kind::Auto,
        stamp,
        Destination::Folder {
            dir: &folder,
            prefix: AUTO_PREFIX,
        },
    )?;
    let removed = prune(&folder, config.keep);
    tracing::info!(path = %report.path.display(), pruned = removed, "automatic backup made");
    remember_version(roots);
    Ok(Some(report))
}
