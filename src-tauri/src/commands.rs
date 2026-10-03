//! Tauri commands invoked by the frontend.

use std::time::Instant;

use serde::Serialize;
use sevak_core::{Action, ResultItem};
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
    /// `launch`, `open_path`, `open_url`, `copy_text` or `custom`.
    pub action: &'static str,
}

fn action_kind(action: &Action) -> &'static str {
    match action {
        Action::Launch { .. } => "launch",
        Action::OpenPath { .. } => "open_path",
        Action::OpenUrl { .. } => "open_url",
        Action::CopyText { .. } => "copy_text",
        Action::Custom { .. } => "custom",
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
///
/// Actions that hand over to another program (launch, open a path or URL) hide
/// the window *first*: the platform call can take a second (a slow `.lnk`), and
/// the user's intent is already clear. If it then fails the window comes back
/// as it was, with the error. Copying is instant and keeps the old order.
#[tauri::command]
pub async fn execute(app: AppHandle, id: String, ticket: u64) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || run_execute(&app, &id, ticket))
        .await
        .map_err(|err| format!("the action did not finish: {err}"))?
}

/// Whether the action leaves Sevak for another program.
fn hands_over(action: &Action) -> bool {
    matches!(
        action,
        Action::Launch { .. } | Action::OpenPath { .. } | Action::OpenUrl { .. }
    )
}

fn run_execute(app: &AppHandle, id: &str, ticket: u64) -> Result<(), String> {
    let state = app.state::<AppState>();
    let search = &state.search;
    let Some((item, query)) = search.result(ticket, id) else {
        tracing::warn!(id, "execute: result expired");
        return Err("result expired".to_owned());
    };

    let optimistic = hands_over(&item.action);
    if optimistic {
        window::hide_silently(app);
    }

    let started = Instant::now();
    match search.engine().execute(&item, &query) {
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
