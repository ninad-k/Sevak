//! Sevak core library.
//!
//! This crate holds everything that is independent of the operating system and
//! of the UI shell: configuration, the result model, the plugin contract, fuzzy
//! matching and the search engine. It must never depend on Tauri or contain
//! `#[cfg(target_os = ...)]` code.

pub mod config;
pub mod engine;
pub mod fuzzy;
pub mod model;
pub mod plugin;
pub mod theme;
pub mod usage;

pub use config::{
    ClipboardConfig, Config, ConfigError, ConfigOrigin, GeneralConfig, HotkeyBinding, HotkeyTarget,
    LinuxConfig, PasteConfig, ShellConfig, Snippet, WindowConfig,
};
pub use engine::{EngineOptions, SearchEngine};
pub use fuzzy::FuzzyQuery;
pub use model::{
    Action, AppEntry, IconData, IconSource, LaunchTarget, Modifier, ResultItem, SecondaryAction,
};
pub use plugin::{Plugin, PluginError, PluginResult, ResultsNotifier};
pub use usage::{UsageEntry, UsageError, UsageStore};
