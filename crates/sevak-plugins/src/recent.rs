//! Recent documents: `recent <name>` searches the files you used lately.
//!
//! | Key | Action |
//! |---|---|
//! | Enter | open the file |
//! | `Ctrl+Enter` | show it in the folder |
//! | `Shift+Enter` | copy the path |
//!
//! An empty query lists the newest files first; typing fuzzy-filters by file
//! name. Rows are built like the files plugin's, so the preview pane and the
//! file buffer treat them the same.
//!
//! The list comes from the OS's own recent lists
//! ([`PlatformProvider::recent_files`]). It is read off the keystroke path: at
//! startup and on demand by [`Plugin::refresh`], and again on a background
//! thread when a query finds the cache older than the TTL (the first query
//! after the launcher is shown). [`Plugin::query`] only reads the cache. The
//! list stays in memory; the log has counts, never paths.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use sevak_core::config::RecentConfig;
use sevak_core::model::score;
use sevak_core::{
    Action, FuzzyQuery, IconSource, Modifier, Plugin, PluginError, PluginResult, ResultItem,
    ResultsNotifier,
};
use sevak_platform::recent::RecentFile;
use sevak_platform::PlatformProvider;

use crate::actions::execute_action;
use crate::files::{home_dir, subtitle};

const ID: &str = "recent";
/// Rows an empty query lists.
const MAX_ROWS: usize = 50;
/// The cache is reloaded in the background when older than this.
const TTL: Duration = Duration::from_secs(30);
/// A name that starts with the input beats a looser match.
const PREFIX_BONUS: f64 = 60.0;
const PAYLOAD_NOTHING: &str = "nothing";

#[derive(Default)]
struct Snapshot {
    files: Vec<RecentFile>,
    loaded_at: Option<Instant>,
}

struct Inner {
    max_items: usize,
    platform: Arc<dyn PlatformProvider>,
    snapshot: RwLock<Arc<Snapshot>>,
    loading: AtomicBool,
    notifier: Mutex<Option<ResultsNotifier>>,
}

impl Inner {
    fn snapshot(&self) -> Arc<Snapshot> {
        self.snapshot
            .read()
            .map(|s| Arc::clone(&s))
            .unwrap_or_default()
    }

    /// Reads the OS lists. Slow; background thread.
    fn load(&self) {
        let files = self.platform.recent_files(self.max_items);
        tracing::info!(files = files.len(), "loaded recent documents");
        if let Ok(mut slot) = self.snapshot.write() {
            *slot = Arc::new(Snapshot {
                files,
                loaded_at: Some(Instant::now()),
            });
        }
    }
}

/// The `recent` plugin.
pub struct RecentDocsPlugin {
    config: RecentConfig,
    inner: Arc<Inner>,
    home: Option<PathBuf>,
    ttl: Duration,
}

impl RecentDocsPlugin {
    pub fn new(config: &RecentConfig, platform: Arc<dyn PlatformProvider>) -> Self {
        Self::with_ttl(config, platform, home_dir(), TTL)
    }

    fn with_ttl(
        config: &RecentConfig,
        platform: Arc<dyn PlatformProvider>,
        home: Option<PathBuf>,
        ttl: Duration,
    ) -> Self {
        Self {
            config: config.clone(),
            inner: Arc::new(Inner {
                max_items: config.max_items as usize,
                platform,
                snapshot: RwLock::new(Arc::new(Snapshot::default())),
                loading: AtomicBool::new(false),
                notifier: Mutex::new(None),
            }),
            home,
            ttl,
        }
    }

    fn is_stale(&self, snapshot: &Snapshot) -> bool {
        snapshot.loaded_at.is_none_or(|at| at.elapsed() >= self.ttl)
    }

    /// Starts one background reload unless one is already running, then tells
    /// the shell to re-run the query.
    fn reload_in_background(&self) {
        if self
            .inner
            .loading
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let inner = Arc::clone(&self.inner);
        let spawned = std::thread::Builder::new()
            .name("recent-docs".to_owned())
            .spawn(move || {
                inner.load();
                inner.loading.store(false, Ordering::Release);
                let notifier = inner.notifier.lock().ok().and_then(|n| n.clone());
                if let Some(notify) = notifier {
                    notify(ID);
                }
            });
        if spawned.is_err() {
            self.inner.loading.store(false, Ordering::Release);
        }
    }

    fn status(&self, key: &str, title: &str, subtitle: &str) -> ResultItem {
        ResultItem::new(
            ID,
            key,
            title,
            Action::Custom {
                payload: PAYLOAD_NOTHING.to_owned(),
            },
        )
        .with_subtitle(subtitle)
        .with_icon(IconSource::builtin("file"))
        .with_score(score::KEYWORD)
    }

    fn row(&self, file: &RecentFile, score: f64) -> ResultItem {
        let path_string = file.path.to_string_lossy().into_owned();
        let icon = if cfg!(windows) {
            IconSource::Shell {
                parsing_name: path_string.clone(),
            }
        } else {
            IconSource::builtin("file")
        };
        ResultItem::new(
            ID,
            &path_string,
            &file.name,
            Action::OpenPath {
                path: file.path.clone(),
            },
        )
        .with_secondary(
            "Show in folder",
            Some(Modifier::Ctrl),
            Action::RevealPath {
                path: file.path.clone(),
            },
        )
        .with_secondary(
            "Copy path",
            Some(Modifier::Shift),
            Action::CopyText {
                text: path_string.clone(),
            },
        )
        .with_subtitle(subtitle(&file.path, self.home.as_deref()))
        .with_icon(icon)
        .with_score(score)
    }

    fn matches(&self, files: &[RecentFile], input: &str) -> Vec<ResultItem> {
        if input.is_empty() {
            return files
                .iter()
                .take(MAX_ROWS)
                .enumerate()
                .map(|(i, f)| self.row(f, score::KEYWORD - i as f64))
                .collect();
        }
        let lower = input.to_lowercase();
        let mut names = FuzzyQuery::new(input);
        let mut scored: Vec<(f64, usize, &RecentFile)> = files
            .iter()
            .enumerate()
            .filter_map(|(i, file)| {
                let s = f64::from(names.score(&file.name)?);
                let bonus = if file.name.to_lowercase().starts_with(&lower) {
                    PREFIX_BONUS
                } else {
                    0.0
                };
                Some((s + bonus, i, file))
            })
            .collect();
        // Equal scores keep the most recent first.
        scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        scored
            .into_iter()
            .take(MAX_ROWS)
            .map(|(s, _, f)| self.row(f, score::KEYWORD + s))
            .collect()
    }
}

impl Plugin for RecentDocsPlugin {
    fn id(&self) -> &str {
        ID
    }

    fn name(&self) -> &str {
        "Recent documents"
    }

    fn description(&self) -> &str {
        "Type `recent` to search the files you used lately, newest first, from the OS's own recent lists. Read locally; nothing is sent anywhere."
    }

    fn keyword(&self) -> Option<&str> {
        Some(&self.config.keyword)
    }

    fn global(&self) -> bool {
        false
    }

    fn query(&self, input: &str) -> Vec<ResultItem> {
        let snapshot = self.inner.snapshot();
        if self.is_stale(&snapshot) {
            self.reload_in_background();
        }
        if snapshot.loaded_at.is_none() {
            return vec![self.status(
                "loading",
                "Reading recent documents…",
                "Try again in a moment",
            )];
        }
        if snapshot.files.is_empty() {
            return vec![self.status(
                "empty",
                "No recent documents found",
                "Files you open in other apps appear here",
            )];
        }
        self.matches(&snapshot.files, input.trim())
    }

    fn resolve(&self, id: &str) -> Option<ResultItem> {
        let key = id.strip_prefix("recent:")?;
        let snapshot = self.inner.snapshot();
        let file = snapshot
            .files
            .iter()
            .find(|f| Path::new(key) == f.path.as_path())?;
        Some(self.row(file, score::KEYWORD))
    }

    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        match &item.action {
            Action::Custom { payload } if payload == PAYLOAD_NOTHING => Ok(()),
            Action::Custom { .. } => Err(PluginError::Unsupported(item.id.clone())),
            action => execute_action(self.inner.platform.as_ref(), action),
        }
    }

    fn refresh(&self) -> PluginResult<()> {
        self.inner.load();
        Ok(())
    }

    fn index_size(&self) -> Option<usize> {
        Some(self.inner.snapshot().files.len())
    }

    fn attach_notifier(&self, notifier: ResultsNotifier) {
        if let Ok(mut slot) = self.inner.notifier.lock() {
            *slot = Some(notifier);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;
    use crate::test_util::MockPlatform;

    fn file(path: &str, secs: u64) -> RecentFile {
        let path = PathBuf::from(path);
        RecentFile {
            name: path.file_name().unwrap().to_string_lossy().into_owned(),
            path,
            last_used: Some(UNIX_EPOCH + Duration::from_secs(secs)),
        }
    }

    fn platform() -> Arc<MockPlatform> {
        let platform = MockPlatform::empty();
        *platform.recent.lock().unwrap() = vec![
            file("/home/me/Documents/report.pdf", 300),
            file("/home/me/Documents/notes.txt", 200),
            file("/home/me/Pictures/Report cover.png", 100),
            file("/mnt/data/budget.xlsx", 50),
        ];
        platform
    }

    fn plugin(platform: &Arc<MockPlatform>) -> RecentDocsPlugin {
        let plugin = RecentDocsPlugin::with_ttl(
            &RecentConfig::default(),
            platform.clone(),
            Some(PathBuf::from("/home/me")),
            Duration::from_secs(3600),
        );
        plugin.refresh().unwrap();
        plugin
    }

    fn titles(plugin: &RecentDocsPlugin, input: &str) -> Vec<String> {
        let mut rows = plugin.query(input);
        rows.sort_by(|a, b| b.score.total_cmp(&a.score));
        rows.into_iter().map(|r| r.title).collect()
    }

    #[test]
    fn empty_query_lists_newest_first() {
        let platform = platform();
        let plugin = plugin(&platform);
        assert_eq!(
            titles(&plugin, ""),
            ["report.pdf", "notes.txt", "Report cover.png", "budget.xlsx"]
        );
    }

    #[test]
    fn typing_fuzzy_filters_by_name() {
        let platform = platform();
        let plugin = plugin(&platform);
        assert_eq!(
            titles(&plugin, "report"),
            ["report.pdf", "Report cover.png"]
        );
        assert_eq!(titles(&plugin, "bdgt"), ["budget.xlsx"]);
        assert!(titles(&plugin, "zzzzq").is_empty());
    }

    #[test]
    fn rows_are_built_like_file_results() {
        let platform = platform();
        let plugin = plugin(&platform);
        let rows = plugin.query("notes");
        let row = &rows[0];
        assert_eq!(row.id, "recent:/home/me/Documents/notes.txt");
        // The subtitle uses the platform's separator (`~\Documents` on Windows).
        assert_eq!(row.subtitle.replace('\\', "/"), "~/Documents");
        assert_eq!(
            row.action,
            Action::OpenPath {
                path: PathBuf::from("/home/me/Documents/notes.txt")
            }
        );
        let secondary: Vec<_> = row
            .secondary
            .iter()
            .map(|s| (s.label.as_str(), s.modifier))
            .collect();
        assert_eq!(
            secondary,
            [
                ("Show in folder", Some(Modifier::Ctrl)),
                ("Copy path", Some(Modifier::Shift))
            ]
        );
    }

    #[test]
    fn query_reads_the_cache_only() {
        let platform = platform();
        let plugin = plugin(&platform);
        for input in ["", "re", "report"] {
            plugin.query(input);
        }
        assert_eq!(*platform.recent_calls.lock().unwrap(), [50]);
    }

    #[test]
    fn max_items_is_the_limit_asked_of_the_os() {
        let platform = platform();
        let config = RecentConfig {
            max_items: 2,
            ..RecentConfig::default()
        };
        let plugin =
            RecentDocsPlugin::with_ttl(&config, platform.clone(), None, Duration::from_secs(3600));
        plugin.refresh().unwrap();
        assert_eq!(*platform.recent_calls.lock().unwrap(), [2]);
        assert_eq!(plugin.index_size(), Some(2));
    }

    #[test]
    fn staleness_follows_the_ttl() {
        let platform = platform();
        let plugin = plugin(&platform);
        assert!(!plugin.is_stale(&plugin.inner.snapshot()));
        assert!(plugin.is_stale(&Snapshot::default()));
        let stale =
            RecentDocsPlugin::with_ttl(&RecentConfig::default(), platform, None, Duration::ZERO);
        stale.refresh().unwrap();
        assert!(stale.is_stale(&stale.inner.snapshot()));
    }

    #[test]
    fn before_the_first_load_one_status_row_says_so() {
        let platform = MockPlatform::empty();
        let plugin = RecentDocsPlugin::with_ttl(
            &RecentConfig::default(),
            platform,
            None,
            Duration::from_secs(3600),
        );
        // Block the background reload so the test stays deterministic.
        plugin.inner.loading.store(true, Ordering::Release);
        let rows = plugin.query("x");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "Reading recent documents…");
        assert!(plugin.execute(&rows[0]).is_ok());
    }

    #[test]
    fn an_empty_list_says_so() {
        let platform = MockPlatform::empty();
        let plugin = plugin(&platform);
        assert_eq!(titles(&plugin, ""), ["No recent documents found"]);
    }

    #[test]
    fn resolve_rebuilds_a_row_and_execute_opens_it() {
        let platform = platform();
        let plugin = plugin(&platform);
        let row = plugin
            .resolve("recent:/home/me/Documents/report.pdf")
            .unwrap();
        plugin.execute(&row).unwrap();
        assert_eq!(
            *platform.opened_paths.lock().unwrap(),
            [PathBuf::from("/home/me/Documents/report.pdf")]
        );
        assert!(plugin.resolve("recent:/nope").is_none());
        assert!(plugin
            .resolve("files:/home/me/Documents/report.pdf")
            .is_none());
    }

    #[test]
    fn keyword_follows_the_config() {
        let platform = platform();
        let config = RecentConfig {
            keyword: "rec".into(),
            ..RecentConfig::default()
        };
        let plugin = RecentDocsPlugin::new(&config, platform);
        assert_eq!(plugin.keyword(), Some("rec"));
        assert!(!plugin.global());
    }
}
