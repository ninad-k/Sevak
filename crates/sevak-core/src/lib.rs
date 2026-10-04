//! Sevak core library.
//!
//! This crate holds everything that is independent of the operating system and
//! of the UI shell: configuration, the result model, the plugin contract, fuzzy
//! matching and the search engine. It must never depend on Tauri or contain
//! `#[cfg(target_os = ...)]` code.

pub mod checksum;
pub mod config;
pub mod diagnostics;
pub mod engine;
pub mod fuzzy;
pub mod model;
pub mod plugin;
pub mod preview;
pub mod selection;
pub mod theme;
pub mod theme_file;
pub mod theme_store;
pub mod usage;
pub mod window_layout;

pub use config::{
    ActionsConfig, ClipboardConfig, Config, ConfigError, ConfigOrigin, ExpandOn, FileBufferConfig,
    GeneralConfig, HotkeyBinding, HotkeyTarget, LinuxConfig, PasteConfig, ShellConfig, Snippet,
    SnippetsConfig, UpdateChannel, WindowConfig, WindowManagementConfig,
};
pub use engine::{EngineOptions, SearchEngine};
pub use fuzzy::FuzzyQuery;
pub use model::{
    Action, AppEntry, ClipContent, IconData, IconSource, LaunchTarget, Modifier, PreviewHint,
    ResultItem, SecondaryAction, ViewHint,
};
pub use plugin::{Plugin, PluginError, PluginResult, ResultsNotifier};
pub use selection::{Selection, SelectionKind};
pub use usage::{UsageEntry, UsageError, UsageStore};
