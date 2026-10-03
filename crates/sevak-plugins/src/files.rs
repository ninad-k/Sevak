//! File and folder search over a pre-built in-memory index.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Instant;

use sevak_core::config::FilesConfig;
use sevak_core::{Action, FuzzyQuery, IconSource, Plugin, PluginResult, ResultItem};
use sevak_platform::PlatformProvider;
use walkdir::{DirEntry, WalkDir};

use crate::actions::execute_action;

/// Hard limit on indexed entries, to bound memory and per-query work.
pub const MAX_INDEX_ENTRIES: usize = 100_000;
const MAX_CANDIDATES: usize = 50;
const MIN_QUERY_CHARS: usize = 2;

/// Directories never descended into, matched case-insensitively.
const PRUNED_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "__pycache__",
    ".cache",
    "venv",
    ".venv",
    "appdata",
];

const EXACT_NAME_BONUS: f64 = 120.0;
const EXACT_STEM_BONUS: f64 = 100.0;
const PREFIX_BONUS: f64 = 40.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

/// Searches files and folders below the configured directories by name.
pub struct FilesPlugin {
    config: FilesConfig,
    keyword: Option<String>,
    home: Option<PathBuf>,
    platform: Arc<dyn PlatformProvider>,
    index: RwLock<Arc<Vec<FileEntry>>>,
}

impl FilesPlugin {
    /// Uses the current user's home directory (`USERPROFILE` / `HOME`) to
    /// expand `~` in the configured directories.
    pub fn new(config: FilesConfig, platform: Arc<dyn PlatformProvider>) -> Self {
        Self::with_home(config, platform, home_dir())
    }

    pub fn with_home(
        config: FilesConfig,
        platform: Arc<dyn PlatformProvider>,
        home: Option<PathBuf>,
    ) -> Self {
        let keyword = Some(config.keyword.trim().to_owned()).filter(|k| !k.is_empty());
        Self {
            config,
            keyword,
            home,
            platform,
            index: RwLock::new(Arc::new(Vec::new())),
        }
    }

    fn snapshot(&self) -> Arc<Vec<FileEntry>> {
        self.index
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn set_index(&self, entries: Vec<FileEntry>) {
        *self
            .index
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Arc::new(entries);
    }

    fn search(&self, input: &str) -> Vec<ResultItem> {
        let input = input.trim();
        if input.chars().count() < MIN_QUERY_CHARS {
            return Vec::new();
        }
        let mut query = FuzzyQuery::for_paths(input);
        if query.is_empty() {
            return Vec::new();
        }
        let index = self.snapshot();

        let mut scored: Vec<(f64, usize)> = Vec::new();
        for (i, entry) in index.iter().enumerate() {
            if let Some(s) = query.score(&entry.name) {
                scored.push((f64::from(s) + name_bonus(&entry.name, input), i));
            }
        }

        let by_score_desc = |a: &(f64, usize), b: &(f64, usize)| b.0.total_cmp(&a.0);
        if scored.len() > MAX_CANDIDATES {
            scored.select_nth_unstable_by(MAX_CANDIDATES - 1, by_score_desc);
            scored.truncate(MAX_CANDIDATES);
        }
        scored.sort_by(by_score_desc);

        scored
            .into_iter()
            .map(|(score, i)| {
                let entry = &index[i];
                let path_string = entry.path.to_string_lossy().into_owned();
                let icon = if cfg!(windows) {
                    IconSource::Shell {
                        parsing_name: path_string.clone(),
                    }
                } else if entry.is_dir {
                    IconSource::builtin("folder")
                } else {
                    IconSource::builtin("file")
                };
                ResultItem::new(
                    "files",
                    &path_string,
                    &entry.name,
                    Action::OpenPath {
                        path: entry.path.clone(),
                    },
                )
                .with_subtitle(subtitle(&entry.path, self.home.as_deref()))
                .with_icon(icon)
                .with_score(score)
            })
            .collect()
    }
}

/// Bonus for names that equal or start with the (ASCII-case-insensitive) input.
fn name_bonus(name: &str, input: &str) -> f64 {
    if name.eq_ignore_ascii_case(input) {
        return EXACT_NAME_BONUS;
    }
    if let Some((stem, _)) = name.rsplit_once('.') {
        if !stem.is_empty() && stem.eq_ignore_ascii_case(input) {
            return EXACT_STEM_BONUS;
        }
    }
    match name.get(..input.len()) {
        Some(prefix) if prefix.eq_ignore_ascii_case(input) => PREFIX_BONUS,
        _ => 0.0,
    }
}

/// The parent directory, with the home prefix shown as `~`.
fn subtitle(path: &Path, home: Option<&Path>) -> String {
    let parent = path.parent().unwrap_or(path);
    if let Some(home) = home {
        if let Ok(rest) = parent.strip_prefix(home) {
            if rest.as_os_str().is_empty() {
                return "~".to_owned();
            }
            return Path::new("~").join(rest).display().to_string();
        }
    }
    parent.display().to_string()
}

pub(crate) fn home_dir() -> Option<PathBuf> {
    let vars: [&str; 2] = if cfg!(windows) {
        ["USERPROFILE", "HOME"]
    } else {
        ["HOME", "USERPROFILE"]
    };
    vars.iter()
        .filter_map(std::env::var_os)
        .find(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Expands a leading `~`, `~/` or `~\` using `home`. Anything else (including
/// `~user`) is returned unchanged.
pub fn expand_home(path: &str, home: Option<&Path>) -> PathBuf {
    let Some(home) = home else {
        return PathBuf::from(path);
    };
    if path == "~" {
        return home.to_path_buf();
    }
    match path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        Some(rest) => home.join(rest),
        None => PathBuf::from(path),
    }
}

fn is_skipped(entry: &DirEntry, include_hidden: bool) -> bool {
    if entry.depth() == 0 {
        return false;
    }
    let name = entry.file_name().to_string_lossy();
    if !include_hidden && name.starts_with('.') {
        return true;
    }
    entry.file_type().is_dir() && PRUNED_DIRS.iter().any(|p| name.eq_ignore_ascii_case(p))
}

/// Walks `roots`, returning at most `cap` entries; the bool is true when the
/// cap cut the walk short.
fn scan(
    roots: &[PathBuf],
    max_depth: usize,
    include_hidden: bool,
    cap: usize,
) -> (Vec<FileEntry>, bool) {
    let mut entries = Vec::new();
    if max_depth == 0 {
        // walkdir would lower `min_depth` to 0 and yield the root itself.
        return (entries, false);
    }
    for root in roots {
        let walker = WalkDir::new(root)
            .min_depth(1)
            .max_depth(max_depth)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| !is_skipped(entry, include_hidden));
        for result in walker {
            let entry = match result {
                Ok(entry) => entry,
                Err(err) => {
                    tracing::debug!(%err, "skipping unreadable entry");
                    continue;
                }
            };
            if entries.len() >= cap {
                return (entries, true);
            }
            entries.push(FileEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                is_dir: entry.file_type().is_dir(),
                path: entry.into_path(),
            });
        }
    }
    (entries, false)
}

impl Plugin for FilesPlugin {
    fn id(&self) -> &str {
        "files"
    }

    fn name(&self) -> &str {
        "Files"
    }

    fn description(&self) -> &str {
        "Finds files and folders in your configured directories."
    }

    fn keyword(&self) -> Option<&str> {
        self.keyword.as_deref()
    }

    fn global(&self) -> bool {
        self.config.global || self.keyword.is_none()
    }

    fn query(&self, input: &str) -> Vec<ResultItem> {
        self.search(input)
    }

    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        execute_action(self.platform.as_ref(), &item.action)
    }

    fn refresh(&self) -> PluginResult<()> {
        let started = Instant::now();
        let mut roots = Vec::new();
        for directory in &self.config.directories {
            let root = expand_home(directory, self.home.as_deref());
            if root.is_dir() {
                roots.push(root);
            } else {
                tracing::debug!(directory = %root.display(), "files directory missing; skipped");
            }
        }

        let (entries, capped) = scan(
            &roots,
            self.config.max_depth,
            self.config.include_hidden,
            MAX_INDEX_ENTRIES,
        );
        if capped {
            tracing::warn!(
                limit = MAX_INDEX_ENTRIES,
                "file index reached its size limit; remaining files are not searchable"
            );
        }
        tracing::info!(
            count = entries.len(),
            roots = roots.len(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "files indexed"
        );
        self.set_index(entries);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::test_util::MockPlatform;

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"x").unwrap();
    }

    fn config_for(dirs: &[&Path]) -> FilesConfig {
        FilesConfig {
            directories: dirs.iter().map(|d| d.display().to_string()).collect(),
            max_depth: 4,
            include_hidden: false,
            keyword: "f".into(),
            global: true,
        }
    }

    fn indexed(config: FilesConfig, home: Option<PathBuf>) -> FilesPlugin {
        let plugin = FilesPlugin::with_home(config, MockPlatform::empty(), home);
        plugin.refresh().unwrap();
        plugin
    }

    fn names(plugin: &FilesPlugin) -> Vec<String> {
        let mut names: Vec<String> = plugin.snapshot().iter().map(|e| e.name.clone()).collect();
        names.sort();
        names
    }

    fn titles(plugin: &FilesPlugin, query: &str) -> Vec<String> {
        plugin.query(query).into_iter().map(|r| r.title).collect()
    }

    #[test]
    fn expands_home() {
        let home = Path::new("/home/ninad");
        assert_eq!(expand_home("~", Some(home)), home);
        assert_eq!(
            expand_home("~/Documents", Some(home)),
            home.join("Documents")
        );
        assert_eq!(expand_home("~\\Desktop", Some(home)), home.join("Desktop"));
        assert_eq!(expand_home("~/", Some(home)), home.join(""));
        assert_eq!(expand_home("/etc", Some(home)), PathBuf::from("/etc"));
        assert_eq!(
            expand_home("~other/x", Some(home)),
            PathBuf::from("~other/x")
        );
        assert_eq!(expand_home("a/~/b", Some(home)), PathBuf::from("a/~/b"));
        assert_eq!(expand_home("~/x", None), PathBuf::from("~/x"));
    }

    #[test]
    fn indexes_files_and_folders_but_not_the_root() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("a.txt"));
        touch(&dir.path().join("sub/b.txt"));
        let plugin = indexed(config_for(&[dir.path()]), None);
        assert_eq!(names(&plugin), ["a.txt", "b.txt", "sub"]);
        let snapshot = plugin.snapshot();
        assert!(snapshot.iter().find(|e| e.name == "sub").unwrap().is_dir);
        assert!(!snapshot.iter().find(|e| e.name == "a.txt").unwrap().is_dir);
    }

    #[test]
    fn respects_max_depth() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("l1.txt"));
        touch(&dir.path().join("d/l2.txt"));
        touch(&dir.path().join("d/e/l3.txt"));

        let mut config = config_for(&[dir.path()]);
        config.max_depth = 1;
        assert_eq!(names(&indexed(config.clone(), None)), ["d", "l1.txt"]);

        config.max_depth = 2;
        assert_eq!(
            names(&indexed(config.clone(), None)),
            ["d", "e", "l1.txt", "l2.txt"]
        );

        config.max_depth = 3;
        assert_eq!(names(&indexed(config.clone(), None)).len(), 5);

        config.max_depth = 0;
        assert!(names(&indexed(config, None)).is_empty());
    }

    #[test]
    fn skips_hidden_entries_unless_included() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("visible.txt"));
        touch(&dir.path().join(".secret.txt"));
        touch(&dir.path().join(".dotdir/inner.txt"));

        let mut config = config_for(&[dir.path()]);
        assert_eq!(names(&indexed(config.clone(), None)), ["visible.txt"]);

        config.include_hidden = true;
        assert_eq!(
            names(&indexed(config, None)),
            [".dotdir", ".secret.txt", "inner.txt", "visible.txt"]
        );
    }

    #[test]
    fn prunes_heavy_directories_even_when_hidden_are_included() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("keep.txt"));
        for pruned in [
            "node_modules",
            ".git",
            "target",
            "__pycache__",
            ".cache",
            "venv",
            ".venv",
            "AppData",
        ] {
            touch(&dir.path().join(pruned).join("inner.txt"));
            touch(&dir.path().join("proj").join(pruned).join("inner.txt"));
        }

        let mut config = config_for(&[dir.path()]);
        config.include_hidden = true;
        assert_eq!(names(&indexed(config, None)), ["keep.txt", "proj"]);
    }

    #[test]
    fn a_root_that_is_itself_hidden_or_pruned_is_still_walked() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join(".config");
        touch(&root.join("a.txt"));
        assert_eq!(names(&indexed(config_for(&[&root]), None)), ["a.txt"]);
    }

    #[test]
    fn expands_tilde_directories_and_tolerates_missing_ones() {
        let home = tempfile::tempdir().unwrap();
        touch(&home.path().join("Documents/report.txt"));
        let config = FilesConfig {
            directories: vec![
                "~/Documents".into(),
                "~/DoesNotExist".into(),
                "/definitely/not/a/real/dir".into(),
            ],
            ..config_for(&[])
        };
        let plugin = indexed(config, Some(home.path().to_path_buf()));
        assert_eq!(names(&plugin), ["report.txt"]);
    }

    #[test]
    fn scan_cap_stops_the_walk() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..10 {
            touch(&dir.path().join(format!("f{i}.txt")));
        }
        let (entries, capped) = scan(&[dir.path().to_path_buf()], 4, false, 4);
        assert_eq!(entries.len(), 4);
        assert!(capped);
        let (entries, capped) = scan(&[dir.path().to_path_buf()], 4, false, 10);
        assert_eq!(entries.len(), 10);
        assert!(!capped);
    }

    #[test]
    fn short_queries_return_nothing() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("a.txt"));
        let plugin = indexed(config_for(&[dir.path()]), None);
        assert!(plugin.query("a").is_empty());
        assert!(plugin.query(" a ").is_empty());
        assert!(plugin.query("").is_empty());
        assert!(!plugin.query("a.").is_empty());
    }

    #[test]
    fn empty_before_first_refresh() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("report.txt"));
        let plugin = FilesPlugin::with_home(config_for(&[dir.path()]), MockPlatform::empty(), None);
        assert!(plugin.query("report").is_empty());
    }

    #[test]
    fn exact_and_prefix_names_rank_first() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("annual_report_final.docx"));
        touch(&dir.path().join("my report.pdf"));
        touch(&dir.path().join("reports/readme.md"));
        touch(&dir.path().join("Report.TXT"));
        touch(&dir.path().join("unrelated.txt"));
        let plugin = indexed(config_for(&[dir.path()]), None);

        let found = titles(&plugin, "report");
        assert_eq!(found[0], "Report.TXT", "{found:?}");
        assert_eq!(found[1], "reports", "{found:?}");
        assert!(!found.contains(&"unrelated.txt".to_owned()));
        assert!(!found.contains(&"readme.md".to_owned()));
        assert_eq!(found.len(), 4);
    }

    #[test]
    fn exact_full_name_beats_stem_match() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("notes"));
        touch(&dir.path().join("notes.txt"));
        touch(&dir.path().join("notes.old.txt"));
        let plugin = indexed(config_for(&[dir.path()]), None);
        let found = titles(&plugin, "notes");
        assert_eq!(found[0], "notes");
        assert_eq!(found[1], "notes.txt");
    }

    #[test]
    fn result_shape_and_home_relative_subtitle() {
        let home = tempfile::tempdir().unwrap();
        let file = home.path().join("Documents").join("taxes.pdf");
        touch(&file);
        let config = FilesConfig {
            directories: vec!["~/Documents".into()],
            ..config_for(&[])
        };
        let plugin = indexed(config, Some(home.path().to_path_buf()));

        let results = plugin.query("taxes");
        assert_eq!(results.len(), 1);
        let item = &results[0];
        assert_eq!(item.id, format!("files:{}", file.display()));
        assert_eq!(item.title, "taxes.pdf");
        assert_eq!(
            item.subtitle,
            Path::new("~").join("Documents").display().to_string()
        );
        assert_eq!(item.action, Action::OpenPath { path: file.clone() });
        if cfg!(windows) {
            assert_eq!(
                item.icon,
                Some(IconSource::Shell {
                    parsing_name: file.to_string_lossy().into_owned()
                })
            );
        } else {
            assert_eq!(item.icon, Some(IconSource::builtin("file")));
        }
    }

    #[test]
    fn subtitle_outside_home_is_the_plain_parent() {
        assert_eq!(
            subtitle(Path::new("/srv/data/x.txt"), Some(Path::new("/home/me"))),
            Path::new("/srv/data").display().to_string()
        );
        assert_eq!(
            subtitle(Path::new("/home/me/x.txt"), Some(Path::new("/home/me"))),
            "~"
        );
    }

    #[test]
    fn folders_get_folder_icons_off_windows() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("photos/a.jpg"));
        let plugin = indexed(config_for(&[dir.path()]), None);
        let item = &plugin.query("photos")[0];
        if !cfg!(windows) {
            assert_eq!(item.icon, Some(IconSource::builtin("folder")));
        }
    }

    #[test]
    fn keyword_and_global_follow_config() {
        let platform = MockPlatform::empty();
        let plugin = FilesPlugin::with_home(FilesConfig::default(), platform.clone(), None);
        assert_eq!(plugin.id(), "files");
        assert_eq!(plugin.name(), "Files");
        assert_eq!(plugin.keyword(), Some("f"));
        assert!(plugin.global());

        let config = FilesConfig {
            global: false,
            ..FilesConfig::default()
        };
        let plugin = FilesPlugin::with_home(config, platform.clone(), None);
        assert!(!plugin.global());

        let config = FilesConfig {
            keyword: "  ".into(),
            global: false,
            ..FilesConfig::default()
        };
        let plugin = FilesPlugin::with_home(config, platform, None);
        assert_eq!(plugin.keyword(), None);
        assert!(
            plugin.global(),
            "without a keyword the plugin must be global"
        );
    }

    #[test]
    fn execute_opens_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("open_me.txt");
        touch(&file);
        let platform = MockPlatform::empty();
        let plugin = FilesPlugin::with_home(config_for(&[dir.path()]), platform.clone(), None);
        plugin.refresh().unwrap();
        let results = plugin.query("open_me");
        plugin.execute(&results[0]).unwrap();
        assert_eq!(*platform.opened_paths.lock().unwrap(), vec![file]);
    }

    #[test]
    fn at_most_fifty_candidates_best_first() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..80 {
            touch(&dir.path().join(format!("doc_{i:03}.txt")));
        }
        touch(&dir.path().join("doc.txt"));
        let plugin = indexed(config_for(&[dir.path()]), None);
        let results = plugin.query("doc");
        assert_eq!(results.len(), MAX_CANDIDATES);
        assert_eq!(results[0].title, "doc.txt");
        assert!(results.windows(2).all(|w| w[0].score >= w[1].score));
    }

    /// Query latency over a synthetic index at the 100k cap. Prints timings
    /// (run with `--nocapture`); asserts nothing about speed.
    #[test]
    fn query_timing_over_100k_entries() {
        const WORDS: &[&str] = &[
            "report",
            "invoice",
            "photo",
            "holiday",
            "budget",
            "notes",
            "meeting",
            "draft",
            "final",
            "backup",
            "screenshot",
            "resume",
            "presentation",
            "archive",
            "design",
            "project",
            "readme",
            "setup",
            "installer",
            "download",
        ];
        const EXTS: &[&str] = &[
            "txt", "pdf", "docx", "png", "jpg", "xlsx", "zip", "md", "rs",
        ];

        let entries: Vec<FileEntry> = (0..MAX_INDEX_ENTRIES)
            .map(|i| {
                let name = format!(
                    "{}_{}_{i}.{}",
                    WORDS[i % WORDS.len()],
                    WORDS[(i / 7) % WORDS.len()],
                    EXTS[i % EXTS.len()]
                );
                FileEntry {
                    path: PathBuf::from(format!("/home/u/Documents/dir{}/{name}", i % 500)),
                    name,
                    is_dir: i % 50 == 0,
                }
            })
            .collect();

        let plugin = FilesPlugin::with_home(
            FilesConfig::default(),
            MockPlatform::empty(),
            Some(PathBuf::from("/home/u")),
        );
        plugin.set_index(entries);

        for query in [
            "re",
            "report",
            "rprt",
            "invoice 2024",
            "zzzz",
            "holiday photo",
        ] {
            let _ = plugin.query(query); // warm up
            let runs = 5;
            let started = Instant::now();
            let mut hits = 0;
            for _ in 0..runs {
                hits = plugin.query(query).len();
            }
            let per_query = started.elapsed() / runs;
            println!(
                "files query {query:?}: {:.2} ms/query ({hits} results, 100k entries)",
                per_query.as_secs_f64() * 1000.0
            );
        }
    }
}
