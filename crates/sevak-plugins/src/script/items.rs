//! Turning what a script printed into [`ResultItem`]s.
//!
//! Scripts are untrusted input: every field is validated, sizes are capped, and
//! one malformed item is skipped without losing its siblings. The same rules
//! apply to the persistent protocol, to one-shot Sevak-format output and (in
//! `alfred.rs`) to Alfred Script Filter output.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;
use sevak_core::model::score;
use sevak_core::{Action, IconSource, ResultItem};
use sevak_platform::icon_file;

use super::manifest::relative_inside;

/// Items kept from one answer; the list shows at most 20 anyway.
pub const MAX_ITEMS: usize = 50;
const MAX_TITLE_CHARS: usize = 200;
const MAX_SUBTITLE_CHARS: usize = 300;
const MAX_KEY_CHARS: usize = 200;

/// What the converters need to know about the plugin they work for.
#[derive(Debug, Clone, Copy)]
pub struct ItemContext<'a> {
    pub plugin_id: &'a str,
    /// The plugin folder: relative icon paths resolve inside it.
    pub dir: &'a Path,
    /// Whether `custom` actions make sense (only a persistent process can be
    /// asked to `execute` them later).
    pub allow_custom: bool,
}

/// The score of the `index`-th item that gave none.
///
/// Items without a score keep the order the script gave them. The gap between
/// neighbours is wide enough that the engine's usage boost (a few dozen points
/// for a regularly used result) does not shuffle them; a script that wants
/// usage-based ordering can give explicit, closer scores.
pub fn default_score(index: usize) -> f64 {
    (score::KEYWORD - 1.0 - 200.0 * index as f64).max(1.0)
}

/// Scripts may rank their own results but never above built-in keyword rows:
/// anything they say is clamped below [`score::KEYWORD`].
pub fn clamp_score(value: Option<f64>, index: usize) -> f64 {
    match value {
        Some(score) if score.is_finite() => score.clamp(0.0, score::KEYWORD - 1.0),
        _ => default_score(index),
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    let text = text.trim();
    match text.char_indices().nth(max_chars) {
        Some((end, _)) => text[..end].to_owned(),
        None => text.to_owned(),
    }
}

/// Hands out unique result keys, so two rows with the same title do not
/// collapse into one (the engine de-duplicates by result id).
#[derive(Default)]
pub struct KeyAllocator {
    seen: HashSet<String>,
}

impl KeyAllocator {
    pub fn unique(&mut self, key: &str) -> String {
        let base = truncate(key, MAX_KEY_CHARS);
        let mut candidate = base.clone();
        let mut n = 1;
        while !self.seen.insert(candidate.clone()) {
            n += 1;
            candidate = format!("{base}#{n}");
        }
        candidate
    }
}

#[derive(Debug, Deserialize)]
struct RawItem {
    key: Option<String>,
    title: String,
    #[serde(default)]
    subtitle: String,
    icon: Option<Value>,
    action: Option<Value>,
    score: Option<f64>,
}

/// Converts the `items` of a persistent-protocol `results` message (or of a
/// one-shot Sevak-format document). Bad items are logged and skipped.
pub fn convert_items(ctx: ItemContext<'_>, values: &[Value]) -> Vec<ResultItem> {
    let mut keys = KeyAllocator::default();
    let mut items = Vec::new();
    for (index, value) in values.iter().take(MAX_ITEMS).enumerate() {
        let raw: RawItem = match serde_json::from_value(value.clone()) {
            Ok(raw) => raw,
            Err(err) => {
                tracing::warn!(plugin = ctx.plugin_id, index, %err, "skipping a malformed result item");
                continue;
            }
        };
        let title = truncate(&raw.title, MAX_TITLE_CHARS);
        if title.is_empty() {
            tracing::warn!(
                plugin = ctx.plugin_id,
                index,
                "skipping a result item without a title"
            );
            continue;
        }
        let action = match raw.action {
            None => Action::CopyText {
                text: title.clone(),
            },
            Some(value) => match serde_json::from_value::<Action>(value) {
                Ok(Action::Custom { .. }) if !ctx.allow_custom => {
                    tracing::warn!(
                        plugin = ctx.plugin_id,
                        index,
                        "skipping an item with a custom action: only persistent plugins can handle them"
                    );
                    continue;
                }
                // Not part of the script protocol: elevating, typing into
                // another app and file-manager reveals stay with built-ins.
                Ok(
                    Action::RunAsAdmin { .. }
                    | Action::PasteText { .. }
                    | Action::RevealPath { .. },
                ) => {
                    tracing::warn!(
                        plugin = ctx.plugin_id,
                        index,
                        "skipping an item with an action script plugins cannot use"
                    );
                    continue;
                }
                Ok(action) => absolutize(action, ctx.dir),
                Err(err) => {
                    tracing::warn!(plugin = ctx.plugin_id, index, %err, "skipping an item with an invalid action");
                    continue;
                }
            },
        };
        let key = keys.unique(raw.key.as_deref().unwrap_or(&title));
        let mut item = ResultItem::new(ctx.plugin_id, key, title, action)
            .with_subtitle(truncate(&raw.subtitle, MAX_SUBTITLE_CHARS))
            .with_score(clamp_score(raw.score, index));
        if let Some(icon) = raw.icon.as_ref().and_then(|icon| icon_from_json(ctx, icon)) {
            item = item.with_icon(icon);
        }
        items.push(item);
    }
    items
}

/// A relative `open_path` is relative to the plugin folder, where the script
/// runs; the platform layer would otherwise resolve it against Sevak's own
/// working directory.
fn absolutize(action: Action, dir: &Path) -> Action {
    match action {
        Action::OpenPath { path } if path.is_relative() => Action::OpenPath {
            path: dir.join(path),
        },
        other => other,
    }
}

fn icon_from_json(ctx: ItemContext<'_>, value: &Value) -> Option<IconSource> {
    match value.get("kind")?.as_str()? {
        "builtin" => {
            let name = value.get("name")?.as_str()?;
            let valid = !name.is_empty()
                && name.len() <= 32
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
            valid.then(|| IconSource::builtin(name))
        }
        "file" => plugin_icon(ctx.dir, value.get("path")?.as_str()?),
        _ => None,
    }
}

/// The image `relative` inside the plugin folder `dir` as an icon, or `None`
/// when the path escapes the folder (`..`, absolute paths, symlinks pointing
/// elsewhere), does not exist, or is not an image the UI can show.
pub fn plugin_icon(dir: &Path, relative: &str) -> Option<IconSource> {
    let path = relative_inside(dir, relative)?;
    icon_file::mime_for(&path)?;
    let real = path.canonicalize().ok()?;
    let root = dir.canonicalize().ok()?;
    (real.starts_with(&root) && real.is_file()).then_some(IconSource::File { path })
}

/// Expands a leading `~` for paths scripts print (`~/notes.txt`).
pub fn expand_tilde(path: &str) -> PathBuf {
    crate::files::expand_home(path, crate::files::home_dir().as_deref())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn ctx(dir: &Path) -> ItemContext<'_> {
        ItemContext {
            plugin_id: "script:t",
            dir,
            allow_custom: true,
        }
    }

    fn convert(value: Value) -> Vec<ResultItem> {
        convert_items(ctx(Path::new("plugin")), value.as_array().unwrap())
    }

    #[test]
    fn converts_a_full_item() {
        let items = convert(json!([{
            "key": "greeting",
            "title": "Hello",
            "subtitle": "Enter to copy",
            "icon": {"kind": "builtin", "name": "copy"},
            "action": {"type": "copy_text", "text": "Hello"},
            "score": 42.0
        }]));
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item.id, "script:t:greeting");
        assert_eq!(item.plugin_id, "script:t");
        assert_eq!(item.title, "Hello");
        assert_eq!(item.subtitle, "Enter to copy");
        assert_eq!(item.icon, Some(IconSource::builtin("copy")));
        assert_eq!(
            item.action,
            Action::CopyText {
                text: "Hello".into()
            }
        );
        assert_eq!(item.score, 42.0);
    }

    #[test]
    fn actions_outside_the_protocol_are_skipped() {
        let items = convert(json!([
            {"title": "admin", "action": {"type": "run_as_admin", "target": {"kind": "shortcut", "path": "x.lnk"}}},
            {"title": "paste", "action": {"type": "paste_text", "text": "x", "restore_clipboard": false}},
            {"title": "reveal", "action": {"type": "reveal_path", "path": "/tmp"}},
            {"title": "ok", "action": {"type": "copy_text", "text": "ok"}}
        ]));
        let titles: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, ["ok"]);
    }

    #[test]
    fn defaults_keep_the_script_order_below_the_keyword_score() {
        let items = convert(json!([{"title": "a"}, {"title": "b"}, {"title": "c"}]));
        assert_eq!(items[0].action, Action::CopyText { text: "a".into() });
        let scores: Vec<f64> = items.iter().map(|i| i.score).collect();
        assert!(scores.windows(2).all(|w| w[0] > w[1]));
        assert!(scores[0] < score::KEYWORD);
        assert_eq!(items[0].id, "script:t:a");
    }

    #[test]
    fn scores_are_clamped_below_keyword() {
        assert_eq!(clamp_score(Some(1e9), 0), score::KEYWORD - 1.0);
        assert_eq!(
            clamp_score(Some(score::EXACT_ANSWER), 3),
            score::KEYWORD - 1.0
        );
        assert_eq!(clamp_score(Some(-5.0), 0), 0.0);
        assert_eq!(clamp_score(Some(f64::NAN), 1), default_score(1));
        assert_eq!(clamp_score(None, 1), default_score(1));
        assert!(default_score(10_000) >= 1.0);
    }

    #[test]
    fn skips_bad_items_but_keeps_the_rest() {
        let items = convert(json!([
            {"title": "ok"},
            {"no_title": true},
            {"title": "   "},
            {"title": "bad action", "action": {"type": "format_disk"}},
            "not even an object",
            {"title": "also ok"}
        ]));
        let titles: Vec<_> = items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, ["ok", "also ok"]);
    }

    #[test]
    fn duplicate_keys_become_distinct_ids() {
        let items = convert(json!([
            {"title": "same"}, {"title": "same"}, {"key": "same", "title": "x"}
        ]));
        let ids: Vec<_> = items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["script:t:same", "script:t:same#2", "script:t:same#3"]);
    }

    #[test]
    fn custom_actions_need_a_persistent_plugin() {
        let value = json!([{"title": "x", "action": {"type": "custom", "payload": "p"}}]);
        let items = convert_items(
            ItemContext {
                allow_custom: false,
                ..ctx(Path::new("p"))
            },
            value.as_array().unwrap(),
        );
        assert!(items.is_empty());
        assert_eq!(convert(value).len(), 1);
    }

    #[test]
    fn caps_the_number_and_length_of_everything() {
        let many: Vec<Value> = (0..200)
            .map(|i| json!({"title": format!("t{i}")}))
            .collect();
        assert_eq!(convert_items(ctx(Path::new("p")), &many).len(), MAX_ITEMS);
        let long = "é".repeat(1_000);
        let items = convert(json!([{"title": long, "subtitle": long}]));
        assert_eq!(items[0].title.chars().count(), MAX_TITLE_CHARS);
        assert_eq!(items[0].subtitle.chars().count(), MAX_SUBTITLE_CHARS);
    }

    #[test]
    fn relative_open_path_is_anchored_in_the_plugin_folder() {
        let items = convert(json!([
            {"title": "a", "action": {"type": "open_path", "path": "docs/readme.txt"}},
            {"title": "b", "action": {"type": "open_url", "url": "https://example.com"}}
        ]));
        assert_eq!(
            items[0].action,
            Action::OpenPath {
                path: Path::new("plugin").join("docs/readme.txt")
            }
        );
        assert_eq!(
            items[1].action,
            Action::OpenUrl {
                url: "https://example.com".into()
            }
        );
    }

    #[test]
    fn file_icons_must_stay_inside_the_plugin_folder() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("plugin");
        std::fs::create_dir_all(dir.join("img")).unwrap();
        std::fs::write(dir.join("img/a.png"), b"png").unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        std::fs::write(root.path().join("secret.png"), b"png").unwrap();

        assert_eq!(
            plugin_icon(&dir, "img/a.png"),
            Some(IconSource::File {
                path: dir.join("img/a.png")
            })
        );
        assert_eq!(plugin_icon(&dir, "../secret.png"), None);
        assert_eq!(plugin_icon(&dir, "img/missing.png"), None);
        assert_eq!(plugin_icon(&dir, "notes.txt"), None);
        let absolute = root.path().join("secret.png");
        assert_eq!(plugin_icon(&dir, absolute.to_str().unwrap()), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_out_of_the_folder_is_not_an_icon() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("plugin");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(root.path().join("secret.png"), b"png").unwrap();
        std::os::unix::fs::symlink(root.path().join("secret.png"), dir.join("link.png")).unwrap();
        assert_eq!(plugin_icon(&dir, "link.png"), None);
    }

    #[test]
    fn icon_json_is_validated() {
        let dir = Path::new("p");
        let c = ctx(dir);
        assert_eq!(
            icon_from_json(c, &json!({"kind": "builtin", "name": "web"})),
            Some(IconSource::builtin("web"))
        );
        assert_eq!(
            icon_from_json(c, &json!({"kind": "builtin", "name": "../x"})),
            None
        );
        assert_eq!(
            icon_from_json(c, &json!({"kind": "shell", "parsing_name": "C:\\x"})),
            None
        );
        assert_eq!(icon_from_json(c, &json!("nonsense")), None);
    }
}
