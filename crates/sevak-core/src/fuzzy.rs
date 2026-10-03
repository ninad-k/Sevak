//! Fuzzy matching shared by every plugin, built on `nucleo-matcher` (the
//! matching engine behind the `nucleo` crate).
//!
//! The full `nucleo` crate adds a threaded, incremental worker meant for
//! hundreds of thousands of items. Sevak's indexes hold a few thousand at most,
//! which the bare matcher scores synchronously in well under a millisecond, so
//! the worker would only add latency and threads.

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

/// A parsed query plus a reusable matcher. Create one per query and score every
/// candidate with it; creation allocates, scoring does not.
pub struct FuzzyQuery {
    pattern: Pattern,
    matcher: Matcher,
    buf: Vec<char>,
}

impl FuzzyQuery {
    /// Smart case (lowercase input matches any case) and Unicode normalization
    /// (`e` matches `é`). Whitespace separates terms that must all match.
    pub fn new(query: &str) -> Self {
        Self {
            pattern: Pattern::parse(query, CaseMatching::Smart, Normalization::Smart),
            matcher: Matcher::new(Config::DEFAULT),
            buf: Vec::new(),
        }
    }

    /// Like [`FuzzyQuery::new`], but tuned for file paths (path separators get
    /// a bonus as word boundaries).
    pub fn for_paths(query: &str) -> Self {
        let mut query = Self::new(query);
        query.matcher = Matcher::new(Config::DEFAULT.match_paths());
        query
    }

    pub fn is_empty(&self) -> bool {
        self.pattern.atoms.is_empty()
    }

    /// Score of `haystack` against the query, or `None` if it does not match.
    pub fn score(&mut self, haystack: &str) -> Option<u32> {
        let haystack = Utf32Str::new(haystack, &mut self.buf);
        self.pattern.score(haystack, &mut self.matcher)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subsequence_matches_and_non_matches() {
        let mut query = FuzzyQuery::new("fir");
        assert!(query.score("Firefox").is_some());
        assert!(query.score("Files").is_none());
    }

    #[test]
    fn prefix_beats_scattered_match() {
        let mut query = FuzzyQuery::new("code");
        let prefix = query.score("Code - OSS").unwrap();
        let scattered = query.score("Color Dialog Editor").unwrap_or(0);
        assert!(prefix > scattered);
    }

    #[test]
    fn smart_case_and_normalization() {
        let mut query = FuzzyQuery::new("cafe");
        assert!(query.score("Café Menu").is_some());
        let mut query = FuzzyQuery::new("Cafe");
        assert!(query.score("cafe").is_none());
    }

    #[test]
    fn empty_query_is_empty() {
        assert!(FuzzyQuery::new("   ").is_empty());
        assert!(!FuzzyQuery::new("a").is_empty());
    }
}
