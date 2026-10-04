//! System tray icon and menu.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::AppHandle;

use crate::{app, settings, updater, window};

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

/// Whether the tray icon exists (for the diagnostics report).
pub fn exists(app: &AppHandle) -> bool {
    app.tray_by_id(TRAY_ID).is_some()
}

fn build(app: &AppHandle) -> tauri::Result<()> {
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "show", "Show", true, None::<&str>)?,
            &MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?,
            &MenuItem::with_id(app, "reload", "Reload index", true, None::<&str>)?,
            &MenuItem::with_id(app, "update", "Check for updates", true, None::<&str>)?,
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
            "settings" => settings::open(app),
            "reload" => app::reload(app),
            "update" => updater::check_now(app),
            "quit" => app::quit(app),
            other => tracing::debug!("unhandled tray menu item {other}"),
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                // A click on the icon goes straight to Settings; the menu (right
                // click) still has Show for the launcher, and the hotkey opens it.
                settings::open(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}
