//! What the user typed after your keyword.

use std::str::FromStr;

/// The text after the extension's keyword (`hello Ada` with the keyword `hello`
/// gives `Ada`; the keyword alone gives an empty query).
///
/// ```no_run
/// use sevak_extension_sdk::Query;
///
/// let query = Query::new("convert 5 km  to miles");
/// assert_eq!(query.word(0), Some("convert"));
/// assert_eq!(query.rest_from(1), "5 km  to miles");
/// assert_eq!(query.words().len(), 5);
///
/// let quoted = Query::new(r#"find "my file.txt" in docs"#);
/// assert_eq!(quoted.args(), ["find", "my file.txt", "in", "docs"]);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    raw: String,
}

impl Query {
    /// A query for `raw`, exactly as Sevak sent it.
    pub fn new(raw: impl Into<String>) -> Self {
        Self { raw: raw.into() }
    }

    /// The text exactly as Sevak sent it, spaces included.
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// The text without leading and trailing white space.
    pub fn text(&self) -> &str {
        self.raw.trim()
    }

    /// Whether there is nothing but white space (the user typed only the keyword).
    pub fn is_empty(&self) -> bool {
        self.text().is_empty()
    }

    /// The words, separated by white space.
    pub fn words(&self) -> Vec<&str> {
        self.text().split_whitespace().collect()
    }

    /// The `n`th word (counting from 0).
    pub fn word(&self, n: usize) -> Option<&str> {
        self.text().split_whitespace().nth(n)
    }

    /// The text after the first `n` words, as typed (inner spacing kept); an
    /// empty string when there are no more.
    pub fn rest_from(&self, n: usize) -> &str {
        let mut rest = self.text();
        for _ in 0..n {
            rest = match rest.find(char::is_whitespace) {
                Some(end) => rest[end..].trim_start(),
                None => "",
            };
        }
        rest
    }

    /// The first word and everything after it: `("convert", "5 km to miles")`.
    /// Both parts are empty for an empty query.
    pub fn split_first(&self) -> (&str, &str) {
        (self.word(0).unwrap_or(""), self.rest_from(1))
    }

    /// The words, where a `"quoted phrase"` is one word (the quotes are not
    /// part of it). An unclosed quote runs to the end of the text.
    pub fn args(&self) -> Vec<String> {
        let mut args = Vec::new();
        let mut current = String::new();
        let mut in_quotes = false;
        let mut started = false;
        for ch in self.text().chars() {
            match ch {
                '"' => {
                    in_quotes = !in_quotes;
                    started = true;
                }
                c if c.is_whitespace() && !in_quotes => {
                    if started {
                        args.push(std::mem::take(&mut current));
                        started = false;
                    }
                }
                c => {
                    current.push(c);
                    started = true;
                }
            }
        }
        if started {
            args.push(current);
        }
        args
    }

    /// The whole text parsed as `T` (`query.parse::<f64>()`); `None` when it
    /// does not parse.
    pub fn parse<T: FromStr>(&self) -> Option<T> {
        self.text().parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_blank_queries() {
        for raw in ["", "   ", "\t\n"] {
            let query = Query::new(raw);
            assert!(query.is_empty(), "{raw:?}");
            assert!(query.words().is_empty());
            assert_eq!(query.split_first(), ("", ""));
            assert_eq!(query.rest_from(3), "");
        }
    }

    #[test]
    fn text_trims_but_raw_does_not() {
        let query = Query::new("  hello world \n");
        assert_eq!(query.raw(), "  hello world \n");
        assert_eq!(query.text(), "hello world");
    }

    #[test]
    fn words_and_rest() {
        let query = Query::new("  a  b   c d ");
        assert_eq!(query.words(), ["a", "b", "c", "d"]);
        assert_eq!(query.word(2), Some("c"));
        assert_eq!(query.word(4), None);
        assert_eq!(query.rest_from(0), "a  b   c d");
        assert_eq!(query.rest_from(2), "c d");
        assert_eq!(query.rest_from(4), "");
        assert_eq!(query.split_first(), ("a", "b   c d"));
        assert_eq!(Query::new("solo").split_first(), ("solo", ""));
    }

    #[test]
    fn unicode_words() {
        let query = Query::new("naïve café 日本語");
        assert_eq!(query.words(), ["naïve", "café", "日本語"]);
        assert_eq!(query.rest_from(2), "日本語");
    }

    #[test]
    fn quoted_arguments() {
        assert_eq!(
            Query::new(r#"a "b c" d"#).args(),
            ["a", "b c", "d"],
            "a quoted phrase is one argument"
        );
        assert_eq!(
            Query::new(r#""" x"#).args(),
            ["", "x"],
            "an empty phrase is kept"
        );
        assert_eq!(
            Query::new(r#"a "b c"#).args(),
            ["a", "b c"],
            "unclosed quote"
        );
        assert_eq!(Query::new(r#"pre"fix suf"fix"#).args(), ["prefix suffix"]);
        assert!(Query::new("   ").args().is_empty());
    }

    #[test]
    fn parsing_numbers() {
        assert_eq!(Query::new(" 42 ").parse::<u32>(), Some(42));
        assert_eq!(Query::new("4.5").parse::<f64>(), Some(4.5));
        assert_eq!(Query::new("x").parse::<u32>(), None);
    }
}
