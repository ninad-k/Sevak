//! Shared application state, managed by Tauri.

use std::sync::{Mutex, MutexGuard, RwLock};
use std::time::Instant;

use serde::Serialize;
use sevak_core::config::Theme;
use sevak_core::theme::{self, ResolvedAppearance};
use sevak_core::Config;
use sevak_platform::{AppPaths, DisplayServer};

use crate::search::Search;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HotkeyMode {
    /// Sevak grabs the key itself.
    Global,
    /// The desktop runs `sevak --toggle` (Wayland).
    External,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HotkeyStatus {
    pub accelerator: String,
    pub mode: HotkeyMode,
    pub error: Option<String>,
}

/// How one `[[hotkey]]` entry fared, in the order of the config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CustomHotkeyStatus {
    pub key: String,
    /// What the key does, e.g. `Run apps:firefox.desktop`.
    pub description: String,
    /// Why it is not active (bad key, taken by another app, duplicate).
    pub error: Option<String>,
}

/// Snapshot sent to the frontend (`get_status` and `sevak:status`).
#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub version: String,
    pub display: String,
    pub hotkey: HotkeyStatus,
    pub custom_hotkeys: Vec<CustomHotkeyStatus>,
    /// The Universal Actions key; `None` when `actions_hotkey` is empty.
    pub actions_hotkey: Option<CustomHotkeyStatus>,
    /// The configured theme; the frontend resolves `system` itself.
    pub theme: Theme,
    /// The CSS for the accent, font, radius and custom stylesheet.
    pub appearance: ResolvedAppearance,
    /// The search index is being (re)built.
    pub indexing: bool,
}

pub struct AppState {
    pub paths: AppPaths,
    pub display: DisplayServer,
    pub config: RwLock<Config>,
    pub search: Search,
    pub hotkey: RwLock<HotkeyStatus>,
    pub custom_hotkeys: RwLock<Vec<CustomHotkeyStatus>>,
    pub actions_hotkey: RwLock<Option<CustomHotkeyStatus>>,
    pub appearance: RwLock<ResolvedAppearance>,
    /// When the window was last shown.
    pub last_shown: Mutex<Option<Instant>>,
    /// When the window last hid itself because it lost focus.
    pub last_blur_hide: Mutex<Option<Instant>>,
}

impl AppState {
    pub fn new(paths: AppPaths, display: DisplayServer, config: Config) -> Self {
        let hotkey = HotkeyStatus {
            accelerator: config.general.hotkey.clone(),
            mode: HotkeyMode::Global,
            error: None,
        };
        let search = Search::new(&paths, &config);
        let appearance = theme::resolve(&config.appearance, &paths.config_dir);
        Self {
            paths,
            display,
            search,
            config: RwLock::new(config),
            hotkey: RwLock::new(hotkey),
            custom_hotkeys: RwLock::new(Vec::new()),
            actions_hotkey: RwLock::new(None),
            appearance: RwLock::new(appearance),
            last_shown: Mutex::new(None),
            last_blur_hide: Mutex::new(None),
        }
    }

    /// Clones the current config so no lock is held while calling into Tauri.
    pub fn config(&self) -> Config {
        self.config
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Re-validates the appearance settings and re-reads the custom stylesheet
    /// (after a config reload).
    pub fn refresh_appearance(&self) {
        let resolved = theme::resolve(&self.config().appearance, &self.paths.config_dir);
        *self
            .appearance
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = resolved;
    }

    pub fn status(&self) -> Status {
        let hotkey = self
            .hotkey
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        Status {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            display: self.display.as_str().to_owned(),
            hotkey,
            custom_hotkeys: self
                .custom_hotkeys
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone(),
            actions_hotkey: self
                .actions_hotkey
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone(),
            theme: self.config().appearance.theme,
            appearance: self
                .appearance
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone(),
            indexing: self.search.is_indexing(),
        }
    }
}

/// Locks a mutex, recovering the data if a panicking thread poisoned it.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
