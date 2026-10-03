//! Capturing what the user has selected in another app (Universal Actions).
//!
//! The only way to read a selection that works in every app is to ask the app
//! to copy it. So the capture borrows the clipboard: it saves what is there,
//! presses Ctrl+C / Cmd+C in the app that has focus, waits (briefly) for the
//! clipboard to change, reads text and/or the file list, and puts the original
//! contents back. The order of operations is the same everywhere and lives
//! here; the OS-specific half (release the hotkey's modifiers, press the key,
//! tell when the clipboard changed) is the `windows`, `macos` and `linux`
//! modules'.
//!
//! The selection is only ever returned to the caller. Nothing here logs it, and
//! while the clipboard holds it [`crate::clipboard::SyntheticCopy`] keeps the
//! clipboard history from recording it.

use std::path::PathBuf;
use std::thread::sleep;
use std::time::{Duration, Instant};

use sevak_core::Selection;

use crate::clipboard::{ClipboardSnapshot, SyntheticCopy};
use crate::error::Result;
use crate::paste::ForegroundApp;

/// How long to wait for the app to put its copy on the clipboard. Apps answer
/// in a few milliseconds; a slow one gets this long, no longer.
pub const COPY_TIMEOUT: Duration = Duration::from_millis(300);
/// How long to wait for the user to let go of the hotkey's modifier keys.
pub const MODIFIER_TIMEOUT: Duration = Duration::from_millis(600);
const POLL_INTERVAL: Duration = Duration::from_millis(15);

/// What the caller wants from [`crate::PlatformProvider::capture_selection`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureOptions {
    /// Linux (X11): try the PRIMARY selection before pressing any key.
    pub use_primary_selection: bool,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        Self {
            use_primary_selection: true,
        }
    }
}

/// The outcome of a capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionCapture {
    /// The text and/or files that were selected.
    Selected(Selection),
    /// The app copied nothing: there was no selection.
    Nothing,
    /// Capturing is not possible here right now; the reason is for the user.
    /// (Wayland, a terminal window, no Accessibility permission, ...)
    Unavailable(String),
}

/// The clipboard as a capture sees it after the copy keystroke.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct CapturedContent {
    pub text: Option<String>,
    pub files: Vec<PathBuf>,
    /// The app that filled the clipboard marked it secret (a password field):
    /// it is ignored and never read.
    pub sensitive: bool,
}

impl CapturedContent {
    fn is_empty(&self) -> bool {
        self.sensitive
            || (self.files.is_empty() && self.text.as_deref().is_none_or(|t| t.trim().is_empty()))
    }
}

/// The clipboard operations a capture needs; a trait so the order of
/// operations can be tested without touching the real clipboard.
pub(crate) trait CaptureClipboard {
    /// The clipboard's change counter, where the OS has one.
    fn sequence(&self) -> Option<u64>;
    fn snapshot(&self) -> ClipboardSnapshot;
    fn clear(&self);
    fn read(&self) -> CapturedContent;
    fn restore(&self, snapshot: &ClipboardSnapshot);
}

/// The OS half of a capture.
pub(crate) trait CaptureDriver {
    /// The app in front, if it can be told.
    fn foreground_app(&self) -> Option<ForegroundApp>;
    /// Whether Ctrl+C would interrupt a program running in a terminal window
    /// (everywhere but macOS, where copy is Cmd+C).
    fn copy_can_interrupt_terminals(&self) -> bool {
        true
    }
    /// Waits for the user to release the hotkey's modifier keys (Ctrl, Alt,
    /// Shift, Win/Super), and if they are still down after
    /// [`MODIFIER_TIMEOUT`], lets go of them for the app.
    fn release_modifiers(&self);
    /// Sends Ctrl+C / Cmd+C to the focused window.
    fn press_copy(&self) -> Result<()>;
}

/// Terminal programs (matched against the foreground app's names). Ctrl+C in
/// one of them is "interrupt", whether or not something is selected.
const TERMINALS: &[&str] = &[
    "windowsterminal",
    "wt",
    "cmd",
    "conhost",
    "openconsole",
    "powershell",
    "pwsh",
    "powershell_ise",
    "mintty",
    "putty",
    "alacritty",
    "kitty",
    "wezterm",
    "wezterm-gui",
    "foot",
    "footclient",
    "gnome-terminal",
    "gnome-terminal-server",
    "kgx",
    "konsole",
    "xterm",
    "uxterm",
    "urxvt",
    "rxvt",
    "st",
    "tilix",
    "terminator",
    "xfce4-terminal",
    "mate-terminal",
    "lxterminal",
    "qterminal",
    "terminology",
    "guake",
    "yakuake",
    "sakura",
    "hyper",
    "tabby",
    "ghostty",
    "rio",
    "x-terminal-emulator",
];

/// True if `app` is a terminal emulator or console window.
pub fn is_terminal(app: &ForegroundApp) -> bool {
    TERMINALS.iter().any(|terminal| app.matches(terminal))
}

/// Captures the selection by copying it. See the module documentation.
///
/// `delays` is false in tests (no sleeping).
pub(crate) fn capture_by_copy(
    clipboard: &dyn CaptureClipboard,
    driver: &dyn CaptureDriver,
    delays: bool,
) -> SelectionCapture {
    if driver.copy_can_interrupt_terminals() {
        if let Some(app) = driver.foreground_app().filter(is_terminal) {
            return SelectionCapture::Unavailable(format!(
                "{} is a terminal: Ctrl+C there would interrupt the running program, \
                 so Sevak does not press it",
                app.name
            ));
        }
    }

    // Until the clipboard is back as it was, the history must not see it.
    let _borrowed = SyntheticCopy::begin();

    driver.release_modifiers();

    let saved = clipboard.snapshot();
    let before = clipboard.sequence();
    if before.is_none() {
        // Without a change counter "the clipboard changed" can only be told by
        // it holding something, so start from empty.
        clipboard.clear();
    }

    let pressed = driver.press_copy();
    let content = match pressed {
        Ok(()) => wait_for_copy(clipboard, before, delays),
        Err(err) => {
            clipboard.restore(&saved);
            return SelectionCapture::Unavailable(format!("Could not press copy ({err})"));
        }
    };

    // Put the clipboard back before anything else, whatever was found.
    clipboard.restore(&saved);

    match content {
        Some(content) => match Selection::from_parts(content.text, content.files) {
            Some(selection) => SelectionCapture::Selected(selection),
            None => SelectionCapture::Nothing,
        },
        None => SelectionCapture::Nothing,
    }
}

/// Polls until the clipboard holds a new, non-empty, non-secret copy, or
/// [`COPY_TIMEOUT`] passes.
fn wait_for_copy(
    clipboard: &dyn CaptureClipboard,
    before: Option<u64>,
    delays: bool,
) -> Option<CapturedContent> {
    let deadline = Instant::now() + COPY_TIMEOUT;
    loop {
        let changed = match before {
            Some(before) => clipboard.sequence() != Some(before),
            // The clipboard was emptied: anything on it is the copy.
            None => true,
        };
        if changed {
            let content = clipboard.read();
            if content.sensitive {
                // A password field: not a selection, and never to be read.
                return None;
            }
            if !content.is_empty() {
                return Some(content);
            }
        }
        if !delays || Instant::now() >= deadline {
            return None;
        }
        sleep(POLL_INTERVAL);
    }
}

/// The real clipboard, with the OS's change counter and secret-marker check
/// (the same two functions the clipboard history uses) plugged in.
pub(crate) struct SystemClipboardCapture {
    pub sequence: fn() -> Option<u64>,
    pub read: fn() -> Result<crate::paste::ClipboardRead>,
}

impl CaptureClipboard for SystemClipboardCapture {
    fn sequence(&self) -> Option<u64> {
        (self.sequence)()
    }

    fn snapshot(&self) -> ClipboardSnapshot {
        crate::clipboard::snapshot()
    }

    fn clear(&self) {
        if let Err(err) = crate::clipboard::clear() {
            tracing::debug!("could not empty the clipboard: {err}");
        }
    }

    fn read(&self) -> CapturedContent {
        // A clipboard another program is holding reads as empty; the poll
        // tries again.
        match (self.read)() {
            Ok(read) if read.sensitive => CapturedContent {
                sensitive: true,
                ..CapturedContent::default()
            },
            Ok(read) => CapturedContent {
                text: read.text,
                files: crate::clipboard::get_files().unwrap_or_default(),
                sensitive: false,
            },
            Err(_) => CapturedContent::default(),
        }
    }

    fn restore(&self, snapshot: &ClipboardSnapshot) {
        if let Err(err) = crate::clipboard::restore(snapshot) {
            tracing::warn!("could not put the clipboard back: {err}");
        }
    }
}

/// The clipboard's current files or text as a selection (the opt-in fallback
/// for systems where nothing can be captured). Reads only; the clipboard is
/// not touched.
pub fn clipboard_selection() -> Option<Selection> {
    let files = crate::clipboard::get_files().unwrap_or_default();
    let text = crate::clipboard::get_text().ok().flatten();
    Selection::from_parts(text, files)
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;

    /// A clipboard and a keyboard in one: pressing copy puts `on_copy` on the
    /// clipboard (bumping the counter) unless there is nothing selected.
    struct Fake {
        clipboard: RefCell<ClipboardSnapshot>,
        sequence: Cell<u64>,
        has_sequence: bool,
        /// What the app copies; `None`: nothing selected.
        on_copy: Option<CapturedContent>,
        /// The copy appears only on the Nth poll after the keystroke.
        sensitive: bool,
        key_error: bool,
        app: Option<ForegroundApp>,
        interrupts_terminals: bool,
        log: RefCell<Vec<&'static str>>,
        polls_before_copy: Cell<u32>,
    }

    impl Fake {
        fn new(on_copy: Option<CapturedContent>) -> Self {
            Self {
                clipboard: RefCell::new(ClipboardSnapshot {
                    text: Some("original".into()),
                    ..ClipboardSnapshot::default()
                }),
                sequence: Cell::new(1),
                has_sequence: true,
                on_copy,
                sensitive: false,
                key_error: false,
                app: Some(ForegroundApp::new("Notepad")),
                interrupts_terminals: true,
                log: RefCell::default(),
                polls_before_copy: Cell::new(0),
            }
        }

        fn text(text: &str) -> CapturedContent {
            CapturedContent {
                text: Some(text.into()),
                ..CapturedContent::default()
            }
        }

        fn run(&self) -> SelectionCapture {
            capture_by_copy(self, self, false)
        }
    }

    impl CaptureClipboard for Fake {
        fn sequence(&self) -> Option<u64> {
            self.has_sequence.then(|| self.sequence.get())
        }
        fn snapshot(&self) -> ClipboardSnapshot {
            self.log.borrow_mut().push("snapshot");
            self.clipboard.borrow().clone()
        }
        fn clear(&self) {
            self.log.borrow_mut().push("clear");
            *self.clipboard.borrow_mut() = ClipboardSnapshot::default();
        }
        fn read(&self) -> CapturedContent {
            self.log.borrow_mut().push("read");
            let clipboard = self.clipboard.borrow();
            CapturedContent {
                text: clipboard.text.clone(),
                files: clipboard.files.clone(),
                sensitive: self.sensitive,
            }
        }
        fn restore(&self, snapshot: &ClipboardSnapshot) {
            self.log.borrow_mut().push("restore");
            *self.clipboard.borrow_mut() = snapshot.clone();
        }
    }

    impl CaptureDriver for Fake {
        fn foreground_app(&self) -> Option<ForegroundApp> {
            self.app.clone()
        }
        fn copy_can_interrupt_terminals(&self) -> bool {
            self.interrupts_terminals
        }
        fn release_modifiers(&self) {
            self.log.borrow_mut().push("release");
        }
        fn press_copy(&self) -> Result<()> {
            self.log.borrow_mut().push("copy");
            if self.key_error {
                return Err(crate::PlatformError::Unsupported("keys"));
            }
            if let Some(copied) = &self.on_copy {
                self.sequence.set(self.sequence.get() + 1);
                *self.clipboard.borrow_mut() = ClipboardSnapshot {
                    text: copied.text.clone(),
                    files: copied.files.clone(),
                    html: None,
                    image: None,
                };
            }
            Ok(())
        }
    }

    fn selected_text(capture: &SelectionCapture) -> Option<String> {
        match capture {
            SelectionCapture::Selected(selection) => selection.text().map(str::to_owned),
            _ => None,
        }
    }

    #[test]
    fn copies_reads_and_restores_in_that_order() {
        let fake = Fake::new(Some(Fake::text("hello")));
        let capture = fake.run();
        assert_eq!(selected_text(&capture).as_deref(), Some("hello"));
        assert_eq!(
            *fake.log.borrow(),
            ["release", "snapshot", "copy", "read", "restore"]
        );
        assert_eq!(fake.clipboard.borrow().text.as_deref(), Some("original"));
    }

    #[test]
    fn an_image_on_the_clipboard_is_put_back_after_the_capture() {
        let image = crate::media::ClipboardImage::new(1, 1, vec![1, 2, 3, 255]).unwrap();
        let fake = Fake::new(Some(Fake::text("hello")));
        *fake.clipboard.borrow_mut() = ClipboardSnapshot {
            image: Some(image.clone()),
            ..ClipboardSnapshot::default()
        };
        assert_eq!(selected_text(&fake.run()).as_deref(), Some("hello"));
        let restored = fake.clipboard.borrow();
        assert_eq!(restored.image.as_ref(), Some(&image));
        assert_eq!(restored.text, None);
    }

    #[test]
    fn files_are_captured_and_win_over_their_names() {
        let fake = Fake::new(Some(CapturedContent {
            text: Some("a.txt".into()),
            files: vec!["/tmp/a.txt".into()],
            sensitive: false,
        }));
        match fake.run() {
            SelectionCapture::Selected(selection) => {
                assert_eq!(selection.files(), [PathBuf::from("/tmp/a.txt")]);
                assert_eq!(selection.text(), None);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn no_selection_means_nothing_and_the_clipboard_is_left_alone() {
        let fake = Fake::new(None);
        assert_eq!(fake.run(), SelectionCapture::Nothing);
        assert_eq!(fake.clipboard.borrow().text.as_deref(), Some("original"));
    }

    #[test]
    fn systems_without_a_change_counter_start_from_an_empty_clipboard() {
        let mut fake = Fake::new(None);
        fake.has_sequence = false;
        // Nothing selected: the stale "original" must not be mistaken for a copy.
        assert_eq!(fake.run(), SelectionCapture::Nothing);
        assert_eq!(
            fake.log.borrow()[..4],
            ["release", "snapshot", "clear", "copy"]
        );
        assert_eq!(fake.clipboard.borrow().text.as_deref(), Some("original"));

        let mut fake = Fake::new(Some(Fake::text("picked")));
        fake.has_sequence = false;
        assert_eq!(selected_text(&fake.run()).as_deref(), Some("picked"));
        assert_eq!(fake.clipboard.borrow().text.as_deref(), Some("original"));
    }

    #[test]
    fn an_originally_empty_clipboard_is_emptied_again() {
        let fake = Fake::new(Some(Fake::text("hello")));
        *fake.clipboard.borrow_mut() = ClipboardSnapshot::default();
        assert!(selected_text(&fake.run()).is_some());
        assert!(fake.clipboard.borrow().is_empty());
    }

    #[test]
    fn a_copy_that_equals_the_old_clipboard_still_counts() {
        // The counter changed, so it is a copy even if the text is the same.
        let fake = Fake::new(Some(Fake::text("original")));
        assert_eq!(selected_text(&fake.run()).as_deref(), Some("original"));
    }

    #[test]
    fn secret_copies_are_not_selections() {
        let mut fake = Fake::new(Some(Fake::text("hunter2")));
        fake.sensitive = true;
        assert_eq!(fake.run(), SelectionCapture::Nothing);
        assert_eq!(fake.clipboard.borrow().text.as_deref(), Some("original"));
    }

    #[test]
    fn blank_copies_are_not_selections() {
        let fake = Fake::new(Some(Fake::text("  \n ")));
        assert_eq!(fake.run(), SelectionCapture::Nothing);
    }

    #[test]
    fn a_failed_keystroke_is_unavailable_and_restores() {
        let mut fake = Fake::new(Some(Fake::text("x")));
        fake.key_error = true;
        match fake.run() {
            SelectionCapture::Unavailable(reason) => assert!(reason.contains("copy"), "{reason}"),
            other => panic!("{other:?}"),
        }
        assert_eq!(fake.clipboard.borrow().text.as_deref(), Some("original"));
        assert_eq!(fake.log.borrow().last(), Some(&"restore"));
    }

    #[test]
    fn terminals_never_get_ctrl_c() {
        for name in [
            "WindowsTerminal",
            "cmd.exe",
            "gnome-terminal-server",
            "Alacritty",
        ] {
            let mut fake = Fake::new(Some(Fake::text("x")));
            fake.app = Some(ForegroundApp::new(name));
            match fake.run() {
                SelectionCapture::Unavailable(reason) => {
                    assert!(reason.contains("terminal"), "{reason}");
                }
                other => panic!("{name}: {other:?}"),
            }
            // Not even the clipboard was touched.
            assert!(fake.log.borrow().is_empty(), "{name}");
        }
    }

    #[test]
    fn terminals_are_fine_where_copy_is_cmd_c() {
        let mut fake = Fake::new(Some(Fake::text("x")));
        fake.app = Some(ForegroundApp::new("Terminal"));
        fake.interrupts_terminals = false;
        assert!(selected_text(&fake.run()).is_some());
    }

    #[test]
    fn terminal_detection_uses_every_name_of_the_app() {
        let app = ForegroundApp::new("Windows Terminal").with_identifier("WindowsTerminal.exe");
        assert!(is_terminal(&app));
        assert!(is_terminal(&ForegroundApp::new("xterm")));
        assert!(!is_terminal(&ForegroundApp::new("Code")));
        assert!(!is_terminal(&ForegroundApp::new("Firefox")));
        assert!(!is_terminal(&ForegroundApp::new("Stella")));
    }

    #[test]
    fn the_history_is_shielded_only_while_the_clipboard_is_borrowed() {
        struct Watch<'a>(&'a Fake, Cell<Vec<bool>>);
        impl CaptureClipboard for Watch<'_> {
            fn sequence(&self) -> Option<u64> {
                self.0.sequence()
            }
            fn snapshot(&self) -> ClipboardSnapshot {
                self.0.snapshot()
            }
            fn clear(&self) {
                self.0.clear();
            }
            fn read(&self) -> CapturedContent {
                let mut seen = self.1.take();
                seen.push(crate::clipboard::synthetic_copy_in_progress());
                self.1.set(seen);
                self.0.read()
            }
            fn restore(&self, snapshot: &ClipboardSnapshot) {
                let mut seen = self.1.take();
                seen.push(crate::clipboard::synthetic_copy_in_progress());
                self.1.set(seen);
                self.0.restore(snapshot);
            }
        }

        let fake = Fake::new(Some(Fake::text("hello")));
        let watch = Watch(&fake, Cell::new(Vec::new()));
        capture_by_copy(&watch, &fake, false);
        // Shielded while reading and while restoring.
        assert_eq!(watch.1.take(), [true, true]);
    }

    #[test]
    fn a_slow_app_is_waited_for() {
        // The first poll finds the counter unchanged; with delays on, the
        // second one finds the copy.
        struct Slow(Fake);
        impl CaptureClipboard for Slow {
            fn sequence(&self) -> Option<u64> {
                // Changes only from the second call after the keystroke.
                let calls = self.0.polls_before_copy.get();
                self.0.polls_before_copy.set(calls + 1);
                if calls < 3 {
                    Some(1)
                } else {
                    Some(2)
                }
            }
            fn snapshot(&self) -> ClipboardSnapshot {
                self.0.snapshot()
            }
            fn clear(&self) {
                self.0.clear();
            }
            fn read(&self) -> CapturedContent {
                Fake::text("late")
            }
            fn restore(&self, snapshot: &ClipboardSnapshot) {
                self.0.restore(snapshot);
            }
        }
        let slow = Slow(Fake::new(None));
        match capture_by_copy(&slow, &slow.0, true) {
            SelectionCapture::Selected(selection) => assert_eq!(selection.text(), Some("late")),
            other => panic!("{other:?}"),
        }
    }
}
