//! Shared application state, managed by Tauri.

use std::sync::{Mutex, MutexGuard, RwLock};
use std::time::Instant;

use serde::Serialize;
use sevak_core::Config;
use sevak_platform::{AppPaths, DisplayServer};

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

/// Snapshot sent to the frontend (`get_status` and `sevak:status`).
#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub version: String,
    pub display: String,
    pub hotkey: HotkeyStatus,
}

pub struct AppState {
    pub paths: AppPaths,
    pub display: DisplayServer,
    pub config: RwLock<Config>,
    pub hotkey: RwLock<HotkeyStatus>,
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
        Self {
            paths,
            display,
            config: RwLock::new(config),
            hotkey: RwLock::new(hotkey),
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
        }
    }
}

/// Locks a mutex, recovering the data if a panicking thread poisoned it.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
