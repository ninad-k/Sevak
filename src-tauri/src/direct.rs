//! Requests that go beyond "show an empty search bar": open with text already
//! typed (`--query`, `[[hotkey]] query`) and run a result without showing
//! anything (`--run`, `[[hotkey]] run`).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::state::AppState;
use crate::window;

/// How long `run` waits for the first indexing pass: right after startup the
/// result it names may not be indexed yet.
const INDEX_WAIT: Duration = Duration::from_secs(15);

/// What the UI is told when the launcher is shown (`sevak:show`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ShowPayload {
    /// Text to put in the search field, caret at the end.
    pub query: Option<String>,
    /// A message to show instead of results.
    pub error: Option<String>,
}

impl ShowPayload {
    fn is_empty(&self) -> bool {
        self.query.is_none() && self.error.is_none()
    }
}

/// Bridges the gap before the UI has loaded: a `sevak:show` emitted then is
/// lost, so a payload that matters is kept until the UI asks for it.
#[derive(Default)]
struct Handshake {
    ui_ready: bool,
    pending: Option<ShowPayload>,
}

impl Handshake {
    /// The payload to emit now, or `None` if it was kept for later.
    fn deliver(&mut self, payload: ShowPayload) -> Option<ShowPayload> {
        if self.ui_ready || payload.is_empty() {
            Some(payload)
        } else {
            self.pending = Some(payload);
            None
        }
    }

    /// The UI is listening: hands over what it missed.
    fn take(&mut self) -> Option<ShowPayload> {
        self.ui_ready = true;
        self.pending.take()
    }
}

static HANDSHAKE: Mutex<Handshake> = Mutex::new(Handshake {
    ui_ready: false,
    pending: None,
});

fn handshake() -> std::sync::MutexGuard<'static, Handshake> {
    HANDSHAKE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// See [`Handshake::deliver`].
pub fn deliver(payload: ShowPayload) -> Option<ShowPayload> {
    handshake().deliver(payload)
}

/// Called by the launcher UI once it listens for `sevak:show`.
#[tauri::command]
pub fn take_pending_show() -> Option<ShowPayload> {
    handshake().take()
}

/// Shows the launcher with `query` typed in.
pub fn open_with_query(app: &AppHandle, query: String) {
    window::show_with(
        app,
        ShowPayload {
            query: Some(query),
            error: None,
        },
    );
}

/// Runs result `id` in the background. If it cannot be found or fails, the
/// launcher opens with the reason instead, so the key never seems dead.
pub fn run_result(app: &AppHandle, id: String) {
    // Now, while the user's app still has focus: a snippet or clipboard entry
    // bound to a key pastes into it.
    if let Some(state) = app.try_state::<AppState>() {
        state.search.platform.remember_foreground_app();
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || match try_run(&app, &id) {
        Ok(()) => {
            tracing::info!(id, "ran result directly");
            // A launcher left open would only be in the way of what just started.
            let visible = app
                .get_webview_window(window::MAIN_LABEL)
                .is_some_and(|w| w.is_visible().unwrap_or(false));
            if visible {
                window::hide(&app);
            }
        }
        Err(reason) => {
            tracing::warn!(id, "could not run result directly: {reason}");
            window::show_with(
                &app,
                ShowPayload {
                    query: None,
                    error: Some(reason),
                },
            );
        }
    });
}

fn try_run(app: &AppHandle, id: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    let search = &state.search;

    let started = Instant::now();
    while search.is_indexing() && started.elapsed() < INDEX_WAIT {
        std::thread::sleep(Duration::from_millis(100));
    }

    let engine = search.engine();
    let item = engine.resolve(id).ok_or_else(|| {
        format!("Cannot run \"{id}\": no such result (is the plugin on and the item installed?)")
    })?;
    // A key bound to a destructive command (shut down) still asks first.
    if let Some(question) = engine.confirmation(&item) {
        if !crate::commands::confirmed(app, &item.title, question) {
            tracing::info!(id, "run: declined at the confirmation");
            return Ok(());
        }
    }
    // No query was typed, so the usage statistics get none either.
    engine.execute(&item, "").map_err(|err| err.to_string())?;
    search.saver.poke();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(text: &str) -> ShowPayload {
        ShowPayload {
            query: Some(text.to_owned()),
            error: None,
        }
    }

    #[test]
    fn a_query_before_the_ui_loaded_waits_for_it() {
        let mut handshake = Handshake::default();
        assert_eq!(handshake.deliver(query("> ")), None);
        assert_eq!(handshake.take(), Some(query("> ")));
        // Handed over once.
        assert_eq!(handshake.take(), None);
    }

    #[test]
    fn the_newest_waiting_payload_wins() {
        let mut handshake = Handshake::default();
        handshake.deliver(query("a"));
        handshake.deliver(query("b"));
        assert_eq!(handshake.take(), Some(query("b")));
    }

    #[test]
    fn after_the_ui_loaded_everything_is_emitted_directly() {
        let mut handshake = Handshake::default();
        assert_eq!(handshake.take(), None);
        assert_eq!(handshake.deliver(query("x")), Some(query("x")));
        assert_eq!(handshake.take(), None);
    }

    #[test]
    fn a_plain_show_is_always_emitted() {
        let mut handshake = Handshake::default();
        assert_eq!(
            handshake.deliver(ShowPayload::default()),
            Some(ShowPayload::default())
        );
        assert_eq!(handshake.take(), None);
    }
}
