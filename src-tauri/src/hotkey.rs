//! The global hotkeys: the show/hide key, the Universal Actions key and the
//! `[[hotkey]]` entries.
//!
//! On Windows, macOS and X11 Sevak grabs the keys itself through the
//! global-shortcut plugin. Two things the plugin cannot do are handled on top:
//!
//! - **Windows** reserves every Windows-key combination (Win+Space switches the
//!   input language) and refuses a key another app has registered. Those keys
//!   are taken with a low-level keyboard hook instead
//!   (`sevak_platform::hotkey_hook`): it sees the key first and swallows it, so
//!   neither Windows nor the other app reacts. No Windows setting changes.
//! - **macOS and GNOME** reserve Cmd+Space / Super+Space for Spotlight and the
//!   input-source switcher. When registering fails Sevak asks, once, whether it
//!   may switch that OS shortcut off (see [`crate::takeover`]); the answer is
//!   remembered.
//!
//! On Wayland applications cannot grab keys, so the desktop owns the bindings
//! and runs `sevak --toggle` / `--actions` / `--query` / `--run`; the plugin is
//! not even installed there because its X11 backend can fail to initialise
//! without a display, which would abort startup.

use std::sync::{mpsc, Arc, Mutex, OnceLock};

use serde::Serialize;
use sevak_core::config::{HotkeyBinding, HotkeyTarget};
use sevak_core::Config;
use sevak_platform::accelerator::{self, Combo, KeyNames};
use sevak_platform::gnome::{CustomShortcut, CustomTarget};
use sevak_platform::hotkey_hook::{self, Binding, HookEvent, HookSink};
use sevak_platform::{DisplayServer, HotkeyStrategy};
use tauri::{AppHandle, Emitter, Manager, Wry};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::state::{AppState, CustomHotkeyStatus, HotkeyMode, HotkeyStatus, Mechanism};
use crate::takeover::{self, ChangedSetting, Followup, TakeoverRecord};
use crate::{direct, selection, window};

/// What the Universal Actions key is called in the status.
const ACTIONS_DESCRIPTION: &str = "Universal Actions";

/// Emitted when the keyboard hook recorded a shortcut for the settings window.
pub const EVENT_RECORDED: &str = "sevak:hotkey-recorded";

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

/// Parses a shortcut like the plugin does, after rewriting the key names it
/// does not know (`Win`, `Windows`, `Meta` mean `Super`). The error is the
/// parser's reason without its "please report this" plea.
pub fn parse_shortcut(text: &str) -> Result<Shortcut, String> {
    accelerator::normalize(text)
        .parse::<Shortcut>()
        .map_err(|err| parse_error_reason(&err))
}

/// What a pressed key does, decided from the live config: the main key
/// toggles, the actions key captures the selection, an entry key does its
/// entry's action. (An unknown key can only be the previous main key, kept
/// after a failed change, or the fallback key.)
fn pressed(app: &AppHandle, shortcut: &Shortcut) {
    let config = app.state::<AppState>().config();
    let is = |accelerator: &str| {
        parse_shortcut(accelerator).is_ok_and(|parsed| parsed.id() == shortcut.id())
    };
    if !is(&config.general.hotkey) {
        let actions = config.general.actions_hotkey.as_str();
        if !actions.is_empty() && is(actions) {
            selection::trigger(app);
            return;
        }
        if let Some(binding) = all_bindings(app, &config).iter().find(|b| is(&b.key)) {
            run_binding(app, binding);
            return;
        }
    }
    window::toggle(app);
}

/// Does what an entry's key is for.
fn run_binding(app: &AppHandle, binding: &HotkeyBinding) {
    match binding.target() {
        Ok(HotkeyTarget::Query(query)) => direct::open_with_query(app, query),
        Ok(HotkeyTarget::Run(id)) => direct::run_result(app, id),
        Err(reason) => tracing::warn!(key = binding.key, "hotkey has no action: {reason}"),
    }
}

/// The `[[hotkey]]` entries of the config, then the hotkey triggers of the
/// workflows that may run (bound to `run` ids; never written to the config).
fn all_bindings(app: &AppHandle, config: &Config) -> Vec<HotkeyBinding> {
    let mut bindings = config.hotkeys.clone();
    if let Some(state) = app.try_state::<AppState>() {
        bindings.extend(state.search.workflows.hotkey_bindings(config));
    }
    bindings
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

// ---- Keys delivered by the keyboard hook ------------------------------------

/// Which of Sevak's keys a hook binding stands for. The id travels through the
/// hook; the entries are numbered in the order of [`HOOK_ENTRIES`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    Main,
    Actions,
    Entry(usize),
}

impl Slot {
    fn id(self) -> u32 {
        match self {
            Self::Main => 0,
            Self::Actions => 1,
            Self::Entry(index) => 2 + u32::try_from(index).unwrap_or(0),
        }
    }

    fn from_id(id: u32) -> Self {
        match id {
            0 => Self::Main,
            1 => Self::Actions,
            n => Self::Entry(usize::try_from(n - 2).unwrap_or(0)),
        }
    }
}

/// The entries as registered last, for the keys the hook reports.
static HOOK_ENTRIES: Mutex<Vec<HotkeyBinding>> = Mutex::new(Vec::new());

/// A hook key was pressed: does what the plugin's handler would.
fn dispatch(app: &AppHandle, slot: Slot) {
    match slot {
        Slot::Main => window::toggle(app),
        Slot::Actions => selection::trigger(app),
        Slot::Entry(index) => {
            let binding = HOOK_ENTRIES
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get(index)
                .cloned();
            match binding {
                Some(binding) => run_binding(app, &binding),
                None => tracing::warn!("a hook key without an entry was pressed"),
            }
        }
    }
}

/// What the settings window is told when the hook recorded a shortcut.
#[derive(Debug, Clone, Serialize)]
struct RecordedPayload {
    /// The shortcut, or `None` if Escape cancelled the recording.
    accelerator: Option<String>,
}

/// The hook's thread must not do any work: its events go to a thread of their
/// own, which toggles the window and so on.
fn hook_sink(app: &AppHandle) -> HookSink {
    static SENDER: OnceLock<Option<Mutex<mpsc::Sender<HookEvent>>>> = OnceLock::new();
    let sender = SENDER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<HookEvent>();
        let app = app.clone();
        std::thread::Builder::new()
            .name("sevak-hotkey-events".to_owned())
            .spawn(move || {
                for event in rx {
                    match event {
                        HookEvent::Pressed(id) => dispatch(&app, Slot::from_id(id)),
                        HookEvent::Recorded(accelerator) => {
                            let _ = app.emit(
                                EVENT_RECORDED,
                                RecordedPayload {
                                    accelerator: Some(accelerator),
                                },
                            );
                        }
                        HookEvent::RecordCancelled => {
                            let _ = app.emit(EVENT_RECORDED, RecordedPayload { accelerator: None });
                        }
                    }
                }
            })
            .ok()
            .map(|_| Mutex::new(tx))
    });
    let sender = sender.as_ref().map(|sender| {
        sender
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    });
    Arc::new(move |event| {
        if let Some(sender) = &sender {
            let _ = sender.send(event);
        }
    })
}

/// Removes the keyboard hook (on quit).
pub fn shutdown() {
    hotkey_hook::shutdown();
}

/// How a key is first tried: the keyboard hook for the keys the OS reserves
/// (Windows-key combinations), where there is one; everything else through the
/// plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    Plugin,
    Hook(Combo),
}

fn route(text: &str, hook_supported: bool) -> Route {
    match Combo::parse(text) {
        Ok(combo) if hook_supported && combo.needs_hook() => Route::Hook(combo),
        _ => Route::Plugin,
    }
}

// ---- Registering ---------------------------------------------------------------

/// (Re)registers the hotkeys from the current config and records the outcome.
/// Failures (bad syntax, key owned by another app, duplicates) are reported in
/// the status, never fatal.
pub fn apply(app: &AppHandle) -> HotkeyStatus {
    apply_with(app, true)
}

/// [`apply`] after the user answered a question about an OS shortcut (or the
/// change was made again): never asks or changes anything else.
pub fn apply_settled(app: &AppHandle) -> HotkeyStatus {
    apply_with(app, false)
}

fn apply_with(app: &AppHandle, may_change_os: bool) -> HotkeyStatus {
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
    // The config's entries, then the workflows' hotkey triggers.
    let bindings = all_bindings(app, &config);
    let record = takeover::load_record(&state.paths);
    let names = key_names(state.display);

    let (status, actions_status, custom_results) = match state.display.hotkey_strategy() {
        HotkeyStrategy::External => {
            tracing::info!(
                "hotkeys are managed by the desktop on this session; \
                 run `sevak --setup-hotkey` to bind {accelerator} to `sevak --toggle`{}{}",
                if actions_key.is_empty() {
                    ""
                } else {
                    ", the actions_hotkey to `--actions`"
                },
                if bindings.is_empty() {
                    ""
                } else {
                    " and the [[hotkey]] entries to `--query` / `--run`"
                }
            );
            let description = describe_main(&MainFacts {
                accelerator: &accelerator,
                names,
                external: true,
                error: None,
                conflict: false,
                hooked: false,
                fallback: None,
                gnome: sevak_platform::session::is_gnome(),
                record: &record,
            });
            let status = main_status(&accelerator, HotkeyMode::External, None, description);
            let customs = vec![KeyResult::external(); bindings.len()];
            (status, KeyResult::external(), customs)
        }
        HotkeyStrategy::InApp => {
            let mut registered = register_all(
                app,
                &accelerator,
                previous.as_deref(),
                &actions_key,
                &bindings,
            );
            let mut fallback: Option<String> = None;
            // Cmd+Space is Spotlight's even where registering it appears to work,
            // so on macOS the question is asked whatever the registration said.
            let macos_cmd_space =
                state.display == DisplayServer::MacOS && accelerator::is_super_space(&accelerator);
            if registered.main.error.is_some() || macos_cmd_space {
                match takeover::after_failure(app, &accelerator, may_change_os) {
                    Followup::Retry => {
                        registered = register_all(
                            app,
                            &accelerator,
                            previous.as_deref(),
                            &actions_key,
                            &bindings,
                        );
                    }
                    Followup::FallBack(key) => {
                        let retry =
                            register_all(app, &key, previous.as_deref(), &actions_key, &bindings);
                        if retry.main.error.is_none() {
                            tracing::info!("{accelerator} stays with the OS; using {key} instead");
                            registered = retry;
                            fallback = Some(key);
                        }
                    }
                    Followup::Asking | Followup::None => {}
                }
            }
            match &registered.main.error {
                None if fallback.is_none() => {
                    if registered.main.hooked {
                        tracing::info!(
                            "global hotkey {accelerator} is taken with the Windows keyboard hook"
                        );
                    } else {
                        tracing::info!("registered global hotkey {accelerator}");
                    }
                }
                None => {}
                Some(err) => tracing::warn!("could not register hotkey {accelerator}: {err}"),
            }
            if let Some(result) = &registered.actions {
                if let Some(err) = &result.error {
                    tracing::warn!("could not register the actions hotkey {actions_key}: {err}");
                }
            }
            let description = describe_main(&MainFacts {
                accelerator: &accelerator,
                names,
                external: false,
                error: registered.main.error.as_deref(),
                conflict: registered.main.conflict,
                hooked: registered.main.hooked,
                fallback: fallback.as_deref(),
                gnome: sevak_platform::session::is_gnome(),
                record: &record,
            });
            let status = main_status(
                &accelerator,
                HotkeyMode::Global,
                registered.main.error.clone(),
                description,
            );
            (
                status,
                registered.actions.unwrap_or_else(KeyResult::ok),
                registered.customs,
            )
        }
    };

    let actions_status = (!actions_key.is_empty()).then(|| CustomHotkeyStatus {
        key: actions_key,
        description: ACTIONS_DESCRIPTION.to_owned(),
        error: actions_status.error.clone(),
        mechanism: actions_status.mechanism(),
    });

    let customs: Vec<CustomHotkeyStatus> = bindings
        .iter()
        .zip(custom_results)
        .map(|(binding, result)| {
            match &result.error {
                None => tracing::debug!(key = binding.key, "hotkey entry: {}", binding.describe()),
                Some(err) => {
                    tracing::warn!(key = binding.key, "could not register hotkey entry: {err}");
                }
            }
            CustomHotkeyStatus {
                key: binding.key.clone(),
                description: binding.describe(),
                mechanism: result.mechanism(),
                error: result.error,
            }
        })
        .collect();

    *HOOK_ENTRIES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = bindings;
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

/// The key names of the OS Sevak is running on.
fn key_names(display: DisplayServer) -> KeyNames {
    match display {
        DisplayServer::Windows => KeyNames::Windows,
        DisplayServer::MacOS => KeyNames::MacOs,
        _ => KeyNames::Linux,
    }
}

/// How one key fared.
#[derive(Debug, Clone, PartialEq, Eq)]
struct KeyResult {
    error: Option<String>,
    /// Delivered by the keyboard hook.
    hooked: bool,
    /// The desktop owns the key (Wayland).
    external: bool,
    /// The error is the system saying another application or the system itself
    /// has the key (not a typo, a duplicate or a missing hook).
    conflict: bool,
}

impl KeyResult {
    fn ok() -> Self {
        Self {
            error: None,
            hooked: false,
            external: false,
            conflict: false,
        }
    }

    fn external() -> Self {
        Self {
            external: true,
            ..Self::ok()
        }
    }

    fn failed(error: impl Into<String>) -> Self {
        Self {
            error: Some(error.into()),
            ..Self::ok()
        }
    }

    fn mechanism(&self) -> Mechanism {
        if self.error.is_some() {
            Mechanism::Inactive
        } else if self.hooked {
            Mechanism::WindowsHook
        } else if self.external {
            Mechanism::Desktop
        } else {
            Mechanism::Registered
        }
    }
}

/// What [`register_all`] managed, key by key.
struct Registered {
    main: KeyResult,
    /// The Universal Actions key's result (`None` when it is off).
    actions: Option<KeyResult>,
    /// One result per `[[hotkey]]` entry.
    customs: Vec<KeyResult>,
}

/// The keys claimed so far, to catch duplicates, and the ones the hook serves.
#[derive(Default)]
struct Claims {
    ids: Vec<u32>,
    combos: Vec<Combo>,
    hooked: Vec<Binding>,
}

impl Claims {
    fn is_taken(&self, shortcut: Option<&Shortcut>, combo: Option<&Combo>) -> bool {
        shortcut.is_some_and(|shortcut| self.ids.contains(&shortcut.id()))
            || combo.is_some_and(|combo| self.combos.contains(combo))
    }

    /// Claims one key: through the hook if that is its route, else through the
    /// plugin, and through the hook after all if the plugin cannot have it.
    fn claim(
        &mut self,
        app: &AppHandle,
        slot: Slot,
        text: &str,
        hook_supported: bool,
    ) -> KeyResult {
        let shortcut = parse_shortcut(text);
        let combo = Combo::parse(text).ok();
        let hookable = hook_supported && combo.is_some();
        if shortcut.is_err() && !hookable {
            return KeyResult::failed(invalid_hotkey(text, &shortcut.err().unwrap_or_default()));
        }
        if self.is_taken(shortcut.as_ref().ok(), combo.as_ref()) {
            return KeyResult::failed(match slot {
                Slot::Main => "this key is already in use".to_owned(),
                Slot::Actions => "this key is already used by the main shortcut".to_owned(),
                Slot::Entry(_) => "this key is already used by the main shortcut, the actions shortcut or another entry".to_owned(),
            });
        }

        let by_hook = |claims: &mut Self, combo: Combo| {
            claims.combos.push(combo);
            claims.hooked.push(Binding {
                id: slot.id(),
                combo,
            });
            KeyResult {
                hooked: true,
                ..KeyResult::ok()
            }
        };
        if let Route::Hook(combo) = route(text, hook_supported) {
            return by_hook(self, combo);
        }
        let Ok(shortcut) = shortcut else {
            // Only the hook can read it.
            return by_hook(self, combo.expect("hookable has a combo"));
        };
        match app.global_shortcut().register(shortcut) {
            Ok(()) => {
                self.ids.push(shortcut.id());
                if let Some(combo) = combo {
                    self.combos.push(combo);
                }
                KeyResult::ok()
            }
            // Another app has the key: the hook takes it from them.
            Err(_) if hookable => {
                tracing::info!("another application has {text}; taking it with the keyboard hook");
                by_hook(self, combo.expect("hookable has a combo"))
            }
            Err(err) => KeyResult {
                conflict: true,
                ..KeyResult::failed(err.to_string())
            },
        }
    }
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
    let hook_supported = hotkey_hook::supported();

    if let Err(err) = shortcuts.unregister_all() {
        let err = err.to_string();
        return Registered {
            main: KeyResult::failed(err.clone()),
            actions: (!actions.is_empty()).then(|| KeyResult::failed(err.clone())),
            customs: vec![KeyResult::failed(err); bindings.len()],
        };
    }

    let mut claims = Claims::default();
    let mut main = claims.claim(app, Slot::Main, accelerator, hook_supported);
    if let Some(err) = main.error.clone() {
        let restored = previous
            .filter(|previous| *previous != accelerator)
            .map(|previous| {
                (
                    previous,
                    claims.claim(app, Slot::Main, previous, hook_supported),
                )
            })
            .filter(|(_, result)| result.error.is_none());
        main = match restored {
            Some((previous, result)) => KeyResult {
                error: Some(format!("{err}; {previous} still works")),
                ..result
            },
            None => main,
        };
    }

    let mut actions =
        (!actions.is_empty()).then(|| claims.claim(app, Slot::Actions, actions, hook_supported));

    let mut customs: Vec<KeyResult> = bindings
        .iter()
        .enumerate()
        .map(|(index, binding)| match binding.target() {
            Err(reason) => KeyResult::failed(reason),
            Ok(_) => claims.claim(app, Slot::Entry(index), &binding.key, hook_supported),
        })
        .collect();

    // The hook itself: installed while there is a key for it, removed when not.
    if hook_supported {
        if let Err(err) = hotkey_hook::configure(claims.hooked.clone(), hook_sink(app)) {
            tracing::warn!("the keyboard hook could not be installed: {err}");
            let message = format!("the Windows keyboard hook could not be installed: {err}");
            for binding in &claims.hooked {
                let result = match Slot::from_id(binding.id) {
                    Slot::Main => &mut main,
                    Slot::Actions => match actions.as_mut() {
                        Some(result) => result,
                        None => continue,
                    },
                    Slot::Entry(index) => match customs.get_mut(index) {
                        Some(result) => result,
                        None => continue,
                    },
                };
                *result = KeyResult::failed(message.clone());
            }
        }
    }

    Registered {
        main,
        actions,
        customs,
    }
}

fn invalid_hotkey(accelerator: &str, reason: &str) -> String {
    format!("invalid hotkey \"{accelerator}\": {reason}")
}

// ---- What the status says -------------------------------------------------------

/// What is known about the main key once registering is over.
struct MainFacts<'a> {
    accelerator: &'a str,
    names: KeyNames,
    /// The desktop owns the key (Wayland).
    external: bool,
    error: Option<&'a str>,
    /// The error is the system saying the key is taken.
    conflict: bool,
    hooked: bool,
    /// The key used instead of `accelerator` (macOS, Spotlight kept).
    fallback: Option<&'a str>,
    /// Running under GNOME.
    gnome: bool,
    record: &'a TakeoverRecord,
}

/// How the main key is delivered, in words, and what the user may do about it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MainDescription {
    mechanism: Mechanism,
    note: Option<String>,
    can_take_over: bool,
    can_restore: bool,
}

fn describe_main(facts: &MainFacts<'_>) -> MainDescription {
    let macos = facts.names == KeyNames::MacOs;
    let shown = accelerator::display(facts.accelerator, facts.names);
    let super_space = accelerator::is_super_space(facts.accelerator);
    let changed = takeover::changed_setting(facts.record, macos);
    let can_restore = facts.record.can_restore();
    let can_take_over = super_space
        && !facts.external
        && ((macos && !facts.record.spotlight_disabled())
            || (!macos
                && facts.names == KeyNames::Linux
                && facts.gnome
                && !facts.record.gnome_moved()));
    let owner = if macos {
        "Spotlight"
    } else {
        "GNOME's input-source switcher"
    };

    let (mechanism, note) = if facts.external {
        (
            Mechanism::Desktop,
            Some(
                "The desktop owns this shortcut and runs `sevak --toggle`. Use \"Set up GNOME \
                 shortcut\" (or `sevak --setup-hotkey`) after changing it."
                    .to_owned(),
            ),
        )
    } else if facts.error.is_some() {
        let note = if can_take_over {
            Some(format!(
                "{shown} is used by {owner}. Sevak can use it if you allow it (below)."
            ))
        } else if facts.conflict {
            Some(format!(
                "{shown} is already used by another application or by the system. Choose \
                 another shortcut, or free this one where it is set."
            ))
        } else {
            None
        };
        (Mechanism::Inactive, note)
    } else if facts.hooked {
        let windows_key = Combo::parse(facts.accelerator).is_ok_and(|combo| combo.needs_hook());
        let note = if windows_key {
            format!(
                "Taken over with the Windows keyboard hook: while Sevak runs, Windows no longer \
                 sees {shown}. No Windows setting was changed."
            )
        } else {
            format!(
                "Another application had registered {shown}, so Sevak took it over with the \
                 Windows keyboard hook: while Sevak runs, that application no longer sees it."
            )
        };
        (Mechanism::WindowsHook, Some(note))
    } else if let Some(fallback) = facts.fallback {
        let fallback = accelerator::display(fallback, facts.names);
        (
            Mechanism::FallbackKey,
            Some(format!(
                "{shown} stays with {owner}, as you chose, so Sevak is using {fallback} \
                 instead. You can let Sevak use {shown} below."
            )),
        )
    } else if super_space && changed == Some(ChangedSetting::Spotlight) {
        (
            Mechanism::SpotlightDisabled,
            Some(format!(
                "Spotlight's shortcut is turned off, with your permission, so Sevak can use \
                 {shown}. You can restore it below."
            )),
        )
    } else if super_space && changed == Some(ChangedSetting::GnomeInputSources) {
        (
            Mechanism::GnomeInputSourceMoved,
            Some(format!(
                "GNOME's input-source shortcut was moved to Ctrl+Super+Space, with your \
                 permission, so Sevak can use {shown}. You can restore it below."
            )),
        )
    } else {
        (Mechanism::Registered, None)
    };
    MainDescription {
        mechanism,
        note,
        can_take_over,
        can_restore,
    }
}

fn main_status(
    accelerator: &str,
    mode: HotkeyMode,
    error: Option<String>,
    description: MainDescription,
) -> HotkeyStatus {
    HotkeyStatus {
        accelerator: accelerator.to_owned(),
        mode,
        error,
        mechanism: description.mechanism,
        note: description.note,
        can_take_over: description.can_take_over,
        can_restore: description.can_restore,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::takeover::{GnomeRecord, MovedKey, SpotlightChoice};

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
    fn the_windows_key_has_the_names_users_type() {
        let reference = parse_shortcut("Super+Space").unwrap();
        for name in [
            "Win+Space",
            "windows + space",
            "Meta+Space",
            "Cmd+Space",
            "SUPER+space",
        ] {
            assert_eq!(
                parse_shortcut(name)
                    .unwrap_or_else(|err| panic!("{name}: {err}"))
                    .id(),
                reference.id(),
                "{name}"
            );
        }
        // Nothing else became valid.
        for bad in ["", "Win+", "Banana+Space", "Win", "Ctrl+Shift"] {
            assert!(parse_shortcut(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn invalid_keys_are_reported_with_their_reason() {
        let err = parse_shortcut("Banana+K").unwrap_err();
        assert!(!err.contains("please report"));
        let text = invalid_hotkey("Banana+K", &err);
        assert!(text.starts_with("invalid hotkey \"Banana+K\": "), "{text}");
    }

    #[test]
    fn windows_key_combinations_go_to_the_hook_and_only_where_there_is_one() {
        let space = Combo::parse("Super+Space").unwrap();
        assert_eq!(route("Super+Space", true), Route::Hook(space));
        assert_eq!(route("win + space", true), Route::Hook(space));
        assert_eq!(route("Super+Space", false), Route::Plugin);
        assert_eq!(route("Alt+Space", true), Route::Plugin);
        assert_eq!(route("Ctrl+Alt+K", true), Route::Plugin);
        assert_eq!(route("Banana", true), Route::Plugin);
    }

    #[test]
    fn slots_survive_the_trip_through_the_hook() {
        for slot in [Slot::Main, Slot::Actions, Slot::Entry(0), Slot::Entry(7)] {
            assert_eq!(Slot::from_id(slot.id()), slot);
        }
        let ids: Vec<u32> = [Slot::Main, Slot::Actions, Slot::Entry(0), Slot::Entry(1)]
            .iter()
            .map(|slot| slot.id())
            .collect();
        assert_eq!(ids, [0, 1, 2, 3]);
    }

    fn facts<'a>(accelerator: &'a str, record: &'a TakeoverRecord) -> MainFacts<'a> {
        MainFacts {
            accelerator,
            names: KeyNames::Windows,
            external: false,
            error: None,
            conflict: false,
            hooked: false,
            fallback: None,
            gnome: false,
            record,
        }
    }

    fn spotlight_off() -> TakeoverRecord {
        TakeoverRecord {
            spotlight: Some(SpotlightChoice::Disabled),
            ..TakeoverRecord::default()
        }
    }

    fn gnome_moved() -> TakeoverRecord {
        TakeoverRecord {
            gnome: Some(GnomeRecord {
                declined: false,
                moves: vec![MovedKey {
                    schema: "s".into(),
                    key: "k".into(),
                    previous: vec![],
                    updated: vec![],
                }],
            }),
            ..TakeoverRecord::default()
        }
    }

    #[test]
    fn a_plain_registration_needs_no_explanation() {
        let record = TakeoverRecord::default();
        let described = describe_main(&facts("Alt+Space", &record));
        assert_eq!(described.mechanism, Mechanism::Registered);
        assert_eq!(described.note, None);
        assert!(!described.can_take_over && !described.can_restore);
    }

    #[test]
    fn the_windows_hook_is_named_and_says_nothing_was_changed() {
        let record = TakeoverRecord::default();
        let described = describe_main(&MainFacts {
            hooked: true,
            ..facts("Super+Space", &record)
        });
        assert_eq!(described.mechanism, Mechanism::WindowsHook);
        let note = described.note.unwrap();
        assert!(note.contains("Windows keyboard hook"), "{note}");
        assert!(note.contains("Win+Space"), "{note}");
        assert!(note.contains("No Windows setting was changed"), "{note}");
        // A key another app had registered is explained differently.
        let described = describe_main(&MainFacts {
            hooked: true,
            ..facts("Ctrl+Alt+T", &record)
        });
        assert!(described.note.unwrap().contains("Another application"));
    }

    #[test]
    fn cmd_space_failing_on_macos_offers_the_takeover() {
        let record = TakeoverRecord::default();
        let described = describe_main(&MainFacts {
            names: KeyNames::MacOs,
            error: Some("already registered"),
            ..facts("Super+Space", &record)
        });
        assert_eq!(described.mechanism, Mechanism::Inactive);
        assert!(described.can_take_over && !described.can_restore);
        let note = described.note.unwrap();
        assert!(note.contains("Cmd+Space is used by Spotlight"), "{note}");
    }

    #[test]
    fn a_turned_off_spotlight_shortcut_is_reported_and_can_be_restored() {
        let record = spotlight_off();
        let described = describe_main(&MainFacts {
            names: KeyNames::MacOs,
            ..facts("Super+Space", &record)
        });
        assert_eq!(described.mechanism, Mechanism::SpotlightDisabled);
        assert!(described.can_restore && !described.can_take_over);
        assert!(described.note.unwrap().contains("with your permission"));
        // Restorable even when the shortcut is something else now.
        let described = describe_main(&MainFacts {
            names: KeyNames::MacOs,
            ..facts("Alt+Space", &record)
        });
        assert_eq!(described.mechanism, Mechanism::Registered);
        assert!(described.can_restore);
    }

    #[test]
    fn declining_spotlight_uses_option_space_and_says_so() {
        let record = TakeoverRecord {
            spotlight: Some(SpotlightChoice::Declined),
            ..TakeoverRecord::default()
        };
        let described = describe_main(&MainFacts {
            names: KeyNames::MacOs,
            fallback: Some("Alt+Space"),
            ..facts("Super+Space", &record)
        });
        assert_eq!(described.mechanism, Mechanism::FallbackKey);
        assert!(described.can_take_over);
        let note = described.note.unwrap();
        assert!(note.contains("Option+Space"), "{note}");
        assert!(note.contains("Cmd+Space"), "{note}");
    }

    #[test]
    fn gnome_input_sources_are_reported_and_restorable() {
        let none = TakeoverRecord::default();
        let described = describe_main(&MainFacts {
            names: KeyNames::Linux,
            gnome: true,
            error: Some("grab failed"),
            ..facts("Super+Space", &none)
        });
        assert!(described.can_take_over);
        assert!(described
            .note
            .unwrap()
            .contains("GNOME's input-source switcher"));
        // Not GNOME: nothing to offer, but the conflict is said clearly.
        let described = describe_main(&MainFacts {
            names: KeyNames::Linux,
            error: Some("grab failed"),
            conflict: true,
            ..facts("Super+Space", &none)
        });
        assert!(!described.can_take_over);
        let note = described.note.unwrap();
        assert!(
            note.contains("Super+Space is already used by another application or by the system"),
            "{note}"
        );

        let record = gnome_moved();
        let described = describe_main(&MainFacts {
            names: KeyNames::Linux,
            gnome: true,
            ..facts("Super+Space", &record)
        });
        assert_eq!(described.mechanism, Mechanism::GnomeInputSourceMoved);
        assert!(described.can_restore && !described.can_take_over);
    }

    #[test]
    fn the_desktop_owned_key_is_described_as_such() {
        let record = gnome_moved();
        let described = describe_main(&MainFacts {
            names: KeyNames::Linux,
            external: true,
            gnome: true,
            ..facts("Super+Space", &record)
        });
        assert_eq!(described.mechanism, Mechanism::Desktop);
        assert!(described.note.unwrap().contains("sevak --toggle"));
        assert!(!described.can_take_over);
        assert!(described.can_restore);
    }

    #[test]
    fn other_errors_get_no_takeover() {
        let record = TakeoverRecord::default();
        let described = describe_main(&MainFacts {
            error: Some("invalid hotkey"),
            ..facts("Alt+Space", &record)
        });
        assert_eq!(described.mechanism, Mechanism::Inactive);
        assert!(!described.can_take_over && described.note.is_none());
        // Neither does a duplicate or a missing hook say "another application".
        let described = describe_main(&MainFacts {
            error: Some("the Windows keyboard hook could not be installed: denied"),
            ..facts("Super+Space", &record)
        });
        assert!(described.note.is_none());
    }

    #[test]
    fn a_keys_mechanism_follows_how_it_was_claimed() {
        assert_eq!(KeyResult::ok().mechanism(), Mechanism::Registered);
        assert_eq!(KeyResult::external().mechanism(), Mechanism::Desktop);
        assert_eq!(KeyResult::failed("x").mechanism(), Mechanism::Inactive);
        let hooked = KeyResult {
            hooked: true,
            ..KeyResult::ok()
        };
        assert_eq!(hooked.mechanism(), Mechanism::WindowsHook);
    }
}
