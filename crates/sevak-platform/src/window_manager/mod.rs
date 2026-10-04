//! Window management: snapping, resizing and moving the window you were using,
//! and listing and focusing open windows.
//!
//! The geometry (which rectangle a layout means) lives in
//! [`sevak_core::window_layout`]; this module is the bridge to the operating
//! system. The [`crate::PlatformProvider`] trait has a handful of small
//! primitives (list windows, focus one, read and set the rectangle of the
//! window that was in front before the launcher opened, list monitors), each
//! implemented per OS, and [`apply_command`] composes them with the layout
//! math. Because it only talks to the trait, it is tested here with a fake
//! provider and needs no real window.
//!
//! # The target window
//!
//! When the launcher opens it takes focus, so "the active window" is Sevak's
//! own. The shell calls [`crate::PlatformProvider::remember_foreground_app`]
//! just before showing the launcher (the same mechanism that pasting uses) and
//! the per-OS code remembers the window that had focus then; that window is
//! the [`crate::PlatformProvider::target_window`].
//!
//! # Support
//!
//! [`WindowSupport`] says whether anything here can work right now and, if not,
//! why in words for the user (Wayland sessions, a missing macOS permission).
//! The remaining primitives have default implementations that fail with
//! [`PlatformError::Unsupported`].

// Pure helpers for one OS each, compiled everywhere so their tests run on
// every development machine.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) mod osascript;
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) mod x11_geometry;

use std::fmt;
use std::sync::{Mutex, OnceLock};

use sevak_core::window_layout::{plan, Rect, WindowCommand, POSITION_TOLERANCE};

use crate::error::{PlatformError, Result};
use crate::provider::PlatformProvider;

/// What the window manager code says when the OS has no implementation.
pub const UNSUPPORTED_REASON: &str = "Window management is not supported on this system";

/// Longest identifier [`WindowId::new`] accepts.
const MAX_ID_LEN: usize = 64;

/// Identifies one top-level window to the OS code that made it.
///
/// The text is opaque to everyone else (a handle on Windows, a window id on
/// X11, a process and index on macOS). It ends up in result payloads, so it is
/// restricted to a short, harmless alphabet: callers cannot smuggle anything
/// but an identifier through it, and each backend parses it strictly again.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WindowId(String);

impl WindowId {
    /// `None` unless `text` is 1 to 64 ASCII letters, digits or `-_:.`.
    pub fn new(text: &str) -> Option<Self> {
        let valid = !text.is_empty()
            && text.len() <= MAX_ID_LEN
            && text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':' | b'.'));
        valid.then(|| Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WindowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// An open top-level window, as the window switcher lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    pub id: WindowId,
    pub title: String,
    /// The program that owns it (`firefox`, `Code`, `Safari`).
    pub app: String,
    pub minimized: bool,
}

/// Where the window the layouts act on is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowState {
    pub id: WindowId,
    pub title: String,
    pub app: String,
    /// The visible rectangle (without invisible resize borders or shadows).
    pub rect: Rect,
    pub maximized: bool,
    pub minimized: bool,
}

/// Whether window management can work on this system right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowSupport {
    Available,
    /// It cannot, for this reason (written for the user).
    Unavailable(String),
}

impl WindowSupport {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }

    /// The reason, or `None` when available.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Available => None,
            Self::Unavailable(reason) => Some(reason),
        }
    }
}

/// How many windows [`RestoreMemory`] remembers; the oldest is forgotten first.
const MEMORY_CAPACITY: usize = 32;

#[derive(Debug, Clone, Copy)]
struct Saved {
    /// Where the window was before Sevak first moved it.
    original: Rect,
    /// Where Sevak last put it.
    applied: Rect,
}

/// Remembers where windows were before Sevak moved them, for `restore`.
///
/// A window that Sevak moved and the user then moved by hand has a new
/// starting point: the next layout remembers that position instead (see
/// [`RestoreMemory::baseline`]).
#[derive(Debug, Default)]
pub struct RestoreMemory {
    entries: Mutex<Vec<(WindowId, Saved)>>,
}

impl RestoreMemory {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<(WindowId, Saved)>> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Where `id` was before Sevak moved it, if it did.
    pub fn restore_point(&self, id: &WindowId) -> Option<Rect> {
        self.lock()
            .iter()
            .find(|(known, _)| known == id)
            .map(|(_, saved)| saved.original)
    }

    /// The rectangle a restore should return `id` to if a layout is applied to
    /// it now, while it is at `current`: the remembered original when the
    /// window is still where Sevak put it, else `current`.
    pub fn baseline(&self, id: &WindowId, current: Rect) -> Rect {
        self.lock()
            .iter()
            .find(|(known, _)| known == id)
            .filter(|(_, saved)| saved.applied.approx_eq(current, POSITION_TOLERANCE))
            .map_or(current, |(_, saved)| saved.original)
    }

    pub fn remember(&self, id: &WindowId, original: Rect, applied: Rect) {
        let mut entries = self.lock();
        entries.retain(|(known, _)| known != id);
        if entries.len() >= MEMORY_CAPACITY {
            entries.remove(0);
        }
        entries.push((id.clone(), Saved { original, applied }));
    }

    pub fn forget(&self, id: &WindowId) {
        self.lock().retain(|(known, _)| known != id);
    }
}

/// The memory the launcher uses; it lives as long as the process, so settings
/// reloads (which rebuild the plugins) do not lose it.
pub fn shared_memory() -> &'static RestoreMemory {
    static MEMORY: OnceLock<RestoreMemory> = OnceLock::new();
    MEMORY.get_or_init(RestoreMemory::new)
}

/// Applies `command` to the window that was in front before the launcher
/// opened. `gap` is the configured gap in logical pixels.
pub fn apply_command(
    provider: &dyn PlatformProvider,
    memory: &RestoreMemory,
    command: WindowCommand,
    gap: i32,
) -> Result<()> {
    if let WindowSupport::Unavailable(reason) = provider.window_support() {
        return Err(PlatformError::Message(reason));
    }
    let target = provider.target_window()?;
    let monitors = provider.list_monitors()?;
    let restore_point = memory.restore_point(&target.id);
    let rect = plan(command, target.rect, &monitors, gap, restore_point)
        .map_err(|err| PlatformError::Message(err.to_string()))?;

    provider.set_window_rect(&target.id, rect)?;

    if command == WindowCommand::Restore {
        memory.forget(&target.id);
    } else {
        let original = memory.baseline(&target.id, target.rect);
        // Apps round and clamp sizes; remember what they ended up with so
        // "did the user move it since?" compares with the real rectangle.
        let applied = provider
            .window_state(&target.id)
            .map_or(rect, |state| state.rect);
        memory.remember(&target.id, original, applied);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use sevak_core::window_layout::{Layout, Monitor};

    use super::*;
    use crate::error::Result;

    fn id(text: &str) -> WindowId {
        WindowId::new(text).unwrap()
    }

    fn r(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect::new(x, y, w, h)
    }

    #[test]
    fn window_ids_are_a_short_harmless_alphabet() {
        for good in [
            "1",
            "123456",
            "x11:0x3a00004",
            "4242:1:ab",
            "a-b_c.d",
            &"a".repeat(64),
        ] {
            assert_eq!(WindowId::new(good).unwrap().as_str(), good);
        }
        for bad in [
            "",
            " ",
            "a b",
            "a;b",
            "a/b",
            "a\\b",
            "a\nb",
            "a$b",
            "$(x)",
            "é",
            &"a".repeat(65),
        ] {
            assert!(WindowId::new(bad).is_none(), "{bad:?}");
        }
        assert_eq!(id("77").to_string(), "77");
    }

    #[test]
    fn restore_memory_keeps_the_original_until_the_user_moves_the_window() {
        let memory = RestoreMemory::new();
        let w = id("1");
        let start = r(100, 100, 800, 600);
        let half = r(0, 0, 960, 1040);
        assert_eq!(memory.restore_point(&w), None);
        assert_eq!(memory.baseline(&w, start), start, "nothing remembered yet");

        memory.remember(&w, start, half);
        assert_eq!(memory.restore_point(&w), Some(start));
        // Still where Sevak put it (within the tolerance): the original stands.
        assert_eq!(memory.baseline(&w, r(1, 0, 959, 1040)), start);
        // Moved by hand: the new position is the starting point.
        let moved = r(300, 200, 960, 1040);
        assert_eq!(memory.baseline(&w, moved), moved);

        memory.forget(&w);
        assert_eq!(memory.restore_point(&w), None);
    }

    #[test]
    fn restore_memory_forgets_the_oldest_windows() {
        let memory = RestoreMemory::new();
        for n in 0..(MEMORY_CAPACITY + 5) {
            memory.remember(&id(&n.to_string()), r(n as i32, 0, 10, 10), r(0, 0, 5, 5));
        }
        assert_eq!(memory.restore_point(&id("0")), None);
        assert_eq!(memory.restore_point(&id("4")), None);
        assert!(memory.restore_point(&id("5")).is_some());
        assert!(memory
            .restore_point(&id(&(MEMORY_CAPACITY + 4).to_string()))
            .is_some());
        // Remembering a window again replaces its entry, not adds one.
        memory.remember(&id("6"), r(9, 9, 9, 9), r(1, 1, 1, 1));
        assert_eq!(memory.restore_point(&id("6")), Some(r(9, 9, 9, 9)));
        assert_eq!(memory.lock().len(), MEMORY_CAPACITY);
    }

    /// A provider with a fixed desktop that records what it was asked to do.
    struct FakeDesktop {
        support: WindowSupport,
        window: Mutex<WindowState>,
        monitors: Vec<Monitor>,
        /// What `set_window_rect` was given.
        set: Mutex<Vec<(WindowId, Rect)>>,
        /// The app refuses to be smaller than this wide.
        min_width: i32,
    }

    impl FakeDesktop {
        fn new(rect: Rect) -> Self {
            let mut primary = Monitor::new("1", r(0, 0, 1920, 1080), r(0, 0, 1920, 1040));
            primary.primary = true;
            Self {
                support: WindowSupport::Available,
                window: Mutex::new(WindowState {
                    id: id("1"),
                    title: "Doc".into(),
                    app: "editor".into(),
                    rect,
                    maximized: false,
                    minimized: false,
                }),
                monitors: vec![primary],
                set: Mutex::new(Vec::new()),
                min_width: 0,
            }
        }
    }

    impl PlatformProvider for FakeDesktop {
        fn list_applications(&self) -> Result<Vec<sevak_core::AppEntry>> {
            Ok(Vec::new())
        }
        fn launch(&self, _: &sevak_core::LaunchTarget) -> Result<()> {
            Ok(())
        }
        fn load_icon(&self, _: &sevak_core::IconSource, _: u32) -> Result<sevak_core::IconData> {
            Err(PlatformError::Unsupported("icons"))
        }
        fn window_support(&self) -> WindowSupport {
            self.support.clone()
        }
        fn target_window(&self) -> Result<WindowState> {
            Ok(self.window.lock().unwrap().clone())
        }
        fn window_state(&self, _: &WindowId) -> Result<WindowState> {
            Ok(self.window.lock().unwrap().clone())
        }
        fn list_monitors(&self) -> Result<Vec<Monitor>> {
            Ok(self.monitors.clone())
        }
        fn set_window_rect(&self, window: &WindowId, rect: Rect) -> Result<()> {
            self.set.lock().unwrap().push((window.clone(), rect));
            let mut state = self.window.lock().unwrap();
            state.rect = Rect {
                width: rect.width.max(self.min_width),
                ..rect
            };
            Ok(())
        }
    }

    fn run(desktop: &FakeDesktop, memory: &RestoreMemory, command: WindowCommand) -> Result<()> {
        apply_command(desktop, memory, command, 0)
    }

    #[test]
    fn a_layout_moves_the_target_window_and_restore_brings_it_back() {
        let start = r(200, 150, 800, 600);
        let desktop = FakeDesktop::new(start);
        let memory = RestoreMemory::new();

        run(&desktop, &memory, WindowCommand::Layout(Layout::LeftHalf)).unwrap();
        run(&desktop, &memory, WindowCommand::Layout(Layout::TopRight)).unwrap();
        run(&desktop, &memory, WindowCommand::Restore).unwrap();

        let set = desktop.set.lock().unwrap();
        assert_eq!(
            set.iter().map(|(_, rect)| *rect).collect::<Vec<_>>(),
            [r(0, 0, 960, 1040), r(960, 0, 960, 520), start],
            "two snaps, then back to the very first position"
        );
        assert!(set.iter().all(|(window, _)| *window == id("1")));
        drop(set);
        // Restoring used the memory up.
        assert!(matches!(
            run(&desktop, &memory, WindowCommand::Restore),
            Err(PlatformError::Message(_))
        ));
    }

    #[test]
    fn restore_without_a_snap_says_so() {
        let desktop = FakeDesktop::new(r(0, 0, 500, 400));
        let err = run(&desktop, &RestoreMemory::new(), WindowCommand::Restore).unwrap_err();
        assert!(err.to_string().contains("nothing to restore"), "{err}");
        assert!(desktop.set.lock().unwrap().is_empty());
    }

    #[test]
    fn moving_the_window_by_hand_makes_that_position_the_new_original() {
        let desktop = FakeDesktop::new(r(200, 150, 800, 600));
        let memory = RestoreMemory::new();
        run(&desktop, &memory, WindowCommand::Layout(Layout::LeftHalf)).unwrap();
        // The user drags it somewhere else...
        desktop.window.lock().unwrap().rect = r(500, 300, 700, 500);
        run(&desktop, &memory, WindowCommand::Layout(Layout::Maximize)).unwrap();
        run(&desktop, &memory, WindowCommand::Restore).unwrap();
        let last = desktop.set.lock().unwrap().last().unwrap().1;
        assert_eq!(last, r(500, 300, 700, 500));
    }

    #[test]
    fn the_rectangle_the_app_really_took_is_what_gets_remembered() {
        let mut desktop = FakeDesktop::new(r(200, 150, 800, 600));
        desktop.min_width = 1000;
        let memory = RestoreMemory::new();
        run(&desktop, &memory, WindowCommand::Layout(Layout::LeftHalf)).unwrap();
        // The app stayed 1000 wide although 960 was asked for; a second snap
        // finds it "still where Sevak put it" and keeps the first original.
        run(&desktop, &memory, WindowCommand::Layout(Layout::TopHalf)).unwrap();
        run(&desktop, &memory, WindowCommand::Restore).unwrap();
        assert_eq!(
            desktop.set.lock().unwrap().last().unwrap().1,
            r(200, 150, 800, 600)
        );
    }

    #[test]
    fn unavailable_window_management_is_explained_and_touches_nothing() {
        let mut desktop = FakeDesktop::new(r(0, 0, 500, 400));
        desktop.support = WindowSupport::Unavailable("Wayland does not allow it".into());
        let err = run(
            &desktop,
            &RestoreMemory::new(),
            WindowCommand::Layout(Layout::Maximize),
        )
        .unwrap_err();
        assert_eq!(err.to_string(), "Wayland does not allow it");
        assert!(desktop.set.lock().unwrap().is_empty());
        assert_eq!(desktop.support.reason(), Some("Wayland does not allow it"));
        assert!(!desktop.support.is_available());
    }

    #[test]
    fn display_moves_need_a_second_display() {
        let desktop = FakeDesktop::new(r(100, 100, 800, 600));
        let err = run(&desktop, &RestoreMemory::new(), WindowCommand::NextDisplay).unwrap_err();
        assert!(err.to_string().contains("one display"), "{err}");

        let mut two = FakeDesktop::new(r(100, 100, 800, 600));
        two.monitors.push(Monitor::new(
            "2",
            r(1920, 0, 1920, 1080),
            r(1920, 0, 1920, 1040),
        ));
        let memory = RestoreMemory::new();
        run(&two, &memory, WindowCommand::NextDisplay).unwrap();
        assert_eq!(
            two.set.lock().unwrap()[0].1,
            r(2020, 100, 800, 600),
            "same size display: shifted by the display offset"
        );
        // It can be undone.
        run(&two, &memory, WindowCommand::Restore).unwrap();
        assert_eq!(two.set.lock().unwrap()[1].1, r(100, 100, 800, 600));
    }

    #[test]
    fn the_default_provider_has_no_window_management() {
        struct Bare;
        impl PlatformProvider for Bare {
            fn list_applications(&self) -> Result<Vec<sevak_core::AppEntry>> {
                Ok(Vec::new())
            }
            fn launch(&self, _: &sevak_core::LaunchTarget) -> Result<()> {
                Ok(())
            }
            fn load_icon(
                &self,
                _: &sevak_core::IconSource,
                _: u32,
            ) -> Result<sevak_core::IconData> {
                Err(PlatformError::Unsupported("icons"))
            }
        }
        let bare = Bare;
        assert_eq!(
            bare.window_support(),
            WindowSupport::Unavailable(UNSUPPORTED_REASON.to_owned())
        );
        assert!(matches!(
            bare.list_windows(),
            Err(PlatformError::Unsupported(_))
        ));
        assert!(matches!(
            bare.list_monitors(),
            Err(PlatformError::Unsupported(_))
        ));
        assert!(matches!(
            bare.target_window(),
            Err(PlatformError::Unsupported(_))
        ));
        assert!(matches!(
            bare.focus_window(&id("1")),
            Err(PlatformError::Unsupported(_))
        ));
        assert!(matches!(
            bare.set_window_rect(&id("1"), r(0, 0, 1, 1)),
            Err(PlatformError::Unsupported(_))
        ));
        let err = apply_command(
            &bare,
            &RestoreMemory::new(),
            WindowCommand::Layout(Layout::Maximize),
            0,
        )
        .unwrap_err();
        assert_eq!(err.to_string(), UNSUPPORTED_REASON);
    }
}
