//! Universal Actions: what to offer for the text, URL or files the user had
//! selected in another app.
//!
//! The shell captures the selection when the Universal Actions hotkey is
//! pressed and asks the engine for [`Plugin::selection_actions`]. This plugin
//! is the one that answers for the built-in actions; it has no keyword and
//! never answers typed queries.
//!
//! Every action is built from the selection when the hotkey is pressed, so
//! "Uppercase" already carries the uppercased text. Nothing about the selection
//! is in a result id, and the plugin opts out of the usage statistics
//! ([`Plugin::tracks_usage`]), so running an action leaves nothing behind.
//!
//! | Selection | Actions |
//! |---|---|
//! | Text | search each web engine, Large Type, copy, paste as plain text, calculate, [`transform`]s (case, trim, URL and Base64 encoding, JSON) |
//! | URL(s) | open, copy, Large Type |
//! | Files and folders | open, show in folder, copy path, open in terminal (folders), run as administrator (Windows programs), send to Sevak (path browsing) |
//!
//! Transformations replace the selection in the app (paste over it) where
//! pasting works, and copy otherwise; `Ctrl+Enter` always copies instead.

pub mod transform;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sevak_core::config::{ShellConfig, WebSearchEngine};
use sevak_core::selection::{preview, MAX_LISTED_ITEMS};
use sevak_core::{
    Action, Config, IconSource, LaunchTarget, Modifier, Plugin, PluginError, PluginResult,
    ResultItem, Selection, SelectionKind,
};
use sevak_platform::PlatformProvider;

use crate::actions::execute_action;
use crate::calculator::CalculatorPlugin;
use crate::web_search::search_url;

const PLUGIN_ID: &str = "selection";

/// Titles and previews are cut to this many characters.
const TITLE_CHARS: usize = 40;
const SUBTITLE_CHARS: usize = 80;
/// Longest web query sent: a search URL has to stay a usable length.
const SEARCH_CHARS: usize = 300;
/// Large Type shows this much of a long selection.
const LARGE_TYPE_CHARS: usize = 2_000;
/// Longest text offered to the calculator.
const CALCULATE_CHARS: usize = 200;

const TERMINAL_PREFIX: &str = "terminal:";
const LARGE_TYPE_PREFIX: &str = "large_type:";
const SEARCH_PREFIX: &str = "search:";

/// What an action asks of the launcher window itself instead of the OS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiRequest {
    /// Show the text as Large Type.
    LargeType(String),
    /// Put the text in the search box (path browsing).
    Search(String),
}

/// The window request behind `action`, if it is one. The shell passes these to
/// the launcher instead of executing them.
pub fn ui_request(action: &Action) -> Option<UiRequest> {
    let Action::Custom { payload } = action else {
        return None;
    };
    if let Some(text) = payload.strip_prefix(LARGE_TYPE_PREFIX) {
        Some(UiRequest::LargeType(text.to_owned()))
    } else {
        payload
            .strip_prefix(SEARCH_PREFIX)
            .map(|text| UiRequest::Search(text.to_owned()))
    }
}

pub struct SelectionPlugin {
    platform: Arc<dyn PlatformProvider>,
    engines: Vec<WebSearchEngine>,
    /// `[paste] restore_clipboard`, for the paste actions.
    restore_clipboard: bool,
    shell: ShellConfig,
    calculator: CalculatorPlugin,
}

impl SelectionPlugin {
    pub fn new(config: &Config, platform: Arc<dyn PlatformProvider>) -> Self {
        Self {
            engines: config.web_search.clone(),
            restore_clipboard: config.paste.restore_clipboard,
            shell: config.shell.clone(),
            // No currency: the rates would need the network and a cache.
            calculator: CalculatorPlugin::new(platform.clone()),
            platform,
        }
    }

    fn item(key: &str, title: impl Into<String>, action: Action, icon: &str) -> ResultItem {
        ResultItem::new(PLUGIN_ID, key, title, action).with_icon(IconSource::builtin(icon))
    }

    /// `text` as a row that replaces the selection when pasting works (with
    /// `Ctrl+Enter` to copy instead), else one that copies.
    fn replacement(&self, key: &str, title: &str, text: String, icon: &str) -> ResultItem {
        let subtitle = preview(&text, SUBTITLE_CHARS);
        let copy = Action::CopyText { text: text.clone() };
        let item = if self.platform.paste_support().is_available() {
            Self::item(
                key,
                title,
                Action::PasteText {
                    text,
                    restore_clipboard: self.restore_clipboard,
                },
                icon,
            )
            .with_secondary("Copy", Some(Modifier::Ctrl), copy)
        } else {
            Self::item(key, title, copy, icon)
        };
        item.with_subtitle(subtitle)
    }

    fn text_actions(&self, text: &str) -> Vec<ResultItem> {
        let mut items = Vec::new();

        // A path typed into a document is as good as one from a file manager.
        if let Some(path) = existing_path(text) {
            items.extend(self.file_actions(&[path]));
        }

        let terms = search_terms(text);
        for engine in &self.engines {
            let url = search_url(&engine.url, &terms);
            items.push(
                Self::item(
                    &format!("search:{}", engine.keyword),
                    format!(
                        "Search {} for \u{201c}{}\u{201d}",
                        engine.name,
                        preview(text, TITLE_CHARS)
                    ),
                    Action::OpenUrl { url: url.clone() },
                    "web",
                )
                .with_secondary(
                    "Copy URL",
                    Some(Modifier::Shift),
                    Action::CopyText { text: url.clone() },
                )
                .with_subtitle(preview(&url, SUBTITLE_CHARS)),
            );
        }

        items.push(Self::large_type(text));
        items.push(
            Self::item(
                "copy",
                "Copy text",
                Action::CopyText {
                    text: text.to_owned(),
                },
                "copy",
            )
            .with_subtitle(preview(text, SUBTITLE_CHARS)),
        );
        if self.platform.paste_support().is_available() {
            items.push(
                Self::item(
                    "paste_plain",
                    "Paste as plain text",
                    Action::PasteText {
                        text: text.to_owned(),
                        restore_clipboard: self.restore_clipboard,
                    },
                    "file",
                )
                .with_subtitle("Replaces the selection without its formatting"),
            );
        }

        if let Some(answer) = self.calculation(text) {
            items.push(self.replacement(
                "calc",
                &format!("Calculate: {answer}"),
                answer,
                "calculator",
            ));
        }

        for transform in transform::applicable(text) {
            items.push(self.replacement(
                &format!("t:{}", transform.key),
                transform.label,
                transform.result,
                "file",
            ));
        }
        items
    }

    /// The calculator's answer for a one-line `text` that is a calculation or
    /// a unit conversion.
    fn calculation(&self, text: &str) -> Option<String> {
        let text = text.trim();
        if text.is_empty() || text.contains('\n') || text.chars().count() > CALCULATE_CHARS {
            return None;
        }
        self.calculator
            .query(text)
            .into_iter()
            .find_map(|item| match item.action {
                Action::CopyText { text } => Some(text),
                _ => None,
            })
    }

    fn large_type(text: &str) -> ResultItem {
        let shown: String = text.trim().chars().take(LARGE_TYPE_CHARS).collect();
        Self::item(
            "large_type",
            "Show as Large Type",
            Action::Custom {
                payload: format!("{LARGE_TYPE_PREFIX}{shown}"),
            },
            "web",
        )
    }

    fn url_actions(&self, urls: &[String]) -> Vec<ResultItem> {
        let mut items = Vec::new();
        for (index, url) in urls.iter().take(MAX_LISTED_ITEMS).enumerate() {
            let title = if urls.len() == 1 {
                "Open URL".to_owned()
            } else {
                format!("Open {}", preview(url, TITLE_CHARS))
            };
            items.push(
                Self::item(
                    &format!("url:open:{index}"),
                    title,
                    Action::OpenUrl { url: url.clone() },
                    "web",
                )
                .with_subtitle(preview(url, SUBTITLE_CHARS)),
            );
        }
        let joined = urls.join("\n");
        items.push(
            Self::item(
                "url:copy",
                if urls.len() == 1 {
                    "Copy URL"
                } else {
                    "Copy URLs"
                },
                Action::CopyText {
                    text: joined.clone(),
                },
                "copy",
            )
            .with_subtitle(preview(&joined, SUBTITLE_CHARS)),
        );
        items.push(Self::large_type(&joined));
        items
    }

    fn file_actions(&self, files: &[PathBuf]) -> Vec<ResultItem> {
        let mut items = Vec::new();
        for (index, path) in files.iter().take(MAX_LISTED_ITEMS).enumerate() {
            let title = if files.len() == 1 {
                format!("Open \u{201c}{}\u{201d}", display_name(path))
            } else {
                format!("Open {}", display_name(path))
            };
            items.push(
                Self::item(
                    &format!("file:open:{index}"),
                    title,
                    Action::OpenPath { path: path.clone() },
                    if path.is_dir() { "folder" } else { "file" },
                )
                .with_subtitle(preview(&path.to_string_lossy(), SUBTITLE_CHARS)),
            );
        }
        let first = &files[0];
        items.push(Self::item(
            "file:reveal",
            "Show in folder",
            Action::RevealPath {
                path: first.clone(),
            },
            "folder",
        ));
        let paths: Vec<String> = files
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        items.push(
            Self::item(
                "file:copy_path",
                if files.len() == 1 {
                    "Copy path"
                } else {
                    "Copy paths"
                },
                Action::CopyText {
                    text: paths.join("\n"),
                },
                "copy",
            )
            .with_subtitle(preview(&paths.join(" "), SUBTITLE_CHARS)),
        );

        if files.len() == 1 {
            if first.is_dir() {
                items.push(Self::item(
                    "file:terminal",
                    "Open in terminal",
                    Action::Custom {
                        payload: format!("{TERMINAL_PREFIX}{}", first.to_string_lossy()),
                    },
                    "terminal",
                ));
            }
            if let Some(target) = self.admin_target(first) {
                items.push(Self::item(
                    "file:admin",
                    "Run as administrator",
                    Action::RunAsAdmin { target },
                    "file",
                ));
            }
        }

        // Path browsing starts at the folder itself, or at the file.
        let browse = if first.is_dir() {
            browse_path(first)
        } else {
            first.to_string_lossy().into_owned()
        };
        items.push(
            Self::item(
                "file:send",
                "Send to Sevak",
                Action::Custom {
                    payload: format!("{SEARCH_PREFIX}{browse}"),
                },
                "file",
            )
            .with_subtitle("Browse from here in the search box"),
        );
        items
    }

    /// The program to start elevated for `path`, when this system can and the
    /// file is something Windows runs.
    fn admin_target(&self, path: &Path) -> Option<LaunchTarget> {
        if !self.platform.can_run_as_admin() || !path.is_file() {
            return None;
        }
        let extension = path.extension()?.to_string_lossy().to_ascii_lowercase();
        match extension.as_str() {
            "lnk" => Some(LaunchTarget::Shortcut {
                path: path.to_path_buf(),
            }),
            "exe" | "bat" | "cmd" | "com" | "msi" => Some(LaunchTarget::Executable {
                path: path.to_path_buf(),
                args: Vec::new(),
                working_dir: None,
            }),
            _ => None,
        }
    }
}

/// The file or folder name, or the whole path for a root.
fn display_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(|| path.to_string_lossy(), |name| name.to_string_lossy())
        .into_owned()
}

/// `dir` with a trailing separator, which the path browser lists.
fn browse_path(dir: &Path) -> String {
    let mut text = dir.to_string_lossy().into_owned();
    if !text.ends_with(['/', '\\']) {
        text.push(std::path::MAIN_SEPARATOR);
    }
    text
}

/// A web query for `text`: one line, spaces collapsed, not absurdly long.
fn search_terms(text: &str) -> String {
    let mut terms = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if let Some((end, _)) = terms.char_indices().nth(SEARCH_CHARS) {
        terms.truncate(end);
    }
    terms
}

/// `text` as an absolute path of something that exists (surrounding quotes
/// allowed), for a path selected in a document or a terminal's output.
fn existing_path(text: &str) -> Option<PathBuf> {
    let text = text.trim();
    let text = text
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(text);
    if text.is_empty() || text.len() > 4096 || text.contains(['\n', '\r', '\0']) {
        return None;
    }
    // Selected text is whatever a web page or document said: a UNC path in it
    // must not be probed, because that makes Windows contact (and sign in to)
    // the named server.
    if sevak_core::preview::is_network_path(text) {
        return None;
    }
    let path = Path::new(text);
    (path.is_absolute() && path.exists()).then(|| path.to_path_buf())
}

impl Plugin for SelectionPlugin {
    fn id(&self) -> &str {
        PLUGIN_ID
    }

    fn name(&self) -> &str {
        "Universal Actions"
    }

    fn description(&self) -> &str {
        "Offers actions for the text, URL or files you had selected in another app (the Universal Actions shortcut)."
    }

    fn keyword(&self) -> Option<&str> {
        None
    }

    fn query(&self, _input: &str) -> Vec<ResultItem> {
        Vec::new()
    }

    fn tracks_usage(&self) -> bool {
        false
    }

    fn selection_actions(&self, selection: &Selection) -> Vec<ResultItem> {
        if selection.is_too_large() {
            return Vec::new();
        }
        match selection.kind() {
            SelectionKind::Files => self.file_actions(selection.files()),
            SelectionKind::Urls => self.url_actions(&selection.urls().unwrap_or_default()),
            SelectionKind::Text => selection
                .text()
                .map(|text| self.text_actions(text))
                .unwrap_or_default(),
        }
    }

    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        if let Action::Custom { payload } = &item.action {
            if let Some(dir) = payload.strip_prefix(TERMINAL_PREFIX) {
                return self
                    .platform
                    .open_terminal_in(Path::new(dir), &self.shell)
                    .map_err(PluginError::other);
            }
            if ui_request(&item.action).is_some() {
                return Err(PluginError::Message(
                    "that action is carried out by the launcher window".to_owned(),
                ));
            }
        }
        execute_action(self.platform.as_ref(), &item.action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::MockPlatform;

    #[test]
    fn network_paths_in_selected_text_are_never_probed() {
        for text in [
            r"\\attacker.example\share\x",
            "//attacker.example/share/x",
            r#""\\attacker.example@SSL\share""#,
            r"\\?\UNC\attacker.example\share",
        ] {
            assert_eq!(existing_path(text), None, "{text}");
        }
        // A local absolute path that exists is still recognised.
        let here = std::env::current_dir().unwrap();
        assert_eq!(existing_path(&here.to_string_lossy()), Some(here));
    }

    fn engines() -> Vec<WebSearchEngine> {
        WebSearchEngine::defaults()
    }

    fn plugin_on(platform: Arc<MockPlatform>) -> SelectionPlugin {
        let config = Config {
            web_search: engines(),
            ..Config::default()
        };
        SelectionPlugin::new(&config, platform)
    }

    fn plugin() -> (SelectionPlugin, Arc<MockPlatform>) {
        let platform = MockPlatform::empty();
        (plugin_on(platform.clone()), platform)
    }

    fn actions_for(plugin: &SelectionPlugin, text: &str) -> Vec<ResultItem> {
        plugin.selection_actions(&Selection::from_text(text).unwrap())
    }

    fn keys(items: &[ResultItem]) -> Vec<&str> {
        items
            .iter()
            .map(|item| item.id.strip_prefix("selection:").unwrap())
            .collect()
    }

    fn find<'a>(items: &'a [ResultItem], key: &str) -> &'a ResultItem {
        items
            .iter()
            .find(|item| item.id == format!("selection:{key}"))
            .unwrap_or_else(|| panic!("no {key} in {:?}", keys(items)))
    }

    #[test]
    fn text_offers_every_engine_then_the_rest() {
        let (plugin, _) = plugin();
        let items = actions_for(&plugin, "rust traits");
        let keys = keys(&items);
        assert_eq!(
            keys[..8],
            [
                "search:g",
                "search:yt",
                "search:gh",
                "large_type",
                "copy",
                "paste_plain",
                "t:upper",
                "t:title"
            ]
        );
        assert!(keys.contains(&"t:base64_encode"));

        let google = find(&items, "search:g");
        assert_eq!(
            google.title,
            "Search Google for \u{201c}rust traits\u{201d}"
        );
        assert_eq!(
            google.action,
            Action::OpenUrl {
                url: "https://www.google.com/search?q=rust%20traits".into()
            }
        );
        assert_eq!(google.secondary[0].modifier, Some(Modifier::Shift));
    }

    #[test]
    fn multi_line_text_is_searched_as_one_line_and_titled_briefly() {
        let (plugin, _) = plugin();
        let items = actions_for(&plugin, "first line\n  second   line\n");
        let google = find(&items, "search:g");
        assert_eq!(
            google.action,
            Action::OpenUrl {
                url: "https://www.google.com/search?q=first%20line%20second%20line".into()
            }
        );
        assert!(google.title.contains("first line second line"));
        // The copy keeps the text exactly.
        assert_eq!(
            find(&items, "copy").action,
            Action::CopyText {
                text: "first line\n  second   line\n".into()
            }
        );
    }

    #[test]
    fn long_text_is_cut_for_searching_and_titles_but_never_for_copying() {
        let (plugin, _) = plugin();
        let text = "word ".repeat(200);
        let items = actions_for(&plugin, &text);
        let Action::OpenUrl { url } = &find(&items, "search:g").action else {
            panic!()
        };
        assert!(url.len() < 600, "{}", url.len());
        assert!(find(&items, "search:g").title.chars().count() < 80);
        assert_eq!(
            find(&items, "copy").action,
            Action::CopyText { text: text.clone() }
        );
    }

    #[test]
    fn transforms_paste_over_the_selection_and_copy_with_ctrl() {
        let (plugin, _) = plugin();
        let items = actions_for(&plugin, "hello world");
        let upper = find(&items, "t:upper");
        assert_eq!(
            upper.action,
            Action::PasteText {
                text: "HELLO WORLD".into(),
                restore_clipboard: false
            }
        );
        assert_eq!(upper.subtitle, "HELLO WORLD");
        assert_eq!(upper.secondary.len(), 1);
        assert_eq!(upper.secondary[0].modifier, Some(Modifier::Ctrl));
        assert_eq!(
            upper.secondary[0].action,
            Action::CopyText {
                text: "HELLO WORLD".into()
            }
        );
        assert_eq!(
            find(&items, "t:title")
                .secondary_as_primary(0)
                .unwrap()
                .action,
            Action::CopyText {
                text: "Hello World".into()
            }
        );
    }

    #[test]
    fn the_paste_setting_reaches_the_paste_actions() {
        let config = Config {
            paste: sevak_core::PasteConfig {
                restore_clipboard: true,
            },
            ..Config::default()
        };
        let plugin = SelectionPlugin::new(&config, MockPlatform::empty());
        let items = actions_for(&plugin, "abc");
        assert_eq!(
            find(&items, "paste_plain").action,
            Action::PasteText {
                text: "abc".into(),
                restore_clipboard: true
            }
        );
    }

    #[test]
    fn where_pasting_is_impossible_everything_copies() {
        let platform = MockPlatform::empty();
        *platform.copy_only.lock().unwrap() = Some("Wayland".into());
        let plugin = plugin_on(platform);
        let items = actions_for(&plugin, "hello world");
        assert!(!keys(&items).contains(&"paste_plain"));
        for item in &items {
            assert!(
                !matches!(item.action, Action::PasteText { .. }),
                "{} pastes",
                item.id
            );
            assert!(
                item.secondary
                    .iter()
                    .all(|s| !matches!(s.action, Action::PasteText { .. })),
                "{} pastes",
                item.id
            );
        }
        let upper = find(&items, "t:upper");
        assert_eq!(
            upper.action,
            Action::CopyText {
                text: "HELLO WORLD".into()
            }
        );
        assert!(upper.secondary.is_empty());
    }

    #[test]
    fn calculations_and_conversions_are_offered_when_the_text_parses() {
        let (plugin, _) = plugin();
        let items = actions_for(&plugin, "2 + 2 * 3");
        let calc = find(&items, "calc");
        assert_eq!(calc.title, "Calculate: 8");
        assert_eq!(
            calc.action,
            Action::PasteText {
                text: "8".into(),
                restore_clipboard: false
            }
        );

        let items = actions_for(&plugin, "10 km in mi");
        assert!(find(&items, "calc").title.starts_with("Calculate: 6.21"));

        assert!(!keys(&actions_for(&plugin, "hello world")).contains(&"calc"));
        // Several lines are not one calculation.
        assert!(!keys(&actions_for(&plugin, "2+2\n3+3")).contains(&"calc"));
    }

    #[test]
    fn transforms_appear_only_where_they_apply() {
        let (plugin, _) = plugin();
        let json = actions_for(&plugin, r#"{"b":1,"a":[1,2]}"#);
        assert!(keys(&json).contains(&"t:json_pretty"));
        assert!(!keys(&json).contains(&"t:json_minify"));
        let encoded = actions_for(&plugin, "Zm9vYmFy");
        assert_eq!(find(&encoded, "t:base64_decode").subtitle, "foobar");
        let escaped = actions_for(&plugin, "a%20b");
        assert!(keys(&escaped).contains(&"t:url_decode"));
        assert!(!keys(&actions_for(&plugin, "plain")).contains(&"t:url_decode"));
    }

    #[test]
    fn large_type_carries_the_text_for_the_window() {
        let (plugin, _) = plugin();
        let items = actions_for(&plugin, "  big words  ");
        assert_eq!(
            ui_request(&find(&items, "large_type").action),
            Some(UiRequest::LargeType("big words".into()))
        );
        let long = "x".repeat(LARGE_TYPE_CHARS + 500);
        let items = actions_for(&plugin, &long);
        let Some(UiRequest::LargeType(shown)) = ui_request(&find(&items, "large_type").action)
        else {
            panic!()
        };
        assert_eq!(shown.chars().count(), LARGE_TYPE_CHARS);
        assert_eq!(ui_request(&Action::CopyText { text: "x".into() }), None);
        assert_eq!(
            ui_request(&Action::Custom {
                payload: "other".into()
            }),
            None
        );
    }

    #[test]
    fn a_url_gets_open_and_copy() {
        let (plugin, _) = plugin();
        let selection = Selection::from_text("https://example.com/a?b=c").unwrap();
        let items = plugin.selection_actions(&selection);
        assert_eq!(keys(&items), ["url:open:0", "url:copy", "large_type"]);
        assert_eq!(items[0].title, "Open URL");
        assert_eq!(
            items[0].action,
            Action::OpenUrl {
                url: "https://example.com/a?b=c".into()
            }
        );
        assert_eq!(items[1].title, "Copy URL");
    }

    #[test]
    fn several_urls_are_listed_one_by_one_up_to_a_limit() {
        let (plugin, _) = plugin();
        let text = (0..8)
            .map(|i| format!("https://e.test/{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let items = plugin.selection_actions(&Selection::from_text(text.clone()).unwrap());
        let opens = items
            .iter()
            .filter(|i| i.id.starts_with("selection:url:open"))
            .count();
        assert_eq!(opens, MAX_LISTED_ITEMS);
        assert_eq!(find(&items, "url:copy").title, "Copy URLs");
        assert_eq!(find(&items, "url:copy").action, Action::CopyText { text });
        assert_eq!(find(&items, "url:open:1").title, "Open https://e.test/1");
    }

    fn selection_of(paths: &[&Path]) -> Selection {
        Selection::from_files(paths.iter().map(|p| p.to_path_buf()).collect()).unwrap()
    }

    #[test]
    fn a_file_gets_open_reveal_copy_and_send() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes.txt");
        std::fs::write(&file, "x").unwrap();
        let (plugin, _) = plugin();
        let items = plugin.selection_actions(&selection_of(&[&file]));
        assert_eq!(
            keys(&items),
            ["file:open:0", "file:reveal", "file:copy_path", "file:send"]
        );
        assert_eq!(items[0].title, "Open \u{201c}notes.txt\u{201d}");
        assert_eq!(items[0].action, Action::OpenPath { path: file.clone() });
        assert_eq!(items[1].action, Action::RevealPath { path: file.clone() });
        assert_eq!(
            items[2].action,
            Action::CopyText {
                text: file.to_string_lossy().into_owned()
            }
        );
        // The file itself is where browsing starts.
        assert_eq!(
            ui_request(&items[3].action),
            Some(UiRequest::Search(file.to_string_lossy().into_owned()))
        );
    }

    #[test]
    fn a_folder_also_opens_in_a_terminal_and_browses_from_inside() {
        let dir = tempfile::tempdir().unwrap();
        let (plugin, platform) = plugin();
        let items = plugin.selection_actions(&selection_of(&[dir.path()]));
        assert!(keys(&items).contains(&"file:terminal"));

        plugin.execute(find(&items, "file:terminal")).unwrap();
        assert_eq!(
            *platform.terminal_dirs.lock().unwrap(),
            [dir.path().to_path_buf()]
        );

        let Some(UiRequest::Search(browse)) = ui_request(&find(&items, "file:send").action) else {
            panic!()
        };
        assert!(browse.ends_with(['/', '\\']), "{browse}");
        assert!(browse.starts_with(&*dir.path().to_string_lossy()));
    }

    #[test]
    fn running_as_administrator_needs_support_and_a_program() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("setup.exe");
        let document = dir.path().join("setup.txt");
        std::fs::write(&program, "x").unwrap();
        std::fs::write(&document, "x").unwrap();

        let (plain, _) = plugin();
        let items = plain.selection_actions(&selection_of(&[&program]));
        assert!(!keys(&items).contains(&"file:admin"));

        let elevated = plugin_on(MockPlatform::with_admin());
        let items = elevated.selection_actions(&selection_of(&[&program]));
        assert_eq!(
            find(&items, "file:admin").action,
            Action::RunAsAdmin {
                target: LaunchTarget::Executable {
                    path: program,
                    args: Vec::new(),
                    working_dir: None
                }
            }
        );
        let items = elevated.selection_actions(&selection_of(&[&document]));
        assert!(!keys(&items).contains(&"file:admin"));
    }

    #[test]
    fn several_files_are_opened_one_by_one_and_copied_together() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        let (plugin, _) = plugin();
        let items = plugin.selection_actions(&selection_of(&[&a, &b]));
        assert_eq!(
            keys(&items),
            [
                "file:open:0",
                "file:open:1",
                "file:reveal",
                "file:copy_path",
                "file:send"
            ]
        );
        assert_eq!(items[0].title, "Open a.txt");
        assert_eq!(find(&items, "file:copy_path").title, "Copy paths");
        assert_eq!(
            find(&items, "file:copy_path").action,
            Action::CopyText {
                text: format!("{}\n{}", a.display(), b.display())
            }
        );
    }

    #[test]
    fn a_path_selected_as_text_gets_the_file_actions_too() {
        let dir = tempfile::tempdir().unwrap();
        let (plugin, _) = plugin();
        let quoted = format!("\"{}\"", dir.path().display());
        for text in [dir.path().display().to_string(), quoted] {
            let items = actions_for(&plugin, &text);
            assert_eq!(keys(&items)[0], "file:open:0", "{text}");
            assert!(keys(&items).contains(&"search:g"));
        }
        // A missing or relative path is just text.
        let missing = dir.path().join("missing");
        assert!(!keys(&actions_for(&plugin, &missing.to_string_lossy())).contains(&"file:reveal"));
        assert!(!keys(&actions_for(&plugin, "relative/path")).contains(&"file:reveal"));
    }

    #[test]
    fn nothing_of_the_selection_is_in_an_id_and_ids_are_unique() {
        let (plugin, _) = plugin();
        let secret = "hunter2 correct horse";
        let items = actions_for(&plugin, secret);
        let mut ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
        assert!(ids.iter().all(|id| !id.contains("hunter2")), "{ids:?}");
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), items.len());
        assert!(items.iter().all(|i| i.plugin_id == "selection"));
    }

    #[test]
    fn running_an_action_goes_through_the_platform() {
        let (plugin, platform) = plugin();
        let items = actions_for(&plugin, "hello");
        plugin.execute(find(&items, "search:g")).unwrap();
        plugin.execute(find(&items, "copy")).unwrap();
        plugin.execute(find(&items, "t:upper")).unwrap();
        assert_eq!(platform.opened_urls.lock().unwrap().len(), 1);
        assert_eq!(*platform.clipboard.lock().unwrap(), ["hello"]);
        assert_eq!(
            *platform.pasted.lock().unwrap(),
            [("HELLO".to_owned(), false)]
        );
    }

    #[test]
    fn window_actions_are_not_executable_here() {
        let (plugin, platform) = plugin();
        let items = actions_for(&plugin, "hello");
        assert!(plugin.execute(find(&items, "large_type")).is_err());
        assert!(platform.opened_urls.lock().unwrap().is_empty());
        let unknown = ResultItem::new(
            "selection",
            "x",
            "x",
            Action::Custom {
                payload: "nonsense".into(),
            },
        );
        assert!(matches!(
            plugin.execute(&unknown),
            Err(PluginError::Unsupported(_))
        ));
    }

    #[test]
    fn it_never_answers_typed_queries_and_leaves_no_usage() {
        let (plugin, _) = plugin();
        assert!(plugin.query("hello").is_empty());
        assert!(!plugin.tracks_usage());
        assert!(plugin.keyword().is_none());
    }

    #[test]
    fn oversized_text_gets_no_actions() {
        let (plugin, _) = plugin();
        let huge = Selection::from_text("x".repeat(sevak_core::selection::MAX_SELECTION_BYTES + 1))
            .unwrap();
        assert!(plugin.selection_actions(&huge).is_empty());
    }

    #[test]
    fn search_terms_are_one_short_line() {
        assert_eq!(search_terms("  a \n b\t c "), "a b c");
        assert_eq!(
            search_terms(&"\u{e9}".repeat(400)).chars().count(),
            SEARCH_CHARS
        );
        assert_eq!(search_terms("short"), "short");
    }

    #[test]
    fn browse_paths_end_in_a_separator_exactly_once() {
        let with = browse_path(Path::new(if cfg!(windows) { r"C:\a\" } else { "/a/" }));
        assert_eq!(with.matches(['/', '\\']).count(), 2);
        assert!(
            browse_path(Path::new(if cfg!(windows) { r"C:\a" } else { "/a" }))
                .ends_with(std::path::MAIN_SEPARATOR)
        );
    }
}
