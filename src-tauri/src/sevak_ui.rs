//! Sevak UI in the shell: opening a `show_ui` node's UI in the launcher panel
//! or a window of its own, and carrying its events back to the workflow.
//!
//! The workflow's thread blocks in [`show`] until the UI sends something that
//! ends it. Everything else the UI asks for (running one of the node's
//! scripts) is answered on that same thread, through the [`UiHost`] the
//! workflow engine hands in, so a script runs exactly as an approved
//! `run_script` node would and the UI never reaches the engine any other way.
//!
//! The UI window gets no plugin permissions at all (`capabilities/sevak-ui.json`
//! is empty): it only calls the three commands below, and loads nothing but
//! the app's own page.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sevak_plugins::workflow::ui::{self, UiTarget};
use sevak_plugins::workflow::{UiHost, UiOutcome, UiSession, UiUpdate};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, Window, WindowEvent};
use tauri_plugin_notification::NotificationExt;

use crate::direct::ShowPayload;
use crate::state::AppState;
use crate::window;

/// Label prefix of a UI window; the label is `sevak-ui-<id>`.
pub const LABEL_PREFIX: &str = "sevak-ui-";
/// How often a waiting workflow checks whether Sevak is shutting down.
const POLL: Duration = Duration::from_millis(250);
/// A script gets its own timeout from the engine; this is the longest the
/// window waits for the answer.
const SCRIPT_WAIT: Duration = Duration::from_secs(30);
const DEFAULT_SIZE: (u32, u32) = (440, 340);

/// What the renderer needs to draw a UI.
#[derive(Clone, PartialEq, Serialize)]
pub struct UiPayload {
    pub id: String,
    pub target: UiTarget,
    pub workflow: String,
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub definition: ui::UiDefinition,
    pub images: std::collections::BTreeMap<String, String>,
    pub vars: std::collections::BTreeMap<String, String>,
    pub query: String,
    pub scripts: Vec<String>,
}

// By hand: the definition and the variables hold what the user typed.
impl std::fmt::Debug for UiPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiPayload")
            .field("id", &self.id)
            .field("target", &self.target)
            .finish()
    }
}

/// What the renderer tells Sevak.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UiEvent {
    /// Continue the workflow from `port`.
    Output {
        port: String,
        arg: Option<String>,
        state: Value,
    },
    /// The UI was dismissed or closed itself.
    Close {
        state: Option<Value>,
    },
    /// Run one of the node's scripts.
    Run {
        script: String,
        input: Value,
    },
    Notify {
        text: String,
    },
    Copy {
        text: String,
    },
}

enum Msg {
    Done(UiOutcome),
    Run {
        script: String,
        input: Value,
        reply: Sender<Result<UiUpdate, String>>,
    },
}

struct Live {
    tx: Sender<Msg>,
    payload: UiPayload,
    query: String,
}

static NEXT: AtomicU64 = AtomicU64::new(1);
static LIVE: Mutex<Option<HashMap<String, Live>>> = Mutex::new(None);

fn with_live<T>(work: impl FnOnce(&mut HashMap<String, Live>) -> T) -> T {
    let mut guard = LIVE.lock().unwrap_or_else(|p| p.into_inner());
    work(guard.get_or_insert_with(HashMap::new))
}

fn payload_of(id: &str, session: &UiSession) -> UiPayload {
    let definition = &session.definition;
    let title = if definition.title.trim().is_empty() {
        session.workflow.clone()
    } else {
        definition.title.clone()
    };
    UiPayload {
        id: id.to_owned(),
        target: session.target,
        workflow: session.workflow.clone(),
        title,
        width: definition.width.unwrap_or(DEFAULT_SIZE.0),
        height: definition.height.unwrap_or(DEFAULT_SIZE.1),
        definition: definition.clone(),
        images: session.images.clone(),
        vars: session.vars.clone(),
        query: session.query.clone(),
        scripts: session.scripts.iter().cloned().collect(),
    }
}

/// Opens `session` and blocks until it ends. `None` when it cannot be shown.
pub fn show(app: &AppHandle, session: &UiSession, host: &dyn UiHost) -> Option<UiOutcome> {
    let id = NEXT.fetch_add(1, Ordering::SeqCst).to_string();
    let payload = payload_of(&id, session);
    let (tx, rx): (Sender<Msg>, Receiver<Msg>) = mpsc::channel();
    with_live(|live| {
        live.insert(
            id.clone(),
            Live {
                tx,
                payload: payload.clone(),
                query: session.query.clone(),
            },
        );
    });
    let shown = match session.target {
        UiTarget::Launcher => {
            window::show_with(
                app,
                ShowPayload {
                    ui: Some(payload),
                    ..ShowPayload::default()
                },
            );
            true
        }
        UiTarget::Window => open_window(app, &id, &payload, session),
    };
    if !shown {
        with_live(|live| live.remove(&id));
        return None;
    }
    let outcome = wait(&rx, host, session);
    with_live(|live| live.remove(&id));
    if session.target == UiTarget::Window {
        if let Some(window) = app.get_webview_window(&format!("{LABEL_PREFIX}{id}")) {
            let _ = window.destroy();
        }
    }
    Some(outcome)
}

fn closed(session: &UiSession) -> UiOutcome {
    UiOutcome {
        port: ui::CLOSED.to_owned(),
        arg: Some(session.query.clone()),
        state: Value::Null,
    }
}

fn wait(rx: &Receiver<Msg>, host: &dyn UiHost, session: &UiSession) -> UiOutcome {
    loop {
        match rx.recv_timeout(POLL) {
            Ok(Msg::Done(outcome)) => return outcome,
            Ok(Msg::Run {
                script,
                input,
                reply,
            }) => {
                let _ = reply.send(host.run_script(&script, &input));
            }
            Err(RecvTimeoutError::Timeout) => {
                if host.cancelled() {
                    return closed(session);
                }
            }
            Err(RecvTimeoutError::Disconnected) => return closed(session),
        }
    }
}

fn open_window(app: &AppHandle, id: &str, payload: &UiPayload, session: &UiSession) -> bool {
    window::unhide_app(app);
    let label = format!("{LABEL_PREFIX}{id}");
    let mut builder = WebviewWindowBuilder::new(
        app,
        &label,
        WebviewUrl::App(format!("index.html#{LABEL_PREFIX}{id}").into()),
    )
    .title(payload.title.clone())
    .inner_size(f64::from(payload.width), f64::from(payload.height))
    .min_inner_size(200.0, 100.0)
    .resizable(true)
    .center()
    .decorations(session.titlebar)
    .always_on_top(session.always_on_top)
    .focused(true);
    // Must match the launcher's: WebView2 refuses a second webview on the same
    // data folder with different arguments.
    if let Some(args) = crate::webview_args::from_env() {
        builder = builder.additional_browser_args(&args);
    }
    match builder.build() {
        Ok(_) => true,
        Err(err) => {
            tracing::error!("could not open a workflow UI window: {err}");
            false
        }
    }
}

/// A UI window closed (by the user, or by us): the workflow carries on from
/// `closed`.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if !matches!(event, WindowEvent::Destroyed) {
        return;
    }
    if let Some(id) = window.label().strip_prefix(LABEL_PREFIX) {
        end(id, None);
    }
}

/// Ends the session `id` as dismissed, if it is still open.
fn end(id: &str, state: Option<Value>) {
    let Some((tx, query)) = with_live(|live| live.get(id).map(|l| (l.tx.clone(), l.query.clone())))
    else {
        return;
    };
    let _ = tx.send(Msg::Done(UiOutcome {
        port: ui::CLOSED.to_owned(),
        arg: Some(query),
        state: state.unwrap_or(Value::Null),
    }));
}

/// The UI window asks what it is showing.
#[tauri::command]
pub fn sevak_ui_session(id: String) -> Option<UiPayload> {
    with_live(|live| live.get(&id).map(|l| l.payload.clone()))
}

/// Something happened in an open UI.
#[tauri::command]
pub async fn sevak_ui_event(
    app: AppHandle,
    id: String,
    event: UiEvent,
) -> Result<Option<UiUpdate>, String> {
    match event {
        UiEvent::Output { port, arg, state } => {
            let tx = with_live(|live| live.get(&id).map(|l| l.tx.clone()));
            if let Some(tx) = tx {
                let _ = tx.send(Msg::Done(UiOutcome { port, arg, state }));
            }
            Ok(None)
        }
        UiEvent::Close { state } => {
            end(&id, state);
            Ok(None)
        }
        UiEvent::Run { script, input } => {
            let tx = with_live(|live| live.get(&id).map(|l| l.tx.clone()))
                .ok_or("this UI is no longer open")?;
            let (reply, answer) = mpsc::channel();
            tx.send(Msg::Run {
                script,
                input,
                reply,
            })
            .map_err(|_| "this UI is no longer open".to_owned())?;
            tauri::async_runtime::spawn_blocking(move || {
                answer
                    .recv_timeout(SCRIPT_WAIT)
                    .map_err(|_| "the script did not answer in time".to_owned())?
            })
            .await
            .map_err(|err| format!("the script did not finish: {err}"))?
            .map(Some)
        }
        UiEvent::Notify { text } => {
            let known = with_live(|live| live.get(&id).map(|l| l.payload.workflow.clone()));
            if let Some(workflow) = known {
                if let Err(err) = app
                    .notification()
                    .builder()
                    .title(workflow)
                    .body(text)
                    .show()
                {
                    tracing::warn!("could not show a notification: {err}");
                }
            }
            Ok(None)
        }
        UiEvent::Copy { text } => {
            if with_live(|live| live.contains_key(&id)) {
                sevak_platform::clipboard::set_text(&text)
                    .map_err(|err| format!("could not copy: {err}"))?;
            }
            Ok(None)
        }
    }
}

/// What the builder's Validate button reports.
#[derive(Serialize)]
pub struct UiCheck {
    pub ports: Vec<String>,
    pub components: usize,
    pub warnings: Vec<String>,
}

/// Parses and validates a definition the way a run would. `folder` is the
/// workflow's folder (for `file` and image paths); nothing is saved.
#[tauri::command]
pub async fn validate_ui_definition(
    app: AppHandle,
    definition: Option<String>,
    file: Option<String>,
    scripts: Vec<String>,
    folder: Option<String>,
) -> Result<UiCheck, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = {
            let state = app.state::<AppState>();
            let root = state.search.workflows.workflows_dir().to_path_buf();
            match folder.as_deref() {
                Some(name) if sevak_plugins::workflow::model::valid_folder_name(name) => {
                    root.join(name)
                }
                _ => root.join(".sevak-no-workflow"),
            }
        };
        let ids = scripts.into_iter().collect();
        let loaded = ui::load(
            &ui::Source {
                inline: definition.as_deref(),
                file: file.as_deref(),
                dir: &dir,
            },
            &ids,
        )?;
        let (_, warnings) = ui::load_images(&loaded.root, &dir);
        Ok(UiCheck {
            ports: ui::definition_ports(&loaded),
            components: ui::count_components(&loaded.root),
            warnings,
        })
    })
    .await
    .map_err(|err| format!("the check did not finish: {err}"))?
}
