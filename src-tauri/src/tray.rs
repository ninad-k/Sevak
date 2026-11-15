//! System tray icon and menu.

use std::sync::OnceLock;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::state::AppState;
use crate::{app, settings, updater, window};

const TRAY_ID: &str = "main";

/// The tray's words in one language. The windows have their own catalogues
/// (`ui/src/locales`); the tray is drawn by the shell, so it keeps this small
/// table for the same languages (`sevak_core::language::LANGUAGES`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Labels {
    tooltip: &'static str,
    show: &'static str,
    settings: &'static str,
    reload: &'static str,
    update: &'static str,
    quit: &'static str,
}

/// The labels for a shipped language code; English for anything else.
fn labels(lang: &str) -> Labels {
    match lang {
        "es" => Labels {
            tooltip: "Sevak \u{2014} a tu servicio",
            show: "Mostrar",
            settings: "Ajustes",
            reload: "Recargar el índice",
            update: "Buscar actualizaciones",
            quit: "Salir",
        },
        "de" => Labels {
            tooltip: "Sevak \u{2014} zu Diensten",
            show: "Anzeigen",
            settings: "Einstellungen",
            reload: "Index neu laden",
            update: "Nach Updates suchen",
            quit: "Beenden",
        },
        "fr" => Labels {
            tooltip: "Sevak \u{2014} à votre service",
            show: "Afficher",
            settings: "Paramètres",
            reload: "Recharger l'index",
            update: "Rechercher des mises à jour",
            quit: "Quitter",
        },
        "hi" => Labels {
            tooltip: "Sevak \u{2014} आपकी सेवा में",
            show: "दिखाएँ",
            settings: "सेटिंग्स",
            reload: "इंडेक्स फिर से लोड करें",
            update: "अपडेट की जाँच करें",
            quit: "बाहर निकलें",
        },
        _ => Labels {
            tooltip: "Sevak \u{2014} at your service",
            show: "Show",
            settings: "Settings",
            reload: "Reload index",
            update: "Check for updates",
            quit: "Quit",
        },
    }
}

/// The tray's language: `[general] language`, with `auto` read from the OS
/// once (the OS list does not change while Sevak runs).
fn language(app: &AppHandle) -> &'static str {
    static OS: OnceLock<Vec<String>> = OnceLock::new();
    let os = OS.get_or_init(sevak_platform::locale::preferred_languages);
    let setting = app
        .try_state::<AppState>()
        .map(|state| state.config().general.language)
        .unwrap_or_default();
    sevak_core::language::resolve(&setting, os.iter().map(String::as_str))
}

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

/// Redraws the menu and tooltip in the configured language (after a config
/// reload, so a language picked in Settings reaches the tray without a restart).
pub fn relabel(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let words = labels(language(app));
    let result = menu(app, words).and_then(|menu| {
        tray.set_menu(Some(menu))?;
        tray.set_tooltip(Some(words.tooltip))
    });
    if let Err(err) = result {
        tracing::warn!("could not relabel the tray menu: {err}");
    }
}

fn menu(app: &AppHandle, words: Labels) -> tauri::Result<Menu<tauri::Wry>> {
    Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "show", words.show, true, None::<&str>)?,
            &MenuItem::with_id(app, "settings", words.settings, true, None::<&str>)?,
            &MenuItem::with_id(app, "reload", words.reload, true, None::<&str>)?,
            &MenuItem::with_id(app, "update", words.update, true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", words.quit, true, None::<&str>)?,
        ],
    )
}

fn build(app: &AppHandle) -> tauri::Result<()> {
    let words = labels(language(app));
    let menu = menu(app, words)?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(words.tooltip)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shipped_language_has_its_own_tray_labels() {
        let english = labels("en");
        for &lang in sevak_core::language::LANGUAGES {
            let words = labels(lang);
            for text in [
                words.tooltip,
                words.show,
                words.settings,
                words.reload,
                words.update,
                words.quit,
            ] {
                assert!(!text.trim().is_empty(), "{lang} has an empty tray label");
            }
            if lang != "en" {
                assert_ne!(words.show, english.show, "{lang} reuses the English labels");
                assert_ne!(words.quit, english.quit, "{lang} reuses the English labels");
            }
        }
        assert_eq!(labels("xx"), english);
    }
}
