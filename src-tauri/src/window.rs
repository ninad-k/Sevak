//! Show / hide / toggle / position logic for the resident launcher window.

use std::time::{Duration, Instant};

use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, Monitor, PhysicalPosition, WebviewWindow, Window,
    WindowEvent,
};

use crate::direct::{self, ShowPayload};
use crate::state::{lock, AppState};

pub const MAIN_LABEL: &str = "main";
pub const SETTINGS_LABEL: &str = "settings";
pub const EVENT_SHOW: &str = "sevak:show";
pub const EVENT_HIDDEN: &str = "sevak:hidden";
pub const EVENT_STATUS: &str = "sevak:status";

/// A toggle this soon after a blur-hide is the same click/keypress that caused
/// the blur; reopening would make the toggle feel broken.
const BLUR_REOPEN_GUARD: Duration = Duration::from_millis(300);
/// Focus can flicker while a freshly shown window is being activated; blurs
/// inside this window are not the user clicking away.
const SHOW_BLUR_GRACE: Duration = Duration::from_millis(150);

/// Fraction of the work area's height at which the search bar's top edge sits.
const TOP_OFFSET_FRACTION: f64 = 0.25;
/// Height used if the window's size cannot be read (matches tauri.conf.json).
const DEFAULT_HEIGHT: f64 = 92.0;

pub fn show(app: &AppHandle) {
    show_with(app, ShowPayload::default());
}

/// Shows the window; `payload` can prefill the query or carry an error line.
pub fn show_with(app: &AppHandle, payload: ShowPayload) {
    let Some(window) = app.get_webview_window(MAIN_LABEL) else {
        tracing::warn!("show: main window not found");
        return;
    };

    tracing::info!("showing window");
    // Before showing, so the UI clears its query and is ready to focus.
    // Until the UI has loaded it cannot hear this; it asks for what it missed.
    if let Some(payload) = direct::deliver(payload) {
        if let Err(err) = app.emit_to(MAIN_LABEL, EVENT_SHOW, payload) {
            tracing::warn!("show: could not emit {EVENT_SHOW}: {err}");
        }
    }

    position(app, &window);

    unhide_app(app);
    if let Err(err) = window.show() {
        tracing::warn!("show: window.show failed: {err}");
    }
    if let Err(err) = window.set_focus() {
        tracing::warn!("show: set_focus failed: {err}");
    }
    if let Some(state) = app.try_state::<AppState>() {
        *lock(&state.last_shown) = Some(Instant::now());
    }
}

pub fn hide(app: &AppHandle) {
    hide_silently(app);
    announce_hidden(app);
}

/// Hides the window without telling the UI, so it keeps its query and results.
/// Used for an optimistic hide that may have to be undone ([`reveal`]).
pub fn hide_silently(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MAIN_LABEL) else {
        tracing::warn!("hide: main window not found");
        return;
    };
    tracing::info!("hiding window");
    if let Err(err) = window.hide() {
        tracing::warn!("hide: window.hide failed: {err}");
    }
    hide_app(app);
}

/// macOS: hiding the whole (accessory) app, not just its window, is what hands
/// focus back to the app the user was in. Skipped while Settings is open.
#[cfg(target_os = "macos")]
fn hide_app(app: &AppHandle) {
    let settings_open = app
        .get_webview_window(SETTINGS_LABEL)
        .is_some_and(|window| window.is_visible().unwrap_or(false));
    if !settings_open {
        if let Err(err) = app.hide() {
            tracing::warn!("hide: app.hide failed: {err}");
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn hide_app(_app: &AppHandle) {}

/// macOS: undoes [`hide_app`] so the window can be shown and focused.
#[cfg(target_os = "macos")]
pub fn unhide_app(app: &AppHandle) {
    if let Err(err) = app.show() {
        tracing::warn!("show: app.show failed: {err}");
    }
}

#[cfg(not(target_os = "macos"))]
pub fn unhide_app(_app: &AppHandle) {}

/// Tells the UI the window is gone, so it clears its query for the next show.
pub fn announce_hidden(app: &AppHandle) {
    if let Err(err) = app.emit_to(MAIN_LABEL, EVENT_HIDDEN, ()) {
        tracing::warn!("hide: could not emit {EVENT_HIDDEN}: {err}");
    }
}

/// Shows the window again exactly as it was left (after [`hide_silently`]),
/// without the reset that [`show`] triggers.
pub fn reveal(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MAIN_LABEL) else {
        return;
    };
    tracing::info!("revealing window again");
    position(app, &window);
    unhide_app(app);
    if let Err(err) = window.show() {
        tracing::warn!("reveal: window.show failed: {err}");
    }
    if let Err(err) = window.set_focus() {
        tracing::warn!("reveal: set_focus failed: {err}");
    }
    if let Some(state) = app.try_state::<AppState>() {
        *lock(&state.last_shown) = Some(Instant::now());
    }
}

pub fn toggle(app: &AppHandle) {
    let visible = app
        .get_webview_window(MAIN_LABEL)
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false);
    if visible {
        hide(app);
        return;
    }

    let recently_blurred = app
        .try_state::<AppState>()
        .and_then(|state| *lock(&state.last_blur_hide))
        .is_some_and(|at| at.elapsed() < BLUR_REOPEN_GUARD);
    if recently_blurred {
        tracing::debug!("toggle ignored: the window was just hidden by losing focus");
        return;
    }
    show(app);
}

/// Window events: hide on blur, and keep the resident window alive on close.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if window.label() == SETTINGS_LABEL {
        // The shortcut recorder suspends the global hotkey; closing the window
        // mid-recording must not leave it unregistered.
        if matches!(event, WindowEvent::Destroyed) {
            crate::hotkey::apply(window.app_handle());
        }
        return;
    }
    if window.label() != MAIN_LABEL {
        return;
    }
    match event {
        WindowEvent::Focused(false) => {
            let app = window.app_handle();
            let Some(state) = app.try_state::<AppState>() else {
                return;
            };
            if !state.config().general.hide_on_blur || !window.is_visible().unwrap_or(false) {
                return;
            }
            if lock(&state.last_shown).is_some_and(|at| at.elapsed() < SHOW_BLUR_GRACE) {
                tracing::debug!("ignoring blur right after show");
                return;
            }
            tracing::debug!("window lost focus; hiding");
            hide(app);
            *lock(&state.last_blur_hide) = Some(Instant::now());
        }
        WindowEvent::CloseRequested { api, .. } => {
            // Alt+F4 must not destroy the resident window.
            api.prevent_close();
            hide(window.app_handle());
        }
        _ => {}
    }
}

/// Centers the window horizontally on the monitor under the cursor.
///
/// The bar's top edge sits a quarter of the way down the work area rather than
/// at the vertical center: result rows will later grow below it, and with this
/// placement the search field stays put while the window gets taller.
///
/// Native Wayland clients cannot position their own windows, so there this is
/// a harmless no-op (Sevak runs under XWayland by default for this reason).
fn position(app: &AppHandle, window: &WebviewWindow) {
    let width = configured_width(app);
    apply_width(window, width);

    let Some(monitor) = target_monitor(app, window) else {
        if let Err(err) = window.center() {
            tracing::debug!("position: center failed: {err}");
        }
        return;
    };

    let work_area = monitor.work_area();
    let physical_width = f64::from(width) * monitor.scale_factor();
    let x =
        f64::from(work_area.position.x) + (f64::from(work_area.size.width) - physical_width) / 2.0;
    let y =
        f64::from(work_area.position.y) + f64::from(work_area.size.height) * TOP_OFFSET_FRACTION;

    if let Err(err) = window.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32))
    {
        tracing::debug!("position: set_position failed: {err}");
    }
}

fn configured_width(app: &AppHandle) -> u32 {
    app.try_state::<AppState>()
        .map(|state| state.config().window.width)
        .unwrap_or(sevak_core::config::MIN_WINDOW_WIDTH)
}

/// Sets the window's logical width, keeping its current logical height.
fn apply_width(window: &WebviewWindow, width: u32) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let logical_height = window
        .inner_size()
        .map(|size| f64::from(size.height) / scale)
        .unwrap_or(DEFAULT_HEIGHT);
    if let Err(err) = window.set_size(LogicalSize::new(f64::from(width), logical_height)) {
        tracing::debug!("set_size failed: {err}");
    }
}

/// Makes the window's width match the config (startup and config reload).
pub fn apply_configured_width(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_LABEL) {
        apply_width(&window, configured_width(app));
    }
}

/// Monitor under the cursor, else the window's own, else the primary one.
fn target_monitor(app: &AppHandle, window: &WebviewWindow) -> Option<Monitor> {
    app.cursor_position()
        .ok()
        .and_then(|cursor| app.monitor_from_point(cursor.x, cursor.y).ok().flatten())
        .or_else(|| window.current_monitor().ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten())
}
