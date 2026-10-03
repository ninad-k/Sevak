//! System tray icon and menu.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::state::AppState;
use crate::{app, window};

const TRAY_ID: &str = "main";
const TOOLTIP: &str = "Sevak \u{2014} at your service";

/// Creates the tray icon. A missing tray (e.g. stock Fedora GNOME has no
/// StatusNotifier host) is not fatal; the hotkey still works.
pub fn init(app: &AppHandle) {
    if let Err(err) = build(app) {
        tracing::warn!(
            "could not create the tray icon: {err}. Sevak keeps running without it; \
             use your hotkey or `sevak --toggle` to open it and `sevak --quit` to exit"
        );
    } else {
        tracing::info!("tray icon created");
    }
}

fn build(app: &AppHandle) -> tauri::Result<()> {
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "show", "Show", true, None::<&str>)?,
            &MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?,
            &MenuItem::with_id(app, "reload", "Reload index", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?,
        ],
    )?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(TOOLTIP)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => window::show(app),
            "settings" => open_settings(app),
            "reload" => app::reload(app),
            "quit" => app.exit(0),
            other => tracing::debug!("unhandled tray menu item {other}"),
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                window::toggle(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

// A settings UI arrives later; until then "Settings" opens the config file.
fn open_settings(app: &AppHandle) {
    let path = app.state::<AppState>().paths.config_file.clone();
    if let Err(err) = sevak_platform::open::open_in_editor(&path) {
        tracing::error!("could not open {} for editing: {err}", path.display());
    }
}
