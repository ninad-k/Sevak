//! Snippet expansion as you type: starts and stops the keyboard watcher to match
//! `[snippets] auto_expand` (see `sevak_plugins::snippet_expansion` for what it
//! does, and keeps, while it runs).
//!
//! The watcher exists only while the setting is on. [`apply`] runs at startup
//! and after every config reload, replacing the running watcher so new snippets
//! and settings take effect; turning the setting off drops it, which releases the
//! OS keyboard hook.

use std::sync::Mutex;

use serde::Serialize;
use sevak_plugins::snippet_expansion::{ExpansionState, SnippetExpansion};
use tauri::{AppHandle, Manager};

use crate::state::{lock, AppState};

/// The running watcher, if any.
static SERVICE: Mutex<Option<SnippetExpansion>> = Mutex::new(None);
/// How the last [`apply`] went.
static STATE: Mutex<ExpansionState> = Mutex::new(ExpansionState::Off);

/// Expansion as the settings window shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExpansionStatus {
    /// Sevak is watching the keyboard for snippet keywords right now.
    pub active: bool,
    /// Why it is not, when it was asked to (a missing permission, Wayland).
    pub problem: Option<String>,
}

pub fn status() -> ExpansionStatus {
    let state = lock(&STATE);
    ExpansionStatus {
        active: matches!(*state, ExpansionState::Running { .. }),
        problem: state.problem().map(str::to_owned),
    }
}

/// Starts, restarts or stops the watcher for the current config.
pub fn apply(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let config = state.config();
    let platform = state.search.platform.clone();

    // Stop the old watcher first: only one keyboard hook can exist at a time.
    drop(lock(&SERVICE).take());

    let (service, outcome) = SnippetExpansion::start(&config, platform);
    if let Some(problem) = outcome.problem() {
        tracing::warn!("snippet expansion: {problem}");
    }
    *lock(&SERVICE) = service;
    *lock(&STATE) = outcome;
}

/// Stops the watcher (the app is quitting).
pub fn stop() {
    drop(lock(&SERVICE).take());
    *lock(&STATE) = ExpansionState::Off;
}
