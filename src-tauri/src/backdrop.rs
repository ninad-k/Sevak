//! The frosted-glass backdrop behind the search bar (`appearance.blur`).
//!
//! Windows gets Acrylic (Blur on builds without it) and macOS a popover
//! vibrancy; Linux compositors decide that themselves, so the setting is
//! ignored there. The effect fills the whole window, so on Windows the native
//! shadow is switched on with it: that is what makes Windows 11 round the
//! window's corners, which the card then matches (see `theme.rs`).

use tauri::{AppHandle, Manager};

use crate::state::AppState;
use crate::window::MAIN_LABEL;

/// Applies or removes the backdrop to match the resolved appearance. Called at
/// startup and after every config reload.
pub fn apply(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MAIN_LABEL) else {
        return;
    };
    let (blur, radius) = {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        let appearance = state
            .appearance
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (appearance.blur, appearance.radius)
    };
    let result = if blur {
        enable(&window, radius)
    } else {
        disable(&window)
    };
    if let Err(err) = result {
        tracing::warn!(blur, "could not update the window backdrop: {err}");
    }
}

#[cfg(windows)]
fn enable(window: &tauri::WebviewWindow, _radius: u32) -> tauri::Result<()> {
    use tauri::window::{Color, Effect, EffectsBuilder};

    // A near-transparent tint: the card paints the theme's color itself, so the
    // opacity setting decides how frosted the result looks.
    let effects = |effect| {
        EffectsBuilder::new()
            .effect(effect)
            .color(Color(0, 0, 0, 1))
            .build()
    };
    window.set_shadow(true)?;
    if window.set_effects(effects(Effect::Acrylic)).is_err() {
        window.set_effects(effects(Effect::Blur))?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn enable(window: &tauri::WebviewWindow, radius: u32) -> tauri::Result<()> {
    use tauri::window::{Effect, EffectState, EffectsBuilder};

    window.set_effects(
        EffectsBuilder::new()
            .effect(Effect::Popover)
            .state(EffectState::Active)
            .radius(f64::from(radius))
            .build(),
    )
}

#[cfg(not(any(windows, target_os = "macos")))]
fn enable(_window: &tauri::WebviewWindow, _radius: u32) -> tauri::Result<()> {
    Ok(())
}

fn disable(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    if cfg!(any(windows, target_os = "macos")) {
        window.set_effects(None)?;
    }
    if cfg!(windows) {
        window.set_shadow(false)?;
    }
    Ok(())
}
