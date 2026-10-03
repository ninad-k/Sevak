//! Universal Actions: the hotkey (or `sevak --actions`) reads what is selected
//! in the app the user is working in and opens the launcher on the actions for
//! it.
//!
//! The selection is in memory only. It is held as the results of one search
//! ticket ([`Search::store_selection`]), which is dropped when the launcher
//! hides, it is never logged, and the actions are run without touching the
//! usage statistics (see `Plugin::tracks_usage`).

use std::fmt;

use serde::Serialize;
use sevak_core::selection::{preview, MAX_SELECTION_BYTES};
use sevak_core::{Config, Selection};
use sevak_platform::capture::{clipboard_selection, CaptureOptions, SelectionCapture};
use sevak_plugins::selection::{ui_request, UiRequest};
use tauri::{AppHandle, Manager};

use crate::commands::{self, ResultDto};
use crate::direct::ShowPayload;
use crate::state::AppState;
use crate::window;

/// The question shown in the launcher's actions panel.
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct SelectionPayload {
    /// The search ticket the actions can be executed under.
    pub ticket: u64,
    /// "Selected text", "2 selected files and folders", ...
    pub title: String,
    /// A short preview of the selection.
    pub subtitle: String,
    pub actions: Vec<SelectionActionDto>,
}

// By hand: the actions carry the selection's text.
impl fmt::Debug for SelectionPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SelectionPayload")
            .field("ticket", &self.ticket)
            .field("actions", &self.actions.len())
            .finish()
    }
}

/// One action as the launcher sees it: a result row, plus what the window
/// itself must do for the few actions that are not the shell's to run.
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct SelectionActionDto {
    #[serde(flatten)]
    pub result: ResultDto,
    pub window: Option<WindowAction>,
}

impl fmt::Debug for SelectionActionDto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SelectionActionDto")
            .field(&self.result.id)
            .finish()
    }
}

/// An action carried out by the launcher window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WindowAction {
    /// Show `text` as Large Type.
    LargeType { text: String },
    /// Put `query` in the search box.
    Search { query: String },
}

impl From<UiRequest> for WindowAction {
    fn from(request: UiRequest) -> Self {
        match request {
            UiRequest::LargeType(text) => Self::LargeType { text },
            UiRequest::Search(query) => Self::Search { query },
        }
    }
}

/// The hotkey was pressed: capture the selection and show the actions.
pub fn trigger(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    // Pressed again while the launcher is up: close it, like the main key. It
    // also means there is no other app in front to read from.
    let focused = app
        .get_webview_window(window::MAIN_LABEL)
        .is_some_and(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false));
    if focused {
        window::hide(app);
        return;
    }

    // Now, while the user's app has focus: its selection is what we read, and
    // the paste actions return to it.
    state.search.platform.remember_foreground_app();
    let config = state.config();
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || run(&app, &config));
}

fn run(app: &AppHandle, config: &Config) {
    let state = app.state::<AppState>();
    let options = CaptureOptions {
        use_primary_selection: config.actions.use_primary_selection,
    };
    let capture = state.search.platform.capture_selection(&options);
    let outcome = resolve(capture, config.actions.use_clipboard_fallback, || {
        clipboard_selection()
    })
    .and_then(|selection| payload_for(app, &selection));

    match outcome {
        Ok(payload) => {
            tracing::info!(
                actions = payload.actions.len(),
                "showing the actions for the selection"
            );
            window::show_with(
                app,
                ShowPayload {
                    selection: Some(payload),
                    ..ShowPayload::default()
                },
            );
        }
        Err(message) => {
            tracing::info!(%message, "universal actions: nothing to act on");
            window::show_with(
                app,
                ShowPayload {
                    error: Some(message),
                    ..ShowPayload::default()
                },
            );
        }
    }
}

/// Turns what the capture found into the selection to act on, or the message
/// the launcher shows instead. `clipboard` reads the clipboard for the opt-in
/// fallback.
fn resolve(
    capture: SelectionCapture,
    use_clipboard_fallback: bool,
    clipboard: impl FnOnce() -> Option<Selection>,
) -> Result<Selection, String> {
    let selection = match capture {
        SelectionCapture::Selected(selection) => selection,
        SelectionCapture::Nothing => {
            return Err(
                "Nothing is selected. Select text or files in an app, then use \
                        Universal Actions again."
                    .to_owned(),
            )
        }
        SelectionCapture::Unavailable(reason) if use_clipboard_fallback => {
            clipboard().ok_or_else(|| {
                format!("{reason}, and the clipboard holds no text or files to use instead.")
            })?
        }
        SelectionCapture::Unavailable(reason) => {
            return Err(format!(
                "{reason}. To act on the clipboard instead, set use_clipboard_fallback = true \
                 under [actions] in config.toml."
            ))
        }
    };
    if selection.is_too_large() {
        return Err(format!(
            "That selection is too large to act on (over {} kB).",
            MAX_SELECTION_BYTES / 1024
        ));
    }
    Ok(selection)
}

/// Asks the plugins for the actions, stores them under a fresh ticket and
/// describes them for the launcher.
fn payload_for(app: &AppHandle, selection: &Selection) -> Result<SelectionPayload, String> {
    let state = app.state::<AppState>();
    let search = &state.search;
    let items = search.engine().selection_actions(selection);
    if items.is_empty() {
        return Err(
            "No action is available for this selection (is the Universal Actions plugin off?)."
                .to_owned(),
        );
    }

    let dtos = commands::to_dtos(search.icons.describe(&items), &items);
    let actions = items
        .iter()
        .zip(dtos)
        .map(|(item, result)| SelectionActionDto {
            window: ui_request(&item.action).map(WindowAction::from),
            result,
        })
        .collect();
    let ticket = search.next_ticket();
    search.store_selection(ticket, items);

    Ok(SelectionPayload {
        ticket,
        title: selection.describe(),
        subtitle: subtitle_of(selection),
        actions,
    })
}

/// A one-line preview of what was selected.
fn subtitle_of(selection: &Selection) -> String {
    match (selection.text(), selection.files()) {
        (_, [first, rest @ ..]) => {
            let name = first
                .file_name()
                .map_or_else(|| first.to_string_lossy(), |n| n.to_string_lossy());
            match rest.len() {
                0 => name.into_owned(),
                n => format!("{name} and {n} more"),
            }
        }
        (Some(text), []) => preview(text, 80),
        (None, []) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(t: &str) -> Selection {
        Selection::from_text(t).unwrap()
    }

    #[test]
    fn a_captured_selection_is_used_as_is() {
        let resolved = resolve(SelectionCapture::Selected(text("hi")), false, || {
            panic!("the clipboard must not be read")
        });
        assert_eq!(resolved.unwrap().text(), Some("hi"));
    }

    #[test]
    fn nothing_selected_says_so_and_ignores_the_clipboard_fallback() {
        let err = resolve(SelectionCapture::Nothing, true, || {
            Some(text("stale clipboard"))
        })
        .unwrap_err();
        assert!(err.starts_with("Nothing is selected"), "{err}");
    }

    #[test]
    fn an_unavailable_capture_explains_and_offers_the_fallback() {
        let err = resolve(
            SelectionCapture::Unavailable("Wayland does not let apps read it".into()),
            false,
            || panic!("the fallback is off"),
        )
        .unwrap_err();
        assert!(
            err.starts_with("Wayland does not let apps read it. "),
            "{err}"
        );
        assert!(err.contains("use_clipboard_fallback = true"), "{err}");
    }

    #[test]
    fn the_fallback_uses_the_clipboard_only_when_the_user_opted_in() {
        let used = resolve(SelectionCapture::Unavailable("no".into()), true, || {
            Some(text("from the clipboard"))
        });
        assert_eq!(used.unwrap().text(), Some("from the clipboard"));

        let empty = resolve(SelectionCapture::Unavailable("no".into()), true, || None);
        assert!(empty.unwrap_err().contains("clipboard holds no text"));
    }

    #[test]
    fn oversized_selections_are_refused_from_either_source() {
        let huge = || Selection::from_text("x".repeat(MAX_SELECTION_BYTES + 1)).unwrap();
        let err = resolve(SelectionCapture::Selected(huge()), false, || None).unwrap_err();
        assert!(err.contains("too large"), "{err}");
        let err = resolve(SelectionCapture::Unavailable("no".into()), true, || {
            Some(huge())
        })
        .unwrap_err();
        assert!(err.contains("too large"), "{err}");
    }

    #[test]
    fn subtitles_preview_the_selection() {
        assert_eq!(subtitle_of(&text("  some\n text  ")), "some text");
        let one = Selection::from_files(vec!["/tmp/a.txt".into()]).unwrap();
        assert_eq!(subtitle_of(&one), "a.txt");
        let three =
            Selection::from_files(vec!["/tmp/a.txt".into(), "/b".into(), "/c".into()]).unwrap();
        assert_eq!(subtitle_of(&three), "a.txt and 2 more");
    }

    #[test]
    fn window_actions_serialize_with_a_kind() {
        let json = serde_json::to_string(&WindowAction::LargeType { text: "hi".into() }).unwrap();
        assert_eq!(json, r#"{"kind":"large_type","text":"hi"}"#);
        let json =
            serde_json::to_string(&WindowAction::from(UiRequest::Search("/a/".into()))).unwrap();
        assert_eq!(json, r#"{"kind":"search","query":"/a/"}"#);
    }

    #[test]
    fn debug_output_leaves_the_selection_out() {
        let payload = SelectionPayload {
            ticket: 3,
            title: "Selected text".into(),
            subtitle: "my password".into(),
            actions: vec![SelectionActionDto {
                result: ResultDto {
                    id: "selection:copy".into(),
                    title: "Copy my password".into(),
                    subtitle: String::new(),
                    icon: None,
                    plugin_id: "selection".into(),
                    autocomplete: None,
                    action: "copy_text",
                    secondary: Vec::new(),
                    copy_text: Some("my password".into()),
                    tile: false,
                    glyph: None,
                    text_view: false,
                    text_on_enter: false,
                },
                window: None,
            }],
        };
        let shown = format!("{payload:?}");
        assert!(!shown.contains("password"), "{shown}");
        assert!(!format!(
            "{:?}",
            ShowPayload {
                selection: Some(payload),
                ..ShowPayload::default()
            }
        )
        .contains("password"));
    }
}
