//! What the OS knows about words: the answer type of the OS spell checker.
//!
//! Definitions come back as plain text from
//! [`PlatformProvider::system_definition`](crate::PlatformProvider::system_definition);
//! spelling answers use [`Spelling`]. The OS back ends are `macos/dictionary.rs`
//! (Dictionary Services) and `windows/spell.rs` (`ISpellChecker`).

/// The OS spell checker's verdict on one word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spelling {
    /// The word is spelled correctly in the checker's language.
    pub correct: bool,
    /// Suggested replacements, best first; empty for a correct word.
    pub suggestions: Vec<String>,
}
