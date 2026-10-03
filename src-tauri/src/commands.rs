//! Tauri commands invoked by the frontend.

use std::time::Instant;

use serde::Serialize;
use sevak_core::{Action, Modifier, ResultItem};
use tauri::{AppHandle, LogicalSize, Manager, State, WebviewWindow};

use crate::icons::IconDto;
use crate::state::{AppState, Status};
use crate::window;

const MIN_HEIGHT: f64 = 40.0;
const MAX_HEIGHT: f64 = 900.0;

#[tauri::command]
pub fn hide_window(app: AppHandle) {
    window::hide(&app);
}

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> Status {
    state.status()
}

/// The frontend reports its rendered content height; the window follows it.
#[tauri::command]
pub fn set_content_height(window: WebviewWindow, state: State<'_, AppState>, height: f64) {
    if !height.is_finite() {
        tracing::debug!("set_content_height ignored: {height}");
        return;
    }
    let width = f64::from(state.config().window.width);
    let size = LogicalSize::new(width, height.clamp(MIN_HEIGHT, MAX_HEIGHT));
    if let Err(err) = window.set_size(size) {
        tracing::warn!("set_content_height: set_size failed: {err}");
    }
}

/// Large Type: `true` stretches the window over the screen, `false` restores
/// it. An error means the window could not be stretched; the UI then shows the
/// text inside the launcher instead.
#[tauri::command]
pub fn set_large_type(app: AppHandle, on: bool) -> Result<(), String> {
    if on {
        window::enter_large_type(&app)
    } else {
        window::leave_large_type(&app);
        Ok(())
    }
}

/// One row of search results as the UI sees it.
#[derive(Debug, Serialize)]
pub struct ResultDto {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub icon: Option<IconDto>,
    pub plugin_id: String,
    /// What Tab turns the input into, when the plugin offers a completion.
    pub autocomplete: Option<String>,
    /// `launch`, `open_path`, `open_url`, `copy_text`, `reveal_path`,
    /// `run_as_admin` or `custom`.
    pub action: &'static str,
    /// Secondary actions, in the order `execute`'s `action` index refers to.
    pub secondary: Vec<SecondaryDto>,
    /// What Ctrl+C copies for this row (path, URL or value), if anything.
    pub copy_text: Option<String>,
}

/// A secondary action as the UI sees it; its payload stays in the shell.
#[derive(Debug, Serialize)]
pub struct SecondaryDto {
    pub label: String,
    pub modifier: Option<Modifier>,
    pub kind: &'static str,
}

fn action_kind(action: &Action) -> &'static str {
    match action {
        Action::Launch { .. } => "launch",
        Action::OpenPath { .. } => "open_path",
        Action::OpenUrl { .. } => "open_url",
        Action::CopyText { .. } => "copy_text",
        Action::Custom { .. } => "custom",
        Action::RevealPath { .. } => "reveal_path",
        Action::RunAsAdmin { .. } => "run_as_admin",
    }
}

fn to_dtos(icons: Vec<Option<IconDto>>, items: &[ResultItem]) -> Vec<ResultDto> {
    items
        .iter()
        .zip(icons)
        .map(|(item, icon)| ResultDto {
            id: item.id.clone(),
            title: item.title.clone(),
            subtitle: item.subtitle.clone(),
            icon,
            plugin_id: item.plugin_id.clone(),
            autocomplete: item.autocomplete.clone(),
            action: action_kind(&item.action),
            secondary: item
                .secondary
                .iter()
                .map(|s| SecondaryDto {
                    label: s.label.clone(),
                    modifier: s.modifier,
                    kind: action_kind(&s.action),
                })
                .collect(),
            copy_text: item.copy_text(),
        })
        .collect()
}

/// A search's results, numbered so `execute` can name the set the UI shows.
#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub ticket: u64,
    pub results: Vec<ResultDto>,
}

/// Queries the engine. Async so it never runs on the main thread; a query is
/// in-memory work (the engine logs a warning when it exceeds 16 ms).
#[tauri::command]
pub async fn search(app: AppHandle, query: String) -> SearchResponse {
    let started = Instant::now();
    let state = app.state::<AppState>();
    let search = &state.search;
    let ticket = search.next_ticket();

    let items = search.engine().query(&query);
    let dtos = to_dtos(search.icons.describe(&items), &items);
    let count = items.len();
    search.store_results(ticket, query.clone(), items);

    tracing::debug!(
        query_chars = query.chars().count(),
        results = count,
        elapsed_us = started.elapsed().as_micros() as u64,
        "search"
    );
    SearchResponse {
        ticket,
        results: dtos,
    }
}

/// Executed queries, most recent first, for Up/Down recall on an empty input
/// (empty when `[search] query_history` is off).
#[tauri::command]
pub fn query_history(state: State<'_, AppState>) -> Vec<String> {
    state.search.engine().history()
}

/// Executes result `id` of search `ticket`, the result set the UI is showing.
/// `action` is the index of one of the result's secondary actions; without it
/// the primary action runs.
///
/// Actions that hand over to another program (launch, open a path or URL) hide
/// the window *first*: the platform call can take a second (a slow `.lnk`), and
/// the user's intent is already clear. If it then fails the window comes back
/// as it was, with the error. Copying is instant and keeps the old order.
#[tauri::command]
pub async fn execute(
    app: AppHandle,
    id: String,
    ticket: u64,
    action: Option<usize>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || run_execute(&app, &id, ticket, action))
        .await
        .map_err(|err| format!("the action did not finish: {err}"))?
}

/// Copies the most useful text of result `id` of search `ticket` (Ctrl+C):
/// its path, URL or value. Like Enter on a calculator result, it hides the
/// window afterwards. Not a launch, so usage statistics are untouched.
#[tauri::command]
pub async fn copy_result(app: AppHandle, id: String, ticket: u64) -> Result<(), String> {
    let state = app.state::<AppState>();
    let Some((item, _query)) = state.search.result(ticket, &id) else {
        return Err("result expired".to_owned());
    };
    let Some(text) = item.copy_text() else {
        return Err("nothing to copy for this result".to_owned());
    };
    sevak_platform::clipboard::set_text(&text).map_err(|err| err.to_string())?;
    window::hide(&app);
    Ok(())
}

/// Whether the action leaves Sevak for another program.
fn hands_over(action: &Action) -> bool {
    matches!(
        action,
        Action::Launch { .. }
            | Action::OpenPath { .. }
            | Action::OpenUrl { .. }
            | Action::RevealPath { .. }
            | Action::RunAsAdmin { .. }
    )
}

fn run_execute(
    app: &AppHandle,
    id: &str,
    ticket: u64,
    choice: Option<usize>,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let search = &state.search;
    let Some((item, query)) = search.result(ticket, id) else {
        tracing::warn!(id, "execute: result expired");
        return Err("result expired".to_owned());
    };

    let action = match choice {
        None => &item.action,
        Some(index) => match item.secondary.get(index) {
            Some(secondary) => &secondary.action,
            None => {
                tracing::warn!(id, index, "execute: no such action");
                return Err("that action is not available".to_owned());
            }
        },
    };
    let optimistic = hands_over(action);
    if optimistic {
        window::hide_silently(app);
    }

    let started = Instant::now();
    let outcome = match choice {
        None => search.engine().execute(&item, &query),
        Some(index) => search.engine().execute_secondary(&item, index, &query),
    };
    match outcome {
        Ok(()) => {
            tracing::info!(
                id,
                plugin = item.plugin_id,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "executed result"
            );
            search.saver.poke();
            if optimistic {
                window::announce_hidden(app);
            } else {
                window::hide(app);
            }
            Ok(())
        }
        Err(err) => {
            tracing::warn!(id, plugin = item.plugin_id, "execute failed: {err}");
            if optimistic {
                window::reveal(app);
            }
            Err(err.to_string())
        }
    }
}
