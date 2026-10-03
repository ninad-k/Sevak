//! The global hotkeys: the show/hide key, the Universal Actions key and the
//! `[[hotkey]]` entries.
//!
//! On Windows and X11 Sevak grabs the keys itself through the global-shortcut
//! plugin. On Wayland applications cannot grab keys, so the desktop owns the
//! bindings and runs `sevak --toggle` / `--actions` / `--query` / `--run`; the
//! plugin is not even installed there because its X11 backend can fail to
//! initialise without a display, which would abort startup.

use sevak_core::config::{HotkeyBinding, HotkeyTarget};
use sevak_core::Config;
use sevak_platform::gnome::{CustomShortcut, CustomTarget};
use sevak_platform::HotkeyStrategy;
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::state::{AppState, CustomHotkeyStatus, HotkeyMode, HotkeyStatus};
use crate::{direct, selection, window};

/// What the Universal Actions key is called in the status.
const ACTIONS_DESCRIPTION: &str = "Universal Actions";

/// The plugin to install, or `None` when the desktop environment owns the key.
pub fn plugin(strategy: HotkeyStrategy) -> Option<tauri::plugin::TauriPlugin<Wry>> {
    match strategy {
        HotkeyStrategy::External => None,
        HotkeyStrategy::InApp => Some(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        pressed(app, shortcut);
                    }
                })
                .build(),
        ),
    }
}

/// What a pressed key does, decided from the live config: the main key
/// toggles, the actions key captures the selection, an entry key does its
/// entry's action. (An unknown key can only be the previous main key, kept
/// after a failed change.)
fn pressed(app: &AppHandle, shortcut: &Shortcut) {
    let config = app.state::<AppState>().config();
    let is = |accelerator: &str| {
        accelerator
            .parse::<Shortcut>()
            .is_ok_and(|parsed| parsed.id() == shortcut.id())
    };
    if !is(&config.general.hotkey) {
        let actions = config.general.actions_hotkey.as_str();
        if !actions.is_empty() && is(actions) {
            selection::trigger(app);
            return;
        }
        if let Some(binding) = config.hotkeys.iter().find(|b| is(&b.key)) {
            match binding.target() {
                Ok(HotkeyTarget::Query(query)) => direct::open_with_query(app, query),
                Ok(HotkeyTarget::Run(id)) => direct::run_result(app, id),
                Err(reason) => tracing::warn!(key = binding.key, "hotkey has no action: {reason}"),
            }
            return;
        }
    }
    window::toggle(app);
}

/// A shortcut parse error without the parser's "please report this to ..." plea.
pub fn parse_error_reason(err: &impl ToString) -> String {
    let reason = err.to_string();
    reason
        .split(", if you feel")
        .next()
        .unwrap_or(&reason)
        .to_owned()
}

/// The Universal Actions key and the `[[hotkey]]` entries as desktop shortcuts
/// for `--setup-hotkey`. Entries without a usable action are skipped.
pub fn custom_shortcuts(config: &Config) -> Vec<CustomShortcut> {
    let actions = (!config.general.actions_hotkey.is_empty()).then(|| CustomShortcut {
        hotkey: config.general.actions_hotkey.clone(),
        target: CustomTarget::Actions,
    });
    actions
        .into_iter()
        .chain(config.hotkeys.iter().filter_map(|binding| {
            let target = match binding.target().ok()? {
                HotkeyTarget::Query(text) => CustomTarget::Query(text),
                HotkeyTarget::Run(id) => CustomTarget::Run(id),
            };
            Some(CustomShortcut {
                hotkey: binding.key.clone(),
                target,
            })
        }))
        .collect()
}

/// (Re)registers the hotkeys from the current config and records the outcome.
/// Failures (bad syntax, key owned by another app, duplicates) are reported in
/// the status, never fatal.
pub fn apply(app: &AppHandle) -> HotkeyStatus {
    let state = app.state::<AppState>();
    let config = state.config();
    let accelerator = config.general.hotkey.clone();
    // The key that currently works, to fall back to if the new one cannot be used.
    let previous = {
        let current = state.hotkey.read().unwrap_or_else(|p| p.into_inner());
        (current.mode == HotkeyMode::Global && current.error.is_none())
            .then(|| current.accelerator.clone())
    };

    let actions_key = config.general.actions_hotkey.clone();
    let (status, actions_error, custom_errors) = match state.display.hotkey_strategy() {
        HotkeyStrategy::External => {
            tracing::info!(
                "hotkeys are managed by the desktop on this session; \
                 run `sevak --setup-hotkey` to bind {accelerator} to `sevak --toggle`{}{}",
                if actions_key.is_empty() {
                    ""
                } else {
                    ", the actions_hotkey to `--actions`"
                },
                if config.hotkeys.is_empty() {
                    ""
                } else {
                    " and the [[hotkey]] entries to `--query` / `--run`"
                }
            );
            let status = HotkeyStatus {
                accelerator,
                mode: HotkeyMode::External,
                error: None,
            };
            (status, None, vec![None; config.hotkeys.len()])
        }
        HotkeyStrategy::InApp => {
            let registered = register_all(
                app,
                &accelerator,
                previous.as_deref(),
                &actions_key,
                &config.hotkeys,
            );
            match &registered.main {
                None => tracing::info!("registered global hotkey {accelerator}"),
                Some(err) => tracing::warn!("could not register hotkey {accelerator}: {err}"),
            }
            if let Some(err) = &registered.actions {
                tracing::warn!("could not register the actions hotkey {actions_key}: {err}");
            }
            let status = HotkeyStatus {
                accelerator,
                mode: HotkeyMode::Global,
                error: registered.main,
            };
            (status, registered.actions, registered.customs)
        }
    };
    let actions_status = (!actions_key.is_empty()).then(|| CustomHotkeyStatus {
        key: actions_key,
        description: ACTIONS_DESCRIPTION.to_owned(),
        error: actions_error,
    });

    let customs: Vec<CustomHotkeyStatus> = config
        .hotkeys
        .iter()
        .zip(custom_errors)
        .map(|(binding, error)| {
            match &error {
                None => tracing::debug!(key = binding.key, "hotkey entry: {}", binding.describe()),
                Some(err) => {
                    tracing::warn!(key = binding.key, "could not register hotkey entry: {err}");
                }
            }
            CustomHotkeyStatus {
                key: binding.key.clone(),
                description: binding.describe(),
                error,
            }
        })
        .collect();

    *state
        .hotkey
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = status.clone();
    *state
        .custom_hotkeys
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = customs;
    *state
        .actions_hotkey
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = actions_status;
    status
}

/// What [`register_all`] managed, key by key.
struct Registered {
    /// The main key's error.
    main: Option<String>,
    /// The Universal Actions key's error (always `None` when it is off).
    actions: Option<String>,
    /// One result per `[[hotkey]]` entry.
    customs: Vec<Option<String>>,
}

/// Replaces every registration with the main key, the Universal Actions key
/// (`actions`, empty for none) and the entries' keys.
///
/// If the main key cannot be parsed or registered, `previous` (the key that
/// worked until now) is bound instead, so a typo in `config.toml` never leaves
/// Sevak without a hotkey.
fn register_all(
    app: &AppHandle,
    accelerator: &str,
    previous: Option<&str>,
    actions: &str,
    bindings: &[HotkeyBinding],
) -> Registered {
    let shortcuts = app.global_shortcut();
    // Parse everything before touching the current registrations.
    let main = accelerator
        .parse::<Shortcut>()
        .map_err(|err| invalid_hotkey(accelerator, &err));
    let actions_parsed = (!actions.is_empty()).then(|| {
        actions
            .parse::<Shortcut>()
            .map_err(|err| invalid_hotkey(actions, &err))
    });
    let parsed: Vec<Result<Shortcut, String>> = bindings.iter().map(parse_binding).collect();

    if let Err(err) = shortcuts.unregister_all() {
        let err = err.to_string();
        return Registered {
            main: Some(err.clone()),
            actions: actions_parsed.map(|_| err.clone()),
            customs: vec![Some(err); bindings.len()],
        };
    }

    let still_active = |err: String| match previous {
        Some(previous) if previous != accelerator => format!("{err}; {previous} still works"),
        _ => err,
    };
    let mut taken: Vec<u32> = Vec::new();

    let main_error = match main.and_then(|shortcut| {
        shortcuts
            .register(shortcut)
            .map(|()| shortcut.id())
            .map_err(|err| err.to_string())
    }) {
        Ok(id) => {
            taken.push(id);
            None
        }
        Err(err) => {
            let restored = previous
                .and_then(|previous| previous.parse::<Shortcut>().ok())
                .filter(|previous| shortcuts.register(*previous).is_ok());
            if let Some(previous) = restored {
                taken.push(previous.id());
                Some(still_active(err))
            } else {
                Some(err)
            }
        }
    };

    let actions_error = actions_parsed.and_then(|shortcut| {
        let shortcut = match shortcut {
            Ok(shortcut) => shortcut,
            Err(err) => return Some(err),
        };
        if taken.contains(&shortcut.id()) {
            return Some("this key is already used by the main shortcut".into());
        }
        match shortcuts.register(shortcut) {
            Ok(()) => {
                taken.push(shortcut.id());
                None
            }
            Err(err) => Some(err.to_string()),
        }
    });

    let custom_errors = parsed
        .into_iter()
        .map(|shortcut| {
            let shortcut = shortcut?;
            if taken.contains(&shortcut.id()) {
                return Err(
                    "this key is already used by the main shortcut, the actions shortcut or another entry"
                        .into(),
                );
            }
            shortcuts
                .register(shortcut)
                .map_err(|err| err.to_string())?;
            taken.push(shortcut.id());
            Ok(())
        })
        .map(Result::err)
        .collect();
    Registered {
        main: main_error,
        actions: actions_error,
        customs: custom_errors,
    }
}

/// Parses an entry's key, and checks it has something to do.
fn parse_binding(binding: &HotkeyBinding) -> Result<Shortcut, String> {
    binding.target().map_err(str::to_owned)?;
    binding
        .key
        .parse::<Shortcut>()
        .map_err(|err| invalid_hotkey(&binding.key, &err))
}

fn invalid_hotkey(accelerator: &str, err: &impl ToString) -> String {
    format!(
        "invalid hotkey \"{accelerator}\": {}",
        parse_error_reason(err)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(key: &str, query: Option<&str>, run: Option<&str>) -> HotkeyBinding {
        HotkeyBinding {
            key: key.to_owned(),
            query: query.map(str::to_owned),
            run: run.map(str::to_owned),
        }
    }

    #[test]
    fn desktop_shortcuts_skip_entries_without_an_action() {
        let config = Config {
            general: sevak_core::GeneralConfig {
                actions_hotkey: String::new(),
                ..Default::default()
            },
            hotkeys: vec![
                binding("Ctrl+Alt+T", Some("> "), None),
                binding("Ctrl+Alt+X", None, None),
                binding("Ctrl+Alt+F", None, Some(" apps:firefox.desktop ")),
                binding("Ctrl+Alt+B", Some("a"), Some("b")),
            ],
            ..Config::default()
        };
        assert_eq!(
            custom_shortcuts(&config),
            vec![
                CustomShortcut {
                    hotkey: "Ctrl+Alt+T".to_owned(),
                    target: CustomTarget::Query("> ".to_owned()),
                },
                CustomShortcut {
                    hotkey: "Ctrl+Alt+F".to_owned(),
                    target: CustomTarget::Run("apps:firefox.desktop".to_owned()),
                },
            ]
        );
    }

    #[test]
    fn the_actions_key_is_bound_first_and_only_when_set() {
        let config = Config {
            hotkeys: vec![binding("Ctrl+Alt+T", Some("> "), None)],
            ..Config::default()
        };
        let shortcuts = custom_shortcuts(&config);
        assert_eq!(shortcuts.len(), 2);
        assert_eq!(
            shortcuts[0],
            CustomShortcut {
                hotkey: "Ctrl+Alt+Space".to_owned(),
                target: CustomTarget::Actions,
            }
        );
        let off = Config {
            general: sevak_core::GeneralConfig {
                actions_hotkey: String::new(),
                ..Default::default()
            },
            ..Config::default()
        };
        assert!(custom_shortcuts(&off).is_empty());
    }

    #[test]
    fn parse_errors_lose_the_report_plea() {
        let err = "Couldn't recognize \"Banana\" as a valid key for hotkey, if you feel like it \
                   should be, please report this to https://github.com/tauri-apps/muda";
        assert_eq!(
            parse_error_reason(&err),
            "Couldn't recognize \"Banana\" as a valid key for hotkey"
        );
        assert_eq!(parse_error_reason(&"plain"), "plain");
    }

    #[test]
    fn invalid_entries_are_reported_with_their_reason() {
        assert!(parse_binding(&binding("Ctrl+K", None, None)).is_err());
        let err = parse_binding(&binding("Banana+K", Some("x"), None)).unwrap_err();
        assert!(err.starts_with("invalid hotkey \"Banana+K\": "), "{err}");
        assert!(!err.contains("please report"));
        assert!(parse_binding(&binding("Ctrl+Alt+K", Some("x"), None)).is_ok());
    }
}
