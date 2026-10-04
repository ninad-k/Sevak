//! What Sevak tells an extension through its environment.
//!
//! Sevak starts extensions with a scrubbed environment (only a small base set
//! plus what the manifest's `inherit_env` names) and adds these variables.

use std::path::PathBuf;

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

/// The extension's id (`SEVAK_PLUGIN_ID`), such as `script:rust-hello`.
pub fn plugin_id() -> Option<String> {
    var("SEVAK_PLUGIN_ID")
}

/// The folder the extension is installed in (`SEVAK_PLUGIN_DIR`), which is
/// also its working directory.
pub fn plugin_dir() -> Option<PathBuf> {
    var("SEVAK_PLUGIN_DIR").map(PathBuf::from)
}

/// A folder the extension may keep its own files in (`SEVAK_PLUGIN_DATA`). It is
/// created on demand by the extension, not by Sevak, and removed when the
/// extension is uninstalled from Settings > Extensions.
pub fn plugin_data_dir() -> Option<PathBuf> {
    var("SEVAK_PLUGIN_DATA").map(PathBuf::from)
}

/// The version of Sevak that started the extension (`SEVAK_VERSION`).
pub fn sevak_version() -> Option<String> {
    var("SEVAK_VERSION")
}
