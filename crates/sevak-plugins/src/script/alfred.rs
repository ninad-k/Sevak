//! Alfred Script Filter compatibility.
//!
//! A one-shot plugin with `format = "alfred"` prints Alfred's Script Filter JSON
//! and Sevak maps it to its own results, so existing Alfred scripts that do not
//! depend on macOS can run unchanged:
//!
//! ```json
//! {"items": [{"uid": "x", "title": "T", "subtitle": "S", "arg": "https://example.com",
//!             "icon": {"path": "icon.png"}, "valid": true}]}
//! ```
//!
//! # Mapping
//!
//! | Alfred                         | Sevak                                              |
//! |--------------------------------|----------------------------------------------------|
//! | `title`, `subtitle`            | the same                                           |
//! | `uid`                          | the result key (stable ids; falls back to `title`) |
//! | `arg` (first one if an array)  | the action, see below                              |
//! | `icon.path`                    | an icon inside the plugin folder                   |
//! | `valid: false`                 | the row is shown; Enter copies its title           |
//! | `type: "file"` / `file:skipcheck` | `arg` is a path to open                         |
//! | `autocomplete`, `quicklookurl`, `mods`, `variables`, `text`, `rerun` | ignored   |
//!
//! `arg` becomes an action by shape: an `http://`, `https://` or `mailto:`
//! address opens in the browser or mail client; an existing absolute path (or
//! `~/...`) opens with its default program; anything else is copied. Other URL
//! schemes (`slack://`, `obsidian://`) are copied, because Sevak's platform
//! layer only opens web and mail links.

use std::path::Path;

use serde::Deserialize;
use serde_json::Value;
use sevak_core::{Action, ResultItem};

use super::items::{clamp_score, expand_tilde, plugin_icon, ItemContext, KeyAllocator, MAX_ITEMS};

#[derive(Debug, Deserialize)]
struct Document {
    #[serde(default)]
    items: Vec<Value>,
}

#[derive(Debug, Deserialize)]
struct AlfredItem {
    uid: Option<String>,
    title: String,
    subtitle: Option<String>,
    arg: Option<Value>,
    icon: Option<AlfredIcon>,
    valid: Option<bool>,
    #[serde(rename = "type")]
    kind: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AlfredIcon {
    path: Option<String>,
    /// `fileicon` and `filetype` ask macOS for a file's icon; unsupported.
    #[serde(rename = "type")]
    kind: Option<String>,
}

/// Parses the whole stdout of an Alfred script.
pub fn parse(ctx: ItemContext<'_>, stdout: &str) -> Result<Vec<ResultItem>, String> {
    let text = stdout.trim_start_matches('\u{feff}').trim();
    let document: Document = serde_json::from_str(text)
        .map_err(|err| format!("the output is not Script Filter JSON: {err}"))?;
    let mut keys = KeyAllocator::default();
    let mut items = Vec::new();
    for (index, value) in document.items.iter().take(MAX_ITEMS).enumerate() {
        let item: AlfredItem = match serde_json::from_value(value.clone()) {
            Ok(item) => item,
            Err(err) => {
                tracing::warn!(plugin = ctx.plugin_id, index, %err, "skipping a malformed Alfred item");
                continue;
            }
        };
        let title = item.title.trim().to_owned();
        if title.is_empty() {
            continue;
        }
        let key = keys.unique(
            item.uid
                .as_deref()
                .filter(|uid| !uid.is_empty())
                .unwrap_or(&title),
        );
        let action = action_for(&item, &title);
        // Alfred orders by the script, so scripts never give a score.
        let mut result = ResultItem::new(ctx.plugin_id, key, title, action)
            .with_subtitle(item.subtitle.unwrap_or_default().trim())
            .with_score(clamp_score(None, index));
        let icon = item
            .icon
            .as_ref()
            .and_then(|icon| match (&icon.path, &icon.kind) {
                (Some(path), None) => plugin_icon(ctx.dir, path),
                _ => None,
            });
        if let Some(icon) = icon {
            result = result.with_icon(icon);
        }
        items.push(result);
    }
    Ok(items)
}

fn action_for(item: &AlfredItem, title: &str) -> Action {
    let copy_title = || Action::CopyText {
        text: title.to_owned(),
    };
    if item.valid == Some(false) {
        return copy_title();
    }
    let arg = match &item.arg {
        Some(Value::String(arg)) => arg.as_str(),
        Some(Value::Array(args)) => args.iter().find_map(Value::as_str).unwrap_or_default(),
        _ => "",
    };
    if arg.is_empty() {
        return copy_title();
    }
    let lower = arg.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:")
    {
        return Action::OpenUrl {
            url: arg.to_owned(),
        };
    }
    let is_file = matches!(item.kind.as_deref(), Some("file" | "file:skipcheck"));
    let path = expand_tilde(arg);
    if path.is_absolute() && (is_file || Path::new(&path).exists()) {
        return Action::OpenPath { path };
    }
    Action::CopyText {
        text: arg.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn run(dir: &Path, doc: Value) -> Vec<ResultItem> {
        let ctx = ItemContext {
            plugin_id: "script:a",
            dir,
            allow_custom: false,
        };
        parse(ctx, &doc.to_string()).unwrap()
    }

    fn action(arg: Value) -> Action {
        run(
            Path::new("p"),
            json!({"items": [{"title": "T", "arg": arg}]}),
        )
        .remove(0)
        .action
    }

    #[test]
    fn maps_a_typical_item() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("icon.png"), b"png").unwrap();
        let items = run(
            dir.path(),
            json!({"items": [{
                "uid": "abc", "title": "Docs", "subtitle": "Open the docs",
                "arg": "https://example.com/docs", "autocomplete": "docs",
                "icon": {"path": "icon.png"}, "valid": true, "quicklookurl": "x"
            }]}),
        );
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item.id, "script:a:abc");
        assert_eq!(item.title, "Docs");
        assert_eq!(item.subtitle, "Open the docs");
        assert_eq!(
            item.action,
            Action::OpenUrl {
                url: "https://example.com/docs".into()
            }
        );
        assert!(item.icon.is_some());
    }

    #[test]
    fn arg_shape_decides_the_action() {
        assert!(matches!(
            action(json!("HTTP://example.com")),
            Action::OpenUrl { .. }
        ));
        assert!(matches!(
            action(json!("mailto:a@b.c")),
            Action::OpenUrl { .. }
        ));
        // Schemes Sevak cannot open are copied rather than failing on Enter.
        assert_eq!(
            action(json!("slack://channel?id=1")),
            Action::CopyText {
                text: "slack://channel?id=1".into()
            }
        );
        assert_eq!(
            action(json!("plain text")),
            Action::CopyText {
                text: "plain text".into()
            }
        );
        // The first string of an array.
        assert!(matches!(
            action(json!(["https://a.example", "second"])),
            Action::OpenUrl { .. }
        ));
    }

    #[test]
    fn existing_absolute_paths_open() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("note.txt");
        std::fs::write(&file, b"x").unwrap();
        let arg = file.to_string_lossy().into_owned();
        assert_eq!(action(json!(arg)), Action::OpenPath { path: file });
        // A path that does not exist is just text...
        let missing = dir.path().join("missing.txt");
        assert!(matches!(
            action(json!(missing.to_string_lossy())),
            Action::CopyText { .. }
        ));
        // ...unless the script says it is a file.
        let items = run(
            Path::new("p"),
            json!({"items": [{"title": "T", "type": "file:skipcheck", "arg": missing.to_string_lossy()}]}),
        );
        assert!(matches!(items[0].action, Action::OpenPath { .. }));
    }

    #[test]
    fn invalid_or_arg_less_items_copy_their_title() {
        let items = run(
            Path::new("p"),
            json!({"items": [
                {"title": "No results", "valid": false, "arg": "https://x.example"},
                {"title": "Nothing to do"}
            ]}),
        );
        assert_eq!(
            items[0].action,
            Action::CopyText {
                text: "No results".into()
            }
        );
        assert_eq!(
            items[1].action,
            Action::CopyText {
                text: "Nothing to do".into()
            }
        );
    }

    #[test]
    fn keeps_the_script_order_and_unique_ids() {
        let items = run(
            Path::new("p"),
            json!({"items": [{"title": "A"}, {"title": "A"}, {"uid": "z", "title": "B"}]}),
        );
        let ids: Vec<_> = items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["script:a:A", "script:a:A#2", "script:a:z"]);
        assert!(items[0].score > items[1].score && items[1].score > items[2].score);
    }

    #[test]
    fn rejects_output_that_is_not_json() {
        let ctx = ItemContext {
            plugin_id: "script:a",
            dir: Path::new("p"),
            allow_custom: false,
        };
        assert!(parse(ctx, "Traceback (most recent call last):").is_err());
        // A byte-order mark and surrounding whitespace are fine; no items is fine.
        assert!(parse(ctx, "\u{feff} {\"items\": []}\n").unwrap().is_empty());
        assert!(parse(ctx, "{}").unwrap().is_empty());
    }

    #[test]
    fn mac_only_icons_are_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let items = run(
            dir.path(),
            json!({"items": [{"title": "T", "icon": {"type": "fileicon", "path": "/Applications/X.app"}}]}),
        );
        assert!(items[0].icon.is_none());
    }
}
