//! The data shared between plugins, the platform layer and the UI shell.
//!
//! Everything here is plain data: serializable, OS-agnostic, and free of
//! behavior beyond small constructors.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// One row in the result list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResultItem {
    /// Stable across queries and restarts; usage statistics are keyed by it.
    /// Convention: `<plugin id>:<plugin-specific key>`, e.g. `app:firefox.desktop`.
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub icon: Option<IconSource>,
    /// Relevance, higher is better. Fuzzy results use nucleo's scale (roughly
    /// 16-20 points per matched character plus bonuses); see [`score`] for the
    /// constants non-fuzzy plugins use. The engine adds usage boosts on top.
    pub score: f64,
    pub plugin_id: String,
    pub action: Action,
    /// What Tab turns the search input into when this row is selected, if the
    /// plugin offers one (a keyword to keep typing after, a folder to drill
    /// into). Relative to the plugin's own input: when the row came from a
    /// keyword route the engine prefixes the typed keyword.
    #[serde(default)]
    pub autocomplete: Option<String>,
}

impl ResultItem {
    pub fn new(
        plugin_id: impl Into<String>,
        key: impl AsRef<str>,
        title: impl Into<String>,
        action: Action,
    ) -> Self {
        let plugin_id = plugin_id.into();
        Self {
            id: format!("{plugin_id}:{}", key.as_ref()),
            title: title.into(),
            subtitle: String::new(),
            icon: None,
            score: 0.0,
            plugin_id,
            action,
            autocomplete: None,
        }
    }

    #[must_use]
    pub fn with_subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = subtitle.into();
        self
    }

    #[must_use]
    pub fn with_icon(mut self, icon: IconSource) -> Self {
        self.icon = Some(icon);
        self
    }

    #[must_use]
    pub fn with_score(mut self, score: f64) -> Self {
        self.score = score;
        self
    }

    /// Sets the text Tab completes the input to (see [`ResultItem::autocomplete`]).
    #[must_use]
    pub fn with_autocomplete(mut self, text: impl Into<String>) -> Self {
        self.autocomplete = Some(text.into());
        self
    }
}

/// Score constants for results that do not come from fuzzy matching.
pub mod score {
    /// A result that answers the query outright (e.g. a calculation).
    pub const EXACT_ANSWER: f64 = 10_000.0;
    /// A result produced because the user typed the plugin's keyword.
    pub const KEYWORD: f64 = 5_000.0;
    /// Shown only when nothing else matched.
    pub const FALLBACK: f64 = 0.0;
}

/// What happens when a result is activated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    Launch {
        target: LaunchTarget,
    },
    OpenPath {
        path: PathBuf,
    },
    OpenUrl {
        url: String,
    },
    CopyText {
        text: String,
    },
    /// Plugin-defined; only the owning plugin's `execute` understands it.
    Custom {
        payload: String,
    },
}

/// An installed application, as discovered by the platform layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppEntry {
    /// Stable identifier: the desktop-file id on Linux (`org.mozilla.firefox.desktop`),
    /// the AppUserModelID for packaged Windows apps, or a normalized shortcut
    /// path relative to its Start Menu root for `.lnk` entries.
    pub id: String,
    pub name: String,
    /// `Comment` / `GenericName` / shortcut description, if any.
    pub description: Option<String>,
    /// Extra search terms (desktop `Keywords`, `GenericName`, executable stem).
    pub keywords: Vec<String>,
    pub icon: Option<IconSource>,
    pub target: LaunchTarget,
}

/// How to start an application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LaunchTarget {
    /// A Windows `.lnk` file, launched through the shell so its arguments,
    /// working directory and MSI "advertised" targets all keep working.
    Shortcut { path: PathBuf },
    /// A packaged (UWP / Store / MSIX) app, launched via `shell:AppsFolder\<aumid>`.
    PackagedApp { app_user_model_id: String },
    /// A freedesktop `.desktop` entry.
    DesktopEntry {
        desktop_id: String,
        path: PathBuf,
        /// `Exec` split into argv with field codes (`%f %u %F %U %i %c %k`) removed.
        exec: Vec<String>,
        terminal: bool,
        working_dir: Option<PathBuf>,
    },
    /// A plain executable.
    Executable {
        path: PathBuf,
        args: Vec<String>,
        working_dir: Option<PathBuf>,
    },
}

/// Where a result's icon comes from. The shell turns this into image bytes
/// (via the platform provider) when the UI asks for it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IconSource {
    /// An image on disk (png, svg or ico).
    File { path: PathBuf },
    /// A Windows shell item (file path or `shell:AppsFolder\<aumid>`) whose
    /// icon is extracted on demand.
    Shell { parsing_name: String },
    /// A glyph bundled with the UI: `app`, `calculator`, `web`, `file`,
    /// `folder`, `copy`, `plugin`.
    Builtin { name: String },
}

impl IconSource {
    pub fn builtin(name: &str) -> Self {
        Self::Builtin {
            name: name.to_owned(),
        }
    }
}

/// Encoded image bytes ready to serve to the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconData {
    /// `image/png`, `image/svg+xml` or `image/x-icon`.
    pub mime: &'static str,
    pub bytes: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_id_is_namespaced_by_plugin() {
        let item = ResultItem::new(
            "app",
            "firefox.desktop",
            "Firefox",
            Action::OpenUrl {
                url: "https://example.com".into(),
            },
        );
        assert_eq!(item.id, "app:firefox.desktop");
        assert_eq!(item.plugin_id, "app");
    }

    #[test]
    fn autocomplete_is_optional() {
        let item = ResultItem::new("p", "k", "t", Action::CopyText { text: "x".into() });
        assert_eq!(item.autocomplete, None);
        assert_eq!(
            item.with_autocomplete("g ").autocomplete.as_deref(),
            Some("g ")
        );
    }

    #[test]
    fn action_serializes_with_type_tag() {
        let json = serde_json::to_string(&Action::CopyText { text: "4".into() }).unwrap();
        assert_eq!(json, r#"{"type":"copy_text","text":"4"}"#);
    }
}
