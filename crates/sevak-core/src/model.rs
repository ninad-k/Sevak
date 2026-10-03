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
    /// What Enter does.
    pub action: Action,
    /// Other things the user can do with this result, shown in the action
    /// panel and run with a modifier + Enter. Empty for most plugins; add with
    /// [`ResultItem::with_secondary`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secondary: Vec<SecondaryAction>,
    /// What Tab turns the search input into when this row is selected, if the
    /// plugin offers one (a keyword to keep typing after, a folder to drill
    /// into). Relative to the plugin's own input: when the row came from a
    /// keyword route the engine prefixes the typed keyword.
    #[serde(default)]
    pub autocomplete: Option<String>,
    /// What the preview pane (Shift / Ctrl+Y) shows for this row when what its
    /// action refers to is not enough. `None` for most plugins: the shell then
    /// derives the preview from the action (a file's contents, a URL, the text
    /// to copy or paste). Add with [`ResultItem::with_preview`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<PreviewHint>,
    /// How the UI should present this row: as a tile in a grid, or with a
    /// long text for the Text View. `None` is an ordinary list row. Add with
    /// [`ResultItem::with_view`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<ViewHint>,
    /// What Large Type (Ctrl+L) shows for this row instead of its title: a
    /// phone number for a contact, say. `None` shows the title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub large_text: Option<String>,
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
            secondary: Vec::new(),
            autocomplete: None,
            preview: None,
            view: None,
            large_text: None,
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

    /// Sets what the preview pane shows (see [`ResultItem::preview`]).
    #[must_use]
    pub fn with_preview(mut self, preview: PreviewHint) -> Self {
        self.preview = Some(preview);
        self
    }

    /// Sets how the row is presented (see [`ResultItem::view`]).
    #[must_use]
    pub fn with_view(mut self, view: ViewHint) -> Self {
        self.view = Some(view);
        self
    }

    /// Shows this row as a tile in the Grid View, with `glyph` (an emoji, a
    /// short symbol) as the tile's picture; without one the row's icon is the
    /// picture. The title is the tile's label.
    #[must_use]
    pub fn as_tile(self, glyph: Option<&str>) -> Self {
        self.with_view(ViewHint::Grid {
            glyph: glyph.map(str::to_owned),
        })
    }

    /// Whether this row asks to be a tile in the Grid View.
    pub fn is_tile(&self) -> bool {
        matches!(self.view, Some(ViewHint::Grid { .. }))
    }

    /// Sets the text Large Type shows (see [`ResultItem::large_text`]).
    #[must_use]
    pub fn with_large_text(mut self, text: impl Into<String>) -> Self {
        self.large_text = Some(text.into());
        self
    }

    /// Adds a secondary action. `modifier` is the key held with Enter to run it
    /// directly; the action panel lists every secondary action, with or
    /// without one. Give each modifier to at most one action.
    #[must_use]
    pub fn with_secondary(
        mut self,
        label: impl Into<String>,
        modifier: Option<Modifier>,
        action: Action,
    ) -> Self {
        self.secondary.push(SecondaryAction {
            label: label.into(),
            modifier,
            action,
        });
        self
    }

    /// This item with its `index`th secondary action as its action (and no
    /// secondary actions): what the owning plugin's `execute` and
    /// `confirmation` see when that action is picked. `None` for an unknown
    /// index.
    pub fn secondary_as_primary(&self, index: usize) -> Option<ResultItem> {
        let secondary = self.secondary.get(index)?;
        let mut derived = self.clone();
        derived.action = secondary.action.clone();
        derived.secondary.clear();
        Some(derived)
    }

    /// The text most worth copying from this result: the calculator's value, a
    /// file or application path, a URL. `None` when there is nothing sensible
    /// (a packaged Windows app, a plugin-defined action).
    pub fn copy_text(&self) -> Option<String> {
        self.action.copy_text()
    }
}

/// What the preview pane shows for a result (see [`ResultItem::preview`]).
///
/// Paths in a hint are never read on the UI's say-so: the shell reads only
/// what the result it holds refers to, with size limits (see
/// [`crate::preview`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PreviewHint {
    /// Text, shown as it is (monospace, scrollable).
    Text { text: String },
    /// A file or folder: its contents, a picture, a listing or its details.
    Path { path: PathBuf },
    /// A link: the address and title only. Nothing is fetched from the network.
    Url { url: String, title: Option<String> },
    /// Labelled facts: a calculation's expression and result, an emoji's name.
    Details { rows: Vec<(String, String)> },
}

/// How a result wants to be presented (see [`ResultItem::view`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ViewHint {
    /// A long text for the scrollable Text View (Ctrl+T), instead of the
    /// one-line row. With `on_enter`, Enter opens the view rather than running
    /// the action: for rows that exist only to show text, such as a script's
    /// output.
    Text { text: String, on_enter: bool },
    /// A tile in the Grid View, drawn when every result of a search is a tile.
    /// `glyph` is the picture (an emoji); without one the row's icon is drawn.
    Grid { glyph: Option<String> },
}

/// A modifier key held together with Enter to pick a secondary action. The UI
/// maps `Ctrl` to Cmd on macOS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modifier {
    Ctrl,
    Shift,
    Alt,
}

/// An alternative way to act on a result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SecondaryAction {
    /// Shown in the action panel: "Show in folder", "Copy path".
    pub label: String,
    /// Runs this action from the result list with `<modifier>+Enter`.
    pub modifier: Option<Modifier>,
    pub action: Action,
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
    /// Types `text` into the app that had focus before Sevak opened (copy,
    /// hide Sevak, refocus that app, Ctrl+V / Cmd+V). Where pasting is not
    /// possible the text is only copied; see `PlatformProvider::paste_support`.
    PasteText {
        text: String,
        /// Put the clipboard's previous text back afterwards (`[paste]`).
        restore_clipboard: bool,
    },
    /// Puts an image or a list of files on the clipboard and pastes it into the
    /// app that had focus before Sevak opened, like [`Action::PasteText`] does
    /// for text.
    PasteClip {
        content: ClipContent,
        /// Put the clipboard's previous text back afterwards (`[paste]`).
        restore_clipboard: bool,
    },
    /// Puts an image or a list of files on the clipboard, without pasting.
    CopyClip {
        content: ClipContent,
    },
    /// Plugin-defined; only the owning plugin's `execute` understands it.
    Custom {
        payload: String,
    },
    /// Shows a file or folder selected in the system file manager.
    RevealPath {
        path: PathBuf,
    },
    /// Starts an application with administrator rights (Windows only; plugins
    /// offer it only when the platform reports support for it).
    RunAsAdmin {
        target: LaunchTarget,
    },
}

impl Action {
    /// The text most worth copying for this action; see [`ResultItem::copy_text`].
    /// For `PasteText` that is the text it would paste. A plugin may carry a
    /// template there (snippets expand `{date}` when run), so copy through the
    /// plugin ([`crate::SearchEngine::copy`]) rather than putting this text on
    /// the clipboard directly.
    pub fn copy_text(&self) -> Option<String> {
        match self {
            Self::CopyText { text } | Self::PasteText { text, .. } => Some(text.clone()),
            Self::OpenUrl { url } => Some(url.clone()),
            Self::OpenPath { path } | Self::RevealPath { path } => {
                Some(path.to_string_lossy().into_owned())
            }
            Self::Launch { target } | Self::RunAsAdmin { target } => target
                .path()
                .map(|path| path.to_string_lossy().into_owned()),
            Self::PasteClip { content, .. } | Self::CopyClip { content } => content.copy_text(),
            Self::Custom { .. } => None,
        }
    }
}

/// Something other than text that can be put on the clipboard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClipContent {
    /// A PNG file whose pixels become the clipboard's image.
    Image { path: PathBuf },
    /// Files and folders, as a file manager's "copy" leaves them.
    Files { paths: Vec<PathBuf> },
}

impl ClipContent {
    /// The text a plain Ctrl+C copies for this content: the paths of the
    /// files, one per line. An image has no useful text.
    pub fn copy_text(&self) -> Option<String> {
        match self {
            Self::Image { .. } => None,
            Self::Files { paths } if paths.is_empty() => None,
            Self::Files { paths } => Some(
                paths
                    .iter()
                    .map(|path| path.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        }
    }
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

impl LaunchTarget {
    /// The file or bundle a user would look for in a file manager: the
    /// shortcut, `.desktop` entry, executable or macOS `.app`; none for
    /// packaged apps. macOS apps launch as `open -a <bundle>`, so for those the
    /// bundle (the command's argument) is returned rather than `open`.
    pub fn path(&self) -> Option<&std::path::Path> {
        match self {
            Self::Executable { args, .. } if args.len() == 2 && args[0] == "-a" => {
                Some(std::path::Path::new(&args[1]))
            }
            Self::Shortcut { path }
            | Self::DesktopEntry { path, .. }
            | Self::Executable { path, .. } => Some(path),
            Self::PackagedApp { .. } => None,
        }
    }
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
    /// `folder`, `copy`, `terminal`, `plugin`, `lock`, `sleep`, `restart`,
    /// `power`, `logout`, `trash`, `settings`.
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
    fn new_items_have_no_secondary_actions() {
        let item = ResultItem::new("p", "k", "T", Action::CopyText { text: "x".into() });
        assert!(item.secondary.is_empty());
        // Payloads without the field still deserialize.
        let json = serde_json::to_string(&item).unwrap();
        assert!(!json.contains("secondary"));
        let back: ResultItem = serde_json::from_str(&json).unwrap();
        assert_eq!(back, item);
    }

    #[test]
    fn secondary_actions_keep_their_order_and_modifier() {
        let item = ResultItem::new("p", "k", "T", Action::CopyText { text: "x".into() })
            .with_secondary(
                "Reveal",
                Some(Modifier::Ctrl),
                Action::RevealPath { path: "/a".into() },
            )
            .with_secondary("Copy", None, Action::CopyText { text: "y".into() });
        assert_eq!(item.secondary.len(), 2);
        assert_eq!(item.secondary[0].modifier, Some(Modifier::Ctrl));
        assert_eq!(item.secondary[1].label, "Copy");
        let json = serde_json::to_string(&item.secondary[0]).unwrap();
        assert!(json.contains(r#""modifier":"ctrl""#), "{json}");
        assert!(json.contains(r#""type":"reveal_path""#), "{json}");

        let derived = item.secondary_as_primary(1).unwrap();
        assert_eq!(derived.action, Action::CopyText { text: "y".into() });
        assert!(derived.secondary.is_empty());
        assert_eq!(derived.id, item.id);
        assert_eq!(item.secondary_as_primary(2), None);
    }

    #[test]
    fn copy_text_picks_the_most_useful_text() {
        let text = |action: Action| ResultItem::new("p", "k", "T", action).copy_text();
        assert_eq!(
            text(Action::CopyText { text: "8".into() }).as_deref(),
            Some("8")
        );
        assert_eq!(
            text(Action::PasteText {
                text: "hi".into(),
                restore_clipboard: true
            })
            .as_deref(),
            Some("hi")
        );
        assert_eq!(
            text(Action::OpenUrl {
                url: "https://a.b/?q=1".into()
            })
            .as_deref(),
            Some("https://a.b/?q=1")
        );
        assert_eq!(
            text(Action::OpenPath {
                path: "/tmp/x".into()
            })
            .as_deref(),
            Some("/tmp/x")
        );
        assert_eq!(
            text(Action::Launch {
                target: LaunchTarget::Shortcut {
                    path: "/s/App.lnk".into()
                }
            })
            .as_deref(),
            Some("/s/App.lnk")
        );
        assert_eq!(
            text(Action::Launch {
                target: LaunchTarget::PackagedApp {
                    app_user_model_id: "x!y".into()
                }
            }),
            None
        );
        assert_eq!(
            text(Action::Custom {
                payload: "p".into()
            }),
            None
        );
    }

    #[test]
    fn preview_and_view_hints_are_optional_and_round_trip() {
        let plain = ResultItem::new("p", "k", "T", Action::CopyText { text: "x".into() });
        assert_eq!((&plain.preview, &plain.view), (&None, &None));
        let json = serde_json::to_string(&plain).unwrap();
        assert!(
            !json.contains("preview") && !json.contains("view"),
            "{json}"
        );

        let rich = plain
            .with_preview(PreviewHint::Details {
                rows: vec![("Name".into(), "grinning face".into())],
            })
            .as_tile(Some("😀"));
        assert!(rich.is_tile());
        let json = serde_json::to_string(&rich).unwrap();
        assert!(json.contains(r#""kind":"details""#), "{json}");
        assert!(json.contains(r#""kind":"grid""#), "{json}");
        assert_eq!(serde_json::from_str::<ResultItem>(&json).unwrap(), rich);

        let text = ResultItem::new("p", "k", "T", Action::CopyText { text: "x".into() }).with_view(
            ViewHint::Text {
                text: "long".into(),
                on_enter: true,
            },
        );
        assert!(!text.is_tile());
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

    #[test]
    fn clip_actions_copy_the_file_paths_but_not_an_image() {
        let image = ClipContent::Image {
            path: "/c/1.png".into(),
        };
        let files = ClipContent::Files {
            paths: vec!["/a/x.txt".into(), "/a/y.txt".into()],
        };
        assert_eq!(image.copy_text(), None);
        assert_eq!(files.copy_text().as_deref(), Some("/a/x.txt\n/a/y.txt"));
        assert_eq!(ClipContent::Files { paths: vec![] }.copy_text(), None);

        let paste = Action::PasteClip {
            content: files,
            restore_clipboard: false,
        };
        assert_eq!(paste.copy_text().as_deref(), Some("/a/x.txt\n/a/y.txt"));
        assert_eq!(Action::CopyClip { content: image }.copy_text(), None);
    }

    #[test]
    fn clip_actions_serialize_with_type_tags() {
        let action = Action::CopyClip {
            content: ClipContent::Image {
                path: "a.png".into(),
            },
        };
        assert_eq!(
            serde_json::to_string(&action).unwrap(),
            r#"{"type":"copy_clip","content":{"kind":"image","path":"a.png"}}"#
        );
    }

    #[test]
    fn paste_action_serializes_with_type_tag() {
        let action = Action::PasteText {
            text: "hi".into(),
            restore_clipboard: true,
        };
        assert_eq!(
            serde_json::to_string(&action).unwrap(),
            r#"{"type":"paste_text","text":"hi","restore_clipboard":true}"#
        );
    }
}
