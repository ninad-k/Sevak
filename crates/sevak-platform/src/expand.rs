//! Replacing text the user just typed with other text, in the app they are
//! typing into (snippet expansion).
//!
//! The order of operations is the same everywhere and lives here: wait for the
//! user to let go of any modifier key, save the clipboard, put the new text on
//! it, press Backspace once per character to remove, press paste, wait for the
//! app to read the clipboard and put it back. The OS-specific half (the key
//! presses) is the `windows`, `macos` and `linux` modules'.
//!
//! This is [`crate::paste`] without the part that returns to a remembered
//! window: the app typed into is already in front.

use std::thread::sleep;
use std::time::Duration;

use crate::clipboard::{self, ClipboardSnapshot, SyntheticCopy};
use crate::error::{PlatformError, Result};

/// Gap between the Backspaces and the paste, so the app has processed the
/// deletion before the new text arrives.
const SETTLE: Duration = Duration::from_millis(80);
/// Gap after the paste before the old clipboard is put back, so the app has
/// read the pasted text (slow apps read it asynchronously).
const RESTORE_DELAY: Duration = Duration::from_millis(300);

/// The OS half of a replacement.
pub(crate) trait ExpandDriver {
    /// Waits for the user to release Shift/Ctrl/Alt/Win, so they do not join
    /// the keys pressed below (Ctrl+Shift+V, Shift+Backspace).
    fn release_modifiers(&self);
    /// Presses and releases Backspace `count` times.
    fn press_backspaces(&self, count: usize) -> Result<()>;
    /// Presses Ctrl+V / Cmd+V.
    fn press_paste(&self) -> Result<()>;
    /// An identity for the window (or app) in front, to tell that it changed:
    /// the keys go to whoever has the focus when they are pressed. `None` if it
    /// cannot be told.
    fn foreground_token(&self) -> Option<u64> {
        None
    }
}

/// The clipboard operations a replacement needs; a trait so the order of
/// operations can be tested without touching the real clipboard.
pub(crate) trait ExpandClipboard {
    fn snapshot(&self) -> ClipboardSnapshot;
    /// Replaces the text, asking the OS to keep it out of its own history.
    fn set_private(&self, text: &str) -> Result<()>;
    fn restore(&self, snapshot: &ClipboardSnapshot);
    /// The clipboard's change counter, where the system has one.
    fn sequence(&self) -> Option<u64>;
    /// The clipboard's text.
    fn text(&self) -> Option<String>;
}

pub(crate) struct SystemExpandClipboard;

impl ExpandClipboard for SystemExpandClipboard {
    fn snapshot(&self) -> ClipboardSnapshot {
        clipboard::snapshot()
    }

    fn set_private(&self, text: &str) -> Result<()> {
        clipboard::set_text_private(text)
    }

    fn restore(&self, snapshot: &ClipboardSnapshot) {
        if let Err(err) = clipboard::restore(snapshot) {
            tracing::warn!("could not put the clipboard back: {err}");
        }
    }

    fn sequence(&self) -> Option<u64> {
        clipboard::sequence()
    }

    fn text(&self) -> Option<String> {
        clipboard::get_text().ok().flatten()
    }
}

/// Removes the last `delete` typed characters and pastes `text` in their place.
/// `Ok(true)` if it did, `Ok(false)` if `still_current` said the user had typed
/// something since the keyword was seen (then nothing is deleted: the Backspaces
/// would remove the wrong characters).
///
/// If the clipboard cannot be set nothing is deleted. If a key press fails
/// after that, the text stays on the clipboard (the old contents are not put
/// back), so the user can finish by hand. `delays` is false in tests.
pub(crate) fn replace_typed_text(
    delete: usize,
    text: &str,
    still_current: &dyn Fn() -> bool,
    clipboard: &dyn ExpandClipboard,
    driver: &dyn ExpandDriver,
    delays: bool,
) -> Result<bool> {
    // Until the clipboard is back as it was, the history must not see `text`.
    let _borrowed = SyntheticCopy::begin();

    // The window the keyword was typed into. Keys are only sent while it is
    // still in front.
    let origin = driver.foreground_token();
    let moved = || origin.is_some() && driver.foreground_token() != origin;

    driver.release_modifiers();

    let saved = clipboard.snapshot();
    clipboard.set_private(text)?;
    let written = clipboard.sequence();
    // Whether the clipboard still holds `text`, and not something copied since.
    let still_ours = || {
        clipboard::still_ours(
            written,
            clipboard.sequence(),
            || clipboard.text(),
            Some(text),
        )
    };
    let put_back = || {
        if still_ours() {
            clipboard.restore(&saved);
        } else {
            tracing::debug!("the clipboard changed during the expansion; it was not restored");
        }
    };

    // The last look before pressing keys: waiting for the modifiers and the
    // clipboard took a moment, in which typing may have gone on.
    if !still_current() || moved() {
        put_back();
        return Ok(false);
    }

    if delete > 0 {
        driver.press_backspaces(delete)?;
        if delays {
            sleep(SETTLE);
        }
    }
    // The last look before the paste. The Backspaces are done and cannot be
    // taken back, but the text must not land in another window: it stays on the
    // clipboard for the user to paste.
    if moved() {
        return Err(PlatformError::Os {
            operation: "paste",
            message:
                "the window in front changed during the expansion; the text is on the clipboard"
                    .to_owned(),
        });
    }
    driver.press_paste()?;

    if delays {
        sleep(RESTORE_DELAY);
    }
    put_back();
    Ok(true)
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;
    use crate::PlatformError;

    #[derive(Default)]
    struct Fake {
        clipboard: RefCell<ClipboardSnapshot>,
        log: RefCell<Vec<String>>,
        clipboard_error: bool,
        backspace_error: bool,
        paste_error: bool,
        sequence: Cell<u64>,
        no_counter: bool,
        /// Somebody copies this while the paste is being handled.
        copied_meanwhile: Option<String>,
        /// What `foreground_token` answers, one entry per call (the last one
        /// repeats).
        tokens: RefCell<Vec<Option<u64>>>,
    }

    impl ExpandClipboard for Fake {
        fn snapshot(&self) -> ClipboardSnapshot {
            self.clipboard.borrow().clone()
        }
        fn sequence(&self) -> Option<u64> {
            (!self.no_counter).then(|| self.sequence.get())
        }
        fn text(&self) -> Option<String> {
            self.clipboard.borrow().text.clone()
        }
        fn set_private(&self, text: &str) -> Result<()> {
            self.sequence.set(self.sequence.get() + 1);
            self.log.borrow_mut().push(format!("set {text}"));
            if self.clipboard_error {
                return Err(PlatformError::Unsupported("clipboard"));
            }
            self.clipboard.borrow_mut().text = Some(text.to_owned());
            Ok(())
        }
        fn restore(&self, snapshot: &ClipboardSnapshot) {
            self.log.borrow_mut().push("restore".into());
            *self.clipboard.borrow_mut() = snapshot.clone();
        }
    }

    impl ExpandDriver for Fake {
        fn release_modifiers(&self) {
            self.log.borrow_mut().push("release".into());
        }
        fn press_backspaces(&self, count: usize) -> Result<()> {
            self.log.borrow_mut().push(format!("backspace {count}"));
            if self.backspace_error {
                Err(PlatformError::Unsupported("keys"))
            } else {
                Ok(())
            }
        }
        fn foreground_token(&self) -> Option<u64> {
            let mut tokens = self.tokens.borrow_mut();
            if tokens.len() > 1 {
                tokens.remove(0)
            } else {
                tokens.first().copied().flatten()
            }
        }
        fn press_paste(&self) -> Result<()> {
            self.log.borrow_mut().push("paste".into());
            if let Some(newer) = &self.copied_meanwhile {
                self.clipboard.borrow_mut().text = Some(newer.clone());
                self.sequence.set(self.sequence.get() + 1);
            }
            if self.paste_error {
                Err(PlatformError::Unsupported("keys"))
            } else {
                Ok(())
            }
        }
    }

    fn run_if(fake: &Fake, delete: usize, current: bool) -> Result<bool> {
        replace_typed_text(
            delete,
            "expanded",
            &|| current,
            fake as &dyn ExpandClipboard,
            fake as &dyn ExpandDriver,
            false,
        )
    }

    fn run(fake: &Fake, delete: usize) -> Result<bool> {
        run_if(fake, delete, true)
    }

    #[test]
    fn typing_that_went_on_cancels_the_replacement() {
        let fake = with_clipboard("old");
        assert!(!run_if(&fake, 4, false).unwrap());
        // Nothing was deleted or pasted, and the clipboard is as it was.
        assert_eq!(*fake.log.borrow(), ["release", "set expanded", "restore"]);
        assert_eq!(fake.clipboard.borrow().text.as_deref(), Some("old"));
    }

    fn with_clipboard(text: &str) -> Fake {
        let fake = Fake::default();
        fake.clipboard.borrow_mut().text = Some(text.into());
        fake
    }

    #[test]
    fn deletes_then_pastes_then_restores_the_clipboard() {
        let fake = with_clipboard("old");
        assert!(run(&fake, 4).unwrap());
        assert_eq!(
            *fake.log.borrow(),
            ["release", "set expanded", "backspace 4", "paste", "restore"]
        );
        assert_eq!(fake.clipboard.borrow().text.as_deref(), Some("old"));
    }

    #[test]
    fn a_copy_made_during_the_expansion_is_not_overwritten_by_the_restore() {
        for no_counter in [false, true] {
            let fake = Fake {
                copied_meanwhile: Some("newer".into()),
                no_counter,
                ..with_clipboard("old")
            };
            assert!(run(&fake, 4).unwrap());
            assert_eq!(
                fake.clipboard.borrow().text.as_deref(),
                Some("newer"),
                "no_counter: {no_counter}"
            );
            assert!(!fake.log.borrow().contains(&"restore".to_owned()));
        }
    }

    fn with_tokens(tokens: &[Option<u64>]) -> Fake {
        Fake {
            tokens: RefCell::new(tokens.to_vec()),
            ..with_clipboard("old")
        }
    }

    #[test]
    fn a_window_change_before_any_key_cancels_the_replacement() {
        // The keyword was typed in window 1; by the time keys would go out,
        // window 2 has the focus.
        let fake = with_tokens(&[Some(1), Some(2)]);
        assert!(!run(&fake, 4).unwrap());
        assert_eq!(*fake.log.borrow(), ["release", "set expanded", "restore"]);
        assert_eq!(fake.clipboard.borrow().text.as_deref(), Some("old"));
    }

    #[test]
    fn a_window_change_after_the_backspaces_keeps_the_text_for_pasting_by_hand() {
        let fake = with_tokens(&[Some(1), Some(1), Some(2)]);
        let err = run(&fake, 4).unwrap_err();
        assert!(err.to_string().contains("window"), "{err}");
        // No paste keystroke reached the new window, and the text is on the
        // clipboard.
        assert_eq!(
            *fake.log.borrow(),
            ["release", "set expanded", "backspace 4"]
        );
        assert_eq!(fake.clipboard.borrow().text.as_deref(), Some("expanded"));
    }

    #[test]
    fn the_same_window_throughout_expands_and_an_unknown_one_is_not_checked() {
        for tokens in [vec![Some(7)], vec![None]] {
            let fake = Fake {
                tokens: RefCell::new(tokens),
                ..with_clipboard("old")
            };
            assert!(run(&fake, 4).unwrap());
        }
    }

    #[test]
    fn nothing_to_delete_means_no_backspaces() {
        let fake = Fake::default();
        assert!(run(&fake, 0).unwrap());
        assert_eq!(
            *fake.log.borrow(),
            ["release", "set expanded", "paste", "restore"]
        );
    }

    #[test]
    fn an_empty_clipboard_is_emptied_again() {
        let fake = Fake::default();
        assert!(run(&fake, 1).unwrap());
        assert!(fake.clipboard.borrow().is_empty());
    }

    #[test]
    fn a_clipboard_that_cannot_be_set_deletes_nothing() {
        let fake = Fake {
            clipboard_error: true,
            ..Fake::default()
        };
        assert!(run(&fake, 3).is_err());
        assert!(!fake
            .log
            .borrow()
            .iter()
            .any(|op| op.starts_with("backspace")));
    }

    #[test]
    fn a_failed_keystroke_leaves_the_text_on_the_clipboard() {
        for (backspace_error, paste_error) in [(true, false), (false, true)] {
            let fake = Fake {
                backspace_error,
                paste_error,
                ..with_clipboard("old")
            };
            assert!(run(&fake, 2).is_err());
            assert_eq!(fake.clipboard.borrow().text.as_deref(), Some("expanded"));
            assert!(!fake.log.borrow().contains(&"restore".to_owned()));
        }
    }
}
