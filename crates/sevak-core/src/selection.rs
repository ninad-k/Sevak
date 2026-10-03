//! What the user had selected in another app (Universal Actions).
//!
//! A [`Selection`] is the text and/or the files and folders that were selected
//! when the Universal Actions hotkey was pressed. It only ever lives in memory:
//! it has no serializer, and its `Debug` output leaves the content out, so it
//! cannot reach a log or a file by accident.

use std::fmt;
use std::path::PathBuf;

/// Text longer than this many bytes is not acted on: pasting a transformed
/// copy of megabytes back over a selection is more likely a mistake than a wish.
pub const MAX_SELECTION_BYTES: usize = 256 * 1024;

/// Most URLs or paths offered one by one for a multi-item selection.
pub const MAX_LISTED_ITEMS: usize = 5;

/// What was selected in the other app.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Selection {
    /// The selected text, if any. Never empty or blank when set.
    text: Option<String>,
    /// Selected files and folders (a file manager's selection). Wins over text.
    files: Vec<PathBuf>,
}

// By hand: the content stays out of anything that formats a Selection.
impl fmt::Debug for Selection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Selection")
            .field("text_bytes", &self.text.as_deref().map_or(0, str::len))
            .field("files", &self.files.len())
            .finish()
    }
}

/// How a selection is acted on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionKind {
    /// Files and folders.
    Files,
    /// One or more URLs, one per line.
    Urls,
    /// Anything else that is text.
    Text,
}

impl Selection {
    /// A text selection; `None` for blank text.
    pub fn from_text(text: impl Into<String>) -> Option<Self> {
        let text = text.into();
        (!text.trim().is_empty()).then_some(Self {
            text: Some(text),
            files: Vec::new(),
        })
    }

    /// A selection of files and folders; `None` for an empty list.
    pub fn from_files(files: Vec<PathBuf>) -> Option<Self> {
        (!files.is_empty()).then_some(Self { text: None, files })
    }

    /// Files win over text: a file manager that also offers the names as text
    /// is still showing files.
    pub fn from_parts(text: Option<String>, files: Vec<PathBuf>) -> Option<Self> {
        Self::from_files(files).or_else(|| text.and_then(Self::from_text))
    }

    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }

    /// True if the text is longer than [`MAX_SELECTION_BYTES`].
    pub fn is_too_large(&self) -> bool {
        self.text
            .as_deref()
            .is_some_and(|t| t.len() > MAX_SELECTION_BYTES)
    }

    pub fn kind(&self) -> SelectionKind {
        if !self.files.is_empty() {
            SelectionKind::Files
        } else if self.urls().is_some() {
            SelectionKind::Urls
        } else {
            SelectionKind::Text
        }
    }

    /// The URLs, when every non-blank line of the text is one (at most 50
    /// lines). Normalized: a bare `www.` address gets `https://`.
    pub fn urls(&self) -> Option<Vec<String>> {
        if !self.files.is_empty() {
            return None;
        }
        let urls: Vec<String> = self
            .text
            .as_deref()?
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(as_url)
            .collect::<Option<_>>()?;
        (!urls.is_empty() && urls.len() <= 50).then_some(urls)
    }

    /// Short human description for the action panel's heading.
    pub fn describe(&self) -> String {
        match self.kind() {
            SelectionKind::Files => match self.files.len() {
                1 => "Selected file or folder".to_owned(),
                n => format!("{n} selected files and folders"),
            },
            SelectionKind::Urls => match self.urls().map_or(0, |u| u.len()) {
                1 => "Selected URL".to_owned(),
                n => format!("{n} selected URLs"),
            },
            SelectionKind::Text => {
                let text = self.text.as_deref().unwrap_or_default();
                let lines = text.lines().filter(|l| !l.trim().is_empty()).count();
                if lines > 1 {
                    format!("Selected text ({lines} lines)")
                } else {
                    "Selected text".to_owned()
                }
            }
        }
    }
}

/// `line` as a URL Sevak can open: `http(s)://` or `mailto:` with something
/// after it and no whitespace, or a bare `www.` address.
fn as_url(line: &str) -> Option<String> {
    if line.chars().any(char::is_whitespace) {
        return None;
    }
    let lower = line.to_ascii_lowercase();
    for scheme in ["http://", "https://", "mailto:"] {
        if lower.starts_with(scheme) {
            return (line.len() > scheme.len()).then(|| line.to_owned());
        }
    }
    let host = lower.strip_prefix("www.")?;
    (host.contains('.') && !host.starts_with('.') && !host.ends_with('.'))
        .then(|| format!("https://{line}"))
}

/// `text` collapsed to one line and cut to `max` characters with an ellipsis,
/// for titles and previews.
pub fn preview(text: &str, max: usize) -> String {
    let mut out = String::new();
    let mut count = 0;
    let mut pending_space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            count += 1;
            pending_space = false;
        }
        if count >= max {
            out.push('\u{2026}');
            return out;
        }
        out.push(ch);
        count += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(t: &str) -> Selection {
        Selection::from_text(t).unwrap()
    }

    #[test]
    fn blank_selections_do_not_exist() {
        assert_eq!(Selection::from_text("  \n\t"), None);
        assert_eq!(Selection::from_files(Vec::new()), None);
        assert_eq!(Selection::from_parts(Some(String::new()), Vec::new()), None);
    }

    #[test]
    fn files_win_over_text() {
        let selection =
            Selection::from_parts(Some("a.txt".into()), vec![PathBuf::from("/tmp/a.txt")]).unwrap();
        assert_eq!(selection.kind(), SelectionKind::Files);
        assert_eq!(selection.text(), None);
        assert_eq!(selection.files(), [PathBuf::from("/tmp/a.txt")]);
    }

    #[test]
    fn classifies_urls_text_and_multiline_text() {
        assert_eq!(text("hello world").kind(), SelectionKind::Text);
        assert_eq!(text("line one\nline two").kind(), SelectionKind::Text);
        assert_eq!(
            text("https://example.com/a?b=c").kind(),
            SelectionKind::Urls
        );
        assert_eq!(text("  HTTP://Example.com \n").kind(), SelectionKind::Urls);
        assert_eq!(text("mailto:me@example.com").kind(), SelectionKind::Urls);
        assert_eq!(
            text("https://a.test\n\nhttps://b.test").urls().unwrap(),
            ["https://a.test", "https://b.test"]
        );
        // One non-URL line makes the whole selection text.
        assert_eq!(text("https://a.test\nnotes").kind(), SelectionKind::Text);
        // Whitespace inside, a bare scheme, other schemes.
        assert_eq!(text("https://a.test is great").kind(), SelectionKind::Text);
        assert_eq!(text("https://").kind(), SelectionKind::Text);
        assert_eq!(text("ftp://a.test").kind(), SelectionKind::Text);
        assert_eq!(text("javascript:alert(1)").kind(), SelectionKind::Text);
    }

    #[test]
    fn bare_www_addresses_get_a_scheme() {
        assert_eq!(
            text("www.example.com/x").urls().unwrap(),
            ["https://www.example.com/x"]
        );
        assert_eq!(text("www.").kind(), SelectionKind::Text);
        assert_eq!(text("www.com").kind(), SelectionKind::Text);
        assert_eq!(text("www").kind(), SelectionKind::Text);
    }

    #[test]
    fn too_many_urls_are_text() {
        let many = (0..51)
            .map(|i| format!("https://a.test/{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(text(&many).kind(), SelectionKind::Text);
    }

    #[test]
    fn size_limit() {
        assert!(!text("small").is_too_large());
        assert!(text(&"x".repeat(MAX_SELECTION_BYTES + 1)).is_too_large());
    }

    #[test]
    fn debug_output_leaves_the_content_out() {
        let shown = format!("{:?}", text("my secret password"));
        assert!(!shown.contains("secret"), "{shown}");
        let files = Selection::from_files(vec![PathBuf::from("/home/me/taxes.pdf")]).unwrap();
        assert!(!format!("{files:?}").contains("taxes"));
    }

    #[test]
    fn descriptions() {
        assert_eq!(text("a").describe(), "Selected text");
        assert_eq!(text("a\nb\n\nc").describe(), "Selected text (3 lines)");
        assert_eq!(text("https://a.test").describe(), "Selected URL");
        assert_eq!(
            text("https://a.test\nhttps://b.test").describe(),
            "2 selected URLs"
        );
        let two = Selection::from_files(vec!["/a".into(), "/b".into()]).unwrap();
        assert_eq!(two.describe(), "2 selected files and folders");
    }

    #[test]
    fn previews_collapse_and_truncate() {
        assert_eq!(preview("  a \n\t b  ", 10), "a b");
        assert_eq!(preview("abcdefghij", 10), "abcdefghij");
        assert_eq!(preview("abcdefghijk", 10), "abcdefghij\u{2026}");
        assert_eq!(preview("h\u{e9}llo w\u{f6}rld", 7), "h\u{e9}llo w\u{2026}");
        assert_eq!(preview("", 5), "");
    }
}
