//! The configuration as the diagnostics report shows it.
//!
//! An allow-list: only the fields named below appear, so a setting added to
//! the config later stays out of reports until someone decides it may go in.
//! Some fields are reported as a fact about the value, never the value:
//! `Count` for lists (folders to search, snippets: how many), `Set` for
//! settings that may hold a name, a path or an address (is it set?), `Bytes`
//! for text (how long). The stylesheet, the folders to search, the snippets,
//! web search addresses, the account and what `[[hotkey]]` entries type or run
//! never appear.

use serde_json::Value;

use crate::config::Config;

/// How one setting is reported.
#[derive(Debug, Clone, Copy)]
enum Show {
    /// The value itself.
    Value,
    /// Only how many entries a list has.
    Count,
    /// Only whether it is set (not empty).
    Set,
    /// Only how many bytes of text it holds.
    Bytes,
}

type Field = (&'static str, Show);

/// `(group title, fields)`. A path is `section.key` of the serialized config.
const GROUPS: &[(&str, &[Field])] = &[
    (
        "General",
        &[
            ("general.hotkey", Show::Value),
            ("general.actions_hotkey", Show::Value),
            ("general.hide_on_blur", Show::Value),
            ("general.launch_at_login", Show::Value),
            ("general.check_for_updates", Show::Value),
            // The beta update channel (stable or beta).
            ("general.update_channel", Show::Value),
            ("window.width", Show::Value),
            ("linux.wayland_use_xwayland", Show::Value),
        ],
    ),
    (
        "Appearance",
        &[
            ("appearance.theme", Show::Value),
            ("appearance.accent", Show::Set),
            ("appearance.font_size", Show::Value),
            ("appearance.font_family", Show::Set),
            ("appearance.opacity", Show::Value),
            ("appearance.blur", Show::Value),
            ("appearance.radius", Show::Value),
            ("appearance.theme_file", Show::Set),
            ("appearance.custom_css", Show::Bytes),
        ],
    ),
    (
        "Search",
        &[
            ("search.max_results", Show::Value),
            ("search.fallback_web_search", Show::Value),
            ("search.query_history", Show::Value),
            ("plugins.disabled", Show::Value),
            ("calculator.currency", Show::Value),
        ],
    ),
    (
        "Files and bookmarks",
        &[
            ("files.directories", Show::Count),
            ("files.max_depth", Show::Value),
            ("files.include_hidden", Show::Value),
            ("files.global", Show::Value),
            ("files.use_os_index", Show::Value),
            ("bookmarks.browsers", Show::Value),
            ("bookmarks.global", Show::Value),
        ],
    ),
    (
        "System, tasks and media",
        &[
            ("system.confirm", Show::Value),
            ("system.disabled", Show::Value),
            ("tasks.confirm", Show::Value),
            ("tasks.disabled", Show::Value),
            ("tasks.global", Show::Value),
            ("media.global", Show::Value),
            ("media.now_playing", Show::Value),
            ("window_management.enabled", Show::Value),
            ("window_management.gap", Show::Value),
            ("window_management.global", Show::Value),
            ("shell.terminal", Show::Value),
            ("shell.shell", Show::Value),
            ("shell.keep_open", Show::Value),
        ],
    ),
    (
        "Pasting, clipboard and snippets",
        &[
            ("paste.restore_clipboard", Show::Value),
            ("actions.use_primary_selection", Show::Value),
            ("actions.use_clipboard_fallback", Show::Value),
            ("clipboard.enabled", Show::Value),
            ("clipboard.max_items", Show::Value),
            ("clipboard.max_item_bytes", Show::Value),
            ("clipboard.ignore_apps", Show::Count),
            ("clipboard.images", Show::Value),
            ("clipboard.files", Show::Value),
            ("clipboard.max_image_bytes", Show::Value),
            ("file_buffer.keep_between_shows", Show::Value),
            ("snippets.auto_expand", Show::Value),
            ("snippets.prefix", Show::Value),
            ("snippets.expand_on", Show::Value),
            ("snippets.case_sensitive", Show::Value),
            ("snippets.ignore_apps", Show::Count),
            ("snippets.expand_in_terminals", Show::Value),
            ("snippet", Show::Count),
        ],
    ),
    (
        "Contacts, 1Password and dictionary",
        &[
            ("contacts.enabled", Show::Value),
            ("contacts.use_system", Show::Value),
            ("contacts.vcard_files", Show::Count),
            ("onepassword.enabled", Show::Value),
            ("onepassword.op_path", Show::Set),
            ("onepassword.account", Show::Set),
            ("onepassword.cache_minutes", Show::Value),
            ("dictionary.use_system", Show::Value),
        ],
    ),
];

/// Top-level sections reported by custom code or deliberately left out; every
/// other section of the config must appear in [`GROUPS`] (a test checks).
#[cfg(test)]
const SPECIAL_SECTIONS: &[&str] = &["web_search", "hotkey"];

/// One group of the summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryGroup {
    pub title: String,
    /// `(setting, value)`; the setting is `section.key` as in `config.toml`.
    pub lines: Vec<(String, String)>,
}

fn lookup<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(root, |value, key| value.get(key))
}

fn plain(value: &Value) -> String {
    match value {
        Value::String(text) if text.is_empty() => "(empty)".to_owned(),
        Value::String(text) => text.clone(),
        Value::Array(items) if items.is_empty() => "(none)".to_owned(),
        Value::Array(items) => items.iter().map(plain).collect::<Vec<_>>().join(", "),
        Value::Null => "(none)".to_owned(),
        other => other.to_string(),
    }
}

fn show(value: &Value, how: Show) -> String {
    match how {
        Show::Value => plain(value),
        Show::Count => match value {
            Value::Array(items) => items.len().to_string(),
            _ => "?".to_owned(),
        },
        Show::Set => {
            let empty = match value {
                Value::String(text) => text.trim().is_empty(),
                Value::Null => true,
                Value::Array(items) => items.is_empty(),
                _ => false,
            };
            if empty { "not set" } else { "set" }.to_owned()
        }
        Show::Bytes => match value {
            Value::String(text) if !text.is_empty() => format!("{} bytes", text.len()),
            _ => "none".to_owned(),
        },
    }
}

/// Why `text` is not a usable config file, in one line (`line 12, column 9:
/// invalid type...`), or `None` if it is. The parser's own message quotes the
/// offending lines of the file; this keeps only its explanation.
pub fn config_problem(text: &str) -> Option<String> {
    let error = Config::from_toml_str(text).err()?;
    let message = error.message().replace(['\n', '\r'], " ");
    Some(match error.span() {
        Some(span) => {
            let before = &text[..span.start.min(text.len())];
            let line = before.matches('\n').count() + 1;
            let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
            format!("line {line}, column {column}: {message}")
        }
        None => message,
    })
}

/// The keywords of the plugins that have their own `keyword` setting.
const KEYWORD_PATHS: &[&str] = &[
    "files.keyword",
    "files.index_keyword",
    "files.content_keyword",
    "bookmarks.keyword",
    "tasks.keyword",
    "media.keyword",
    "window_management.keyword",
    "window_management.switcher_keyword",
    "contacts.keyword",
    "onepassword.keyword",
    "dictionary.define_keyword",
    "dictionary.spell_keyword",
];

/// The configurable keywords of `config`, as `(setting, keyword)`; a search
/// that is off (empty keyword) is left out.
pub fn configured_keywords(config: &Config) -> Vec<(String, String)> {
    let root = serde_json::to_value(config).unwrap_or(Value::Null);
    KEYWORD_PATHS
        .iter()
        .filter_map(|path| {
            let keyword = lookup(&root, path)?.as_str()?.trim();
            (!keyword.is_empty()).then(|| ((*path).to_owned(), keyword.to_owned()))
        })
        .collect()
}

/// The web search engines' keywords (never their names or addresses).
pub fn web_search_keywords(config: &Config) -> Vec<String> {
    config
        .web_search
        .iter()
        .map(|engine| engine.keyword.trim().to_owned())
        .filter(|keyword| !keyword.is_empty())
        .collect()
}

/// The `[[hotkey]]` entries as `key -> kind` (what they type or run is not
/// shown).
pub fn hotkey_entries(config: &Config) -> Vec<String> {
    config
        .hotkeys
        .iter()
        .map(|binding| {
            let kind = match (&binding.query, &binding.run) {
                (Some(_), None) => "opens with text",
                (None, Some(_)) => "runs a result",
                _ => "invalid",
            };
            format!("{} -> {kind}", binding.key.trim())
        })
        .collect()
}

/// Summarizes `config` (see the module docs for what is left out).
pub fn summarize(config: &Config) -> Vec<SummaryGroup> {
    let root = serde_json::to_value(config).unwrap_or(Value::Null);
    let mut groups: Vec<SummaryGroup> = GROUPS
        .iter()
        .map(|(title, fields)| SummaryGroup {
            title: (*title).to_owned(),
            lines: fields
                .iter()
                .filter_map(|(path, how)| {
                    Some(((*path).to_owned(), show(lookup(&root, path)?, *how)))
                })
                .collect(),
        })
        .collect();
    groups.push(SummaryGroup {
        title: "Web search and hotkeys".to_owned(),
        lines: vec![
            (
                "web_search (keywords)".to_owned(),
                join_or_none(&web_search_keywords(config)),
            ),
            (
                "hotkey (extra shortcuts)".to_owned(),
                join_or_none(&hotkey_entries(config)),
            ),
        ],
    });
    groups.retain(|group| !group.lines.is_empty());
    groups
}

fn join_or_none(items: &[String]) -> String {
    if items.is_empty() {
        "(none)".to_owned()
    } else {
        items.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{HotkeyBinding, Snippet, WebSearchEngine};

    fn lines(config: &Config) -> Vec<(String, String)> {
        summarize(config)
            .into_iter()
            .flat_map(|group| group.lines)
            .collect()
    }

    fn value_of(config: &Config, setting: &str) -> Option<String> {
        lines(config)
            .into_iter()
            .find(|(name, _)| name == setting)
            .map(|(_, value)| value)
    }

    #[test]
    fn every_section_of_the_config_is_decided() {
        let root = serde_json::to_value(Config::default()).unwrap();
        for section in root.as_object().unwrap().keys() {
            let reported = GROUPS
                .iter()
                .flat_map(|(_, fields)| fields.iter())
                .any(|(path, _)| path.split('.').next() == Some(section.as_str()));
            assert!(
                reported || SPECIAL_SECTIONS.contains(&section.as_str()),
                "the config section [{section}] is neither in the diagnostics summary nor \
                 deliberately left out: add it to GROUPS or SPECIAL_SECTIONS in \
                 config_summary.rs (new settings stay out of reports until you do)"
            );
        }
    }

    #[test]
    fn every_summarized_path_exists() {
        let root = serde_json::to_value(Config::default()).unwrap();
        for (_, fields) in GROUPS {
            for (path, _) in *fields {
                assert!(
                    lookup(&root, path).is_some(),
                    "{path} is not a config setting"
                );
            }
        }
        for path in KEYWORD_PATHS {
            assert!(
                lookup(&root, path).is_some(),
                "{path} is not a config setting"
            );
        }
    }

    #[test]
    fn the_defaults_are_summarized() {
        let config = Config::default();
        assert_eq!(
            value_of(&config, "general.hotkey").as_deref(),
            Some("Super+Space")
        );
        assert_eq!(
            value_of(&config, "general.check_for_updates").as_deref(),
            Some("true")
        );
        assert_eq!(
            value_of(&config, "appearance.theme").as_deref(),
            Some("system")
        );
        assert_eq!(
            value_of(&config, "appearance.accent").as_deref(),
            Some("not set")
        );
        assert_eq!(
            value_of(&config, "appearance.custom_css").as_deref(),
            Some("none")
        );
        assert_eq!(
            value_of(&config, "plugins.disabled").as_deref(),
            Some("(none)")
        );
        assert_eq!(value_of(&config, "files.directories").as_deref(), Some("3"));
        assert_eq!(
            value_of(&config, "web_search (keywords)").as_deref(),
            Some("g, yt, gh")
        );
        assert_eq!(
            value_of(&config, "hotkey (extra shortcuts)").as_deref(),
            Some("(none)")
        );
        assert_eq!(
            value_of(&config, "general.update_channel").as_deref(),
            Some("stable")
        );
    }

    #[test]
    fn private_values_never_reach_the_summary() {
        let mut config = Config::default();
        config.files.directories = vec![r"C:\Clients\SECRET-DIR".into()];
        config.appearance.custom_css = "body { /* SECRET-CSS */ }".into();
        config.appearance.accent = "#SECRET1".into();
        config.appearance.font_family = "SECRET-FONT".into();
        config.appearance.theme_file = "themes/SECRET-THEME.toml".into();
        config.clipboard.ignore_apps = vec!["SECRET-APP".into()];
        config.snippets.ignore_apps = vec!["SECRET-APP2".into()];
        config.contacts.vcard_files = vec!["~/SECRET-CONTACTS.vcf".into()];
        config.onepassword.op_path = "/SECRET/op".into();
        config.onepassword.account = "SECRET-ACCOUNT.1password.com".into();
        config.snippet = vec![Snippet {
            name: "SECRET-NAME".into(),
            keyword: Some("SECRET-KW".into()),
            text: "SECRET-TEXT".into(),
        }];
        config.web_search = vec![WebSearchEngine {
            keyword: "kw".into(),
            name: "SECRET-ENGINE".into(),
            url: "https://secret.example/?token=SECRET-TOKEN&q={query}".into(),
        }];
        config.hotkeys = vec![
            HotkeyBinding {
                key: "Ctrl+Alt+T".into(),
                query: Some("SECRET-QUERY".into()),
                run: None,
            },
            HotkeyBinding {
                key: "Ctrl+Alt+F".into(),
                query: None,
                run: Some("files:SECRET-RUN".into()),
            },
        ];
        let text = format!("{:?}", summarize(&config));
        assert!(!text.contains("SECRET"), "{text}");

        // The facts about them are there.
        assert_eq!(value_of(&config, "files.directories").as_deref(), Some("1"));
        assert_eq!(value_of(&config, "snippet").as_deref(), Some("1"));
        assert_eq!(
            value_of(&config, "appearance.accent").as_deref(),
            Some("set")
        );
        assert_eq!(
            value_of(&config, "appearance.custom_css").as_deref(),
            Some("25 bytes")
        );
        assert_eq!(
            value_of(&config, "hotkey (extra shortcuts)").as_deref(),
            Some("Ctrl+Alt+T -> opens with text, Ctrl+Alt+F -> runs a result")
        );
        assert_eq!(
            value_of(&config, "web_search (keywords)").as_deref(),
            Some("kw")
        );
        assert!(!format!("{:?}", configured_keywords(&config)).contains("SECRET"));
    }

    #[test]
    fn keywords_that_are_off_are_left_out() {
        let mut config = Config::default();
        config.files.keyword = String::new();
        config.tasks.keyword = "t".into();
        let keywords = configured_keywords(&config);
        assert!(keywords.iter().all(|(name, _)| name != "files.keyword"));
        assert!(keywords.contains(&("tasks.keyword".to_owned(), "t".to_owned())));
        assert!(keywords.contains(&("files.index_keyword".to_owned(), "ff".to_owned())));
    }

    #[test]
    fn a_config_problem_names_the_line_not_the_text() {
        assert_eq!(
            config_problem(
                "[general]
hotkey = \"Alt+Space\"
"
            ),
            None
        );
        assert_eq!(config_problem(""), None);
        let text = "[general]
hotkey = \"SECRET-KEY\"
[window]
width = \"SECRET-WIDTH\"
";
        let problem = config_problem(text).unwrap();
        assert!(problem.starts_with("line 4, column 9: "), "{problem}");
        assert!(!problem.contains("hotkey ="), "{problem}");
        let problem = config_problem(
            "[general
hotkey = 1",
        )
        .unwrap();
        assert!(problem.starts_with("line 1"), "{problem}");
        assert!(!problem.contains('\n'));
    }

    #[test]
    fn the_update_channel_is_shown_as_it_is_set() {
        let mut root = serde_json::to_value(Config::default()).unwrap();
        root["general"]["update_channel"] = Value::String("beta".into());
        let (path, how) = ("general.update_channel", Show::Value);
        assert_eq!(show(lookup(&root, path).unwrap(), how), "beta");
    }
}
