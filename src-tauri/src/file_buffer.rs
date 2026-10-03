//! The file buffer, as the launcher uses it: files and folders collected from
//! the file results (Alt+Up / Alt+Down), then acted on together.
//!
//! The collection lives here, not in the page: the page only ever names a
//! result it was shown (a search ticket and an id), and this module takes the
//! path from the shell's own copy of that result. The disk work is
//! `sevak_plugins::file_buffer`; this module adds the window's side of it: the
//! confirmation, hiding for the actions that hand over to another program, and
//! progress events for the slow ones.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sevak_core::{Action, IconSource, ResultItem, Selection};
use sevak_plugins::file_buffer::{
    self as buffer, Added, BufferAction, FileBuffer, Outcome, RunContext,
};
use tauri::{AppHandle, Emitter, Manager};

use crate::commands;
use crate::icons::IconDto;
use crate::selection::{self, SelectionPayload};
use crate::state::{lock, AppState};
use crate::window;

/// A slow action's progress, for the progress row.
pub const EVENT_PROGRESS: &str = "sevak:buffer-progress";

/// The shell's side of the file buffer: the collection, and flags the window
/// logic checks.
#[derive(Default)]
pub struct BufferState {
    buffer: Mutex<FileBuffer>,
    /// An action is running; a second one waits for the first.
    busy: AtomicBool,
    /// A confirmation dialog is up. It takes the focus, which must not count as
    /// the user clicking away (the launcher would hide with the question
    /// unanswered).
    confirming: AtomicBool,
}

impl BufferState {
    /// Whether losing focus right now is only the confirmation dialog opening.
    pub fn confirming(&self) -> bool {
        self.confirming.load(Ordering::SeqCst)
    }

    /// The launcher was hidden: empty the buffer unless `[file_buffer]
    /// keep_between_shows` is on.
    pub fn on_hidden(&self, keep: bool) {
        if !keep {
            lock(&self.buffer).clear();
        }
    }

    fn items(&self) -> Vec<PathBuf> {
        lock(&self.buffer).items().to_vec()
    }
}

/// Clears `busy` however the action ends.
struct Busy<'a>(&'a AtomicBool);

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// One collected file or folder, as the strip draws it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ItemDto {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub icon: Option<IconDto>,
}

/// An action offered for the buffer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActionDto {
    pub key: &'static str,
    pub label: &'static str,
    /// Asks for a destination folder next.
    pub destination: bool,
}

/// The buffer's contents and what can be done with them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BufferDto {
    pub items: Vec<ItemDto>,
    pub actions: Vec<ActionDto>,
}

/// How an action ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunDto {
    pub message: String,
    /// Everything worked.
    pub ok: bool,
    /// The user answered the confirmation with Cancel; nothing happened.
    pub declined: bool,
    /// The launcher went away (the action handed over, or copied).
    pub hidden: bool,
    pub buffer: BufferDto,
}

/// A slow action's progress.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ProgressDto {
    label: &'static str,
    done: usize,
    total: usize,
    name: String,
}

/// Where Move to… / Copy to… should put the items.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DestinationArg {
    /// A folder row of the results on screen.
    Result { id: String, ticket: u64 },
    /// A path typed into the search box.
    Text { text: String },
}

fn actions() -> Vec<ActionDto> {
    BufferAction::ALL
        .into_iter()
        .map(|action| ActionDto {
            key: action.key(),
            label: action.label(),
            destination: action.needs_destination(),
        })
        .collect()
}

/// The icon a file result would show, so the chips match the rows.
fn icon_source(path: &Path, is_dir: bool) -> IconSource {
    if cfg!(windows) {
        IconSource::Shell {
            parsing_name: path.to_string_lossy().into_owned(),
        }
    } else if is_dir {
        IconSource::builtin("folder")
    } else {
        IconSource::builtin("file")
    }
}

fn describe(app: &AppHandle) -> BufferDto {
    let state = app.state::<AppState>();
    let entries: Vec<(PathBuf, bool)> = state
        .file_buffer
        .items()
        .into_iter()
        .map(|path| {
            let is_dir = std::fs::metadata(&path).is_ok_and(|meta| meta.is_dir());
            (path, is_dir)
        })
        .collect();
    // Icons are registered the way result rows' are.
    let rows: Vec<ResultItem> = entries
        .iter()
        .map(|(path, is_dir)| {
            ResultItem::new(
                "files",
                path.to_string_lossy(),
                "",
                Action::OpenPath { path: path.clone() },
            )
            .with_icon(icon_source(path, *is_dir))
        })
        .collect();
    let icons = state.search.icons.describe(&rows);
    BufferDto {
        items: entries
            .into_iter()
            .zip(icons)
            .map(|((path, is_dir), icon)| ItemDto {
                name: path
                    .file_name()
                    .unwrap_or(path.as_os_str())
                    .to_string_lossy()
                    .into_owned(),
                path: path.to_string_lossy().into_owned(),
                is_dir,
                icon,
            })
            .collect(),
        actions: actions(),
    }
}

/// The buffer as it is now (the page asks when it is shown).
#[tauri::command]
pub fn file_buffer_get(app: AppHandle) -> BufferDto {
    describe(&app)
}

/// The path of a file or folder row of the results on screen.
fn file_row_path(app: &AppHandle, id: &str, ticket: u64) -> Result<PathBuf, String> {
    let state = app.state::<AppState>();
    let Some((item, _query)) = state.search.result(ticket, id) else {
        return Err("result expired".to_owned());
    };
    match item.action {
        Action::OpenPath { path } if item.plugin_id == "files" => Ok(path),
        _ => Err("Only files and folders from the file results can be collected.".to_owned()),
    }
}

/// Adds result `id` of search `ticket` to the buffer (Alt+Up / Alt+Down).
#[tauri::command]
pub fn file_buffer_add(app: AppHandle, id: String, ticket: u64) -> Result<BufferDto, String> {
    let path = file_row_path(&app, &id, ticket)?;
    let state = app.state::<AppState>();
    let added = lock(&state.file_buffer.buffer).add(path);
    if added == Added::Full {
        return Err(format!(
            "The file buffer is full ({} items).",
            buffer::MAX_ITEMS
        ));
    }
    Ok(describe(&app))
}

/// Takes one item out: the one at `index`, or the last (Alt+Left).
#[tauri::command]
pub fn file_buffer_remove(app: AppHandle, index: Option<usize>) -> BufferDto {
    {
        let state = app.state::<AppState>();
        let mut buffer = lock(&state.file_buffer.buffer);
        match index {
            Some(index) => buffer.remove(index),
            None => buffer.remove_last(),
        };
    }
    describe(&app)
}

/// Empties the buffer (Alt+Backspace / Alt+Delete).
#[tauri::command]
pub fn file_buffer_clear(app: AppHandle) -> BufferDto {
    lock(&app.state::<AppState>().file_buffer.buffer).clear();
    describe(&app)
}

/// The Universal Actions list for the collected items: the actions for files
/// that the selection plugin offers, over the buffer.
#[tauri::command]
pub fn file_buffer_selection(app: AppHandle) -> Result<SelectionPayload, String> {
    let items = app.state::<AppState>().file_buffer.items();
    let selection =
        Selection::from_files(items).ok_or_else(|| "The file buffer is empty.".to_owned())?;
    selection::payload_for(&app, &selection)
}

fn destination_of(app: &AppHandle, arg: Option<DestinationArg>) -> Result<PathBuf, String> {
    match arg {
        Some(DestinationArg::Result { id, ticket }) => {
            let path = file_row_path(app, &id, ticket)?;
            if path.is_dir() {
                Ok(path)
            } else {
                Err(format!("{} is not a folder.", path.display()))
            }
        }
        Some(DestinationArg::Text { text }) => buffer::resolve_destination(&text),
        None => Err("Pick a destination folder first.".to_owned()),
    }
}

/// Runs buffer action `key` on everything collected. For the slow ones the page
/// shows the progress events until this returns.
#[tauri::command]
pub async fn file_buffer_run(
    app: AppHandle,
    key: String,
    destination: Option<DestinationArg>,
) -> Result<RunDto, String> {
    tauri::async_runtime::spawn_blocking(move || run_action(&app, &key, destination))
        .await
        .map_err(|err| format!("the action did not finish: {err}"))?
}

fn run_action(
    app: &AppHandle,
    key: &str,
    destination: Option<DestinationArg>,
) -> Result<RunDto, String> {
    let action = BufferAction::from_key(key).ok_or("that action is not available")?;
    let state = app.state::<AppState>();
    let buffer_state = &state.file_buffer;
    if buffer_state.busy.swap(true, Ordering::SeqCst) {
        return Err("Another file operation is still running.".to_owned());
    }
    let _busy = Busy(&buffer_state.busy);

    let items = buffer_state.items();
    if items.is_empty() {
        return Err("The file buffer is empty.".to_owned());
    }
    let dest = if action.needs_destination() {
        Some(destination_of(app, destination)?)
    } else {
        None
    };

    if let Some(question) = buffer::confirmation(action, &items, dest.as_deref()) {
        buffer_state.confirming.store(true, Ordering::SeqCst);
        let yes = commands::confirmed(app, action.label(), question);
        buffer_state.confirming.store(false, Ordering::SeqCst);
        if !yes {
            tracing::info!(action = key, "file buffer: declined at the confirmation");
            return Ok(RunDto {
                message: String::new(),
                ok: true,
                declined: true,
                hidden: false,
                buffer: describe(app),
            });
        }
    }

    // Opening things in other programs: the launcher gets out of the way first,
    // and comes back if nothing worked.
    let hands_over = action.hands_over();
    if hands_over {
        window::hide_silently(app);
    }

    let config = state.config();
    let context = RunContext {
        platform: state.search.platform.as_ref(),
        shell: &config.shell,
    };
    let outcome = buffer::run(
        &context,
        action,
        &items,
        dest.as_deref(),
        &mut |done, total, name| {
            let progress = ProgressDto {
                label: action.label(),
                done,
                total,
                name: name.to_owned(),
            };
            if let Err(err) = app.emit_to(window::MAIN_LABEL, EVENT_PROGRESS, progress) {
                tracing::debug!("could not emit {EVENT_PROGRESS}: {err}");
            }
        },
    );
    tracing::info!(
        action = key,
        total = outcome.total,
        failed = outcome.failed,
        "file buffer: action finished"
    );

    finish(app, action, &outcome, hands_over)
}

/// Updates the buffer and the window after an action.
fn finish(
    app: &AppHandle,
    action: BufferAction,
    outcome: &Outcome,
    hands_over: bool,
) -> Result<RunDto, String> {
    let state = app.state::<AppState>();
    if action.consumes_items() {
        lock(&state.file_buffer.buffer).forget(&outcome.done);
    }
    let copied = matches!(action, BufferAction::CopyPaths | BufferAction::CopyFiles);
    let hidden = if (hands_over || copied) && outcome.is_ok() {
        // Like Enter on a copy: done, so the launcher goes away.
        if hands_over {
            window::announce_hidden(app);
        } else {
            window::hide(app);
        }
        true
    } else {
        if hands_over {
            window::reveal(app);
        }
        false
    };
    Ok(RunDto {
        message: outcome.message.clone(),
        ok: outcome.is_ok(),
        declined: false,
        hidden,
        buffer: describe(app),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_buffer_is_emptied_on_hide_unless_kept() {
        let state = BufferState::default();
        lock(&state.buffer).add("/a".into());
        state.on_hidden(true);
        assert_eq!(state.items(), [PathBuf::from("/a")]);
        state.on_hidden(false);
        assert!(state.items().is_empty());
    }

    #[test]
    fn every_buffer_action_is_offered_in_order_with_its_destination_need() {
        let offered = actions();
        assert_eq!(offered.len(), BufferAction::ALL.len());
        assert_eq!(offered[0].key, "open_all");
        let needing: Vec<&str> = offered
            .iter()
            .filter(|a| a.destination)
            .map(|a| a.key)
            .collect();
        assert_eq!(needing, ["move_to", "copy_to"]);
        let json = serde_json::to_string(&offered[4]).unwrap();
        assert_eq!(
            json,
            r#"{"key":"move_to","label":"Move to…","destination":true}"#
        );
    }

    #[test]
    fn destinations_arrive_tagged() {
        let text: DestinationArg =
            serde_json::from_str(r#"{"kind":"text","text":"~/Documents/"}"#).unwrap();
        assert!(matches!(text, DestinationArg::Text { text } if text == "~/Documents/"));
        let row: DestinationArg =
            serde_json::from_str(r#"{"kind":"result","id":"files:/a","ticket":7}"#).unwrap();
        assert!(matches!(row, DestinationArg::Result { ticket: 7, .. }));
    }

    #[test]
    fn busy_is_released_when_an_action_ends() {
        let flag = AtomicBool::new(true);
        drop(Busy(&flag));
        assert!(!flag.load(Ordering::SeqCst));
    }
}
