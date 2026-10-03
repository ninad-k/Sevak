//! Sevak core library.
//!
//! This crate holds everything that is independent of the operating system and
//! of the UI shell: configuration, and (from Phase 2 on) the search engine,
//! result model, ranking and plugin contracts. It must never depend on Tauri or
//! contain `#[cfg(target_os = ...)]` code.

pub mod config;

pub use config::{Config, ConfigError, ConfigOrigin, GeneralConfig, LinuxConfig, WindowConfig};
