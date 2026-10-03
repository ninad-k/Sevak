//! User configuration stored as TOML.
//!
//! Every field has a default and every section is `#[serde(default)]`, so a
//! config written by an older Sevak (or a hand-trimmed one) keeps loading as new
//! options are added. Unknown keys are ignored for the same reason.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The file written on first run. It mirrors [`Config::default`] (enforced by a
/// unit test) but carries comments, which `toml::to_string` cannot produce.
pub const DEFAULT_CONFIG_TOML: &str = r#"# Sevak configuration
#
# Created with default values on first run. Edit it, then choose "Reload index"
# from the tray menu (or restart Sevak) to apply changes.

[general]
# Shortcut that shows and hides Sevak. Examples: "Alt+Space", "Ctrl+Space",
# "Super+Space", "Ctrl+Shift+K".
# On Linux Wayland sessions applications cannot grab global keys. Run
# `sevak --setup-hotkey` to bind this key to `sevak --toggle` in GNOME instead.
hotkey = "Alt+Space"

# Hide the window when it loses focus.
hide_on_blur = true

[window]
# Width of the search window in logical pixels (400-1600).
width = 720

[linux]
# On Wayland sessions, draw Sevak's window through XWayland. Native Wayland
# windows cannot position themselves and may be refused focus, so this keeps the
# launcher centered and typeable. Set to false to use the native Wayland backend.
wayland_use_xwayland = true
"#;

pub const MIN_WINDOW_WIDTH: u32 = 400;
pub const MAX_WINDOW_WIDTH: u32 = 1600;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: GeneralConfig,
    pub window: WindowConfig,
    pub linux: LinuxConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    /// Accelerator string, e.g. `"Alt+Space"`. Parsed by the shell, because the
    /// accepted key names depend on the hotkey backend.
    pub hotkey: String,
    pub hide_on_blur: bool,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            hotkey: "Alt+Space".to_owned(),
            hide_on_blur: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowConfig {
    /// Logical width of the launcher window.
    pub width: u32,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self { width: 720 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LinuxConfig {
    pub wayland_use_xwayland: bool,
}

impl Default for LinuxConfig {
    fn default() -> Self {
        Self {
            wayland_use_xwayland: true,
        }
    }
}

/// How the configuration returned by [`Config::load_or_create`] was obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigOrigin {
    /// No file existed; defaults were written to disk.
    Created,
    /// An existing file was parsed.
    Loaded,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("cannot access config file {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid config file {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: Box<toml::de::Error>,
    },
}

impl Config {
    /// Parses TOML text and normalizes out-of-range values.
    pub fn from_toml_str(text: &str) -> Result<Self, toml::de::Error> {
        let config: Config = toml::from_str(text)?;
        Ok(config.normalized())
    }

    /// Loads the config at `path`, writing [`DEFAULT_CONFIG_TOML`] there first
    /// if the file does not exist yet.
    ///
    /// A file that exists but fails to parse is reported as an error and left
    /// untouched, so a typo never costs the user their settings.
    pub fn load_or_create(path: &Path) -> Result<(Self, ConfigOrigin), ConfigError> {
        let io_err = |source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        };

        match fs::read_to_string(path) {
            Ok(text) => {
                let config = Self::from_toml_str(&text).map_err(|source| ConfigError::Parse {
                    path: path.to_path_buf(),
                    source: Box::new(source),
                })?;
                Ok((config, ConfigOrigin::Loaded))
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(io_err)?;
                }
                fs::write(path, DEFAULT_CONFIG_TOML).map_err(io_err)?;
                Ok((Self::default(), ConfigOrigin::Created))
            }
            Err(err) => Err(io_err(err)),
        }
    }

    /// Clamps values into their supported ranges.
    #[must_use]
    pub fn normalized(mut self) -> Self {
        self.window.width = self.window.width.clamp(MIN_WINDOW_WIDTH, MAX_WINDOW_WIDTH);
        let hotkey = self.general.hotkey.trim();
        self.general.hotkey = if hotkey.is_empty() {
            GeneralConfig::default().hotkey
        } else {
            hotkey.to_owned()
        };
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_template_matches_default_struct() {
        let parsed = Config::from_toml_str(DEFAULT_CONFIG_TOML).expect("template parses");
        assert_eq!(parsed, Config::default());
    }

    #[test]
    fn empty_file_yields_defaults() {
        assert_eq!(Config::from_toml_str("").unwrap(), Config::default());
    }

    #[test]
    fn partial_file_fills_missing_fields() {
        let config = Config::from_toml_str("[general]\nhotkey = \"Ctrl+Space\"\n").unwrap();
        assert_eq!(config.general.hotkey, "Ctrl+Space");
        assert!(config.general.hide_on_blur);
        assert_eq!(config.window, WindowConfig::default());
        assert_eq!(config.linux, LinuxConfig::default());
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let config = Config::from_toml_str("[future]\nthing = 1\n[general]\nnew_key = true\n");
        assert_eq!(config.unwrap(), Config::default());
    }

    #[test]
    fn out_of_range_values_are_normalized() {
        let config =
            Config::from_toml_str("[general]\nhotkey = \"  \"\n[window]\nwidth = 10\n").unwrap();
        assert_eq!(config.general.hotkey, "Alt+Space");
        assert_eq!(config.window.width, MIN_WINDOW_WIDTH);

        let config = Config::from_toml_str("[window]\nwidth = 99999\n").unwrap();
        assert_eq!(config.window.width, MAX_WINDOW_WIDTH);
    }

    #[test]
    fn wrong_types_are_rejected() {
        assert!(Config::from_toml_str("[general]\nhide_on_blur = \"yes\"\n").is_err());
    }

    #[test]
    fn load_or_create_writes_defaults_then_reads_them_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("config.toml");

        let (config, origin) = Config::load_or_create(&path).unwrap();
        assert_eq!(origin, ConfigOrigin::Created);
        assert_eq!(config, Config::default());
        assert_eq!(fs::read_to_string(&path).unwrap(), DEFAULT_CONFIG_TOML);

        let (config, origin) = Config::load_or_create(&path).unwrap();
        assert_eq!(origin, ConfigOrigin::Loaded);
        assert_eq!(config, Config::default());
    }

    #[test]
    fn invalid_file_is_reported_and_left_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "[general\nhotkey = ").unwrap();

        let err = Config::load_or_create(&path).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
        assert_eq!(fs::read_to_string(&path).unwrap(), "[general\nhotkey = ");
    }
}
