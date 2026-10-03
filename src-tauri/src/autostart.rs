//! Launch at login, through `tauri-plugin-autostart`.
//!
//! Windows: a value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
//! Linux: `~/.config/autostart/<name>.desktop`. Inside an AppImage the plugin
//! registers the stable `$APPIMAGE` path instead of the temporary mount.
//! The entry always starts Sevak with `--background` (no window at login).

use tauri::plugin::TauriPlugin;
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_autostart::ManagerExt;

use crate::state::AppState;

pub fn plugin() -> TauriPlugin<Wry> {
    tauri_plugin_autostart::Builder::new()
        .arg("--background")
        .build()
}

/// Makes the OS entry match `general.launch_at_login`. Failures are logged,
/// never fatal: a locked-down registry must not stop the launcher.
pub fn sync(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let wanted = state.config().general.launch_at_login;
    let launcher = app.autolaunch();

    let enabled = match launcher.is_enabled() {
        Ok(enabled) => enabled,
        Err(err) => {
            tracing::warn!("could not read the launch-at-login state: {err}");
            return;
        }
    };
    if enabled == wanted {
        tracing::debug!(enabled, "launch at login already matches the config");
        return;
    }

    let result = if wanted {
        launcher.enable()
    } else {
        launcher.disable()
    };
    match result {
        Ok(()) => tracing::info!(enabled = wanted, "launch at login updated"),
        Err(err) => tracing::warn!(
            "could not {} launch at login: {err}",
            if wanted { "enable" } else { "disable" }
        ),
    }
}
