//! Taking the launcher key over from the operating system, with permission.
//!
//! `Super+Space` (Win+Space, Cmd+Space) is the default shortcut, and every OS
//! already uses it for something:
//!
//! - **Windows**: the input-language switcher. Sevak takes the key with a
//!   keyboard hook (`sevak_platform::hotkey_hook`); no Windows setting changes,
//!   so there is nothing to ask and nothing in here.
//! - **macOS**: Spotlight. Sevak can turn Spotlight's shortcut off, but only
//!   after the user says yes, once. "No" falls back to Option+Space.
//! - **GNOME**: the input-source switcher. Sevak can move it from Super+Space to
//!   Ctrl+Super+Space, but only after the user says yes, once.
//!
//! What was decided, and what was changed (so it can be put back), is kept in
//! `hotkey-takeover.json` in the data folder; with it Sevak never asks twice
//! and `sevak --restore-hotkey` and the Settings buttons can undo the change.
//! Other applications' settings are never touched.
//!
//! The flows take the outside world as parameters (the question to ask, the
//! system commands, GNOME's settings, the record) so they are tested with fakes;
//! only [`ask`] and [`FileStore`] are real.

#[cfg(target_os = "linux")]
use std::io::{BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use sevak_platform::accelerator::{self, KeyNames};
use sevak_platform::gnome::{self, Gsettings, InputSourceMove};
use sevak_platform::spotlight::{self, CommandRunner, SpotlightShortcut, SystemRunner};
use sevak_platform::{private_file, session, AppPaths, DisplayServer};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

use crate::state::AppState;
use crate::{hotkey, window};

/// The file in the data folder holding the record.
const FILE_NAME: &str = "hotkey-takeover.json";

/// What the user decided about Spotlight's shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpotlightChoice {
    /// Allowed: Spotlight's shortcut is off.
    Disabled,
    /// Said no (or put it back): never ask again; Sevak uses Option+Space.
    Declined,
}

/// One GNOME shortcut Sevak moved, as stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MovedKey {
    pub schema: String,
    pub key: String,
    pub previous: Vec<String>,
    pub updated: Vec<String>,
}

impl From<&InputSourceMove> for MovedKey {
    fn from(change: &InputSourceMove) -> Self {
        Self {
            schema: change.schema.clone(),
            key: change.key.clone(),
            previous: change.previous.clone(),
            updated: change.updated.clone(),
        }
    }
}

impl From<&MovedKey> for InputSourceMove {
    fn from(moved: &MovedKey) -> Self {
        Self {
            schema: moved.schema.clone(),
            key: moved.key.clone(),
            previous: moved.previous.clone(),
            updated: moved.updated.clone(),
        }
    }
}

/// What the user decided about GNOME's input-source shortcuts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GnomeRecord {
    /// Said no (or put them back): never ask again.
    #[serde(default)]
    pub declined: bool,
    /// What was changed and is still changed.
    #[serde(default)]
    pub moves: Vec<MovedKey>,
}

/// Everything Sevak remembers about taking the key over.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TakeoverRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spotlight: Option<SpotlightChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gnome: Option<GnomeRecord>,
}

impl TakeoverRecord {
    /// Spotlight's shortcut is off because Sevak turned it off.
    pub fn spotlight_disabled(&self) -> bool {
        self.spotlight == Some(SpotlightChoice::Disabled)
    }

    /// GNOME's input-source shortcuts are moved because Sevak moved them.
    pub fn gnome_moved(&self) -> bool {
        self.gnome
            .as_ref()
            .is_some_and(|gnome| !gnome.moves.is_empty())
    }

    /// Whether an OS setting is changed that Sevak can put back.
    pub fn can_restore(&self) -> bool {
        self.spotlight_disabled() || self.gnome_moved()
    }

    fn gnome_declined(&self) -> bool {
        self.gnome.as_ref().is_some_and(|gnome| gnome.declined)
    }
}

/// Where the record lives.
pub trait Store {
    /// The record; empty if there is none or it cannot be read.
    fn load(&self) -> TakeoverRecord;
    fn save(&self, record: &TakeoverRecord) -> Result<(), String>;
}

/// The record in the data folder.
pub struct FileStore {
    path: PathBuf,
}

impl FileStore {
    pub fn new(paths: &AppPaths) -> Self {
        Self {
            path: paths.data_dir.join(FILE_NAME),
        }
    }
}

impl Store for FileStore {
    fn load(&self) -> TakeoverRecord {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|err| {
                tracing::warn!(path = %self.path.display(), "ignoring an unreadable {FILE_NAME}: {err}");
                TakeoverRecord::default()
            }),
            Err(_) => TakeoverRecord::default(),
        }
    }

    fn save(&self, record: &TakeoverRecord) -> Result<(), String> {
        let text = serde_json::to_string_pretty(record).map_err(|err| err.to_string())?;
        private_file::write_atomic(&self.path, text.as_bytes())
            .map_err(|err| format!("could not save {}: {err}", self.path.display()))
    }
}

/// What to do about a launcher key that could not be registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    /// Nothing more to try.
    Nothing,
    /// Ask the user (once).
    Ask,
    /// The user already agreed and the OS undid it: do it again, quietly.
    Redo,
    /// The user said no: use the fallback key.
    FallBack,
}

/// macOS: what to do now that Cmd+Space could not be registered.
pub fn spotlight_plan(shortcut: SpotlightShortcut, record: &TakeoverRecord) -> Plan {
    match (shortcut, record.spotlight) {
        // Cmd+Space is not Spotlight's, so something else has it.
        (SpotlightShortcut::Disabled | SpotlightShortcut::Remapped, _) => Plan::Nothing,
        (SpotlightShortcut::Active, None) => Plan::Ask,
        (SpotlightShortcut::Active, Some(SpotlightChoice::Declined)) => Plan::FallBack,
        (SpotlightShortcut::Active, Some(SpotlightChoice::Disabled)) => Plan::Redo,
    }
}

/// GNOME: what to do now that Super+Space could not be registered, given the
/// input-source shortcuts that use it (`conflicting`).
pub fn gnome_plan(conflicting: bool, record: &TakeoverRecord) -> Plan {
    if !conflicting {
        return Plan::Nothing;
    }
    if record.gnome_declined() {
        Plan::Nothing
    } else if record.gnome.is_some() {
        Plan::Redo
    } else {
        Plan::Ask
    }
}

/// The question about Spotlight.
pub fn spotlight_question(key: &str, fallback: &str) -> String {
    format!(
        "{key} is used by Spotlight. Let Sevak use it?\n\n\
         If you say Yes, Sevak turns off Spotlight's \"Show Spotlight search\" keyboard \
         shortcut. Nothing else about Spotlight changes, and you can turn the shortcut back \
         on at any time with \"Restore Spotlight's shortcut\" in Sevak's Settings (or in \
         System Settings > Keyboard > Keyboard Shortcuts > Spotlight).\n\n\
         If you say No, Sevak uses {fallback} instead. Sevak will not ask again."
    )
}

/// The question about GNOME, with what would change.
pub fn gnome_question(key: &str, changes: &[InputSourceMove]) -> String {
    let lines: String = changes
        .iter()
        .map(|change| format!("  {}\n", change.describe()))
        .collect();
    format!(
        "{key} is used by GNOME to switch input sources. Let Sevak use it?\n\n\
         If you say Yes, Sevak moves GNOME's input-source shortcut to a different key \
         (the same keys with Ctrl added) and remembers the old setting:\n\n{lines}\n\
         You can put it back with \"Restore\" in Sevak's Settings or `sevak --restore-hotkey`.\n\n\
         If you say No, nothing changes and Sevak will not ask again."
    )
}

/// Shows a Yes/No question over everything and waits. Blocks, so only call it
/// off the main thread. Anything but Yes (No, closing the box, no box
/// available) declines.
pub fn ask(app: &AppHandle, message: String) -> bool {
    app.dialog()
        .message(message)
        .title("Sevak")
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::YesNo)
        .blocking_show()
}

/// What a flow did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// An OS setting was changed.
    Changed,
    /// The user said no; it is recorded.
    Declined,
    /// There was nothing to do.
    Nothing,
}

/// The outside world of the flows.
pub struct Env<'a> {
    /// Asks the user; `true` is yes.
    pub confirm: &'a dyn Fn(String) -> bool,
    pub runner: &'a dyn CommandRunner,
    pub gnome: Option<&'a dyn Gsettings>,
    pub store: &'a dyn Store,
}

/// macOS: turns Spotlight's shortcut off. With `ask` the user is asked first;
/// without it they already agreed (see [`Plan::Redo`]).
pub fn take_spotlight(
    env: &Env<'_>,
    ask: bool,
    key: &str,
    fallback: &str,
) -> Result<Outcome, String> {
    let mut record = env.store.load();
    if ask && !(env.confirm)(spotlight_question(key, fallback)) {
        record.spotlight = Some(SpotlightChoice::Declined);
        env.store.save(&record)?;
        tracing::info!("Spotlight's shortcut was kept at the user's request");
        return Ok(Outcome::Declined);
    }
    spotlight::disable(env.runner)?;
    record.spotlight = Some(SpotlightChoice::Disabled);
    env.store.save(&record)?;
    tracing::info!(
        "Spotlight's \"Show Spotlight search\" shortcut was turned off with the user's permission"
    );
    Ok(Outcome::Changed)
}

/// macOS: turns Spotlight's shortcut back on. Sevak will not ask about it
/// again: the user chose it.
pub fn restore_spotlight(env: &Env<'_>) -> Result<(), String> {
    let mut record = env.store.load();
    spotlight::restore(env.runner)?;
    record.spotlight = Some(SpotlightChoice::Declined);
    env.store.save(&record)?;
    tracing::info!("Spotlight's shortcut was turned back on");
    Ok(())
}

/// GNOME: the input-source shortcuts that use `accelerator` (GNOME syntax) and
/// where they would go. Empty if there is no conflict (or no GNOME settings).
pub fn gnome_changes(env: &Env<'_>, accelerator: &str) -> Vec<InputSourceMove> {
    let Some(gnome) = env.gnome else {
        return Vec::new();
    };
    gnome::plan_input_source_move(gnome, accelerator).unwrap_or_else(|err| {
        tracing::debug!(%err, "could not read GNOME's input-source shortcuts");
        Vec::new()
    })
}

/// GNOME: moves the input-source shortcuts off `accelerator`. With `ask` the
/// user is asked first. The record is written before anything changes, so the
/// old values are never lost.
pub fn move_gnome(
    env: &Env<'_>,
    ask: bool,
    key: &str,
    accelerator: &str,
) -> Result<Outcome, String> {
    let Some(gs) = env.gnome else {
        return Ok(Outcome::Nothing);
    };
    let changes = gnome_changes(env, accelerator);
    if changes.is_empty() {
        return Ok(Outcome::Nothing);
    }
    let mut record = env.store.load();
    let before = record.clone();
    if ask && !(env.confirm)(gnome_question(key, &changes)) {
        record.gnome = Some(GnomeRecord {
            declined: true,
            moves: record.gnome.map(|gnome| gnome.moves).unwrap_or_default(),
        });
        env.store.save(&record)?;
        tracing::info!("GNOME's input-source shortcuts were kept at the user's request");
        return Ok(Outcome::Declined);
    }

    // Keep the oldest `previous` of a key we already moved once.
    let mut moves = record
        .gnome
        .take()
        .map(|gnome| gnome.moves)
        .unwrap_or_default();
    for change in &changes {
        match moves.iter_mut().find(|moved| moved.key == change.key) {
            Some(moved) => moved.updated = change.updated.clone(),
            None => moves.push(change.into()),
        }
    }
    record.gnome = Some(GnomeRecord {
        declined: false,
        moves,
    });
    env.store.save(&record)?;
    if let Err(err) = gnome::apply_input_source_move(gs, &changes) {
        let _ = env.store.save(&before);
        return Err(format!("could not change GNOME's shortcuts: {err}"));
    }
    Ok(Outcome::Changed)
}

/// GNOME: puts the input-source shortcuts back as they were. One line per key.
pub fn restore_gnome(env: &Env<'_>) -> Result<Vec<String>, String> {
    let mut record = env.store.load();
    let moves: Vec<InputSourceMove> = record
        .gnome
        .as_ref()
        .map(|gnome| gnome.moves.iter().map(InputSourceMove::from).collect())
        .unwrap_or_default();
    if moves.is_empty() {
        return Ok(Vec::new());
    }
    let Some(gs) = env.gnome else {
        return Err("GNOME's settings are not available here".to_owned());
    };
    let lines = gnome::restore_input_sources(gs, &moves)?;
    for line in &lines {
        tracing::info!("GNOME shortcut: {line}");
    }
    record.gnome = Some(GnomeRecord {
        declined: true,
        moves: Vec::new(),
    });
    env.store.save(&record)?;
    Ok(lines)
}

/// `sevak --restore-hotkey`, and the Settings buttons: puts back whatever Sevak
/// changed. Returns what was done, one line each (empty if nothing was changed).
pub fn restore_all(env: &Env<'_>, macos: bool) -> Result<Vec<String>, String> {
    let mut lines = Vec::new();
    if macos && env.store.load().spotlight_disabled() {
        restore_spotlight(env)?;
        lines.push("Spotlight's \"Show Spotlight search\" shortcut is on again.".to_owned());
    }
    lines.extend(restore_gnome(env)?);
    Ok(lines)
}

/// Whether the record says Sevak changed a setting for `accelerator`'s sake on
/// this OS (macOS: Spotlight, GNOME: input sources), for the status line.
pub fn changed_setting(record: &TakeoverRecord, macos: bool) -> Option<ChangedSetting> {
    if macos && record.spotlight_disabled() {
        Some(ChangedSetting::Spotlight)
    } else if !macos && record.gnome_moved() {
        Some(ChangedSetting::GnomeInputSources)
    } else {
        None
    }
}

/// An OS setting Sevak changed with permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangedSetting {
    Spotlight,
    GnomeInputSources,
}

// ---- Running the flows for real --------------------------------------------------

/// The key Sevak uses when Cmd+Space stays with Spotlight.
const FALLBACK_KEY: &str = "Alt+Space";

/// The record in the data folder.
pub fn load_record(paths: &AppPaths) -> TakeoverRecord {
    FileStore::new(paths).load()
}

/// What `apply` does after the launcher key could not be registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Followup {
    /// Nothing more to try.
    None,
    /// An OS shortcut was switched off (as agreed earlier): register again.
    Retry,
    /// The user kept the OS shortcut: register this key instead.
    FallBack(String),
    /// The user is being asked; the answer re-registers.
    Asking,
}

/// Runs `f` with the real system: the dialog, `defaults`, `gsettings` and the
/// record in the data folder.
fn with_env<T>(app: &AppHandle, f: impl FnOnce(&Env<'_>) -> T) -> T {
    let store = FileStore::new(&app.state::<AppState>().paths);
    let confirm = |message: String| ask(app, message);
    let gnome = gnome::system_gsettings();
    let env = Env {
        confirm: &confirm,
        runner: &SystemRunner,
        gnome: gnome.as_deref(),
        store: &store,
    };
    f(&env)
}

/// Which OS shortcut stands in the way of Super+Space here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    Spotlight,
    Gnome,
}

fn flow_for(display: DisplayServer) -> Option<Flow> {
    match display {
        DisplayServer::MacOS => Some(Flow::Spotlight),
        DisplayServer::Windows => None,
        _ => session::is_gnome().then_some(Flow::Gnome),
    }
}

/// Runs a flow. The key is shown the way this OS names it.
fn run_flow(app: &AppHandle, flow: Flow, accelerator: &str, ask: bool) -> Result<Outcome, String> {
    with_env(app, |env| match flow {
        Flow::Spotlight => take_spotlight(
            env,
            ask,
            &accelerator::display(accelerator, KeyNames::MacOs),
            &accelerator::display(FALLBACK_KEY, KeyNames::MacOs),
        ),
        Flow::Gnome => {
            let gnome_key =
                gnome::to_gnome_accelerator(accelerator).map_err(|err| err.to_string())?;
            move_gnome(
                env,
                ask,
                &accelerator::display(accelerator, KeyNames::Linux),
                &gnome_key,
            )
        }
    })
}

/// Tells every window the hotkey status changed.
fn announce(app: &AppHandle) {
    let status = app.state::<AppState>().status();
    if let Err(err) = app.emit(window::EVENT_STATUS, status) {
        tracing::warn!("could not emit {}: {err}", window::EVENT_STATUS);
    }
}

/// Whether the question is on screen (one at a time).
static ASKING: AtomicBool = AtomicBool::new(false);

/// Asks the user on a thread of its own (the box blocks), then registers again.
fn ask_in_background(app: &AppHandle, flow: Flow, accelerator: String) {
    if ASKING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("sevak-takeover".to_owned())
        .spawn(move || {
            let result = run_flow(&app, flow, &accelerator, true);
            ASKING.store(false, Ordering::SeqCst);
            match result {
                Ok(outcome) => tracing::info!(?outcome, ?flow, "hotkey takeover answered"),
                Err(err) => tracing::warn!(?flow, "hotkey takeover failed: {err}"),
            }
            hotkey::apply_settled(&app);
            announce(&app);
        });
    if spawned.is_err() {
        ASKING.store(false, Ordering::SeqCst);
    }
}

/// What to do now that `accelerator` (the main key) could not be registered.
///
/// Only Super+Space has an OS owner Sevak knows how to ask about: Spotlight on
/// macOS, the input-source switcher on GNOME. `may_change` is false once the
/// user has answered, so nothing is asked or changed twice in a row.
pub fn after_failure(app: &AppHandle, accelerator: &str, may_change: bool) -> Followup {
    if !accelerator::is_super_space(accelerator) {
        return Followup::None;
    }
    let state = app.state::<AppState>();
    let Some(flow) = flow_for(state.display) else {
        return Followup::None;
    };
    let record = load_record(&state.paths);
    let plan = match flow {
        Flow::Spotlight => match spotlight::shortcut_state(&SystemRunner) {
            Ok(shortcut) => spotlight_plan(shortcut, &record),
            Err(err) => {
                tracing::warn!("could not read Spotlight's shortcut: {err}");
                Plan::Nothing
            }
        },
        Flow::Gnome => {
            let conflicting = gnome::to_gnome_accelerator(accelerator)
                .is_ok_and(|key| with_env(app, |env| !gnome_changes(env, &key).is_empty()));
            gnome_plan(conflicting, &record)
        }
    };
    match plan {
        Plan::Nothing => Followup::None,
        Plan::FallBack => Followup::FallBack(FALLBACK_KEY.to_owned()),
        Plan::Ask if may_change => {
            ask_in_background(app, flow, accelerator.to_owned());
            Followup::Asking
        }
        Plan::Redo if may_change => match run_flow(app, flow, accelerator, false) {
            Ok(Outcome::Changed) => Followup::Retry,
            Ok(_) => Followup::None,
            Err(err) => {
                tracing::warn!("could not switch the OS shortcut off again: {err}");
                Followup::None
            }
        },
        Plan::Ask | Plan::Redo => Followup::None,
    }
}

/// Settings: "Let Sevak use Cmd+Space / Super+Space". Asks first.
#[tauri::command]
pub async fn takeover_hotkey(app: AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let accelerator = state.config().general.hotkey;
        let flow = flow_for(state.display)
            .ok_or_else(|| "There is no system shortcut Sevak can take over here.".to_owned())?;
        let outcome = run_flow(&app, flow, &accelerator, true)?;
        hotkey::apply_settled(&app);
        announce(&app);
        Ok(match outcome {
            Outcome::Changed => "Done. Sevak now uses the shortcut.".to_owned(),
            Outcome::Declined => "Nothing was changed.".to_owned(),
            Outcome::Nothing => "Nothing was in the way.".to_owned(),
        })
    })
    .await
    .map_err(|err| format!("the change did not finish: {err}"))?
}

/// Settings: "Restore ..." puts back what Sevak changed.
#[tauri::command]
pub async fn restore_takeover(app: AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let macos = app.state::<AppState>().display == DisplayServer::MacOS;
        let lines = with_env(&app, |env| restore_all(env, macos))?;
        hotkey::apply_settled(&app);
        announce(&app);
        Ok(if lines.is_empty() {
            "Nothing to restore.".to_owned()
        } else {
            lines.join("\n")
        })
    })
    .await
    .map_err(|err| format!("the change did not finish: {err}"))?
}

/// Settings "Set up GNOME shortcut": before binding `hotkey` in GNOME, offers to
/// move GNOME's input-source shortcut if it uses the same key. Returns what to
/// show above the setup report (empty if there was nothing in the way).
pub fn offer_gnome_for_setup(app: &AppHandle, hotkey: &str) -> String {
    let Ok(key) = gnome::to_gnome_accelerator(hotkey) else {
        return String::new();
    };
    with_env(app, |env| {
        if gnome_changes(env, &key).is_empty() {
            return String::new();
        }
        let shown = accelerator::display(hotkey, KeyNames::Linux);
        match move_gnome(env, true, &shown, &key) {
            Ok(Outcome::Changed) => "Moved GNOME's input-source shortcut off this key (put it \
                 back with the Restore button or `sevak --restore-hotkey`).\n\n"
                .to_owned(),
            Ok(Outcome::Declined) => format!(
                "GNOME's input-source shortcut still uses {shown}; it was not changed, so \
                 GNOME may keep the key for itself.\n\n"
            ),
            Ok(Outcome::Nothing) => String::new(),
            Err(err) => format!("Could not move GNOME's input-source shortcut: {err}\n\n"),
        }
    })
}

/// Asks on the terminal. Anything but `y` or `yes` declines.
#[cfg(target_os = "linux")]
fn ask_on_terminal(message: &str) -> bool {
    print!("{message}\n\nContinue? [y/N] ");
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    if std::io::stdin().lock().read_line(&mut answer).is_err() {
        return false;
    }
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// `sevak --setup-hotkey` on GNOME: if GNOME's input-source shortcut uses
/// `hotkey`, explains it and moves it after an explicit yes typed at the
/// terminal. Without a terminal nothing is asked or changed.
#[cfg(target_os = "linux")]
pub fn cli_offer_gnome(hotkey: &str, paths: &AppPaths) {
    let Ok(key) = gnome::to_gnome_accelerator(hotkey) else {
        return;
    };
    let store = FileStore::new(paths);
    let gnome = gnome::system_gsettings();
    let confirm = |message: String| ask_on_terminal(&message);
    let env = Env {
        confirm: &confirm,
        runner: &SystemRunner,
        gnome: gnome.as_deref(),
        store: &store,
    };
    if gnome_changes(&env, &key).is_empty() {
        return;
    }
    let shown = accelerator::display(hotkey, KeyNames::Linux);
    if !std::io::stdin().is_terminal() {
        println!(
            "note: GNOME uses {shown} to switch input sources, which may keep the key from \
             Sevak. Run `sevak --setup-hotkey` in a terminal to be asked about moving it.\n"
        );
        return;
    }
    match move_gnome(&env, true, &shown, &key) {
        Ok(Outcome::Changed) => println!(
            "GNOME's input-source shortcut was moved off {shown}. \
             Put it back with `sevak --restore-hotkey`.\n"
        ),
        Ok(Outcome::Declined) => println!("GNOME's input-source shortcut was not changed.\n"),
        Ok(Outcome::Nothing) => {}
        Err(err) => eprintln!("error: could not move GNOME's input-source shortcut: {err}\n"),
    }
}

/// `sevak --restore-hotkey`: puts back whatever Sevak changed to get its key.
/// Returns the text to print.
pub fn cli_restore(paths: &AppPaths) -> Result<String, String> {
    let store = FileStore::new(paths);
    let gnome = gnome::system_gsettings();
    let confirm = |_: String| false;
    let env = Env {
        confirm: &confirm,
        runner: &SystemRunner,
        gnome: gnome.as_deref(),
        store: &store,
    };
    let lines = restore_all(&env, cfg!(target_os = "macos"))?;
    Ok(if lines.is_empty() {
        "Nothing to restore: Sevak has not changed any system shortcut.".to_owned()
    } else {
        let mut text = lines.join("\n");
        text.push_str(
            "\nSevak's own shortcut may now clash with it again; pick another key in \
             Settings, or run `sevak --setup-hotkey KEY`.",
        );
        text
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct MemoryStore(RefCell<TakeoverRecord>);

    impl Store for MemoryStore {
        fn load(&self) -> TakeoverRecord {
            self.0.borrow().clone()
        }

        fn save(&self, record: &TakeoverRecord) -> Result<(), String> {
            *self.0.borrow_mut() = record.clone();
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeRunner {
        calls: RefCell<Vec<String>>,
        fail: bool,
    }

    impl CommandRunner for FakeRunner {
        fn run(&self, program: &str, args: &[String]) -> Result<String, String> {
            self.calls
                .borrow_mut()
                .push(format!("{program} {}", args.join(" ")));
            if self.fail {
                Err("refused".to_owned())
            } else {
                Ok(String::new())
            }
        }
    }

    #[derive(Default)]
    struct FakeGnome(RefCell<BTreeMap<String, String>>);

    impl FakeGnome {
        fn stock() -> Self {
            let fake = Self::default();
            fake.0.borrow_mut().insert(
                "switch-input-source".to_owned(),
                "['<Super>space', 'XF86Keyboard']".to_owned(),
            );
            fake.0.borrow_mut().insert(
                "switch-input-source-backward".to_owned(),
                "['<Shift><Super>space', '<Shift>XF86Keyboard']".to_owned(),
            );
            fake
        }
    }

    impl Gsettings for FakeGnome {
        fn get(&self, _schema: &str, key: &str) -> sevak_platform::Result<String> {
            Ok(self.0.borrow().get(key).cloned().unwrap_or_default())
        }

        fn set(&self, _schema: &str, key: &str, value: &str) -> sevak_platform::Result<()> {
            self.0.borrow_mut().insert(key.to_owned(), value.to_owned());
            Ok(())
        }

        fn list_recursively(&self, _schema: &str) -> sevak_platform::Result<String> {
            Ok(String::new())
        }
    }

    fn yes(_: String) -> bool {
        true
    }

    fn no(_: String) -> bool {
        false
    }

    fn env<'a>(
        confirm: &'a dyn Fn(String) -> bool,
        runner: &'a FakeRunner,
        gnome: Option<&'a FakeGnome>,
        store: &'a MemoryStore,
    ) -> Env<'a> {
        Env {
            confirm,
            runner,
            gnome: gnome.map(|gnome| gnome as &dyn Gsettings),
            store,
        }
    }

    #[test]
    fn the_spotlight_plan_follows_the_shortcut_and_the_record() {
        let none = TakeoverRecord::default();
        let declined = TakeoverRecord {
            spotlight: Some(SpotlightChoice::Declined),
            ..TakeoverRecord::default()
        };
        let allowed = TakeoverRecord {
            spotlight: Some(SpotlightChoice::Disabled),
            ..TakeoverRecord::default()
        };
        assert_eq!(spotlight_plan(SpotlightShortcut::Active, &none), Plan::Ask);
        assert_eq!(
            spotlight_plan(SpotlightShortcut::Active, &declined),
            Plan::FallBack
        );
        assert_eq!(
            spotlight_plan(SpotlightShortcut::Active, &allowed),
            Plan::Redo
        );
        // Spotlight is not the one holding the key.
        for shortcut in [SpotlightShortcut::Disabled, SpotlightShortcut::Remapped] {
            for record in [&none, &declined, &allowed] {
                assert_eq!(spotlight_plan(shortcut, record), Plan::Nothing);
            }
        }
    }

    #[test]
    fn the_gnome_plan_asks_once() {
        let none = TakeoverRecord::default();
        let declined = TakeoverRecord {
            gnome: Some(GnomeRecord {
                declined: true,
                moves: Vec::new(),
            }),
            ..TakeoverRecord::default()
        };
        let moved = TakeoverRecord {
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
        };
        assert_eq!(gnome_plan(false, &none), Plan::Nothing);
        assert_eq!(gnome_plan(true, &none), Plan::Ask);
        assert_eq!(gnome_plan(true, &declined), Plan::Nothing);
        assert_eq!(gnome_plan(true, &moved), Plan::Redo);
    }

    #[test]
    fn yes_turns_spotlights_shortcut_off_and_is_remembered() {
        let (runner, store) = (FakeRunner::default(), MemoryStore::default());
        let outcome = take_spotlight(
            &env(&yes, &runner, None, &store),
            true,
            "Cmd+Space",
            "Option+Space",
        );
        assert_eq!(outcome, Ok(Outcome::Changed));
        assert_eq!(store.load().spotlight, Some(SpotlightChoice::Disabled));
        let calls = runner.calls.borrow();
        assert!(calls[0].starts_with("defaults write com.apple.symbolichotkeys"));
        assert!(calls[0].contains("<false/>"));
        assert!(calls[1].ends_with("activateSettings -u"));
    }

    #[test]
    fn no_changes_nothing_and_is_remembered() {
        let (runner, store) = (FakeRunner::default(), MemoryStore::default());
        let outcome = take_spotlight(
            &env(&no, &runner, None, &store),
            true,
            "Cmd+Space",
            "Option+Space",
        );
        assert_eq!(outcome, Ok(Outcome::Declined));
        assert_eq!(store.load().spotlight, Some(SpotlightChoice::Declined));
        assert!(runner.calls.borrow().is_empty());
    }

    #[test]
    fn a_remembered_yes_is_applied_again_without_asking() {
        let (runner, store) = (FakeRunner::default(), MemoryStore::default());
        let never = |_: String| -> bool { panic!("must not ask") };
        let outcome = take_spotlight(
            &env(&never, &runner, None, &store),
            false,
            "Cmd+Space",
            "Option+Space",
        );
        assert_eq!(outcome, Ok(Outcome::Changed));
    }

    #[test]
    fn a_failed_command_is_an_error_and_records_no_permission() {
        let runner = FakeRunner {
            fail: true,
            ..FakeRunner::default()
        };
        let store = MemoryStore::default();
        let outcome = take_spotlight(
            &env(&yes, &runner, None, &store),
            true,
            "Cmd+Space",
            "Option+Space",
        );
        assert!(outcome.is_err());
        assert_eq!(store.load().spotlight, None);
    }

    #[test]
    fn restoring_spotlight_turns_it_on_and_never_asks_again() {
        let (runner, store) = (FakeRunner::default(), MemoryStore::default());
        store
            .save(&TakeoverRecord {
                spotlight: Some(SpotlightChoice::Disabled),
                ..TakeoverRecord::default()
            })
            .unwrap();
        restore_spotlight(&env(&yes, &runner, None, &store)).unwrap();
        assert!(runner.calls.borrow()[0].contains("<true/>"));
        assert_eq!(store.load().spotlight, Some(SpotlightChoice::Declined));
        assert!(!store.load().can_restore());
    }

    #[test]
    fn the_questions_say_what_will_change_and_how_to_undo_it() {
        let text = spotlight_question("Cmd+Space", "Option+Space");
        assert!(text.starts_with("Cmd+Space is used by Spotlight. Let Sevak use it?"));
        assert!(text.contains("Restore Spotlight's shortcut"));
        assert!(text.contains("Option+Space"));
        let change = InputSourceMove {
            schema: "s".into(),
            key: "switch-input-source".into(),
            previous: vec!["<Super>space".into()],
            updated: vec!["<Control><Super>space".into()],
        };
        let text = gnome_question("Super+Space", &[change]);
        assert!(text.contains("switch-input-source: ['<Super>space'] -> ['<Control><Super>space']"));
        assert!(text.contains("sevak --restore-hotkey"));
    }

    #[test]
    fn gnome_yes_moves_the_shortcuts_and_keeps_the_old_values() {
        let (runner, store, gnome) = (
            FakeRunner::default(),
            MemoryStore::default(),
            FakeGnome::stock(),
        );
        let outcome = move_gnome(
            &env(&yes, &runner, Some(&gnome), &store),
            true,
            "Super+Space",
            "<Super>space",
        );
        assert_eq!(outcome, Ok(Outcome::Changed));
        assert_eq!(
            gnome.0.borrow()["switch-input-source"],
            "['<Control><Super>space', 'XF86Keyboard']"
        );
        let record = store.load();
        assert!(record.gnome_moved());
        assert_eq!(record.gnome.as_ref().unwrap().moves.len(), 2);
        assert_eq!(
            record.gnome.unwrap().moves[0].previous,
            ["<Super>space", "XF86Keyboard"]
        );
    }

    #[test]
    fn gnome_no_changes_nothing_and_is_remembered() {
        let (runner, store, gnome) = (
            FakeRunner::default(),
            MemoryStore::default(),
            FakeGnome::stock(),
        );
        let before = gnome.0.borrow().clone();
        let outcome = move_gnome(
            &env(&no, &runner, Some(&gnome), &store),
            true,
            "Super+Space",
            "<Super>space",
        );
        assert_eq!(outcome, Ok(Outcome::Declined));
        assert_eq!(*gnome.0.borrow(), before);
        assert!(store.load().gnome_declined());
        assert!(!store.load().can_restore());
    }

    #[test]
    fn gnome_without_a_conflict_is_left_alone() {
        let (runner, store, gnome) = (
            FakeRunner::default(),
            MemoryStore::default(),
            FakeGnome::stock(),
        );
        let never = |_: String| -> bool { panic!("must not ask") };
        let outcome = move_gnome(
            &env(&never, &runner, Some(&gnome), &store),
            true,
            "Alt+Space",
            "<Alt>space",
        );
        assert_eq!(outcome, Ok(Outcome::Nothing));
        assert_eq!(store.load(), TakeoverRecord::default());
        // And with no GNOME at all.
        let outcome = move_gnome(
            &env(&never, &runner, None, &store),
            true,
            "Super+Space",
            "<Super>space",
        );
        assert_eq!(outcome, Ok(Outcome::Nothing));
    }

    #[test]
    fn restoring_gnome_writes_the_old_values_back() {
        let (runner, store, gnome) = (
            FakeRunner::default(),
            MemoryStore::default(),
            FakeGnome::stock(),
        );
        let original = gnome.0.borrow().clone();
        let environment = env(&yes, &runner, Some(&gnome), &store);
        move_gnome(&environment, true, "Super+Space", "<Super>space").unwrap();
        let lines = restore_gnome(&environment).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(*gnome.0.borrow(), original);
        let record = store.load();
        assert!(!record.gnome_moved());
        // The user put it back: do not ask again.
        assert!(record.gnome_declined());
        // Nothing left to restore.
        assert!(restore_gnome(&environment).unwrap().is_empty());
    }

    #[test]
    fn moving_twice_keeps_the_original_values() {
        let (runner, store, gnome) = (
            FakeRunner::default(),
            MemoryStore::default(),
            FakeGnome::stock(),
        );
        let original = gnome.0.borrow().clone();
        let environment = env(&yes, &runner, Some(&gnome), &store);
        move_gnome(&environment, true, "Super+Space", "<Super>space").unwrap();
        // Something put Super+Space back on the switcher; Sevak moves it again quietly.
        gnome.0.borrow_mut().insert(
            "switch-input-source".to_owned(),
            "['<Super>space']".to_owned(),
        );
        move_gnome(&environment, false, "Super+Space", "<Super>space").unwrap();
        restore_gnome(&environment).unwrap();
        assert_eq!(
            gnome.0.borrow()["switch-input-source"],
            original["switch-input-source"]
        );
    }

    #[test]
    fn restore_all_undoes_what_the_os_has() {
        let (runner, store, gnome) = (
            FakeRunner::default(),
            MemoryStore::default(),
            FakeGnome::stock(),
        );
        let environment = env(&yes, &runner, Some(&gnome), &store);
        assert!(restore_all(&environment, true).unwrap().is_empty());
        take_spotlight(&environment, true, "Cmd+Space", "Option+Space").unwrap();
        let lines = restore_all(&environment, true).unwrap();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("Spotlight"));
        assert!(!store.load().spotlight_disabled());
    }

    #[test]
    fn the_status_names_the_changed_setting() {
        let mut record = TakeoverRecord::default();
        assert_eq!(changed_setting(&record, true), None);
        record.spotlight = Some(SpotlightChoice::Disabled);
        assert_eq!(
            changed_setting(&record, true),
            Some(ChangedSetting::Spotlight)
        );
        assert_eq!(changed_setting(&record, false), None);
        record.spotlight = None;
        record.gnome = Some(GnomeRecord {
            declined: false,
            moves: vec![MovedKey {
                schema: "s".into(),
                key: "k".into(),
                previous: vec![],
                updated: vec![],
            }],
        });
        assert_eq!(
            changed_setting(&record, false),
            Some(ChangedSetting::GnomeInputSources)
        );
    }

    #[test]
    fn the_record_round_trips_through_a_file_and_survives_garbage() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_roots(dir.path().join("config"), dir.path().join("data"));
        let store = FileStore::new(&paths);
        assert_eq!(store.load(), TakeoverRecord::default());
        let record = TakeoverRecord {
            spotlight: Some(SpotlightChoice::Disabled),
            gnome: Some(GnomeRecord {
                declined: false,
                moves: vec![MovedKey {
                    schema: "s".into(),
                    key: "k".into(),
                    previous: vec!["a".into()],
                    updated: vec!["b".into()],
                }],
            }),
        };
        store.save(&record).unwrap();
        assert_eq!(store.load(), record);
        std::fs::write(&store.path, "not json").unwrap();
        assert_eq!(store.load(), TakeoverRecord::default());
        // Unknown and missing fields are fine.
        std::fs::write(&store.path, r#"{"spotlight":"declined","later":1}"#).unwrap();
        assert_eq!(store.load().spotlight, Some(SpotlightChoice::Declined));
    }
}
