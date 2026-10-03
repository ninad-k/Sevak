//! Show / hide / toggle / position logic for the resident launcher window.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, Monitor, PhysicalPosition, PhysicalSize,
    WebviewWindow, Window, WindowEvent,
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
    // While the user's app still has focus: pasting returns to it later.
    if let Some(state) = app.try_state::<AppState>() {
        state.search.platform.remember_foreground_app();
    }
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
/// The actions for a selection (Universal Actions) are dropped with it.
pub fn announce_hidden(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        state.search.forget_selection();
        state
            .file_buffer
            .on_hidden(state.config().file_buffer.keep_between_shows);
    }
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
            if state.file_buffer.confirming() {
                tracing::debug!("ignoring blur: a file buffer confirmation is open");
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
    // A Large Type left over from before the window was hidden.
    leave_large_type(app);
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

    *lock(&HOME_TOP) = Some(y.round() as i32);
    if let Err(err) = window.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32))
    {
        tracing::debug!("position: set_position failed: {err}");
    }
}

/// Top edge (physical pixels) the launcher was last placed at by [`position`],
/// before [`keep_on_screen`] lifted it for a tall window.
static HOME_TOP: Mutex<Option<i32>> = Mutex::new(None);

/// Moves the window up when `logical_height` (a preview pane, the Text View or
/// a grid made it tall) would run past the bottom of its monitor's work area,
/// and back to its usual place when it is short again. A no-op where the window
/// manager does not let us move windows (native Wayland) or before the first
/// placement.
pub fn keep_on_screen(window: &WebviewWindow, logical_height: f64) {
    let Some(home) = *lock(&HOME_TOP) else {
        return;
    };
    let Ok(Some(monitor)) = window.current_monitor() else {
        return;
    };
    let Ok(current) = window.outer_position() else {
        return;
    };
    let scale = window.scale_factor().unwrap_or(1.0);
    let area = monitor.work_area();
    let area_top = area.position.y;
    let area_bottom = area_top + i32::try_from(area.size.height).unwrap_or(i32::MAX / 2);
    let height = (logical_height * scale).round() as i32;
    let top = home.min(area_bottom - height).max(area_top);
    if top != current.y {
        if let Err(err) = window.set_position(PhysicalPosition::new(current.x, top)) {
            tracing::debug!("keep_on_screen: set_position failed: {err}");
        }
    }
}

fn configured_width(app: &AppHandle) -> u32 {
    app.try_state::<AppState>()
        .map(|state| state.window_width())
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

/// The launcher's position and size before [`enter_large_type`], to restore.
static BEFORE_LARGE_TYPE: Mutex<Option<(PhysicalPosition<i32>, PhysicalSize<u32>)>> =
    Mutex::new(None);

/// Stretches the (transparent) window over its monitor's work area so Large
/// Type can fill the screen. The size and position it had are kept for
/// [`leave_large_type`]. Where the window manager refuses to move or resize
/// (native Wayland), the error says so and the UI falls back to the launcher's
/// own size.
pub fn enter_large_type(app: &AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window(MAIN_LABEL)
        .ok_or("the launcher window is missing")?;
    let monitor = target_monitor(app, &window).ok_or("no monitor found")?;
    let before = window
        .outer_position()
        .and_then(|position| window.outer_size().map(|size| (position, size)))
        .map_err(|err| err.to_string())?;

    // Keep the first saved bounds if called twice in a row.
    lock(&BEFORE_LARGE_TYPE).get_or_insert(before);

    let area = monitor.work_area();
    let stretched = window
        .set_position(area.position)
        .and_then(|()| window.set_size(area.size));
    if let Err(err) = stretched {
        leave_large_type(app);
        return Err(err.to_string());
    }
    Ok(())
}

/// Puts the window back to what it was before [`enter_large_type`]. A no-op if
/// Large Type was not active.
pub fn leave_large_type(app: &AppHandle) {
    let Some((position, size)) = lock(&BEFORE_LARGE_TYPE).take() else {
        return;
    };
    let Some(window) = app.get_webview_window(MAIN_LABEL) else {
        return;
    };
    if let Err(err) = window
        .set_size(size)
        .and_then(|()| window.set_position(position))
    {
        tracing::debug!("leave_large_type: could not restore the window: {err}");
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
