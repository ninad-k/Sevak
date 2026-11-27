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
//! | `autocomplete`                 | [`ResultItem::autocomplete`] (what Tab inserts)    |
//! | `mods`                         | secondary actions, see below                       |
//! | `icon.path`                    | an icon inside the plugin folder                   |
//! | `valid: false`                 | the row is shown; Enter copies its title           |
//! | `type: "file"` / `file:skipcheck` | `arg` is a path to open                         |
//! | `quicklookurl`                 | the preview pane: a file path shows the file, an `http(s)` URL is shown as a link (never fetched) |
//! | `text.copy`, `text.largetype`  | what Ctrl+C copies and what Large Type shows       |
//! | `match`                        | what the query is matched against, when the workflow node has "Sevak filters results" on |
//! | `variables` (item and top level) | a workflow's rows pass them on; the item's win   |
//! | `skipknowledge: true` (top level) | usage statistics never reorder the rows         |
//! | `cache: {seconds, loosereload}` | results are kept per query, see [`CacheSpec`]     |
//! | `rerun`                        | the script runs again on that interval, see [`Parsed::rerun`] |
//! | `strictmods`, `icon.resize`, `iconresize` | accepted and ignored                    |
//!
//! `arg` becomes an action by shape: an `http://`, `https://` or `mailto:`
//! address opens in the browser or mail client; an existing absolute path (or
//! `~/...`) opens with its default program; anything else is copied. Other URL
//! schemes (`slack://`, `obsidian://`) are copied, because Sevak's platform
//! layer only opens web and mail links.
//!
//! # Modifiers (`mods`)
//!
//! Each entry of `mods` (`{"alt": {"arg": "...", "subtitle": "...", "valid": true}}`)
//! becomes a secondary action: its `arg` (the item's own when it has none)
//! turns into an action by the same rules, its `subtitle` is the label, and
//! `valid: false` drops it. The key held with Enter follows the platform
//! convention of Sevak's own result actions: `cmd` is `Ctrl+Enter` (Command on
//! macOS), `alt` is `Alt+Enter`, `shift` is `Shift+Enter`. Alfred's `ctrl`, `fn`
//! and combinations such as `cmd+alt` have no key of their own here and are
//! listed in the action panel only.
//!
//! # Workflows
//!
//! A workflow's script filter node (`Format::AlfredWorkflow`) parses the same
//! JSON but keeps what Alfred keeps for the nodes after it: instead of an
//! action every row (and every modifier) carries a [`RawPick`], the `arg` and
//! `variables` the rest of the workflow starts with.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sevak_core::fuzzy::FuzzyQuery;
use sevak_core::{Action, Modifier, PreviewHint, ResultItem};

use super::items::{clamp_score, expand_tilde, plugin_icon, ItemContext, KeyAllocator, MAX_ITEMS};

#[derive(Debug, Deserialize)]
struct Document {
    #[serde(default)]
    items: Vec<Value>,
    /// Read leniently (see [`cache_spec`]): a malformed value never fails the
    /// whole answer.
    #[serde(default)]
    cache: Option<Value>,
    #[serde(default)]
    rerun: Option<Value>,
    #[serde(default)]
    variables: Option<Value>,
    #[serde(default)]
    skipknowledge: Option<Value>,
}

/// Shortest and longest time a script's answer may be cached.
pub const MIN_CACHE_SECONDS: u64 = 5;
pub const MAX_CACHE_SECONDS: u64 = 86_400;
/// Shortest and longest `rerun` interval.
pub const MIN_RERUN: Duration = Duration::from_millis(100);
pub const MAX_RERUN: Duration = Duration::from_secs(5);

/// The top-level `cache` of a Script Filter answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheSpec {
    /// How long the answer counts as fresh (5 to 86 400).
    pub seconds: u64,
    /// After that, show the old answer at once while the script runs again.
    pub loose_reload: bool,
}

/// Everything one Script Filter answer says.
#[derive(Debug)]
pub struct Parsed {
    pub items: Vec<ResultItem>,
    pub cache: Option<CacheSpec>,
    /// Run the script again this often while the same query stays on screen
    /// (0.1 to 5 seconds).
    pub rerun: Option<Duration>,
}

/// How to read an answer.
#[derive(Debug, Clone, Copy, Default)]
pub struct ParseOptions<'a> {
    /// Rows carry a [`RawPick`] (a workflow's script filter).
    pub raw: bool,
    /// Sevak matches this query against each row's `match` (or title) and
    /// drops the rows that do not match: the node's "Sevak filters results".
    pub filter_query: Option<&'a str>,
}

fn cache_spec(value: &Option<Value>) -> Option<CacheSpec> {
    let object = value.as_ref()?.as_object()?;
    let seconds = object.get("seconds")?.as_f64()?;
    if !seconds.is_finite() || seconds <= 0.0 {
        return None;
    }
    Some(CacheSpec {
        seconds: (seconds as u64).clamp(MIN_CACHE_SECONDS, MAX_CACHE_SECONDS),
        loose_reload: object
            .get("loosereload")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn rerun_interval(value: &Option<Value>) -> Option<Duration> {
    let seconds = value.as_ref()?.as_f64()?;
    if !seconds.is_finite() || seconds <= 0.0 {
        return None;
    }
    Some(Duration::from_secs_f64(seconds).clamp(MIN_RERUN, MAX_RERUN))
}

#[derive(Debug, Deserialize)]
struct AlfredItem {
    uid: Option<String>,
    title: String,
    subtitle: Option<String>,
    arg: Option<Value>,
    autocomplete: Option<String>,
    icon: Option<AlfredIcon>,
    valid: Option<bool>,
    #[serde(rename = "type")]
    kind: Option<String>,
    mods: Option<Map<String, Value>>,
    variables: Option<Map<String, Value>>,
    quicklookurl: Option<String>,
    text: Option<AlfredText>,
    #[serde(rename = "match")]
    matcher: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct AlfredText {
    copy: Option<String>,
    largetype: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AlfredIcon {
    path: Option<String>,
    /// `fileicon` and `filetype` ask macOS for a file's icon; unsupported.
    #[serde(rename = "type")]
    kind: Option<String>,
}

/// One entry of an item's `mods`.
#[derive(Debug, Default, Deserialize)]
struct AlfredMod {
    valid: Option<bool>,
    arg: Option<Value>,
    subtitle: Option<String>,
    variables: Option<Map<String, Value>>,
}

/// What a workflow's script filter row hands to the nodes after it: the
/// payload of the row's `Action::Custom`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawPick {
    /// The item's `arg` (empty without one).
    pub arg: String,
    /// The item's `variables`, scalars as text.
    #[serde(default)]
    pub variables: BTreeMap<String, String>,
    /// `false` for rows that only inform (`valid: false`).
    #[serde(default = "yes")]
    pub valid: bool,
    /// The modifier whose entry in `mods` was picked (`alt`, `cmd+alt`, ...);
    /// `None` for the row's main action.
    #[serde(default)]
    pub modifier: Option<String>,
}

fn yes() -> bool {
    true
}

impl RawPick {
    /// The JSON an `Action::Custom` carries.
    pub fn payload(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn from_payload(payload: &str) -> Option<Self> {
        serde_json::from_str(payload).ok()
    }
}

/// Parses the whole stdout of an Alfred script.
#[cfg(test)]
pub fn parse(ctx: ItemContext<'_>, stdout: &str) -> Result<Vec<ResultItem>, String> {
    parse_document(ctx, stdout, ParseOptions::default()).map(|parsed| parsed.items)
}

/// Like [`parse`], for a workflow's script filter: rows carry a [`RawPick`]
/// instead of an action.
#[cfg(test)]
pub fn parse_workflow(ctx: ItemContext<'_>, stdout: &str) -> Result<Vec<ResultItem>, String> {
    parse_document(
        ctx,
        stdout,
        ParseOptions {
            raw: true,
            filter_query: None,
        },
    )
    .map(|parsed| parsed.items)
}

/// The rows plus what the top level of the answer asks for.
pub fn parse_document(
    ctx: ItemContext<'_>,
    stdout: &str,
    options: ParseOptions<'_>,
) -> Result<Parsed, String> {
    let raw = options.raw;
    let text = stdout.trim_start_matches('\u{feff}').trim();
    let document: Document = serde_json::from_str(text)
        .map_err(|err| format!("the output is not Script Filter JSON: {err}"))?;
    let top_variables = match &document.variables {
        Some(Value::Object(map)) => variables_of(&Some(map.clone())),
        _ => BTreeMap::new(),
    };
    let keep_order = matches!(document.skipknowledge, Some(Value::Bool(true)));
    let mut matcher = options
        .filter_query
        .map(str::trim)
        .filter(|query| !query.is_empty())
        .map(FuzzyQuery::new);
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
        let mut score = clamp_score(None, index);
        if let Some(matcher) = matcher.as_mut() {
            let haystack = item
                .matcher
                .as_deref()
                .filter(|text| !text.trim().is_empty())
                .unwrap_or(&title);
            let Some(found) = matcher.score(haystack) else {
                continue;
            };
            if !keep_order {
                // The script's order breaks ties.
                score = clamp_score(Some(f64::from(found) + 1.0 / (index as f64 + 2.0)), index);
            }
        }
        let key = keys.unique(
            item.uid
                .as_deref()
                .filter(|uid| !uid.is_empty())
                .unwrap_or(&title),
        );
        let action = if raw {
            pick_action(&item, None, &top_variables)
        } else {
            action_for(&item, &title)
        };
        // Alfred orders by the script, so scripts never give a score.
        let mut result = ResultItem::new(ctx.plugin_id, key, title.clone(), action)
            .with_subtitle(item.subtitle.clone().unwrap_or_default().trim())
            .with_score(score);
        if keep_order {
            result = result.keep_order();
        }
        if let Some(autocomplete) = item.autocomplete.as_deref().filter(|a| !a.is_empty()) {
            result = result.with_autocomplete(autocomplete);
        }
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
        if let Some(preview) = item
            .quicklookurl
            .as_deref()
            .and_then(|url| preview_hint(url, &title))
        {
            result = result.with_preview(preview);
        }
        if let Some(text) = &item.text {
            if let Some(copy) = text.copy.as_deref().filter(|text| !text.is_empty()) {
                result = result.with_copy_text(copy);
            }
            if let Some(large) = text.largetype.as_deref().filter(|text| !text.is_empty()) {
                result = result.with_large_text(large);
            }
        }
        result = add_mods(result, &item, &title, raw, &top_variables);
        items.push(result);
    }
    Ok(Parsed {
        items,
        cache: cache_spec(&document.cache),
        rerun: rerun_interval(&document.rerun),
    })
}

/// `quicklookurl` as a preview: a web address is shown as a link (the preview
/// never fetches it); an absolute path, `~/...` or `file://` URL shows the
/// file. Anything else (a relative path, another scheme) is ignored.
fn preview_hint(url: &str, title: &str) -> Option<PreviewHint> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Some(PreviewHint::Url {
            url: url.to_owned(),
            title: Some(title.to_owned()),
        });
    }
    let path_text = if lower.starts_with("file://") {
        url["file://".len()..].to_owned()
    } else {
        url.to_owned()
    };
    if path_text.is_empty() || path_text.contains("://") {
        return None;
    }
    let path = expand_tilde(&path_text);
    path.is_absolute().then_some(PreviewHint::Path { path })
}

/// The `arg` of an item or a mod: the first string of an array, a plain
/// string, or nothing.
fn arg_text(arg: &Option<Value>) -> &str {
    match arg {
        Some(Value::String(arg)) => arg.as_str(),
        Some(Value::Array(args)) => args.iter().find_map(Value::as_str).unwrap_or_default(),
        _ => "",
    }
}

/// Scalars of a `variables` object as text; nulls, arrays and objects are
/// dropped.
fn variables_of(variables: &Option<Map<String, Value>>) -> BTreeMap<String, String> {
    variables
        .iter()
        .flatten()
        .filter_map(|(name, value)| {
            let text = match value {
                Value::String(text) => text.clone(),
                Value::Number(number) => number.to_string(),
                Value::Bool(flag) => flag.to_string(),
                _ => return None,
            };
            Some((name.clone(), text))
        })
        .collect()
}

/// The row's main action in a workflow: the raw `arg` and `variables`.
fn pick_action(
    item: &AlfredItem,
    modifier: Option<&AlfredMod>,
    top_variables: &BTreeMap<String, String>,
) -> Action {
    // The answer's own variables sit under the item's.
    let mut variables = top_variables.clone();
    variables.extend(variables_of(&item.variables));
    let (arg, valid, modifier_name) = match modifier {
        None => (arg_text(&item.arg), item.valid != Some(false), None),
        Some(entry) => {
            variables.extend(variables_of(&entry.variables));
            let own = arg_text(&entry.arg);
            (
                if entry.arg.is_some() {
                    own
                } else {
                    arg_text(&item.arg)
                },
                entry.valid != Some(false),
                Some(String::new()),
            )
        }
    };
    Action::Custom {
        payload: RawPick {
            arg: arg.to_owned(),
            variables,
            valid,
            modifier: modifier_name,
        }
        .payload(),
    }
}

/// Alfred's `cmd` is the platform's command key, which in Sevak is the `Ctrl`
/// modifier (Command on macOS); `ctrl`, `fn` and combinations have no key of
/// their own.
fn sevak_modifier(name: &str) -> Option<Modifier> {
    match name {
        "cmd" => Some(Modifier::Ctrl),
        "alt" => Some(Modifier::Alt),
        "shift" => Some(Modifier::Shift),
        _ => None,
    }
}

/// `mods` key to a canonical `cmd+alt` name, or `None` when a part is not an
/// Alfred modifier.
fn canonical_modifier(key: &str) -> Option<String> {
    const ORDER: [&str; 5] = ["cmd", "alt", "ctrl", "shift", "fn"];
    let parts: Vec<String> = key
        .split('+')
        .map(|part| part.trim().to_ascii_lowercase())
        .collect();
    if parts.is_empty() || parts.iter().any(|part| !ORDER.contains(&part.as_str())) {
        return None;
    }
    let ordered: Vec<&str> = ORDER
        .into_iter()
        .filter(|name| parts.iter().any(|part| part == name))
        .collect();
    Some(ordered.join("+"))
}

fn display_modifier(name: &str) -> String {
    name.split('+')
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// Adds the item's `mods` as secondary actions.
fn add_mods(
    mut result: ResultItem,
    item: &AlfredItem,
    title: &str,
    raw: bool,
    top_variables: &BTreeMap<String, String>,
) -> ResultItem {
    let Some(mods) = &item.mods else {
        return result;
    };
    // The canonical order makes the action panel stable whatever order the
    // script printed its keys in.
    let mut entries: Vec<(String, AlfredMod)> = Vec::new();
    for (key, value) in mods {
        let Some(name) = canonical_modifier(key) else {
            tracing::warn!("ignoring an Alfred modifier this Sevak does not know");
            continue;
        };
        let entry: AlfredMod = serde_json::from_value(value.clone()).unwrap_or_default();
        if entry.valid == Some(false) || entries.iter().any(|(seen, _)| *seen == name) {
            continue;
        }
        entries.push((name, entry));
    }
    let rank = |name: &str| {
        ["cmd", "alt", "ctrl", "shift", "fn"]
            .iter()
            .position(|first| name.starts_with(first))
            .unwrap_or(usize::MAX)
    };
    entries.sort_by(|a, b| {
        a.0.matches('+')
            .count()
            .cmp(&b.0.matches('+').count())
            .then(rank(&a.0).cmp(&rank(&b.0)))
            .then(a.0.cmp(&b.0))
    });

    let mut claimed: Vec<Modifier> = Vec::new();
    for (name, entry) in entries {
        let action = if raw {
            match pick_action(item, Some(&entry), top_variables) {
                Action::Custom { payload } => {
                    let mut pick = RawPick::from_payload(&payload).unwrap_or(RawPick {
                        arg: String::new(),
                        variables: BTreeMap::new(),
                        valid: true,
                        modifier: None,
                    });
                    pick.modifier = Some(name.clone());
                    Action::Custom {
                        payload: pick.payload(),
                    }
                }
                other => other,
            }
        } else {
            mod_action(item, &entry, title)
        };
        let key = sevak_modifier(&name).filter(|modifier| !claimed.contains(modifier));
        if let Some(modifier) = key {
            claimed.push(modifier);
        }
        let label = entry
            .subtitle
            .as_deref()
            .map(str::trim)
            .filter(|label| !label.is_empty())
            .map_or_else(
                || default_label(&action, &display_modifier(&name)),
                str::to_owned,
            );
        result = result.with_secondary(label, key, action);
    }
    result
}

/// What to call a modifier without a `subtitle`.
fn default_label(action: &Action, modifier: &str) -> String {
    match action {
        Action::OpenUrl { .. } => "Open link".to_owned(),
        Action::OpenPath { .. } => "Open".to_owned(),
        Action::CopyText { .. } => "Copy".to_owned(),
        _ => format!("{modifier} action"),
    }
}

/// The action of a modifier: its own `arg` by the usual shape rules, else the
/// item's.
fn mod_action(item: &AlfredItem, entry: &AlfredMod, title: &str) -> Action {
    let arg = if entry.arg.is_some() {
        &entry.arg
    } else {
        &item.arg
    };
    action_from(arg, item.kind.as_deref(), title)
}

fn action_for(item: &AlfredItem, title: &str) -> Action {
    if item.valid == Some(false) {
        return Action::CopyText {
            text: title.to_owned(),
        };
    }
    action_from(&item.arg, item.kind.as_deref(), title)
}

fn action_from(arg: &Option<Value>, kind: Option<&str>, title: &str) -> Action {
    let arg = arg_text(arg);
    if arg.is_empty() {
        return Action::CopyText {
            text: title.to_owned(),
        };
    }
    let lower = arg.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:")
    {
        return Action::OpenUrl {
            url: arg.to_owned(),
        };
    }
    let is_file = matches!(kind, Some("file" | "file:skipcheck"));
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
            allow_launch: false,
        };
        parse(ctx, &doc.to_string()).unwrap()
    }

    fn run_workflow(doc: Value) -> Vec<ResultItem> {
        let ctx = ItemContext {
            plugin_id: "workflow:w:f",
            dir: Path::new("p"),
            allow_custom: false,
            allow_launch: false,
        };
        parse_workflow(ctx, &doc.to_string()).unwrap()
    }

    fn pick(action: &Action) -> RawPick {
        let Action::Custom { payload } = action else {
            panic!("not a custom action: {action:?}");
        };
        RawPick::from_payload(payload).unwrap()
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
        assert_eq!(item.autocomplete.as_deref(), Some("docs"));
        assert!(item.icon.is_some());
    }

    #[test]
    fn autocomplete_is_optional_and_empty_means_none() {
        let items = run(
            Path::new("p"),
            json!({"items": [
                {"title": "A", "autocomplete": "alpha "},
                {"title": "B"},
                {"title": "C", "autocomplete": ""}
            ]}),
        );
        assert_eq!(items[0].autocomplete.as_deref(), Some("alpha "));
        assert_eq!(items[1].autocomplete, None);
        assert_eq!(items[2].autocomplete, None);
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
            allow_launch: false,
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

    #[test]
    fn mods_become_secondary_actions_with_the_platform_keys() {
        let items = run(
            Path::new("p"),
            json!({"items": [{
                "title": "Repo", "arg": "https://github.com/a/b",
                "mods": {
                    "shift": {"arg": "git clone https://github.com/a/b", "subtitle": "Copy the clone command"},
                    "cmd": {"arg": "https://github.com/a/b/issues", "subtitle": "Open the issues"},
                    "alt": {"subtitle": "Copy the link", "arg": "plain words"}
                }
            }]}),
        );
        let secondary = &items[0].secondary;
        let summary: Vec<_> = secondary
            .iter()
            .map(|s| (s.label.as_str(), s.modifier))
            .collect();
        // Fixed order: cmd, alt, ctrl, shift - not the order of the JSON keys.
        assert_eq!(
            summary,
            [
                ("Open the issues", Some(Modifier::Ctrl)),
                ("Copy the link", Some(Modifier::Alt)),
                ("Copy the clone command", Some(Modifier::Shift)),
            ]
        );
        assert_eq!(
            secondary[0].action,
            Action::OpenUrl {
                url: "https://github.com/a/b/issues".into()
            }
        );
        assert_eq!(
            secondary[1].action,
            Action::CopyText {
                text: "plain words".into()
            }
        );
        // The primary action is untouched.
        assert!(matches!(items[0].action, Action::OpenUrl { .. }));
    }

    #[test]
    fn mods_without_a_key_of_their_own_are_panel_only() {
        let items = run(
            Path::new("p"),
            json!({"items": [{
                "title": "T", "arg": "https://a.example",
                "mods": {
                    "ctrl": {"arg": "c", "subtitle": "Ctrl thing"},
                    "fn": {"arg": "f"},
                    "cmd+alt": {"arg": "https://b.example", "subtitle": "Both"},
                    "cmd+alt+alt": {"arg": "duplicate of the line above"},
                    "hyper": {"arg": "unknown key"}
                }
            }]}),
        );
        let secondary = &items[0].secondary;
        assert!(secondary.iter().all(|s| s.modifier.is_none()));
        let labels: Vec<_> = secondary.iter().map(|s| s.label.as_str()).collect();
        // Single modifiers first, then combinations; the unknown key and the
        // duplicate combination are gone; no subtitle falls back to the verb.
        assert_eq!(labels, ["Ctrl thing", "Copy", "Both"]);
    }

    #[test]
    fn a_mod_without_an_arg_uses_the_items_and_invalid_mods_are_dropped() {
        let items = run(
            Path::new("p"),
            json!({"items": [{
                "title": "T", "arg": "https://a.example",
                "mods": {
                    "alt": {"subtitle": "Same link"},
                    "shift": {"valid": false, "arg": "x"},
                    "cmd": "not even an object"
                }
            }]}),
        );
        let secondary = &items[0].secondary;
        assert_eq!(secondary.len(), 2);
        assert_eq!(secondary[0].modifier, Some(Modifier::Ctrl));
        assert_eq!(secondary[0].label, "Open link");
        assert_eq!(secondary[1].modifier, Some(Modifier::Alt));
        assert_eq!(
            secondary[1].action,
            Action::OpenUrl {
                url: "https://a.example".into()
            }
        );
    }

    #[test]
    fn workflow_rows_carry_the_raw_arg_variables_and_mods() {
        let items = run_workflow(json!({"items": [
            {
                "uid": "one", "title": "One", "arg": ["first", "second"],
                "variables": {"n": 3, "flag": true, "name": "x", "skipped": null, "nested": {"a": 1}},
                "autocomplete": "one ",
                "mods": {"alt": {"arg": "alt-arg", "variables": {"n": "4"}, "subtitle": "Alt"}}
            },
            {"uid": "two", "title": "Info only", "valid": false, "arg": "ignored"}
        ]}));
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].autocomplete.as_deref(), Some("one "));
        let main = pick(&items[0].action);
        assert_eq!(main.arg, "first");
        assert!(main.valid && main.modifier.is_none());
        let expected: BTreeMap<String, String> = [("n", "3"), ("flag", "true"), ("name", "x")]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();
        assert_eq!(main.variables, expected);

        let alt = pick(&items[0].secondary[0].action);
        assert_eq!(alt.arg, "alt-arg");
        assert_eq!(alt.modifier.as_deref(), Some("alt"));
        // The mod's variables win over the item's.
        assert_eq!(alt.variables["n"], "4");
        assert_eq!(alt.variables["name"], "x");
        assert_eq!(items[0].secondary[0].modifier, Some(Modifier::Alt));

        assert!(!pick(&items[1].action).valid);
    }

    #[test]
    fn display_names_and_canonical_keys() {
        assert_eq!(canonical_modifier("Alt+CMD").as_deref(), Some("cmd+alt"));
        assert_eq!(canonical_modifier("fn").as_deref(), Some("fn"));
        assert_eq!(canonical_modifier("cmd+banana"), None);
        assert_eq!(canonical_modifier(""), None);
        assert_eq!(display_modifier("cmd+alt"), "Cmd+Alt");
    }
    fn document(doc: Value, raw: bool, filter_query: Option<&str>) -> Parsed {
        let ctx = ItemContext {
            plugin_id: "workflow:w:f",
            dir: Path::new("p"),
            allow_custom: false,
            allow_launch: false,
        };
        parse_document(ctx, &doc.to_string(), ParseOptions { raw, filter_query }).unwrap()
    }

    #[test]
    fn cache_is_read_and_clamped() {
        let parsed = |cache: Value| document(json!({"cache": cache, "items": []}), false, None);
        assert_eq!(
            parsed(json!({"seconds": 60, "loosereload": true})).cache,
            Some(CacheSpec {
                seconds: 60,
                loose_reload: true
            })
        );
        assert_eq!(parsed(json!({"seconds": 1})).cache.unwrap().seconds, 5);
        assert_eq!(
            parsed(json!({"seconds": 10_000_000}))
                .cache
                .unwrap()
                .seconds,
            86_400
        );
        assert!(!parsed(json!({"seconds": 9})).cache.unwrap().loose_reload);
        // Anything malformed is no cache, never an error.
        for bad in [
            json!({}),
            json!({"seconds": "x"}),
            json!({"seconds": -3}),
            json!(7),
            json!(null),
        ] {
            assert_eq!(parsed(bad).cache, None);
        }
        assert_eq!(document(json!({"items": []}), false, None).cache, None);
    }

    #[test]
    fn rerun_is_read_and_clamped() {
        let rerun =
            |value: Value| document(json!({"rerun": value, "items": []}), false, None).rerun;
        assert_eq!(rerun(json!(0.5)), Some(Duration::from_millis(500)));
        assert_eq!(rerun(json!(2)), Some(Duration::from_secs(2)));
        assert_eq!(rerun(json!(0.01)), Some(MIN_RERUN));
        assert_eq!(rerun(json!(60)), Some(MAX_RERUN));
        assert_eq!(rerun(json!(0)), None);
        assert_eq!(rerun(json!("fast")), None);
        assert_eq!(document(json!({"items": []}), false, None).rerun, None);
    }

    #[test]
    fn top_level_variables_sit_under_the_items_and_mods_variables() {
        let parsed = document(
            json!({
                "variables": {"theme": "dark", "n": 1, "skipped": [1]},
                "items": [
                    {"title": "A", "arg": "a", "variables": {"n": 2},
                     "mods": {"alt": {"variables": {"theme": "light"}}}},
                    {"title": "B", "arg": "b"}
                ]
            }),
            true,
            None,
        );
        let first = pick(&parsed.items[0].action);
        assert_eq!(first.variables["theme"], "dark");
        assert_eq!(first.variables["n"], "2", "the item's own value wins");
        assert!(!first.variables.contains_key("skipped"));
        assert_eq!(pick(&parsed.items[1].action).variables["n"], "1");
        let alt = pick(&parsed.items[0].secondary[0].action);
        assert_eq!(alt.variables["theme"], "light");
        assert_eq!(alt.variables["n"], "2");
    }

    #[test]
    fn skipknowledge_keeps_the_scripts_order_against_usage() {
        let parsed = |doc: Value| document(doc, false, None).items;
        let on = parsed(json!({"skipknowledge": true, "items": [{"title": "A"}, {"title": "B"}]}));
        assert!(on.iter().all(|item| item.keep_order));
        let off = parsed(json!({"items": [{"title": "A"}]}));
        assert!(!off[0].keep_order);
        let not_true = parsed(json!({"skipknowledge": "yes", "items": [{"title": "A"}]}));
        assert!(!not_true[0].keep_order);
    }

    #[test]
    fn quicklookurl_becomes_a_preview() {
        let preview = |url: &str| {
            document(
                json!({"items": [{"title": "T", "quicklookurl": url}]}),
                false,
                None,
            )
            .items
            .remove(0)
            .preview
        };
        assert_eq!(
            preview("https://example.com/a"),
            Some(PreviewHint::Url {
                url: "https://example.com/a".into(),
                title: Some("T".into())
            })
        );
        let absolute = std::env::temp_dir().join("note.txt");
        assert_eq!(
            preview(&absolute.to_string_lossy()),
            Some(PreviewHint::Path {
                path: absolute.clone()
            })
        );
        assert!(matches!(
            preview(&format!("file://{}", absolute.to_string_lossy())),
            Some(PreviewHint::Path { .. })
        ));
        // Never relative, never another scheme.
        assert_eq!(preview("notes/a.txt"), None);
        assert_eq!(preview("ftp://example.com/a"), None);
        assert_eq!(preview(""), None);
    }

    #[test]
    fn text_overrides_what_copy_and_large_type_use() {
        let items = document(
            json!({"items": [
                {"title": "T", "arg": "https://example.com",
                 "text": {"copy": "copied", "largetype": "BIG"}},
                {"title": "U", "arg": "u", "text": {"copy": ""}}
            ]}),
            false,
            None,
        )
        .items;
        assert_eq!(items[0].copy_text().as_deref(), Some("copied"));
        assert_eq!(items[0].large_text.as_deref(), Some("BIG"));
        assert_eq!(items[1].copy_text().as_deref(), Some("u"));
        assert_eq!(items[1].large_text, None);
    }

    #[test]
    fn match_filters_only_when_asked_and_falls_back_to_the_title() {
        let doc = json!({"items": [
            {"title": "Alpha", "match": "zebra crossing"},
            {"title": "Beta"},
            {"title": "Gamma", "match": "beta tester"}
        ]});
        let titles = |parsed: Parsed| {
            parsed
                .items
                .iter()
                .map(|item| item.title.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(titles(document(doc.clone(), true, None)).len(), 3);
        assert_eq!(
            titles(document(doc.clone(), true, Some("beta"))),
            ["Beta", "Gamma"]
        );
        assert_eq!(
            titles(document(doc.clone(), true, Some("zebra"))),
            ["Alpha"]
        );
        assert_eq!(titles(document(doc.clone(), true, Some("  "))).len(), 3);
        assert!(titles(document(doc, true, Some("qqqq"))).is_empty());
    }

    #[test]
    fn a_filtered_answer_is_ranked_by_match_unless_skipknowledge() {
        let items = |skip: bool| {
            document(
                json!({"skipknowledge": skip, "items": [
                    {"title": "hxoxmxe"}, {"title": "home"}]}),
                false,
                Some("home"),
            )
            .items
        };
        let ranked = items(false);
        assert!(
            ranked[1].score > ranked[0].score,
            "the closer match scores higher"
        );
        let kept = items(true);
        assert!(kept[0].score > kept[1].score, "the script's order stands");
    }

    #[test]
    fn mods_with_ctrl_and_fn_are_listed_without_a_key_of_their_own() {
        let items = run_workflow(json!({"items": [{
            "title": "T", "arg": "a",
            "mods": {
                "ctrl": {"arg": "c", "subtitle": "Control", "valid": true, "variables": {"v": "1"}},
                "fn": {"arg": "f", "subtitle": "Function"},
                "cmd": {"arg": "m", "subtitle": "Command"},
                "alt": {"valid": false}
            }
        }]}));
        let secondary = &items[0].secondary;
        assert_eq!(secondary.len(), 3, "valid:false drops a mod");
        let by_label = |label: &str| secondary.iter().find(|s| s.label == label).unwrap();
        assert_eq!(by_label("Command").modifier, Some(Modifier::Ctrl));
        assert_eq!(by_label("Control").modifier, None);
        assert_eq!(by_label("Function").modifier, None);
        let control = pick(&by_label("Control").action);
        assert_eq!(control.arg, "c");
        assert_eq!(control.variables["v"], "1");
        assert_eq!(control.modifier.as_deref(), Some("ctrl"));
    }

    #[test]
    fn strictmods_and_icon_resize_are_accepted_and_ignored() {
        let parsed = document(
            json!({
                "strictmods": true,
                "items": [{
                    "title": "T", "arg": "a", "strictmods": true, "iconresize": true,
                    "icon": {"path": "missing.png", "resize": true},
                    "mods": {"alt": {"strictmods": true, "icon": {"path": "x.png"}}}
                }]
            }),
            false,
            None,
        );
        assert_eq!(parsed.items.len(), 1);
        assert_eq!(parsed.items[0].title, "T");
    }
}
