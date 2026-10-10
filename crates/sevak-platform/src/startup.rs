//! Per-user launch at sign-in. This module never launches Sevak immediately.
//!
//! Settings and the installer use the same transaction: change the OS entry,
//! persist the preference, and restore the previous entry if either step fails.
//! Routine synchronization refreshes moved executables but respects an entry
//! disabled through Windows Startup Apps, macOS launchctl, or Linux autostart.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use sevak_core::bounded_read::{read_to_string_capped, MAX_CONFIG_BYTES};
use sevak_core::config::{Config, DEFAULT_CONFIG_TOML};

#[cfg(any(not(windows), test))]
mod files;
#[cfg(windows)]
mod windows;

pub const ENTRY_NAME: &str = "Sevak";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Status {
    /// An entry exists, even if the OS has disabled it.
    pub registered: bool,
    pub enabled: bool,
}

/// Injectable boundary: integration tests use an isolated backend and never
/// change the developer's startup applications or profile.
pub trait Backend {
    type Snapshot;
    fn snapshot(&self) -> Result<Self::Snapshot, String>;
    fn status(&self, snapshot: &Self::Snapshot) -> Result<Status, String>;
    /// `explicit` permits re-enabling a user-disabled entry only after consent
    /// through the checkbox or `--set-startup on`.
    fn set_enabled(&self, enabled: bool, explicit: bool) -> Result<(), String>;
    fn restore(&self, snapshot: &Self::Snapshot) -> Result<(), String>;
    /// Uninstalling an older copy must not remove a newer copy's entry.
    fn remove_owned(&self) -> Result<(), String>;
    /// Update only the executable in an existing entry, keeping arguments and
    /// OS approval untouched. An absent entry stays absent.
    fn refresh_target(&self, snapshot: &Self::Snapshot) -> Result<(), String>;
}

pub fn status(backend: &impl Backend) -> Result<Status, String> {
    backend.status(&backend.snapshot()?)
}

pub fn refresh(backend: &impl Backend) -> Result<(), String> {
    let before = backend.snapshot()?;
    if !backend.status(&before)?.registered {
        return Ok(());
    }
    backend
        .refresh_target(&before)
        .map_err(|error| rollback_error(backend, &before, error))
}

/// Saving unrelated preferences does not depend on OS startup permissions.
/// Construct the backend lazily so a denied registry or unavailable desktop
/// cannot prevent a theme or shortcut change.
pub fn save_settings<B: Backend>(
    previous: bool,
    wanted: bool,
    backend: impl FnOnce() -> Result<B, String>,
    persist: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    if previous == wanted {
        persist()
    } else {
        update(&backend()?, wanted, true, persist)
    }
}

/// Change startup and then commit a settings write. A failed OS operation is
/// also rolled back because it may have partially changed an entry.
pub fn update<B: Backend>(
    backend: &B,
    wanted: bool,
    explicit: bool,
    persist: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let before = backend.snapshot()?;
    let state = backend.status(&before)?;
    let change = if wanted {
        explicit || state.enabled || !state.registered
    } else {
        state.registered
    };
    if change {
        let result = backend.set_enabled(wanted, explicit).and_then(|()| {
            let actual = status(backend)?;
            if (wanted && !actual.enabled) || (!wanted && actual.registered) {
                Err("the operating system did not accept the startup change".to_owned())
            } else {
                Ok(())
            }
        });
        if let Err(error) = result {
            return Err(rollback_error(backend, &before, error));
        }
    }
    if let Err(error) = persist() {
        return Err(if change {
            rollback_error(backend, &before, error)
        } else {
            error
        });
    }
    Ok(())
}

fn rollback_error<B: Backend>(backend: &B, before: &B::Snapshot, error: String) -> String {
    match backend.restore(before) {
        Ok(()) => error,
        Err(rollback) => {
            format!("{error}. Restoring the previous startup entry also failed: {rollback}")
        }
    }
}

/// Installer/CLI update. Only this key is edited; comments, unknown keys and
/// even untouched values outside Sevak's supported ranges remain byte-for-byte
/// as supplied by the user (apart from TOML's normal formatting around edits).
pub fn set_preference(
    config_file: &Path,
    wanted: bool,
    backend: &impl Backend,
) -> Result<(), String> {
    let existing = match read_to_string_capped(config_file, MAX_CONFIG_BYTES) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => DEFAULT_CONFIG_TOML.to_owned(),
        Err(error) => return Err(format!("cannot read startup preference: {error}")),
    };
    // Reject malformed typed settings before touching the registration.
    Config::from_toml_str(&existing).map_err(|error| format!("invalid configuration: {error}"))?;
    let text = preference_document(&existing, wanted)?;
    update(backend, wanted, true, || {
        write_atomic(config_file, text.as_bytes())
            .map_err(|error| format!("cannot save startup preference: {error}"))
    })
}

fn preference_document(existing: &str, wanted: bool) -> Result<String, String> {
    let mut document = existing
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| format!("invalid configuration: {error}"))?;
    if document.get("general").is_none() {
        document["general"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let general = document["general"]
        .as_table_like_mut()
        .ok_or_else(|| "general must be a TOML table".to_owned())?;
    let mut value = toml_edit::Value::from(wanted);
    if let Some(old) = general
        .get("launch_at_login")
        .and_then(toml_edit::Item::as_value)
    {
        *value.decor_mut() = old.decor().clone();
    }
    general.insert("launch_at_login", toml_edit::Item::Value(value));
    let text = document.to_string();
    Ok(if existing.contains("\r\n") {
        text.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        text
    })
}

/// Atomic replacement with a unique temporary file (no shared `.tmp` path).
pub fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(contents)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

/// Stable executable and arguments shared by all OS backends.
#[derive(Clone, Debug)]
pub struct LaunchCommand {
    pub executable: PathBuf,
    pub config_file: Option<PathBuf>,
}

impl LaunchCommand {
    pub fn current(config_file: Option<&Path>) -> Result<Self, String> {
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        #[cfg(target_os = "linux")]
        let executable = stable_executable(&executable, std::env::var_os("APPIMAGE").as_deref())?;
        Self::new(executable, config_file.map(Path::to_path_buf))
    }

    pub fn new(executable: PathBuf, config_file: Option<PathBuf>) -> Result<Self, String> {
        for path in std::iter::once(&executable).chain(config_file.iter()) {
            if !path.is_absolute() {
                return Err("startup paths must be absolute".to_owned());
            }
            let text = path.to_str().ok_or("startup paths must be valid Unicode")?;
            if text.chars().any(char::is_control) {
                return Err("startup paths cannot contain control characters".to_owned());
            }
        }
        Ok(Self {
            executable,
            config_file,
        })
    }

    fn args(&self) -> Vec<String> {
        let mut args = vec!["--background".to_owned()];
        if let Some(config) = &self.config_file {
            args.extend(["--config".to_owned(), config.to_string_lossy().into_owned()]);
        }
        args
    }
}

#[cfg(any(target_os = "linux", test))]
fn stable_executable(
    executable: &Path,
    appimage: Option<&std::ffi::OsStr>,
) -> Result<PathBuf, String> {
    match appimage.filter(|value| !value.is_empty()) {
        Some(path) => {
            let path = PathBuf::from(path);
            if !path.is_absolute() || !path.is_file() {
                return Err("APPIMAGE must point to the installed AppImage file".to_owned());
            }
            Ok(path)
        }
        None => Ok(executable.to_path_buf()),
    }
}

#[cfg(not(windows))]
pub use files::Native;
#[cfg(windows)]
pub use windows::Native;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_appimage_file_instead_of_temporary_mount() {
        let temp = tempfile::tempdir().unwrap();
        let image = temp.path().join("Sevak latest.AppImage");
        fs::write(&image, []).unwrap();
        assert_eq!(
            stable_executable(Path::new("/tmp/.mount/sevak"), Some(image.as_os_str())).unwrap(),
            image
        );
        assert!(stable_executable(
            Path::new("/tmp/sevak"),
            Some(std::ffi::OsStr::new("relative.AppImage"))
        )
        .is_err());
        assert_eq!(
            stable_executable(Path::new("/usr/bin/sevak"), None).unwrap(),
            Path::new("/usr/bin/sevak")
        );
    }

    #[test]
    fn preference_edit_preserves_crlf_comments_unknown_keys_and_inline_tables() {
        let original = "# mine\r\n[general]\r\nlaunch_at_login = false # keep this\r\nfuture = 'yes'\r\n[window]\r\nwidth = 99999\r\n";
        assert_eq!(
            preference_document(original, true).unwrap(),
            original.replace("false", "true")
        );
        assert!(
            preference_document("general = { launch_at_login = false, future = 1 }\n", true)
                .unwrap()
                .contains("launch_at_login = true")
        );
    }
}
