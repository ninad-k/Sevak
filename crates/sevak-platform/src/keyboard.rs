//! Watching what is typed, for expanding snippet keywords as you type.
//!
//! This is the one place where Sevak observes keystrokes in other apps, and it
//! is opt-in (`[snippets] auto_expand`). The OS-specific listeners (`windows`,
//! `macos` and `linux` modules) turn raw key events into the three-way
//! [`KeyEvent`] below and nothing else:
//!
//! - they translate with the layout of the app being typed into, so a key is
//!   reported as the character it produced, never as a key code;
//! - they report a character only for plain typing. Anything that moves the
//!   caret, edits the text some other way or starts a shortcut (arrows, Enter,
//!   Escape, Tab, Delete, Ctrl/Alt/Win/Cmd combinations, mouse clicks, a change
//!   of focus) is a [`KeyEvent::Reset`], after which the caller forgets what it
//!   has seen;
//! - they ignore the events Sevak injects itself.
//!
//! What the caller does with the events, and how little it keeps, is the
//! caller's business (see `sevak_plugins::snippet_expansion`); nothing here
//! stores, logs or sends a character, and [`KeyEvent`]'s `Debug` output hides it.

use std::fmt;

use crate::paste::ForegroundApp;

/// One thing the user did at the keyboard (or mouse).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum KeyEvent {
    /// A key typed this character.
    Char(char),
    /// Backspace: the character before the caret is gone.
    Backspace,
    /// The text before the caret can no longer be assumed to be what was typed.
    Reset,
}

// By hand: the derived output would put what the user typed into a log line.
impl fmt::Debug for KeyEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Char(_) => f.write_str("Char(..)"),
            Self::Backspace => f.write_str("Backspace"),
            Self::Reset => f.write_str("Reset"),
        }
    }
}

/// Receives the events, on a thread of the listener's. It must return quickly
/// (the OS gives key hooks a few hundred milliseconds before it drops them), so
/// the usual implementation just sends the event down a channel.
pub type KeySink = Box<dyn Fn(KeyEvent) + Send + 'static>;

/// A running keyboard listener. Dropping it stops the listener and releases
/// the OS hook.
pub struct KeyListener {
    stop: Option<Box<dyn FnOnce() + Send>>,
}

impl KeyListener {
    /// A listener that runs `stop` when dropped. For platform backends (and fakes).
    pub fn new(stop: impl FnOnce() + Send + 'static) -> Self {
        Self {
            stop: Some(Box::new(stop)),
        }
    }
}

impl Drop for KeyListener {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop();
        }
    }
}

impl fmt::Debug for KeyListener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("KeyListener")
    }
}

/// Whether keystrokes can be observed on this system right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyListenerSupport {
    Available,
    /// It could work once the user grants a permission; the text says which.
    NeedsPermission(String),
    /// It cannot work here (Wayland, no X server); the text says why.
    Unavailable(String),
}

impl KeyListenerSupport {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }
}

/// Reason shown where keystrokes cannot be observed for lack of any
/// implementation.
pub const UNSUPPORTED_REASON: &str = "Watching the keyboard is not supported on this system";

/// Where the next typed character would land.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TypingTarget {
    /// The app in front, if it can be told.
    pub app: Option<ForegroundApp>,
    /// The window is one of Sevak's own (the launcher, Settings).
    pub own_window: bool,
    /// The focused control hides what is typed into it (a password box), or the
    /// OS has switched keyboard events to secure mode.
    pub private: bool,
}

/// The diacritics that dead keys type, with the letters they combine with and
/// what comes out, so a letter typed after a dead key can still be followed.
/// Not every combination the OS knows: an unlisted pair is simply "unknown",
/// which makes the caller reset.
const ACCENTS: &[(&[char], &str, &str)] = &[
    (&['´', '\''], "aeiouycnszAEIOUYCNSZ", "áéíóúýćńśźÁÉÍÓÚÝĆŃŚŹ"),
    (&['`'], "aeiouAEIOU", "àèìòùÀÈÌÒÙ"),
    (&['^'], "aeiouAEIOU", "âêîôûÂÊÎÔÛ"),
    (&['~'], "anoANO", "ãñõÃÑÕ"),
    (&['¨', '"'], "aeiouyAEIOU", "äëïöüÿÄËÏÖÜ"),
    (&['°', '˚'], "auAU", "åůÅŮ"),
    (&['¸'], "cCsS", "çÇşŞ"),
];

/// The character typed by a dead key (`dead`, the spacing form of the
/// diacritic) followed by `next`: `'´'` then `'e'` is `'é'`, then a space is the
/// accent itself. `None` if the pair is not one we know.
pub fn compose_dead_key(dead: char, next: char) -> Option<char> {
    if next == ' ' {
        return Some(dead);
    }
    let (_, bases, composed) = ACCENTS
        .iter()
        .find(|(spacing, _, _)| spacing.contains(&dead))?;
    let index = bases.chars().position(|base| base == next)?;
    composed.chars().nth(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_never_shows_the_typed_character() {
        assert_eq!(format!("{:?}", KeyEvent::Char('x')), "Char(..)");
        assert_eq!(format!("{:?}", KeyEvent::Backspace), "Backspace");
        assert_eq!(format!("{:?}", KeyEvent::Reset), "Reset");
    }

    #[test]
    fn dead_keys_compose_with_their_letters() {
        assert_eq!(compose_dead_key('´', 'e'), Some('é'));
        assert_eq!(compose_dead_key('\'', 'E'), Some('É'));
        assert_eq!(compose_dead_key('`', 'a'), Some('à'));
        assert_eq!(compose_dead_key('^', 'o'), Some('ô'));
        assert_eq!(compose_dead_key('~', 'n'), Some('ñ'));
        assert_eq!(compose_dead_key('¨', 'u'), Some('ü'));
        assert_eq!(compose_dead_key('"', 'u'), Some('ü'));
        assert_eq!(compose_dead_key('¸', 'c'), Some('ç'));
    }

    #[test]
    fn a_dead_key_then_space_types_the_accent_itself() {
        assert_eq!(compose_dead_key('´', ' '), Some('´'));
        assert_eq!(compose_dead_key('^', ' '), Some('^'));
    }

    #[test]
    fn an_unknown_pair_is_unknown() {
        assert_eq!(compose_dead_key('´', 'x'), None);
        assert_eq!(compose_dead_key('x', 'e'), None);
        assert_eq!(compose_dead_key('´', '1'), None);
    }

    #[test]
    fn the_accent_tables_line_up() {
        for (spacing, bases, composed) in ACCENTS {
            assert!(!spacing.is_empty());
            assert_eq!(bases.chars().count(), composed.chars().count());
        }
    }

    #[test]
    fn dropping_a_listener_stops_it_once() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let stops = Arc::new(AtomicUsize::new(0));
        let counter = stops.clone();
        let listener = KeyListener::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(stops.load(Ordering::SeqCst), 0);
        drop(listener);
        assert_eq!(stops.load(Ordering::SeqCst), 1);
    }
}
