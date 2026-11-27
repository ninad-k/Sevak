//! `? ` searches Sevak's own settings: the sections of the Settings window and
//! a few well-known options inside them. Enter opens Settings on that section.
//!
//! The list lives in `settings_index.json`. The Settings window imports the
//! same file for its sidebar filter, so the two cannot drift apart.
//!
//! The plugin cannot open a window itself (this crate has no UI). The shell
//! gives it an `opener` closure; `execute` calls it with the section id.

use std::sync::{Arc, OnceLock};

use serde::Deserialize;
use sevak_core::fuzzy::FuzzyQuery;
use sevak_core::model::score;
use sevak_core::{Action, IconSource, Plugin, PluginResult, ResultItem};

/// The keyword that routes a query to this plugin.
pub const KEYWORD: &str = "?";

/// Prefix of the [`Action::Custom`] payload; the section id follows.
pub const PAYLOAD_PREFIX: &str = "open:";

/// One row of the index: a section, or an option inside one.
#[derive(Debug, Clone, Deserialize)]
pub struct IndexEntry {
    /// Section id for a section; `section.option` for an option.
    pub id: String,
    /// The Settings section it opens.
    pub section: String,
    /// English title.
    pub title: String,
    /// Space-separated synonyms.
    pub keywords: String,
}

/// Every searchable entry, sections first.
pub fn settings_index() -> &'static [IndexEntry] {
    static INDEX: OnceLock<Vec<IndexEntry>> = OnceLock::new();
    INDEX.get_or_init(|| {
        serde_json::from_str(include_str!("settings_index.json")).unwrap_or_default()
    })
}

/// Opens Settings on the section with this id.
pub type SettingsOpener = Arc<dyn Fn(&str) + Send + Sync>;

pub struct SettingsSearchPlugin {
    opener: SettingsOpener,
}

impl SettingsSearchPlugin {
    pub fn new(opener: SettingsOpener) -> Self {
        Self { opener }
    }
}

fn available(entry: &IndexEntry) -> bool {
    entry.section != "linux" || cfg!(target_os = "linux")
}

/// Entries matching `input`, best first. An empty input lists the sections.
pub fn filter_index<'a>(index: &'a [IndexEntry], input: &str) -> Vec<(&'a IndexEntry, u32)> {
    let mut query = FuzzyQuery::new(input.trim());
    if query.is_empty() {
        return index
            .iter()
            .filter(|e| available(e) && e.id == e.section)
            .map(|e| (e, 0))
            .collect();
    }
    let mut hits: Vec<(&IndexEntry, u32)> = index
        .iter()
        .filter(|e| available(e))
        .filter_map(|entry| {
            // A title hit counts double, so "theme" lists the Theme option
            // above sections that only mention it in their synonyms.
            let title = query.score(&entry.title).map(|s| s * 2);
            let words = query.score(&entry.keywords);
            title.max(words).map(|s| (entry, s))
        })
        .collect();
    hits.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.title.cmp(&b.0.title)));
    hits
}

impl Plugin for SettingsSearchPlugin {
    fn id(&self) -> &str {
        "settings"
    }

    fn name(&self) -> &str {
        "Sevak settings"
    }

    fn description(&self) -> &str {
        "Type `? ` and a word to find a setting; Enter opens Settings there."
    }

    fn keyword(&self) -> Option<&str> {
        Some(KEYWORD)
    }

    fn global(&self) -> bool {
        false
    }

    fn query(&self, input: &str) -> Vec<ResultItem> {
        filter_index(settings_index(), input)
            .into_iter()
            .map(|(entry, fuzzy)| {
                let is_section = entry.id == entry.section;
                let section_title = settings_index()
                    .iter()
                    .find(|e| e.id == entry.section)
                    .map_or(entry.section.as_str(), |e| e.title.as_str());
                ResultItem::new(
                    self.id(),
                    &entry.id,
                    &entry.title,
                    Action::Custom {
                        payload: format!("{PAYLOAD_PREFIX}{}", entry.section),
                    },
                )
                .with_subtitle(if is_section {
                    "Open this section of Settings".to_owned()
                } else {
                    format!("In Settings: {section_title}")
                })
                .with_icon(IconSource::builtin("settings"))
                .with_score(score::KEYWORD + f64::from(fuzzy))
            })
            .collect()
    }

    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        if let Action::Custom { payload } = &item.action {
            if let Some(section) = payload.strip_prefix(PAYLOAD_PREFIX) {
                (self.opener)(section);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    fn plugin() -> (SettingsSearchPlugin, Arc<Mutex<Vec<String>>>) {
        let opened = Arc::new(Mutex::new(Vec::new()));
        let sink = opened.clone();
        let plugin = SettingsSearchPlugin::new(Arc::new(move |section| {
            sink.lock().unwrap().push(section.to_owned());
        }));
        (plugin, opened)
    }

    #[test]
    fn index_parses_and_options_point_at_sections() {
        let index = settings_index();
        assert!(index.len() > 20);
        for entry in index {
            assert!(
                index.iter().any(|s| s.id == entry.section),
                "{} points at unknown section {}",
                entry.id,
                entry.section
            );
        }
    }

    #[test]
    fn the_ui_reads_the_same_index() {
        let ui = include_str!("../../../ui/src/lib/settings-index.json");
        assert_eq!(ui, include_str!("settings_index.json"));
    }

    #[test]
    fn empty_query_lists_sections_only() {
        let (plugin, _) = plugin();
        let items = plugin.query("");
        assert!(items.iter().any(|i| i.title == "Hotkeys"));
        assert!(items.iter().all(|i| !i.id.contains('.')));
    }

    #[test]
    fn filters_by_title_and_synonym() {
        let (plugin, _) = plugin();
        let items = plugin.query("clipboard history");
        assert_eq!(items[0].title, "Clipboard history");
        let items = plugin.query("dark");
        assert!(items.iter().any(|i| i.title == "Theme"));
        assert!(plugin.query("zzzzqqq").is_empty());
    }

    #[test]
    fn execute_opens_the_section() {
        let (plugin, opened) = plugin();
        let item = plugin
            .query("hotkey")
            .into_iter()
            .find(|i| i.title == "Hotkeys")
            .unwrap();
        assert_eq!(
            item.action,
            Action::Custom {
                payload: "open:hotkeys".to_owned()
            }
        );
        plugin.execute(&item).unwrap();
        assert_eq!(*opened.lock().unwrap(), ["hotkeys"]);
    }

    #[test]
    fn option_rows_open_their_section() {
        let (plugin, opened) = plugin();
        let item = plugin
            .query("encrypt")
            .into_iter()
            .next()
            .expect("clipboard option matches");
        plugin.execute(&item).unwrap();
        assert_eq!(*opened.lock().unwrap(), ["clipboard"]);
    }

    #[test]
    fn foreign_actions_are_ignored() {
        let (plugin, opened) = plugin();
        let item = ResultItem::new(
            "settings",
            "x",
            "x",
            Action::Custom {
                payload: "other".to_owned(),
            },
        );
        plugin.execute(&item).unwrap();
        assert!(opened.lock().unwrap().is_empty());
    }
}
