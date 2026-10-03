//! Well-known Sevak directories.
//!
//! | Purpose | Windows                         | Linux                          |
//! |---------|---------------------------------|--------------------------------|
//! | config  | `%APPDATA%\sevak\config.toml`   | `~/.config/sevak/config.toml`  |
//! | data    | `%APPDATA%\sevak\`              | `~/.local/share/sevak/`        |
//! | logs    | `<data>\logs\`                  | `<data>/logs/`                 |

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_below_roots() {
        let paths = AppPaths::with_roots(PathBuf::from("cfg"), PathBuf::from("data"));
        assert_eq!(paths.config_file, PathBuf::from("cfg/sevak/config.toml"));
        assert_eq!(paths.log_dir, PathBuf::from("data/sevak/logs"));
        assert_eq!(paths.usage_file, PathBuf::from("data/sevak/usage.json"));
    }
}
