//! Well-known Sevak directories.
//!
//! | Purpose | Windows                         | macOS                                            | Linux                          |
//! |---------|---------------------------------|--------------------------------------------------|--------------------------------|
//! | config  | `%APPDATA%\sevak\config.toml`   | `~/Library/Application Support/sevak/config.toml` | `~/.config/sevak/config.toml`  |
//! | data    | `%APPDATA%\sevak\`              | `~/Library/Application Support/sevak/`            | `~/.local/share/sevak/`        |
//! | logs    | `<data>\logs\`                  | `<data>/logs/`                                   | `<data>/logs/`                 |

use std::path::PathBuf;

use crate::error::{PlatformError, Result};

const APP_DIR: &str = "sevak";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub data_dir: PathBuf,
    pub log_dir: PathBuf,
    pub usage_file: PathBuf,
}

impl AppPaths {
    /// Resolves the directories from the OS conventions. Nothing is created on
    /// disk; callers create directories when they first write to them.
    pub fn resolve() -> Result<Self> {
        let config_root = dirs::config_dir().ok_or(PlatformError::MissingDirectory("config"))?;
        let data_root = dirs::data_dir().ok_or(PlatformError::MissingDirectory("data"))?;
        Ok(Self::with_roots(config_root, data_root))
    }

    /// Builds the layout below explicit roots (used by tests and portable setups).
    pub fn with_roots(config_root: PathBuf, data_root: PathBuf) -> Self {
        let config_dir = config_root.join(APP_DIR);
        let data_dir = data_root.join(APP_DIR);
        Self {
            config_file: config_dir.join("config.toml"),
            log_dir: data_dir.join("logs"),
            usage_file: data_dir.join("usage.json"),
            config_dir,
            data_dir,
        }
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
    fn layout_below_roots() {
        let paths = AppPaths::with_roots(PathBuf::from("cfg"), PathBuf::from("data"));
        assert_eq!(paths.config_file, PathBuf::from("cfg/sevak/config.toml"));
        assert_eq!(paths.log_dir, PathBuf::from("data/sevak/logs"));
        assert_eq!(paths.usage_file, PathBuf::from("data/sevak/usage.json"));
    }
}
