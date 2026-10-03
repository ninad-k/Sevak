//! Installed-application search.

use std::sync::{Arc, RwLock};

use sevak_core::{
    Action, AppEntry, FuzzyQuery, IconSource, LaunchTarget, Modifier, Plugin, PluginError,
    PluginResult, ResultItem,
};
use sevak_platform::PlatformProvider;

use crate::actions::execute_action;

const MAX_CANDIDATES: usize = 50;
/// Keyword matches are weaker evidence than a name match.
const KEYWORD_WEIGHT: f64 = 0.7;
/// Descriptions are weaker still.
const DESCRIPTION_WEIGHT: f64 = 0.4;
/// Added when the name starts with the query.
const PREFIX_BONUS: f64 = 30.0;
/// Added when a word inside the name starts with the query.
const WORD_PREFIX_BONUS: f64 = 15.0;
/// Breaks ties in favour of shorter (more specific) names.
const LENGTH_PENALTY: f64 = 0.01;

struct IndexedApp {
    entry: AppEntry,
    name_lower: String,
}

/// Searches the applications reported by the platform provider.
///
/// Queries read an immutable snapshot of the index, so a running
/// [`Plugin::refresh`] never delays them.
pub struct AppsPlugin {
    platform: Arc<dyn PlatformProvider>,
    index: RwLock<Arc<Vec<IndexedApp>>>,
}

impl AppsPlugin {
    pub fn new(platform: Arc<dyn PlatformProvider>) -> Self {
        Self {
            platform,
            index: RwLock::new(Arc::new(Vec::new())),
        }
    }

    fn snapshot(&self) -> Arc<Vec<IndexedApp>> {
        self.index
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

pub(crate) fn name_bonus(name_lower: &str, query_lower: &str) -> f64 {
    if query_lower.is_empty() {
        return 0.0;
    }
    if name_lower.starts_with(query_lower) {
        return PREFIX_BONUS;
    }
    let word_start = name_lower
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| !word.is_empty() && word.starts_with(query_lower));
    if word_start {
        WORD_PREFIX_BONUS
    } else {
        0.0
    }
}

/// Adds the secondary actions an application supports: Ctrl+Enter shows it in
/// the file manager, Shift+Enter copies its path, Alt+Enter runs it as
/// administrator (only where the platform can).
fn with_app_actions(item: ResultItem, target: &LaunchTarget, can_run_as_admin: bool) -> ResultItem {
    let mut item = item;
    if let Some(path) = target.path() {
        item = item
            .with_secondary(
                "Show in folder",
                Some(Modifier::Ctrl),
                Action::RevealPath {
                    path: path.to_path_buf(),
                },
            )
            .with_secondary(
                "Copy path",
                Some(Modifier::Shift),
                Action::CopyText {
                    text: path.to_string_lossy().into_owned(),
                },
            );
    }
    // Store apps cannot be elevated.
    if can_run_as_admin && !matches!(target, LaunchTarget::PackagedApp { .. }) {
        item = item.with_secondary(
            "Run as administrator",
            Some(Modifier::Alt),
            Action::RunAsAdmin {
                target: target.clone(),
            },
        );
    }
    item
}

impl Plugin for AppsPlugin {
    fn id(&self) -> &str {
        "apps"
    }

    fn name(&self) -> &str {
        "Applications"
    }

    fn description(&self) -> &str {
        "Launches installed applications."
    }

    fn keyword(&self) -> Option<&str> {
        None
    }

    fn query(&self, input: &str) -> Vec<ResultItem> {
        let input = input.trim();
        let mut query = FuzzyQuery::new(input);
        if query.is_empty() {
            return Vec::new();
        }
        let query_lower = input.to_lowercase();
        let index = self.snapshot();

        let mut scored: Vec<(f64, &IndexedApp)> = Vec::new();
        for app in index.iter() {
            let entry = &app.entry;
            let mut best = query.score(&entry.name).map(|s| {
                f64::from(s) + name_bonus(&app.name_lower, &query_lower)
                    - app.name_lower.len() as f64 * LENGTH_PENALTY
            });
            for keyword in &entry.keywords {
                if let Some(s) = query.score(keyword) {
                    let s = f64::from(s) * KEYWORD_WEIGHT;
                    best = Some(best.map_or(s, |b| b.max(s)));
                }
            }
            if let Some(description) = &entry.description {
                if let Some(s) = query.score(description) {
                    let s = f64::from(s) * DESCRIPTION_WEIGHT;
                    best = Some(best.map_or(s, |b| b.max(s)));
                }
            }
            if let Some(best) = best {
                scored.push((best, app));
            }
        }

        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        scored.truncate(MAX_CANDIDATES);

        let can_run_as_admin = self.platform.can_run_as_admin();
        scored
            .into_iter()
            .map(|(score, app)| {
                let entry = &app.entry;
                let item = ResultItem::new(
                    "apps",
                    &entry.id,
                    &entry.name,
                    Action::Launch {
                        target: entry.target.clone(),
                    },
                )
                .with_subtitle(
                    entry
                        .description
                        .as_deref()
                        .filter(|d| !d.trim().is_empty())
                        .unwrap_or("Application"),
                )
                .with_icon(
                    entry
                        .icon
                        .clone()
                        .unwrap_or_else(|| IconSource::builtin("app")),
                )
                .with_score(score);
                with_app_actions(item, &entry.target, can_run_as_admin)
            })
            .collect()
    }

    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        execute_action(self.platform.as_ref(), &item.action)
    }

    fn refresh(&self) -> PluginResult<()> {
        let started = std::time::Instant::now();
        let apps = self
            .platform
            .list_applications()
            .map_err(PluginError::other)?;
        let indexed: Vec<IndexedApp> = apps
            .into_iter()
            .map(|entry| IndexedApp {
                name_lower: entry.name.to_lowercase(),
                entry,
            })
            .collect();
        tracing::info!(
            count = indexed.len(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "applications indexed"
        );
        *self
            .index
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Arc::new(indexed);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::test_util::MockPlatform;

    fn app(id: &str, name: &str, keywords: &[&str], description: Option<&str>) -> AppEntry {
        AppEntry {
            id: id.to_owned(),
            name: name.to_owned(),
            description: description.map(str::to_owned),
            keywords: keywords.iter().map(|k| (*k).to_owned()).collect(),
            icon: None,
            target: LaunchTarget::Executable {
                path: PathBuf::from(format!("/bin/{id}")),
                args: Vec::new(),
                working_dir: None,
            },
        }
    }

    fn fixture() -> Vec<AppEntry> {
        vec![
            app("sniffer", "Sniffer", &[], None),
            app("files", "Files", &[], None),
            app("firewall", "Firewall Configuration", &[], None),
            app(
                "firefox",
                "Firefox",
                &["browser", "web"],
                Some("Browse the World Wide Web"),
            ),
            app("safari", "Safari Info Reader", &[], None),
        ]
    }

    fn plugin_with(apps: Vec<AppEntry>) -> (AppsPlugin, Arc<MockPlatform>) {
        let platform = MockPlatform::with_apps(apps);
        let plugin = AppsPlugin::new(platform.clone());
        plugin.refresh().unwrap();
        (plugin, platform)
    }

    #[test]
    fn empty_before_first_refresh() {
        let platform = MockPlatform::with_apps(fixture());
        let plugin = AppsPlugin::new(platform);
        assert!(plugin.query("fir").is_empty());
    }

    #[test]
    fn empty_query_returns_nothing() {
        let (plugin, _) = plugin_with(fixture());
        assert!(plugin.query("   ").is_empty());
    }

    #[test]
    fn prefix_matches_rank_first_and_non_matches_are_absent() {
        let (plugin, _) = plugin_with(fixture());
        let results = plugin.query("fir");
        let titles: Vec<&str> = results.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles[0], "Firefox", "{titles:?}");
        assert!(titles.contains(&"Firewall Configuration"));
        assert!(!titles.contains(&"Sniffer"));
        assert!(!titles.contains(&"Files"));
        let firefox = titles.iter().position(|t| *t == "Firefox").unwrap();
        if let Some(scattered) = titles.iter().position(|t| *t == "Safari Info Reader") {
            assert!(firefox < scattered);
        }
    }

    #[test]
    fn prefix_beats_scattered_name() {
        let (plugin, _) = plugin_with(vec![
            app("a", "Safari Info Reader", &[], None),
            app("b", "Firefox", &[], None),
        ]);
        let results = plugin.query("fir");
        assert_eq!(results[0].title, "Firefox");
    }

    #[test]
    fn keyword_match_finds_app() {
        let (plugin, _) = plugin_with(fixture());
        let results = plugin.query("browser");
        assert_eq!(results.first().map(|r| r.title.as_str()), Some("Firefox"));
    }

    #[test]
    fn name_match_outranks_keyword_match() {
        let (plugin, _) = plugin_with(vec![
            app("a", "Alpha", &["notes"], None),
            app("b", "Notes", &[], None),
        ]);
        let results = plugin.query("notes");
        assert_eq!(results[0].title, "Notes");
    }

    #[test]
    fn result_shape() {
        let (plugin, _) = plugin_with(fixture());
        let results = plugin.query("firefox");
        let item = &results[0];
        assert_eq!(item.id, "apps:firefox");
        assert_eq!(item.plugin_id, "apps");
        assert_eq!(item.subtitle, "Browse the World Wide Web");
        assert_eq!(item.icon, Some(IconSource::builtin("app")));
        assert!(matches!(item.action, Action::Launch { .. }));

        let results = plugin.query("sniffer");
        assert_eq!(results[0].subtitle, "Application");
    }

    #[test]
    fn at_most_fifty_candidates() {
        let apps = (0..200)
            .map(|i| app(&format!("app{i}"), &format!("App {i}"), &[], None))
            .collect();
        let (plugin, _) = plugin_with(apps);
        assert_eq!(plugin.query("app").len(), MAX_CANDIDATES);
    }

    #[test]
    fn refresh_replaces_index() {
        let (plugin, platform) = plugin_with(fixture());
        assert!(!plugin.query("firefox").is_empty());
        *platform.apps.lock().unwrap() = vec![app("x", "Xterm", &[], None)];
        plugin.refresh().unwrap();
        assert!(plugin.query("firefox").is_empty());
        assert_eq!(plugin.query("xterm").len(), 1);
    }

    fn secondary_summary(item: &ResultItem) -> Vec<(&str, Option<Modifier>)> {
        item.secondary
            .iter()
            .map(|s| (s.label.as_str(), s.modifier))
            .collect()
    }

    #[test]
    fn apps_offer_reveal_and_copy_path() {
        let (plugin, _) = plugin_with(fixture());
        let item = plugin.query("firefox").remove(0);
        assert_eq!(
            secondary_summary(&item),
            [
                ("Show in folder", Some(Modifier::Ctrl)),
                ("Copy path", Some(Modifier::Shift)),
            ]
        );
        assert_eq!(
            item.secondary[0].action,
            Action::RevealPath {
                path: PathBuf::from("/bin/firefox")
            }
        );
        assert_eq!(
            item.secondary[1].action,
            Action::CopyText {
                text: PathBuf::from("/bin/firefox").to_string_lossy().into_owned()
            }
        );
        assert_eq!(item.copy_text(), item.secondary[1].action.copy_text());
    }

    #[test]
    fn run_as_administrator_only_where_the_platform_supports_it() {
        let platform = MockPlatform::with_admin();
        *platform.apps.lock().unwrap() = fixture();
        let plugin = AppsPlugin::new(platform.clone());
        plugin.refresh().unwrap();
        let item = plugin.query("firefox").remove(0);
        let admin = item.secondary.last().unwrap();
        assert_eq!(admin.label, "Run as administrator");
        assert_eq!(admin.modifier, Some(Modifier::Alt));

        plugin
            .execute(&ResultItem {
                action: admin.action.clone(),
                ..item.clone()
            })
            .unwrap();
        assert_eq!(platform.elevated.lock().unwrap().len(), 1);
        assert!(platform.launched.lock().unwrap().is_empty());

        let (plain, _) = plugin_with(fixture());
        let item = plain.query("firefox").remove(0);
        assert!(item
            .secondary
            .iter()
            .all(|s| s.modifier != Some(Modifier::Alt)));
    }

    #[test]
    fn packaged_apps_have_no_path_actions_and_cannot_be_elevated() {
        let mut store_app = app("calc", "Calculator", &[], None);
        store_app.target = LaunchTarget::PackagedApp {
            app_user_model_id: "Microsoft.Calc!App".into(),
        };
        let platform = MockPlatform::with_admin();
        *platform.apps.lock().unwrap() = vec![store_app];
        let plugin = AppsPlugin::new(platform);
        plugin.refresh().unwrap();
        let item = plugin.query("calculator").remove(0);
        assert!(item.secondary.is_empty());
        assert_eq!(item.copy_text(), None);
    }

    #[test]
    fn mac_bundles_reveal_the_app_not_open() {
        let mut safari = app("safari", "Safari", &[], None);
        safari.target = LaunchTarget::Executable {
            path: PathBuf::from("/usr/bin/open"),
            args: vec!["-a".into(), "/Applications/Safari.app".into()],
            working_dir: None,
        };
        let (plugin, _) = plugin_with(vec![safari]);
        let item = plugin.query("safari").remove(0);
        assert_eq!(
            item.secondary[0].action,
            Action::RevealPath {
                path: PathBuf::from("/Applications/Safari.app")
            }
        );
        assert_eq!(
            item.copy_text().as_deref(),
            Some("/Applications/Safari.app")
        );
    }

    #[test]
    fn execute_launches_the_right_target() {
        let (plugin, platform) = plugin_with(fixture());
        let results = plugin.query("firefox");
        plugin.execute(&results[0]).unwrap();
        let launched = platform.launched.lock().unwrap();
        assert_eq!(launched.len(), 1);
        assert_eq!(
            launched[0],
            LaunchTarget::Executable {
                path: PathBuf::from("/bin/firefox"),
                args: Vec::new(),
                working_dir: None,
            }
        );
    }
}
