//! macOS: list, focus, move and resize windows, and read the displays, through
//! `osascript` scripts that talk to System Events (see
//! [`crate::window_manager::osascript`] for the scripts, their output format and
//! why a script rather than direct Accessibility calls).
//!
//! # Permissions
//!
//! Moving other apps' windows needs **Accessibility** access for Sevak
//! (System Settings > Privacy & Security > Accessibility); the first use also
//! asks to allow controlling **System Events** (Automation). Without them
//! [`window_support`] reports why, in words naming the settings pane, and no
//! script runs. Nothing is requested silently and nothing prompts at startup.
//!
//! # Limits
//!
//! - The target is the first non-minimized window of the app that was frontmost
//!   when the launcher opened (System Events lists an app's windows front to
//!   back); a window the app does not expose to Accessibility (some games and
//!   video players) cannot be moved.
//! - Window ids are the process id and the window's position in that list, so
//!   they can go stale when windows open or close; the title seen at listing
//!   time is passed along to find the window again.
//! - Native full-screen windows are taken out of full screen before moving.
//! - Coordinates are points; the gap is therefore in points as well.

use std::collections::HashMap;
use std::sync::Mutex;

use sevak_core::window_layout::{Monitor, Rect};

use super::paste;
use crate::error::{PlatformError, Result};
use crate::tasks::capture;
use crate::window_manager::osascript::{
    self, format_id, monitors_from_screens, parse_id, parse_listing, parse_outcome, ScriptListing,
    ScriptWindow, ACCESSIBILITY_HELP, COORDINATE_OFFSET, TITLE_PREFIX,
};
use crate::window_manager::{WindowId, WindowInfo, WindowState, WindowSupport};

const OSASCRIPT: &str = "/usr/bin/osascript";

/// Titles seen by the last listing, by window id, to find a window again.
static TITLES: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

fn titles() -> std::sync::MutexGuard<'static, Option<HashMap<String, String>>> {
    TITLES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn remember_titles(windows: &[ScriptWindow]) {
    let mut guard = titles();
    let map = guard.get_or_insert_with(HashMap::new);
    for window in windows {
        map.insert(format_id(window.pid, window.index), window.title.clone());
    }
}

fn run_script(script: &str, args: &[String]) -> Result<String> {
    let mut all: Vec<String> = vec![
        "-l".to_owned(),
        "JavaScript".to_owned(),
        "-e".to_owned(),
        script.to_owned(),
    ];
    all.extend(args.iter().cloned());
    capture(OSASCRIPT, &all)
}

fn listing(mode: &str) -> Result<ScriptListing> {
    let own = std::process::id().to_string();
    let output = run_script(osascript::LIST_SCRIPT, &[own, mode.to_owned()])?;
    let listing = parse_listing(&output).map_err(PlatformError::Message)?;
    remember_titles(&listing.windows);
    Ok(listing)
}

pub(crate) fn window_support() -> WindowSupport {
    if paste::accessibility_granted() {
        WindowSupport::Available
    } else {
        WindowSupport::Unavailable(ACCESSIBILITY_HELP.to_owned())
    }
}

fn info(window: &ScriptWindow) -> Option<WindowInfo> {
    Some(WindowInfo {
        id: WindowId::new(&format_id(window.pid, window.index))?,
        title: window.title.clone(),
        app: window.app.clone(),
        minimized: window.minimized,
    })
}

fn state(window: &ScriptWindow) -> Option<WindowState> {
    Some(WindowState {
        id: WindowId::new(&format_id(window.pid, window.index))?,
        title: window.title.clone(),
        app: window.app.clone(),
        rect: window.rect,
        maximized: false,
        minimized: window.minimized,
    })
}

pub(crate) fn list_windows() -> Result<Vec<WindowInfo>> {
    if let WindowSupport::Unavailable(reason) = window_support() {
        return Err(PlatformError::Message(reason));
    }
    Ok(listing("all")?
        .windows
        .iter()
        .filter(|window| !window.title.is_empty())
        .filter_map(info)
        .collect())
}

fn parse_window_id(id: &WindowId) -> Result<(i32, usize)> {
    parse_id(id.as_str())
        .ok_or_else(|| PlatformError::Message(format!("{id} is not a window of this system")))
}

/// The argument that carries the title the window had when it was listed.
fn title_argument(id: &WindowId) -> String {
    let title = titles()
        .as_ref()
        .and_then(|map| map.get(id.as_str()).cloned())
        .unwrap_or_default();
    format!("{TITLE_PREFIX}{title}")
}

pub(crate) fn focus_window(id: &WindowId) -> Result<()> {
    if let WindowSupport::Unavailable(reason) = window_support() {
        return Err(PlatformError::Message(reason));
    }
    let (pid, index) = parse_window_id(id)?;
    let output = run_script(
        osascript::FOCUS_SCRIPT,
        &[pid.to_string(), index.to_string(), title_argument(id)],
    )?;
    parse_outcome(&output).map_err(PlatformError::Message)
}

pub(crate) fn target_window() -> Result<WindowState> {
    if let WindowSupport::Unavailable(reason) = window_support() {
        return Err(PlatformError::Message(reason));
    }
    let pid = paste::remembered_pid().ok_or_else(|| {
        PlatformError::Message("There is no previous window to arrange".to_owned())
    })?;
    let listing = listing(&pid.to_string())?;
    // The app's windows come front to back: the first one that is on screen.
    listing
        .windows
        .iter()
        .filter(|window| window.pid == pid && !window.minimized && !window.rect.is_empty())
        .find_map(state)
        .ok_or_else(|| {
            PlatformError::Message("The previous app has no window that can be arranged".to_owned())
        })
}

pub(crate) fn window_state(id: &WindowId) -> Result<WindowState> {
    if let WindowSupport::Unavailable(reason) = window_support() {
        return Err(PlatformError::Message(reason));
    }
    let (pid, index) = parse_window_id(id)?;
    listing(&pid.to_string())?
        .windows
        .iter()
        .find(|window| window.pid == pid && window.index == index)
        .and_then(state)
        .ok_or_else(|| PlatformError::Message("That window has closed".to_owned()))
}

pub(crate) fn set_window_rect(id: &WindowId, rect: Rect) -> Result<()> {
    if let WindowSupport::Unavailable(reason) = window_support() {
        return Err(PlatformError::Message(reason));
    }
    let (pid, index) = parse_window_id(id)?;
    let offset = |value: i32| value.saturating_add(COORDINATE_OFFSET).to_string();
    let output = run_script(
        osascript::SET_RECT_SCRIPT,
        &[
            pid.to_string(),
            index.to_string(),
            title_argument(id),
            offset(rect.x),
            offset(rect.y),
            rect.width.max(1).to_string(),
            rect.height.max(1).to_string(),
        ],
    )?;
    parse_outcome(&output).map_err(PlatformError::Message)
}

pub(crate) fn list_monitors() -> Result<Vec<Monitor>> {
    let monitors = monitors_from_screens(&listing("screens")?.screens);
    if monitors.is_empty() {
        return Err(PlatformError::Message("No display was found".to_owned()));
    }
    Ok(monitors)
}
