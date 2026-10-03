//! Tauri commands invoked by the frontend.

use tauri::{AppHandle, LogicalSize, State, WebviewWindow};

use crate::state::{AppState, Status};
use crate::window;

const MIN_HEIGHT: f64 = 40.0;
const MAX_HEIGHT: f64 = 900.0;

#[tauri::command]
pub fn hide_window(app: AppHandle) {
    window::hide(&app);
}

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> Status {
    state.status()
}

/// The frontend reports its rendered content height; the window follows it.
#[tauri::command]
pub fn set_content_height(window: WebviewWindow, state: State<'_, AppState>, height: f64) {
    if !height.is_finite() {
        tracing::debug!("set_content_height ignored: {height}");
        return;
    }
    let width = f64::from(state.config().window.width);
    let size = LogicalSize::new(width, height.clamp(MIN_HEIGHT, MAX_HEIGHT));
    if let Err(err) = window.set_size(size) {
        tracing::warn!("set_content_height: set_size failed: {err}");
    }
}
