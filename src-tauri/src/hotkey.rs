//! The global show/hide hotkey.
//!
//! On Windows and X11 Sevak grabs the key itself through the global-shortcut
//! plugin. On Wayland applications cannot grab keys, so the desktop owns the
//! binding and runs `sevak --toggle`; the plugin is not even installed there
//! because its X11 backend can fail to initialise without a display, which
//! would abort startup.

use sevak_platform::HotkeyStrategy;
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::state::{AppState, HotkeyMode, HotkeyStatus};
use crate::window;

/// The plugin to install, or `None` when the desktop environment owns the key.
pub fn plugin(strategy: HotkeyStrategy) -> Option<tauri::plugin::TauriPlugin<Wry>> {
    match strategy {
        HotkeyStrategy::External => None,
        HotkeyStrategy::InApp => Some(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        window::toggle(app);
                    }
                })
                .build(),
        ),
    }
}

/// (Re)registers the hotkey from the current config and records the outcome.
/// Failures (bad syntax, key owned by another app) are reported in the status,
/// never fatal.
pub fn apply(app: &AppHandle) -> HotkeyStatus {
    let state = app.state::<AppState>();
    let accelerator = state.config().general.hotkey;
    // The key that currently works, to fall back to if the new one cannot be used.
    let previous = {
        let current = state.hotkey.read().unwrap_or_else(|p| p.into_inner());
        (current.mode == HotkeyMode::Global && current.error.is_none())
            .then(|| current.accelerator.clone())
    };

    let status = match state.display.hotkey_strategy() {
        HotkeyStrategy::External => {
            tracing::info!(
                "hotkeys are managed by the desktop on this session; \
                 run `sevak --setup-hotkey` to bind {accelerator} to `sevak --toggle`"
            );
            HotkeyStatus {
                accelerator,
                mode: HotkeyMode::External,
                error: None,
            }
        }
        HotkeyStrategy::InApp => {
            let error = register(app, &accelerator, previous.as_deref()).err();
            match &error {
                None => tracing::info!("registered global hotkey {accelerator}"),
                Some(err) => tracing::warn!("could not register hotkey {accelerator}: {err}"),
            }
            HotkeyStatus {
                accelerator,
                mode: HotkeyMode::Global,
                error,
            }
        }
    };

    *state
        .hotkey
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = status.clone();
    status
}

/// Binds `accelerator` in place of whatever is registered. If it cannot be
/// parsed or registered, `previous` (the key that worked until now) stays bound,
/// so a typo in `config.toml` never leaves Sevak without a hotkey.
fn register(app: &AppHandle, accelerator: &str, previous: Option<&str>) -> Result<(), String> {
    let still_active = |err: String| match previous {
        Some(previous) if previous != accelerator => format!("{err}; {previous} still works"),
        _ => err,
    };
    // Parse before touching the current registration.
    let shortcut: Shortcut = accelerator
        .parse()
        .map_err(|err| still_active(format!("invalid hotkey \"{accelerator}\": {err}")))?;

    let shortcuts = app.global_shortcut();
    shortcuts.unregister_all().map_err(|err| err.to_string())?;
    if let Err(err) = shortcuts.register(shortcut) {
        let restored = previous
            .and_then(|previous| previous.parse::<Shortcut>().ok())
            .is_some_and(|previous| shortcuts.register(previous).is_ok());
        let err = err.to_string();
        return Err(if restored { still_active(err) } else { err });
    }
    Ok(())
}
