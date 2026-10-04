//! Result items and the closed set of actions.
//!
//! The JSON these types produce is exactly what Sevak's script-plugin protocol
//! documents (`docs/plugins.md`, "Items"). Sevak validates it again on arrival;
//! this module makes the common mistakes hard to write and cuts text to the
//! lengths Sevak keeps so nothing is silently dropped on the other side.

use std::fmt;

use serde::Serialize;
use serde_json::{json, Map, Value};

/// The most items one answer holds; Sevak keeps no more.
pub const MAX_ITEMS: usize = 50;
const MAX_TITLE_CHARS: usize = 200;
const MAX_SUBTITLE_CHARS: usize = 300;
const MAX_KEY_CHARS: usize = 200;
const MAX_TEXT_CHARS: usize = 100_000;
const MAX_GLYPH_CHARS: usize = 16;
/// Sevak clamps a script's score just below the score of its own keyword rows.
const MAX_SCORE: f64 = 4999.0;

/// A row's icon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Icon {
    /// One of Sevak's own glyphs: `app`, `calculator`, `web`, `file`, `folder`,
    /// `copy`, `plugin`.
    Builtin { name: String },
    /// An image inside the extension's folder (`png`, `svg`, `ico`, `jpg`,
    /// `webp`), as a relative path with `/` separators. Paths that leave the
    /// folder are refused by Sevak.
    File { path: String },
}

impl Icon {
    /// One of Sevak's own glyphs.
    pub fn builtin(name: impl Into<String>) -> Self {
        Self::Builtin { name: name.into() }
    }

    /// An image file inside the extension's folder.
    pub fn file(path: impl Into<String>) -> Self {
        Self::File { path: path.into() }
    }
}

/// How an application is started, for [`Action::Launch`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LaunchTarget {
    /// A Windows `.lnk` shortcut.
    Shortcut { path: String },
    /// A Windows Store (UWP/MSIX) app by its application user model id.
    PackagedApp { app_user_model_id: String },
    /// A freedesktop `.desktop` entry.
    DesktopEntry {
        desktop_id: String,
        path: String,
        exec: Vec<String>,
        terminal: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        working_dir: Option<String>,
    },
    /// A plain executable. A relative `path` is relative to the extension's
    /// folder.
    Executable {
        path: String,
        args: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        working_dir: Option<String>,
    },
}

impl LaunchTarget {
    /// A plain executable with no arguments.
    pub fn executable(path: impl Into<String>) -> Self {
        Self::Executable {
            path: path.into(),
            args: Vec::new(),
            working_dir: None,
        }
    }
}

/// What happens when the user picks a row. This is Sevak's **closed** set of
/// extension actions: Sevak performs them itself, an extension cannot ask for
/// anything else, and a row with an action outside the set is dropped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    /// Copies text to the clipboard.
    CopyText { text: String },
    /// Opens a link in the browser or mail client. Only `http://`, `https://`
    /// and `mailto:` links are allowed.
    OpenUrl { url: String },
    /// Opens a file or folder with its default program. A relative path is
    /// relative to the extension's folder.
    OpenPath { path: String },
    /// Sends the payload back to the extension as an [`Execute`] message (only
    /// for persistent extensions; handle it with [`Extension::on_execute`]).
    ///
    /// [`Execute`]: crate::Execute
    /// [`Extension::on_execute`]: crate::Extension::on_execute
    Custom { payload: String },
    /// Starts an application. Sevak drops the row unless the manifest declares
    /// `capabilities = ["launch"]` (the user sees that when allowing the
    /// extension).
    Launch { target: LaunchTarget },
}

impl Action {
    /// Copies `text`.
    pub fn copy_text(text: impl Into<String>) -> Self {
        Self::CopyText { text: text.into() }
    }

    /// Opens `url` (`http`, `https` or `mailto` only).
    pub fn open_url(url: impl Into<String>) -> Self {
        Self::OpenUrl { url: url.into() }
    }

    /// Opens `path`.
    pub fn open_path(path: impl Into<String>) -> Self {
        Self::OpenPath { path: path.into() }
    }

    /// Hands `payload` back to the extension.
    pub fn custom(payload: impl Into<String>) -> Self {
        Self::Custom {
            payload: payload.into(),
        }
    }

    /// Starts `target` (needs the `launch` capability).
    pub fn launch(target: LaunchTarget) -> Self {
        Self::Launch { target }
    }
}

/// Something wrong with an [`Item`] that Sevak would silently cut or drop.
/// [`Item::problems`] lists them; the SDK prints them to stderr in debug builds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    message: String,
}

impl Problem {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// What is wrong, as a sentence.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// One row in the results list.
///
/// ```
/// use sevak_extension_sdk::{Action, Icon, Item};
///
/// let item = Item::new("Rust")
///     .key("rust")
///     .subtitle("The Rust programming language")
///     .icon(Icon::builtin("web"))
///     .action(Action::open_url("https://www.rust-lang.org"));
/// assert!(item.problems().is_empty());
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    key: Option<String>,
    title: String,
    subtitle: String,
    icon: Option<Icon>,
    action: Option<Action>,
    score: Option<f64>,
    /// `Some("text")` or `Some("grid")`.
    view: Option<&'static str>,
    text: Option<String>,
    glyph: Option<String>,
}

impl Item {
    /// A row showing `title`. With no action, Enter copies the title.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            key: None,
            title: title.into(),
            subtitle: String::new(),
            icon: None,
            action: None,
            score: None,
            view: None,
            text: None,
            glyph: None,
        }
    }

    /// A stable id for this row within your extension (defaults to the title).
    /// Sevak's usage statistics are keyed by it, so name what the row *is*
    /// (`project-sevak`), not volatile data (a timestamp).
    #[must_use]
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// The grey line under the title.
    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = subtitle.into();
        self
    }

    /// The row's icon.
    #[must_use]
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// What Enter does.
    #[must_use]
    pub fn action(mut self, action: Action) -> Self {
        self.action = Some(action);
        self
    }

    /// Enter copies the title (the default when no action is set; this says so
    /// in the code).
    #[must_use]
    pub fn copy_on_enter(self) -> Self {
        let text = self.title.clone();
        self.action(Action::copy_text(text))
    }

    /// The row's rank, `0.0` to `4999.0`; higher is higher. Optional: rows
    /// without one keep the order you return them in.
    #[must_use]
    pub fn score(mut self, score: f64) -> Self {
        self.score = Some(score);
        self
    }

    /// Long text for Sevak's Text View (Ctrl+T on the row). With no action set,
    /// Enter opens the view and copying the row copies this text.
    #[must_use]
    pub fn text_view(mut self, text: impl Into<String>) -> Self {
        self.view = Some("text");
        self.text = Some(text.into());
        self
    }

    /// Shows the row as a grid tile with `glyph` (an emoji or a few characters)
    /// as its picture. When every row of an answer is a tile, Sevak shows a grid.
    #[must_use]
    pub fn tile(mut self, glyph: impl Into<String>) -> Self {
        self.view = Some("grid");
        self.glyph = Some(glyph.into());
        self
    }

    /// Everything about this row that Sevak would cut or drop; empty when the
    /// row goes through unchanged.
    pub fn problems(&self) -> Vec<Problem> {
        let mut problems = Vec::new();
        let title = self.title.trim();
        if title.is_empty() {
            problems.push(Problem::new("the title is empty, so Sevak drops the row"));
        }
        if title.chars().count() > MAX_TITLE_CHARS {
            problems.push(Problem::new(format!(
                "the title is longer than {MAX_TITLE_CHARS} characters and is cut"
            )));
        }
        if self.subtitle.trim().chars().count() > MAX_SUBTITLE_CHARS {
            problems.push(Problem::new(format!(
                "the subtitle is longer than {MAX_SUBTITLE_CHARS} characters and is cut"
            )));
        }
        if let Some(score) = self.score {
            if !score.is_finite() {
                problems.push(Problem::new(
                    "the score is not a finite number and is ignored",
                ));
            } else if !(0.0..=MAX_SCORE).contains(&score) {
                problems.push(Problem::new(format!(
                    "the score {score} is outside 0 to {MAX_SCORE} and is clamped"
                )));
            }
        }
        match &self.action {
            Some(Action::OpenUrl { url }) if !is_web_or_mail_link(url) => {
                problems.push(Problem::new(
                    "only http://, https:// and mailto: links can be opened; Sevak drops the row",
                ));
            }
            Some(Action::Launch { .. }) => problems.push(Problem::new(
                "starting applications needs capabilities = [\"launch\"] in plugin.toml; \
                 Sevak drops the row without it",
            )),
            _ => {}
        }
        problems
    }

    /// The row as Sevak's JSON, text cut to the lengths Sevak keeps.
    pub(crate) fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert(
            "title".into(),
            json!(truncate(&self.title, MAX_TITLE_CHARS)),
        );
        if let Some(key) = &self.key {
            map.insert("key".into(), json!(truncate(key, MAX_KEY_CHARS)));
        }
        if !self.subtitle.trim().is_empty() {
            map.insert(
                "subtitle".into(),
                json!(truncate(&self.subtitle, MAX_SUBTITLE_CHARS)),
            );
        }
        if let Some(icon) = &self.icon {
            map.insert("icon".into(), json!(icon));
        }
        if let Some(action) = &self.action {
            map.insert("action".into(), json!(action));
        }
        if let Some(score) = self.score.filter(|score| score.is_finite()) {
            map.insert("score".into(), json!(score.clamp(0.0, MAX_SCORE)));
        }
        if let Some(view) = self.view {
            map.insert("view".into(), json!(view));
        }
        if let Some(text) = &self.text {
            map.insert(
                "text".into(),
                json!(text.chars().take(MAX_TEXT_CHARS).collect::<String>()),
            );
        }
        if let Some(glyph) = &self.glyph {
            map.insert(
                "glyph".into(),
                json!(glyph.chars().take(MAX_GLYPH_CHARS).collect::<String>()),
            );
        }
        Value::Object(map)
    }
}

/// The text Sevak accepts as a link to open.
fn is_web_or_mail_link(url: &str) -> bool {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    ["http://", "https://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
        && !url.chars().any(char::is_control)
}

/// `text` trimmed and cut to `max_chars` characters.
fn truncate(text: &str, max_chars: usize) -> String {
    text.trim().chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_minimal_item_is_just_a_title() {
        assert_eq!(Item::new("Hi").to_value(), json!({"title": "Hi"}));
    }

    #[test]
    fn every_field_has_the_documented_shape() {
        let item = Item::new("Rust")
            .key("rust")
            .subtitle("A language")
            .icon(Icon::builtin("web"))
            .action(Action::open_url("https://www.rust-lang.org"))
            .score(120.0);
        assert_eq!(
            item.to_value(),
            json!({
                "title": "Rust",
                "key": "rust",
                "subtitle": "A language",
                "icon": {"kind": "builtin", "name": "web"},
                "action": {"type": "open_url", "url": "https://www.rust-lang.org"},
                "score": 120.0
            })
        );
    }

    #[test]
    fn actions_serialize_as_sevaks_tagged_objects() {
        let value = |action: Action| serde_json::to_value(action).unwrap();
        assert_eq!(
            value(Action::copy_text("x")),
            json!({"type": "copy_text", "text": "x"})
        );
        assert_eq!(
            value(Action::open_path("notes/a.txt")),
            json!({"type": "open_path", "path": "notes/a.txt"})
        );
        assert_eq!(
            value(Action::custom("save:1")),
            json!({"type": "custom", "payload": "save:1"})
        );
        assert_eq!(
            value(Action::launch(LaunchTarget::executable("bin/tool"))),
            json!({"type": "launch", "target": {"kind": "executable", "path": "bin/tool", "args": []}})
        );
        assert_eq!(
            serde_json::to_value(Icon::file("icon.png")).unwrap(),
            json!({"kind": "file", "path": "icon.png"})
        );
    }

    #[test]
    fn views_carry_their_text_and_glyph() {
        assert_eq!(
            Item::new("Long").text_view("a\nb").to_value(),
            json!({"title": "Long", "view": "text", "text": "a\nb"})
        );
        assert_eq!(
            Item::new("fire").tile("🔥").to_value(),
            json!({"title": "fire", "view": "grid", "glyph": "🔥"})
        );
    }

    #[test]
    fn copy_on_enter_copies_the_title() {
        assert_eq!(
            Item::new("42").copy_on_enter().to_value()["action"],
            json!({"type": "copy_text", "text": "42"})
        );
    }

    #[test]
    fn text_is_cut_to_the_lengths_sevak_keeps() {
        let long = "x".repeat(1000);
        let value = Item::new(long.clone())
            .subtitle(long.clone())
            .key(long)
            .to_value();
        assert_eq!(value["title"].as_str().unwrap().chars().count(), 200);
        assert_eq!(value["subtitle"].as_str().unwrap().chars().count(), 300);
        assert_eq!(value["key"].as_str().unwrap().chars().count(), 200);
        // Cutting is by character, never in the middle of one.
        let wide = Item::new("é".repeat(300)).to_value();
        assert_eq!(wide["title"].as_str().unwrap().chars().count(), 200);
    }

    #[test]
    fn scores_are_clamped_and_non_finite_scores_dropped() {
        assert_eq!(
            Item::new("a").score(99999.0).to_value()["score"],
            json!(4999.0)
        );
        assert_eq!(Item::new("a").score(-5.0).to_value()["score"], json!(0.0));
        assert!(Item::new("a")
            .score(f64::NAN)
            .to_value()
            .get("score")
            .is_none());
    }

    #[test]
    fn problems_name_what_sevak_would_cut_or_drop() {
        assert!(Item::new("fine").copy_on_enter().problems().is_empty());
        assert_eq!(Item::new("  ").problems().len(), 1);
        let long = Item::new("x".repeat(201)).problems();
        assert!(long[0].message().contains("200"), "{long:?}");
        let bad_link = Item::new("a").action(Action::open_url("file:///etc/passwd"));
        assert!(bad_link.problems()[0].message().contains("http"));
        let mail = Item::new("a").action(Action::open_url("mailto:me@example.com"));
        assert!(mail.problems().is_empty());
        let launch = Item::new("a").action(Action::launch(LaunchTarget::executable("x")));
        assert!(launch.problems()[0].message().contains("launch"));
        assert!(!Item::new("a").score(5000.0).problems().is_empty());
        assert!(Item::new("a").score(f64::INFINITY).problems()[0]
            .to_string()
            .contains("finite"));
    }

    #[test]
    fn links_with_control_characters_are_flagged() {
        let item = Item::new("a").action(Action::open_url("https://example.com/\nx"));
        assert_eq!(item.problems().len(), 1);
    }
}
