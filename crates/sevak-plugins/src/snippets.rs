//! Snippets: `s <name>` finds text you defined in `config.toml` and Enter pastes
//! it, with placeholders filled in, into the app you were in.
//!
//! ```toml
//! [[snippet]]
//! name = "Email signature"
//! keyword = "sig"             # optional; also matched by the search
//! text = "Best regards,\nNinad\n{date}"
//! ```
//!
//! # Placeholders
//!
//! | Written            | Becomes                                                |
//! |--------------------|--------------------------------------------------------|
//! | `{date}`           | today, `2026-10-03`                                    |
//! | `{time}`           | now, `14:05`                                           |
//! | `{datetime}`       | `2026-10-03 14:05`                                     |
//! | `{date:FORMAT}`    | today in a [strftime](chrono::format::strftime) format, e.g. `{date:%d %B %Y}` (also `{time:..}`, `{datetime:..}`) |
//! | `{clipboard}`      | the clipboard's text when you pressed Enter            |
//! | `{uuid}`           | a fresh random UUID                                    |
//! | `{{` and `}}`      | a literal `{` and `}`                                  |
//!
//! Anything else in braces (`{unknown}`, `{}`, a lone `{`) is left exactly as
//! written, so code snippets with braces mostly just work.
//!
//! Placeholders are filled in when you press Enter, not while you type, so
//! `{time}` is the time of the paste and `{clipboard}` is read only then.
//!
//! Typing a snippet's keyword in any app and having it expand in place would need
//! a global keyboard hook; that is not part of Sevak (yet).

use std::fmt::Write as _;
use std::sync::Arc;

use chrono::format::StrftimeItems;
use chrono::{DateTime, FixedOffset, Local};
use sevak_core::config::{PasteConfig, Snippet};
use sevak_core::model::score;
use sevak_core::{Action, FuzzyQuery, IconSource, Plugin, PluginResult, ResultItem};
use sevak_platform::{PasteSupport, PlatformProvider};

use crate::actions::execute_action;
use crate::example_uuid::random_uuid_v4;

/// The keyword that routes a query to this plugin.
pub const KEYWORD: &str = "s";

const DEFAULT_DATE: &str = "%Y-%m-%d";
const DEFAULT_TIME: &str = "%H:%M";
const DEFAULT_DATETIME: &str = "%Y-%m-%d %H:%M";
const PAYLOAD_NOTHING: &str = "nothing";
const PREVIEW_CHARS: usize = 80;
/// An exact match on a snippet's keyword outranks any fuzzy match on a name.
const KEYWORD_MATCH_BONUS: f64 = 1_000.0;
/// A match on the keyword counts a little less than one on the name.
const KEYWORD_FUZZY_WEIGHT: f64 = 0.9;

/// What placeholders are filled from. Closures, so the clipboard is only read
/// (and a UUID only generated) when a template asks for it.
pub struct Env<'a> {
    pub now: DateTime<FixedOffset>,
    pub clipboard: &'a dyn Fn() -> Option<String>,
    pub uuid: &'a dyn Fn() -> Option<String>,
}

/// Fills in the placeholders of `template` (see the module docs).
pub fn expand(template: &str, env: &Env<'_>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(at) = rest.find(['{', '}']) {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        if let Some(after) = rest.strip_prefix("{{") {
            out.push('{');
            rest = after;
        } else if let Some(after) = rest.strip_prefix("}}") {
            out.push('}');
            rest = after;
        } else if rest.starts_with('}') {
            out.push('}');
            rest = &rest[1..];
        } else {
            // A single `{`: a placeholder if a `}` follows before another `{`.
            let body = &rest[1..];
            let close = body
                .find(['{', '}'])
                .filter(|&i| body.as_bytes()[i] == b'}');
            match close.and_then(|i| placeholder(&body[..i], env).map(|value| (i, value))) {
                Some((i, value)) => {
                    out.push_str(&value);
                    rest = &body[i + 1..];
                }
                None => {
                    out.push('{');
                    rest = body;
                }
            }
        }
    }
    out.push_str(rest);
    out
}

/// The value of the placeholder written `{inner}`, or `None` if `inner` is not
/// one (the braces then stay in the text).
fn placeholder(inner: &str, env: &Env<'_>) -> Option<String> {
    let (name, format) = match inner.split_once(':') {
        Some((name, format)) => (name, Some(format)),
        None => (inner, None),
    };
    let default_format = match name {
        "date" => DEFAULT_DATE,
        "time" => DEFAULT_TIME,
        "datetime" => DEFAULT_DATETIME,
        "clipboard" if format.is_none() => return Some((env.clipboard)().unwrap_or_default()),
        "uuid" if format.is_none() => return Some((env.uuid)().unwrap_or_default()),
        _ => return None,
    };
    let format = format.filter(|f| !f.is_empty()).unwrap_or(default_format);
    format_time(&env.now, format)
}

/// strftime formatting that reports a bad format instead of panicking
/// (`DateTime::format(..).to_string()` panics on one).
fn format_time(now: &DateTime<FixedOffset>, format: &str) -> Option<String> {
    let mut formatted = String::new();
    write!(
        formatted,
        "{}",
        now.format_with_items(StrftimeItems::new(format))
    )
    .ok()?;
    Some(formatted)
}

/// A snippet with the key its result id is built from.
struct Entry {
    key: String,
    name: String,
    keyword: Option<String>,
    text: String,
}

/// The `s` plugin.
pub struct SnippetsPlugin {
    entries: Vec<Entry>,
    restore_clipboard: bool,
    platform: Arc<dyn PlatformProvider>,
}

impl SnippetsPlugin {
    pub fn new(
        snippets: &[Snippet],
        paste: &PasteConfig,
        platform: Arc<dyn PlatformProvider>,
    ) -> Self {
        let mut entries: Vec<Entry> = Vec::with_capacity(snippets.len());
        for snippet in snippets {
            // The name is the stable key; duplicates get a number so every
            // snippet stays addressable.
            let mut key = snippet.name.trim().to_owned();
            let mut n = 1;
            while entries.iter().any(|entry| entry.key == key) {
                n += 1;
                key = format!("{} #{n}", snippet.name.trim());
            }
            entries.push(Entry {
                key,
                name: snippet.name.trim().to_owned(),
                keyword: snippet.keyword.clone(),
                text: snippet.text.clone(),
            });
        }
        Self {
            entries,
            restore_clipboard: paste.restore_clipboard,
            platform,
        }
    }

    fn row(&self, entry: &Entry, support: &PasteSupport) -> ResultItem {
        let (action, hint) = match support {
            PasteSupport::Available => (
                // The template, not the expansion: `execute` fills it in at the
                // moment of pasting (see the module docs).
                Action::PasteText {
                    text: entry.text.clone(),
                    restore_clipboard: self.restore_clipboard,
                },
                "Enter to paste".to_owned(),
            ),
            PasteSupport::CopyOnly(reason) => (
                Action::CopyText {
                    text: entry.text.clone(),
                },
                format!("Copies to clipboard · {reason}"),
            ),
        };
        let mut subtitle = first_line(&entry.text, PREVIEW_CHARS);
        if let Some(keyword) = &entry.keyword {
            subtitle = format!("{keyword} · {subtitle}");
        }
        ResultItem::new(self.id(), &entry.key, &entry.name, action)
            .with_subtitle(format!("{subtitle} · {hint}"))
            .with_icon(IconSource::builtin("plugin"))
    }

    fn lookup(&self, item: &ResultItem) -> Option<&Entry> {
        let key = item.id.strip_prefix(self.id())?.strip_prefix(':')?;
        self.entries.iter().find(|entry| entry.key == key)
    }

    fn env_expand(&self, template: &str) -> String {
        let clipboard = || self.platform.clipboard_text().ok().flatten();
        let env = Env {
            now: Local::now().fixed_offset(),
            clipboard: &clipboard,
            uuid: &random_uuid_v4,
        };
        expand(template, &env)
    }
}

fn first_line(text: &str, max_chars: usize) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    let mut shown: String = line.chars().take(max_chars).collect();
    if line.chars().count() > max_chars || text.lines().filter(|l| !l.trim().is_empty()).count() > 1
    {
        shown.push('…');
    }
    shown
}

impl Plugin for SnippetsPlugin {
    fn id(&self) -> &str {
        "snippets"
    }

    fn name(&self) -> &str {
        "Snippets"
    }

    fn description(&self) -> &str {
        "Type `s` to paste text from your [[snippet]] entries, with {date}, {clipboard} and more."
    }

    fn keyword(&self) -> Option<&str> {
        Some(KEYWORD)
    }

    fn global(&self) -> bool {
        false
    }

    fn query(&self, input: &str) -> Vec<ResultItem> {
        if self.entries.is_empty() {
            return vec![ResultItem::new(
                self.id(),
                "none",
                "No snippets yet",
                Action::Custom {
                    payload: PAYLOAD_NOTHING.to_owned(),
                },
            )
            .with_subtitle("Add [[snippet]] entries with name and text to config.toml")
            .with_icon(IconSource::builtin("plugin"))
            .with_score(score::KEYWORD)];
        }

        let support = self.platform.paste_support();
        let input = input.trim();
        if input.is_empty() {
            // Config order is the user's order.
            return self
                .entries
                .iter()
                .enumerate()
                .map(|(i, entry)| {
                    self.row(entry, &support)
                        .with_score(score::KEYWORD - i as f64)
                })
                .collect();
        }

        let mut query = FuzzyQuery::new(input);
        let mut rows: Vec<ResultItem> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let by_name = query.score(&entry.name).map(f64::from);
                let by_keyword = entry.keyword.as_deref().and_then(|keyword| {
                    let fuzzy = query.score(keyword).map(f64::from)? * KEYWORD_FUZZY_WEIGHT;
                    let exact = keyword.eq_ignore_ascii_case(input);
                    Some(fuzzy + if exact { KEYWORD_MATCH_BONUS } else { 0.0 })
                });
                let matched = match (by_name, by_keyword) {
                    (Some(a), Some(b)) => a.max(b),
                    (Some(a), None) | (None, Some(a)) => a,
                    (None, None) => return None,
                };
                Some(self.row(entry, &support).with_score(matched))
            })
            .collect();
        rows.sort_by(|a, b| b.score.total_cmp(&a.score));
        rows
    }

    /// `snippets:<name>` (the snippet's key) for a snippet in the config.
    fn resolve(&self, id: &str) -> Option<ResultItem> {
        let key = id.strip_prefix("snippets:")?;
        let entry = self.entries.iter().find(|entry| entry.key == key)?;
        Some(self.row(entry, &self.platform.paste_support()))
    }

    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        let (template, paste) = match &item.action {
            Action::Custom { .. } => return Ok(()),
            // Prefer the current definition (the config may have been reloaded
            // since the query); fall back to the text the row carried.
            Action::PasteText { text, .. } => (text, true),
            Action::CopyText { text } => (text, false),
            other => return execute_action(self.platform.as_ref(), other),
        };
        let template = self
            .lookup(item)
            .map_or(template.as_str(), |e| e.text.as_str());
        let text = self.env_expand(template);
        let action = if paste {
            Action::PasteText {
                text,
                restore_clipboard: self.restore_clipboard,
            }
        } else {
            Action::CopyText { text }
        };
        execute_action(self.platform.as_ref(), &action)
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;
    use crate::test_util::MockPlatform;

    fn now() -> DateTime<FixedOffset> {
        FixedOffset::east_opt(5 * 3600 + 1800)
            .unwrap()
            .with_ymd_and_hms(2026, 10, 3, 14, 5, 9)
            .unwrap()
    }

    fn render(template: &str, clipboard: Option<&str>) -> String {
        let clipboard_value = clipboard.map(str::to_owned);
        let clipboard = move || clipboard_value.clone();
        let uuid = || Some("00000000-0000-4000-8000-000000000000".to_owned());
        expand(
            template,
            &Env {
                now: now(),
                clipboard: &clipboard,
                uuid: &uuid,
            },
        )
    }

    #[test]
    fn plain_text_is_unchanged() {
        assert_eq!(render("", None), "");
        assert_eq!(
            render("just text\nand a line", None),
            "just text\nand a line"
        );
        assert_eq!(render("日本語 é", None), "日本語 é");
    }

    #[test]
    fn date_and_time_placeholders() {
        assert_eq!(render("{date}", None), "2026-10-03");
        assert_eq!(render("{time}", None), "14:05");
        assert_eq!(render("{datetime}", None), "2026-10-03 14:05");
        assert_eq!(
            render("on {date} at {time}!", None),
            "on 2026-10-03 at 14:05!"
        );
    }

    #[test]
    fn date_formats_use_strftime() {
        assert_eq!(render("{date:%d %B %Y}", None), "03 October 2026");
        assert_eq!(render("{date:%H:%M:%S}", None), "14:05:09");
        assert_eq!(render("{date:%A}", None), "Saturday");
        assert_eq!(render("{time:%I:%M %p}", None), "02:05 PM");
        assert_eq!(render("{datetime:%y%m%d}", None), "261003");
        assert_eq!(render("{date:%z}", None), "+0530");
        // An empty format falls back to the default.
        assert_eq!(render("{date:}", None), "2026-10-03");
    }

    #[test]
    fn an_invalid_format_stays_as_written_instead_of_panicking() {
        assert_eq!(render("{date:%Q}", None), "{date:%Q}");
        assert_eq!(render("{date:%}", None), "{date:%}");
        assert_eq!(
            render("a {date:%Q} b {date}", None),
            "a {date:%Q} b 2026-10-03"
        );
    }

    #[test]
    fn clipboard_and_uuid() {
        assert_eq!(render("[{clipboard}]", Some("copied")), "[copied]");
        assert_eq!(render("[{clipboard}]", None), "[]");
        assert_eq!(
            render("id: {uuid}", None),
            "id: 00000000-0000-4000-8000-000000000000"
        );
        assert_eq!(render("{clipboard:x}", Some("c")), "{clipboard:x}");
        assert_eq!(render("{uuid:x}", None), "{uuid:x}");
    }

    #[test]
    fn the_clipboard_is_only_read_when_used() {
        let reads = std::cell::Cell::new(0);
        let clipboard = || {
            reads.set(reads.get() + 1);
            Some("x".to_owned())
        };
        let uuid = || None;
        let env = Env {
            now: now(),
            clipboard: &clipboard,
            uuid: &uuid,
        };
        assert_eq!(expand("{date} {time}", &env), "2026-10-03 14:05");
        assert_eq!(reads.get(), 0);
        assert_eq!(expand("{clipboard}{clipboard}", &env), "xx");
        assert_eq!(reads.get(), 2);
    }

    #[test]
    fn doubled_braces_are_literal() {
        assert_eq!(render("{{date}}", None), "{date}");
        assert_eq!(render("{{", None), "{");
        assert_eq!(render("}}", None), "}");
        assert_eq!(render("{{{date}}}", None), "{2026-10-03}");
        assert_eq!(
            render("fn main() {{ println!(\"{{}}\") }}", None),
            "fn main() { println!(\"{}\") }"
        );
    }

    #[test]
    fn unknown_or_malformed_braces_are_left_alone() {
        assert_eq!(render("{unknown}", None), "{unknown}");
        assert_eq!(render("{}", None), "{}");
        assert_eq!(render("{ date }", None), "{ date }");
        assert_eq!(render("{Date}", None), "{Date}");
        assert_eq!(render("open { never closed", None), "open { never closed");
        assert_eq!(render("lone } close", None), "lone } close");
        assert_eq!(render("{ {date}", None), "{ 2026-10-03");
        assert_eq!(render("{date", None), "{date");
        assert_eq!(render("trailing {", None), "trailing {");
        assert_eq!(render("{{}", None), "{}");
    }

    #[test]
    fn expansion_is_not_recursive() {
        assert_eq!(render("{clipboard}", Some("{date}")), "{date}");
    }

    fn snippet(name: &str, keyword: Option<&str>, text: &str) -> Snippet {
        Snippet {
            name: name.to_owned(),
            keyword: keyword.map(str::to_owned),
            text: text.to_owned(),
        }
    }

    fn plugin(platform: &Arc<MockPlatform>, snippets: &[Snippet]) -> SnippetsPlugin {
        SnippetsPlugin::new(snippets, &PasteConfig::default(), platform.clone())
    }

    fn sample() -> Vec<Snippet> {
        vec![
            snippet("Email signature", Some("sig"), "Best regards,\nNinad"),
            snippet("Meeting link", None, "https://meet.example.com/ninad"),
            snippet("Today", Some("td"), "{date}"),
        ]
    }

    #[test]
    fn metadata() {
        let plugin = plugin(&MockPlatform::empty(), &[]);
        assert_eq!(plugin.id(), "snippets");
        assert_eq!(plugin.keyword(), Some("s"));
        assert!(!plugin.global());
    }

    #[test]
    fn no_snippets_explains_where_to_add_them() {
        let platform = MockPlatform::empty();
        let plugin = plugin(&platform, &[]);
        let rows = plugin.query("");
        assert_eq!(rows[0].title, "No snippets yet");
        plugin.execute(&rows[0]).unwrap();
        assert!(platform.pasted.lock().unwrap().is_empty());
    }

    #[test]
    fn an_empty_query_lists_snippets_in_config_order() {
        let plugin = plugin(&MockPlatform::empty(), &sample());
        let titles: Vec<_> = plugin.query("").into_iter().map(|r| r.title).collect();
        assert_eq!(titles, ["Email signature", "Meeting link", "Today"]);
    }

    #[test]
    fn search_matches_names_and_keywords() {
        let plugin = plugin(&MockPlatform::empty(), &sample());
        let titles =
            |q: &str| -> Vec<String> { plugin.query(q).into_iter().map(|r| r.title).collect() };
        assert_eq!(titles("meet"), ["Meeting link"]);
        assert_eq!(titles("sig"), ["Email signature"]);
        assert_eq!(titles("td"), ["Today"]);
        assert!(titles("zzzz").is_empty());
    }

    #[test]
    fn an_exact_keyword_beats_fuzzy_name_matches() {
        let snippets = vec![
            snippet("Signing off", None, "a"),
            snippet("Other", Some("sig"), "b"),
        ];
        let plugin = plugin(&MockPlatform::empty(), &snippets);
        let titles: Vec<_> = plugin.query("sig").into_iter().map(|r| r.title).collect();
        assert_eq!(titles, ["Other", "Signing off"]);
    }

    #[test]
    fn rows_preview_the_template_and_say_what_enter_does() {
        let plugin = plugin(&MockPlatform::empty(), &sample());
        let rows = plugin.query("");
        assert_eq!(rows[0].subtitle, "sig · Best regards,… · Enter to paste");
        assert_eq!(
            rows[1].subtitle,
            "https://meet.example.com/ninad · Enter to paste"
        );
        assert_eq!(rows[0].id, "snippets:Email signature");
    }

    #[test]
    fn duplicate_names_get_distinct_ids() {
        let snippets = vec![snippet("Hi", None, "one"), snippet("Hi", None, "two")];
        let platform = MockPlatform::empty();
        let plugin = plugin(&platform, &snippets);
        let rows = plugin.query("");
        assert_ne!(rows[0].id, rows[1].id);
        plugin.execute(&rows[1]).unwrap();
        assert_eq!(platform.pasted.lock().unwrap()[0].0, "two");
    }

    #[test]
    fn enter_pastes_the_expanded_text() {
        let platform = MockPlatform::empty();
        *platform.clipboard_now.lock().unwrap() = Some("from clipboard".into());
        let snippets = vec![snippet("Reply", None, "Re: {clipboard} {{ok}}")];
        let plugin = plugin(&platform, &snippets);
        let row = plugin.query("reply").remove(0);
        plugin.execute(&row).unwrap();
        assert_eq!(
            *platform.pasted.lock().unwrap(),
            [("Re: from clipboard {ok}".to_owned(), false)]
        );
    }

    #[test]
    fn ctrl_c_copies_the_expansion_not_the_template() {
        // What `SearchEngine::copy` hands the plugin: the row with a `CopyText`
        // of its `copy_text()`, which is the template.
        let platform = MockPlatform::empty();
        *platform.clipboard_now.lock().unwrap() = Some("x".into());
        let snippets = vec![snippet("Reply", None, "Re: {clipboard}")];
        let plugin = plugin(&platform, &snippets);
        let mut row = plugin.query("reply").remove(0);
        assert_eq!(row.copy_text().as_deref(), Some("Re: {clipboard}"));
        row.action = Action::CopyText {
            text: row.copy_text().unwrap(),
        };
        plugin.execute(&row).unwrap();
        assert_eq!(*platform.clipboard.lock().unwrap(), ["Re: x"]);
        assert!(platform.pasted.lock().unwrap().is_empty());
    }

    #[test]
    fn resolve_finds_a_snippet_by_its_result_id() {
        let platform = MockPlatform::empty();
        let snippets = vec![snippet("Reply", None, "Re: {{x}}")];
        let plugin = plugin(&platform, &snippets);
        let found = plugin.query("reply").remove(0);
        let item = plugin.resolve(&found.id).expect("configured snippet");
        assert_eq!(item.id, "snippets:Reply");
        plugin.execute(&item).unwrap();
        assert_eq!(platform.pasted.lock().unwrap()[0].0, "Re: {x}");
        assert!(plugin.resolve("snippets:Nope").is_none());
        assert!(plugin.resolve("apps:Reply").is_none());
    }

    #[test]
    fn enter_pastes_todays_date() {
        let platform = MockPlatform::empty();
        let plugin = plugin(&platform, &sample());
        let row = plugin.query("td").remove(0);
        plugin.execute(&row).unwrap();
        let (text, _) = platform.pasted.lock().unwrap()[0].clone();
        assert_eq!(text.len(), 10);
        assert_eq!(text.matches('-').count(), 2);
        assert_eq!(text, Local::now().format("%Y-%m-%d").to_string());
    }

    #[test]
    fn paste_honours_restore_clipboard() {
        let platform = MockPlatform::empty();
        let paste = PasteConfig {
            restore_clipboard: true,
        };
        let plugin = SnippetsPlugin::new(&sample(), &paste, platform.clone());
        let row = plugin.query("meet").remove(0);
        plugin.execute(&row).unwrap();
        assert!(platform.pasted.lock().unwrap()[0].1);
    }

    #[test]
    fn where_pasting_is_unavailable_the_expansion_is_copied() {
        let platform = MockPlatform::empty();
        *platform.copy_only.lock().unwrap() = Some("Pasting is not possible on Wayland".into());
        let plugin = plugin(&platform, &sample());
        let row = plugin.query("meet").remove(0);
        assert_eq!(
            row.subtitle,
            "https://meet.example.com/ninad · Copies to clipboard · Pasting is not possible on Wayland"
        );
        plugin.execute(&row).unwrap();
        assert!(platform.pasted.lock().unwrap().is_empty());
        assert_eq!(
            *platform.clipboard.lock().unwrap(),
            ["https://meet.example.com/ninad"]
        );
    }

    #[test]
    fn execute_uses_the_current_definition_after_a_config_change() {
        let platform = MockPlatform::empty();
        let old = plugin(&platform, &[snippet("Greeting", None, "old text")]);
        let row = old.query("").remove(0);
        // The config changed (same name, new text) and a new plugin is live.
        let new = plugin(&platform, &[snippet("Greeting", None, "new text")]);
        new.execute(&row).unwrap();
        assert_eq!(platform.pasted.lock().unwrap()[0].0, "new text");
    }

    #[test]
    fn first_lines_are_shortened() {
        assert_eq!(first_line("one", 10), "one");
        assert_eq!(first_line("one\ntwo", 10), "one…");
        assert_eq!(first_line("\n\n one \n", 10), "one");
        assert_eq!(first_line("abcdef", 3), "abc…");
    }
}
