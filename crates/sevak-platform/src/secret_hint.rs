//! The "this clipboard content is a secret" hint of Linux password managers.
//!
//! KeePassXC (and the other managers that follow KDE's convention) put one more
//! target next to the text they copy: the MIME type
//! `x-kde-passwordManagerHint`, whose value is `secret`. Clipboard managers on
//! KDE and GNOME honour it by not keeping the copy. This module holds the
//! decision; the `linux` module reads the targets from X11 or, on Wayland,
//! through `wl-paste` when it is installed.
//!
//! It is best effort and depends on the source app setting the hint: a password
//! copied from an app that does not set it looks like any other text. The rules:
//!
//! - no hint among the clipboard's targets: not marked;
//! - the hint is there and its value is `secret` (any case, surrounding white
//!   space ignored): marked;
//! - the hint is there but its value cannot be read (the owner did not answer in
//!   time): marked, because an app that advertises the hint wants it respected;
//! - the hint is there with another value (such as `public`): not marked.
//!
//! Everything is decided over the small [`ClipboardTargets`] trait so the logic
//! is tested with fakes, without a display server.

// Used by the Linux backends only; the logic is built (and tested) everywhere.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

/// The target (MIME type) KDE's convention uses.
pub(crate) const HINT_TARGET: &str = "x-kde-passwordManagerHint";

/// What the hint's value must be for the content to count as secret.
const SECRET_VALUE: &str = "secret";

/// The clipboard owner's advertised targets and their data.
pub(crate) trait ClipboardTargets {
    /// Whether the clipboard offers `target`.
    fn has_target(&self, target: &str) -> bool;
    /// The bytes of `target`, or `None` if they could not be read.
    fn read_target(&self, target: &str) -> Option<Vec<u8>>;
}

/// True if the value of the hint says "secret".
pub(crate) fn hint_value_is_secret(value: &[u8]) -> bool {
    std::str::from_utf8(value).is_ok_and(|text| {
        text.trim_matches(|c: char| c.is_whitespace() || c == '\0')
            .eq_ignore_ascii_case(SECRET_VALUE)
    })
}

/// Whether the clipboard's content is marked secret (see the module
/// documentation for the rules).
pub(crate) fn marked_secret(source: &dyn ClipboardTargets) -> bool {
    if !source.has_target(HINT_TARGET) {
        return false;
    }
    source
        .read_target(HINT_TARGET)
        .is_none_or(|value| hint_value_is_secret(&value))
}

/// True if the output of `wl-paste --list-types` (one MIME type per line)
/// offers the hint.
pub(crate) fn lists_hint(output: &str) -> bool {
    output.lines().any(|line| line.trim() == HINT_TARGET)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    struct Fake {
        targets: Vec<&'static str>,
        value: Option<&'static [u8]>,
        reads: Cell<usize>,
    }

    impl Fake {
        fn new(targets: &[&'static str], value: Option<&'static [u8]>) -> Self {
            Self {
                targets: targets.to_vec(),
                value,
                reads: Cell::new(0),
            }
        }
    }

    impl ClipboardTargets for Fake {
        fn has_target(&self, target: &str) -> bool {
            self.targets.contains(&target)
        }
        fn read_target(&self, _target: &str) -> Option<Vec<u8>> {
            self.reads.set(self.reads.get() + 1);
            self.value.map(<[u8]>::to_vec)
        }
    }

    #[test]
    fn a_secret_hint_marks_the_content() {
        let fake = Fake::new(&["UTF8_STRING", HINT_TARGET], Some(b"secret"));
        assert!(marked_secret(&fake));
    }

    #[test]
    fn no_hint_means_not_secret_and_nothing_is_read() {
        let fake = Fake::new(&["UTF8_STRING", "text/plain"], Some(b"secret"));
        assert!(!marked_secret(&fake));
        assert_eq!(fake.reads.get(), 0, "the data is not even requested");
    }

    #[test]
    fn another_value_is_not_secret() {
        for value in [&b"public"[..], b"", b"1", b"secrets"] {
            let fake = Fake::new(&[HINT_TARGET], Some(value));
            assert!(!marked_secret(&fake), "{value:?}");
        }
    }

    #[test]
    fn the_value_is_read_loosely() {
        for value in [&b"SECRET"[..], b"secret\n", b" Secret ", b"secret\0"] {
            assert!(hint_value_is_secret(value), "{value:?}");
        }
        assert!(!hint_value_is_secret(&[0xff, 0xfe]));
    }

    #[test]
    fn an_advertised_hint_that_cannot_be_read_counts_as_secret() {
        let fake = Fake::new(&[HINT_TARGET], None);
        assert!(marked_secret(&fake));
    }

    #[test]
    fn wl_paste_listings_are_read_line_by_line() {
        assert!(lists_hint(
            "text/plain;charset=utf-8\ntext/plain\nx-kde-passwordManagerHint\n"
        ));
        assert!(lists_hint("x-kde-passwordManagerHint\r\n"));
        assert!(!lists_hint("text/plain\nx-kde-passwordManagerHint-not\n"));
        assert!(!lists_hint(""));
    }
}
