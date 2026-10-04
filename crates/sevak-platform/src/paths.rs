//! Well-known Sevak directories.
//!
//! | Purpose | Windows                         | macOS                                            | Linux                          |
//! |---------|---------------------------------|--------------------------------------------------|--------------------------------|
//! | config  | `%APPDATA%\sevak\config.toml`   | `~/Library/Application Support/sevak/config.toml` | `~/.config/sevak/config.toml`  |
//! | data    | `%APPDATA%\sevak\`              | `~/Library/Application Support/sevak/`            | `~/.local/share/sevak/`        |
//! | logs    | `<data>\logs\`                  | `<data>/logs/`                                   | `<data>/logs/`                 |
//!
//! The config directory can be replaced with `--config <path>` or
//! `SEVAK_CONFIG_DIR`, the data directory with `SEVAK_DATA_DIR`.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::error::{PlatformError, Result};

const APP_DIR: &str = "sevak";
const CONFIG_FILE: &str = "config.toml";

/// Environment variable naming the config directory (see [`AppPaths::resolve`]).
pub const CONFIG_DIR_ENV: &str = "SEVAK_CONFIG_DIR";
/// Environment variable naming the data directory (usage statistics, logs).
pub const DATA_DIR_ENV: &str = "SEVAK_DATA_DIR";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub data_dir: PathBuf,
    pub log_dir: PathBuf,
    pub usage_file: PathBuf,
}

impl AppPaths {
    /// Resolves the directories from the OS conventions and the environment:
    /// `SEVAK_CONFIG_DIR` and `SEVAK_DATA_DIR` replace the config and data
    /// directories. Nothing is created on disk; callers create directories when
    /// they first write to them.
    pub fn resolve() -> Result<Self> {
        Self::resolve_with_config(None)
    }

    /// Like [`AppPaths::resolve`], with a `--config` path that takes
    /// precedence over `SEVAK_CONFIG_DIR`.
    pub fn resolve_with_config(config: Option<&Path>) -> Result<Self> {
        let env_path = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
        let config = config
            .map(Path::to_path_buf)
            .or_else(|| env_path(CONFIG_DIR_ENV).map(PathBuf::from));
        let data = env_path(DATA_DIR_ENV).map(PathBuf::from);
        let base = std::env::current_dir().unwrap_or_default();
        let home = dirs::home_dir();

        let (config_dir, config_file) = match config {
            Some(path) => Self::config_location(&path, &base, home.as_deref()),
            None => {
                let root = dirs::config_dir().ok_or(PlatformError::MissingDirectory("config"))?;
                let dir = root.join(APP_DIR);
                let file = dir.join(CONFIG_FILE);
                (dir, file)
            }
        };
        let data_dir = match data {
            Some(path) => absolute(&path, &base, home.as_deref()),
            None => dirs::data_dir()
                .ok_or(PlatformError::MissingDirectory("data"))?
                .join(APP_DIR),
        };
        Ok(Self::from_parts(config_dir, config_file, data_dir))
    }

    /// Builds the layout below explicit roots (used by tests and portable setups).
    pub fn with_roots(config_root: PathBuf, data_root: PathBuf) -> Self {
        let config_dir = config_root.join(APP_DIR);
        let config_file = config_dir.join(CONFIG_FILE);
        Self::from_parts(config_dir, config_file, data_root.join(APP_DIR))
    }

    fn from_parts(config_dir: PathBuf, config_file: PathBuf, data_dir: PathBuf) -> Self {
        Self {
            config_file,
            config_dir,
            log_dir: data_dir.join("logs"),
            usage_file: data_dir.join("usage.json"),
            data_dir,
        }
    }

    /// Creates the data and log folders so that only their owner can read them
    /// (0700 on Unix; on Windows they inherit the profile's own access), and
    /// makes the ones Sevak itself named (`sevak`, `logs`) owner-only if an
    /// earlier version created them wider. The config folder is left to the
    /// user's own settings. Failures are returned, not fatal: the caller logs.
    pub fn ensure_private_dirs(&self) -> std::io::Result<()> {
        for dir in [&self.data_dir, &self.log_dir] {
            crate::private_file::create_private_dir_all(dir)?;
            let ours = dir
                .file_name()
                .is_some_and(|name| name == OsStr::new(APP_DIR) || name == OsStr::new("logs"));
            if ours {
                crate::private_file::restrict_dir(dir)?;
            }
        }
        Ok(())
    }

    /// Where a `--config` / `SEVAK_CONFIG_DIR` value puts the config:
    /// `(config directory, config file)`. A path ending in `.toml` (that is not
    /// an existing directory) names the config file itself and its parent is
    /// the config directory; any other path is the config directory and the
    /// file is `config.toml` inside it. `~` is expanded and a relative path is
    /// taken relative to `base`.
    pub fn config_location(path: &Path, base: &Path, home: Option<&Path>) -> (PathBuf, PathBuf) {
        let path = absolute(path, base, home);
        let is_file = path
            .extension()
            .and_then(OsStr::to_str)
            .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"))
            && !path.is_dir();
        if is_file {
            let dir = path
                .parent()
                .map_or_else(|| base.to_path_buf(), Path::to_path_buf);
            (dir, path)
        } else {
            let file = path.join(CONFIG_FILE);
            (path, file)
        }
    }
}

/// Expands a leading `~` and anchors relative paths at `base`. The path is not
/// canonicalized: it may not exist yet.
fn absolute(path: &Path, base: &Path, home: Option<&Path>) -> PathBuf {
    let expanded = match (path.strip_prefix("~"), home) {
        (Ok(rest), Some(home)) => home.join(rest),
        _ => path.to_path_buf(),
    };
    if expanded.is_absolute() {
        expanded
    } else {
        base.join(expanded)
    }
}

/// Shows `path` with the home directory abbreviated to `~` (always with `/`
/// separators, which Sevak's `~` expansion accepts on every platform).
/// Paths outside the home directory are returned unchanged.
pub fn home_relative(path: &std::path::Path) -> String {
    tilde_path(path, dirs::home_dir().as_deref())
}

fn tilde_path(path: &std::path::Path, home: Option<&std::path::Path>) -> String {
    let Some(rest) = home.and_then(|home| path.strip_prefix(home).ok()) else {
        return path.display().to_string();
    };
    let parts: Vec<_> = rest
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    if parts.is_empty() {
        "~".to_owned()
    } else {
        format!("~/{}", parts.join("/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_data_and_log_folders_are_created() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_roots(root.path().join("config"), root.path().join("data"));
        paths.ensure_private_dirs().unwrap();
        assert!(paths.data_dir.is_dir());
        assert!(paths.log_dir.is_dir());
        assert!(
            !paths.config_dir.exists(),
            "the config folder is not ours to create"
        );
        // Again, with everything there.
        paths.ensure_private_dirs().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn the_data_and_log_folders_are_owner_only_even_if_an_earlier_version_made_them_wider() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_roots(root.path().join("config"), root.path().join("data"));
        std::fs::create_dir_all(&paths.log_dir).unwrap();
        for dir in [&paths.data_dir, &paths.log_dir] {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        paths.ensure_private_dirs().unwrap();
        assert_eq!(mode(&paths.data_dir), 0o700);
        assert_eq!(mode(&paths.log_dir), 0o700);

        // Fresh ones too.
        let fresh = AppPaths::with_roots(root.path().join("c2"), root.path().join("d2"));
        fresh.ensure_private_dirs().unwrap();
        assert_eq!(mode(&fresh.data_dir), 0o700);
        assert_eq!(mode(&fresh.log_dir), 0o700);
    }

    #[test]
    fn home_is_abbreviated() {
        let home = std::path::Path::new("home").join("me");
        assert_eq!(tilde_path(&home, Some(&home)), "~");
        assert_eq!(
            tilde_path(&home.join("Documents").join("Work"), Some(&home)),
            "~/Documents/Work"
        );
        let elsewhere = std::path::Path::new("data").join("shared");
        assert_eq!(
            tilde_path(&elsewhere, Some(&home)),
            elsewhere.display().to_string()
        );
        assert_eq!(tilde_path(&home, None), home.display().to_string());
    }

    #[test]
    fn config_location_for_directories_and_files() {
        let tmp = std::env::temp_dir();
        let base = tmp.join("base");
        let base = base.as_path();
        let home = tmp.join("home").join("me");
        let dir = std::env::temp_dir().join("sevak-paths-test-dir.toml");
        std::fs::create_dir_all(&dir).unwrap();

        // A directory (even one named *.toml) holds config.toml.
        let (config_dir, file) = AppPaths::config_location(&dir, base, None);
        assert_eq!(config_dir, dir);
        assert_eq!(file, dir.join("config.toml"));
        std::fs::remove_dir(&dir).unwrap();

        // A plain name is a directory, relative to `base`.
        let (config_dir, file) = AppPaths::config_location(Path::new("sync/sevak"), base, None);
        assert_eq!(config_dir, base.join("sync/sevak"));
        assert_eq!(file, base.join("sync/sevak").join("config.toml"));

        // A *.toml path names the file; its folder is the config directory.
        let (config_dir, file) = AppPaths::config_location(Path::new("work.TOML"), base, None);
        assert_eq!(config_dir, base);
        assert_eq!(file, base.join("work.TOML"));

        // `~` is expanded.
        let (config_dir, file) =
            AppPaths::config_location(Path::new("~/Dropbox/sevak"), base, Some(&home));
        assert_eq!(config_dir, home.join("Dropbox/sevak"));
        assert_eq!(file, home.join("Dropbox/sevak/config.toml"));
        let (config_dir, _) = AppPaths::config_location(Path::new("~"), base, Some(&home));
        assert_eq!(config_dir, home);
    }

    #[test]
    fn config_override_keeps_data_in_its_usual_place() {
        let dir = std::env::temp_dir().join("sevak-paths-test-cfg");
        let paths = AppPaths::resolve_with_config(Some(&dir)).unwrap();
        assert_eq!(paths.config_dir, dir);
        assert_eq!(paths.config_file, dir.join("config.toml"));
        assert_ne!(paths.data_dir, dir);
        assert_eq!(paths.usage_file, paths.data_dir.join("usage.json"));
        assert_eq!(paths.log_dir, paths.data_dir.join("logs"));
    }

    #[test]
    fn layout_below_roots() {
        let paths = AppPaths::with_roots(PathBuf::from("cfg"), PathBuf::from("data"));
        assert_eq!(paths.config_file, PathBuf::from("cfg/sevak/config.toml"));
        assert_eq!(paths.log_dir, PathBuf::from("data/sevak/logs"));
        assert_eq!(paths.usage_file, PathBuf::from("data/sevak/usage.json"));
    }
}
