//! File and folder search over a pre-built in-memory index, plus browsing of a
//! typed path (`~/Documents/rep`), which lists that one directory live.
//! Queries can carry filters (`report ext:pdf size:>1mb`); see [`filters`].

pub mod filters;

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use chrono::{Local, TimeZone};

use sevak_core::config::FilesConfig;
use sevak_core::model::score;
use sevak_core::{Action, FuzzyQuery, IconSource, Modifier, Plugin, PluginResult, ResultItem};
use sevak_platform::netpath::{self, Refusal};
use sevak_platform::PlatformProvider;
use walkdir::{DirEntry, WalkDir};

use crate::actions::execute_action;
use crate::path_browse::{self, DirReader, PathQuery};
use filters::{Candidate, FileQuery, FilterError};

/// Hard limit on indexed entries, to bound memory and per-query work.
pub const MAX_INDEX_ENTRIES: usize = 100_000;
const MAX_CANDIDATES: usize = 50;
const MIN_QUERY_CHARS: usize = 2;

/// Directories never descended into, matched case-insensitively.
pub(crate) const PRUNED_DIRS: &[&str] = &[
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

/// Path-browsing rows rank above everything else (they are what was typed
/// for), folders above files, and by match quality within each group.
const BROWSE_FOLDER_BONUS: f64 = 2000.0;
const BROWSE_MAX_MATCH: f64 = 1900.0;

/// Rows of a filter-only query (`f ext:pdf`, no name text) are newest first;
/// this is the first row's score, each next one a little lower.
const FILTER_ONLY_SCORE: f64 = 100.0;

/// Whether folder names compare ignoring case in `in:` filters.
pub(crate) const FOLD_CASE: bool = cfg!(any(windows, target_os = "macos"));

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    /// Bytes; 0 for folders.
    pub size: u64,
    /// Last change and creation, in Unix seconds; `None` when unknown (or,
    /// for `created`, not recorded by the file system).
    pub modified: Option<i64>,
    pub created: Option<i64>,
}

impl FileEntry {
    fn candidate(&self) -> Candidate<'_> {
        Candidate {
            name: &self.name,
            path: &self.path,
            is_dir: self.is_dir,
            size: Some(self.size),
            modified: self.modified,
            created: self.created,
        }
    }
}

/// Unix seconds of a file time (negative before 1970).
pub(crate) fn unix_seconds(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
        Err(before) => -i64::try_from(before.duration().as_secs()).unwrap_or(i64::MAX),
    }
}

/// Searches files and folders below the configured directories by name.
pub struct FilesPlugin {
    config: FilesConfig,
    keyword: Option<String>,
    home: Option<PathBuf>,
    platform: Arc<dyn PlatformProvider>,
    index: RwLock<Arc<Vec<FileEntry>>>,
    dirs: DirReader,
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
            dirs: DirReader::default(),
        }
    }

    /// Replaces the index with `entries` without touching the disk, for the
    /// benchmark and the latency test, which need a large index to search.
    #[doc(hidden)]
    pub fn load_entries(&self, entries: Vec<FileEntry>) {
        self.set_index(entries);
    }

    /// The home directory `~` expands to.
    pub(crate) fn home(&self) -> Option<&Path> {
        self.home.as_deref()
    }

    pub(crate) fn allow_network_paths(&self) -> bool {
        self.config.allow_network_paths
    }

    /// Parses the filters in `input` (see [`filters`]) at the current local time.
    pub(crate) fn parse_filters(&self, input: &str) -> Result<FileQuery, FilterError> {
        filters::parse(input, &Local::now(), self.home.as_deref())
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

    /// Lists the directory named by a typed path, filtered by its last segment.
    /// Folders come first; hidden entries only with `include_hidden` or when the
    /// typed segment itself starts with a dot.
    fn browse(&self, typed: &PathQuery) -> Vec<ResultItem> {
        let Some(entries) = self.dirs.list(&typed.dir) else {
            return Vec::new();
        };
        let show_hidden = self.config.include_hidden || typed.filter.starts_with('.');
        let mut query = FuzzyQuery::for_paths(&typed.filter);

        let mut scored: Vec<(f64, String, usize)> = Vec::new();
        for (i, entry) in entries.iter().enumerate() {
            if !show_hidden && entry.name.starts_with('.') {
                continue;
            }
            let matched = if query.is_empty() {
                0.0
            } else {
                match query.score(&entry.name) {
                    Some(s) => f64::from(s) + name_bonus(&entry.name, &typed.filter),
                    None => continue,
                }
            };
            let folder = if entry.is_dir {
                BROWSE_FOLDER_BONUS
            } else {
                0.0
            };
            let total = score::KEYWORD + folder + matched.min(BROWSE_MAX_MATCH);
            scored.push((total, entry.name.to_lowercase(), i));
        }
        // Directories can be large: order fully (best first, then by name, so an
        // unfiltered listing is alphabetical) and keep the head.
        scored.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        scored.truncate(MAX_CANDIDATES);

        scored
            .into_iter()
            .map(|(score, _, i)| {
                let entry = &entries[i];
                let path = typed.dir.join(&entry.name);
                // Folders end in the separator the user typed, so Tab keeps drilling.
                let completion = if entry.is_dir {
                    format!("{}{}{}", typed.typed_dir, entry.name, typed.sep)
                } else {
                    format!("{}{}", typed.typed_dir, entry.name)
                };
                self.row(&entry.name, path, entry.is_dir, score)
                    .with_autocomplete(completion)
            })
            .collect()
    }

    pub(crate) fn row(&self, name: &str, path: PathBuf, is_dir: bool, score: f64) -> ResultItem {
        let path_string = path.to_string_lossy().into_owned();
        let icon = if cfg!(windows) {
            IconSource::Shell {
                parsing_name: path_string.clone(),
            }
        } else if is_dir {
            IconSource::builtin("folder")
        } else {
            IconSource::builtin("file")
        };
        let subtitle = subtitle(&path, self.home.as_deref());
        ResultItem::new(
            "files",
            &path_string,
            name,
            Action::OpenPath { path: path.clone() },
        )
        .with_secondary(
            "Show in folder",
            Some(Modifier::Ctrl),
            Action::RevealPath { path },
        )
        .with_secondary(
            "Copy path",
            Some(Modifier::Shift),
            Action::CopyText {
                text: path_string.clone(),
            },
        )
        .with_subtitle(subtitle)
        .with_icon(icon)
        .with_score(score)
    }

    /// A one-line row saying why a typed path was not looked at. Enter copies
    /// the sentence; nothing else can be done with it.
    fn refusal_row(why: Refusal) -> ResultItem {
        let message = why.message();
        ResultItem::new(
            "files",
            "path-refused",
            message,
            Action::CopyText {
                text: message.to_owned(),
            },
        )
        .with_icon(IconSource::builtin("folder"))
        .with_score(score::KEYWORD)
    }

    /// A row saying why a filter cannot be used, and what would work. Enter
    /// copies the explanation.
    pub(crate) fn filter_error_row(error: &FilterError) -> ResultItem {
        ResultItem::new(
            "files",
            "filter-invalid",
            format!("Cannot use the filter {}", error.token),
            Action::CopyText {
                text: error.to_string(),
            },
        )
        .with_subtitle(error.message.clone())
        .with_icon(IconSource::builtin("file"))
        .with_score(score::KEYWORD)
    }

    /// Adds what the filters looked at (size, dates) and the active filters
    /// themselves to a row's subtitle: `~/Documents · 12 MB · ext:pdf size:>10mb`.
    pub(crate) fn describe(item: &mut ResultItem, query: &FileQuery, entry: &Candidate<'_>) {
        if query.is_plain() {
            return;
        }
        let day = |seconds: i64| {
            Local
                .timestamp_opt(seconds, 0)
                .earliest()
                .map(|t| t.format("%Y-%m-%d").to_string())
        };
        let mut parts = vec![std::mem::take(&mut item.subtitle)];
        if query.shows_size() && !entry.is_dir {
            if let Some(size) = entry.size {
                parts.push(filters::human_size(size));
            }
        }
        if query.shows_modified() {
            if let Some(date) = entry.modified.and_then(day) {
                parts.push(format!("modified {date}"));
            }
        }
        if query.shows_created() {
            if let Some(date) = entry.created.and_then(day) {
                parts.push(format!("created {date}"));
            }
        }
        parts.push(query.summary());
        item.subtitle = parts.join(" \u{b7} ");
    }

    pub(crate) fn search(&self, input: &str) -> Vec<ResultItem> {
        let input = input.trim();
        let allow = self.config.allow_network_paths;
        if let Some(typed) = path_browse::parse(input, self.home.as_deref(), cfg!(windows)) {
            // Before anything is listed: a share would be contacted by the listing.
            if let Some(why) = netpath::refusal(&typed.dir.to_string_lossy(), allow) {
                return vec![Self::refusal_row(why)];
            }
            return self.browse(&typed);
        }
        // `\\server` alone is not browsable, but typing it is still a network path.
        if let Some(why) = netpath::text_refusal(input, allow) {
            return vec![Self::refusal_row(why)];
        }
        if input.chars().count() < MIN_QUERY_CHARS {
            return Vec::new();
        }
        match self.parse_filters(input) {
            Err(error) => vec![Self::filter_error_row(&error)],
            Ok(query) if query.is_plain() => self.search_names(input),
            Ok(query) => self.search_filtered(&query),
        }
    }

    /// The index search without filters: fuzzy over names.
    fn search_names(&self, input: &str) -> Vec<ResultItem> {
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
                self.row(&entry.name, entry.path.clone(), entry.is_dir, score)
            })
            .collect()
    }

    /// The index search with filters. Filters are checked first (no
    /// allocation), then the name text; with no text, matches are listed
    /// newest first.
    fn search_filtered(&self, query: &FileQuery) -> Vec<ResultItem> {
        let text = query.text.as_str();
        let mut fuzzy = Some(FuzzyQuery::for_paths(text)).filter(|q| !q.is_empty());
        if fuzzy.is_none() && !(text.is_empty() && query.can_list_without_text()) {
            return Vec::new();
        }
        let index = self.snapshot();

        let mut scored: Vec<(f64, usize)> = Vec::new();
        for (i, entry) in index.iter().enumerate() {
            if !query.passes_filters(&entry.candidate(), FOLD_CASE) {
                continue;
            }
            let score = match fuzzy.as_mut() {
                Some(fuzzy) => match fuzzy.score(&entry.name) {
                    Some(s) => f64::from(s) + name_bonus(&entry.name, text),
                    None => continue,
                },
                // Newest first; turned into small scores below.
                None => entry.modified.unwrap_or(i64::MIN) as f64,
            };
            if !query.passes_words(&entry.name) {
                continue;
            }
            scored.push((score, i));
        }

        let by_score_desc = |a: &(f64, usize), b: &(f64, usize)| b.0.total_cmp(&a.0);
        if scored.len() > MAX_CANDIDATES {
            scored.select_nth_unstable_by(MAX_CANDIDATES - 1, by_score_desc);
            scored.truncate(MAX_CANDIDATES);
        }
        scored.sort_by(by_score_desc);

        scored
            .into_iter()
            .enumerate()
            .map(|(rank, (score, i))| {
                let entry = &index[i];
                let score = if fuzzy.is_some() {
                    score
                } else {
                    FILTER_ONLY_SCORE - rank as f64 * 0.01
                };
                let mut item = self.row(&entry.name, entry.path.clone(), entry.is_dir, score);
                Self::describe(&mut item, query, &entry.candidate());
                item
            })
            .collect()
    }
}

/// Bonus for names that equal or start with the (ASCII-case-insensitive) input.
pub(crate) fn name_bonus(name: &str, input: &str) -> f64 {
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
pub(crate) fn subtitle(path: &Path, home: Option<&Path>) -> String {
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
            let is_dir = entry.file_type().is_dir();
            // Free on Windows (the directory listing has it); one `stat` elsewhere.
            let (size, modified, created) = match entry.metadata() {
                Ok(meta) => (
                    if is_dir { 0 } else { meta.len() },
                    meta.modified().ok().map(unix_seconds),
                    meta.created().ok().map(unix_seconds),
                ),
                Err(_) => (0, None, None),
            };
            entries.push(FileEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                is_dir,
                path: entry.into_path(),
                size,
                modified,
                created,
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

    /// `files:<full path>` for any path that still exists, indexed or not: a
    /// hotkey bound to a file should keep working outside the search depth.
    fn resolve(&self, id: &str) -> Option<ResultItem> {
        let path = PathBuf::from(id.strip_prefix("files:")?);
        // Asking whether a network path exists would already contact it.
        if netpath::refusal(&path.to_string_lossy(), self.config.allow_network_paths).is_some() {
            return None;
        }
        let metadata = std::fs::metadata(&path).ok()?;
        let name = path.file_name()?.to_string_lossy().into_owned();
        Some(self.row(&name, path, metadata.is_dir(), 0.0))
    }

    fn index_size(&self) -> Option<usize> {
        Some(self.snapshot().len())
    }

    fn refresh(&self) -> PluginResult<()> {
        let started = Instant::now();
        let mut roots = Vec::new();
        for directory in &self.config.directories {
            let root = expand_home(directory, self.home.as_deref());
            if netpath::refusal(&root.to_string_lossy(), self.config.allow_network_paths).is_some()
            {
                tracing::warn!(
                    "a files directory is on the network and was skipped; \
                     set [files] allow_network_paths = true to index it"
                );
                continue;
            }
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
            ..FilesConfig::default()
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
    fn the_index_size_is_the_number_of_entries() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("a.txt"));
        touch(&dir.path().join("b.txt"));
        let plugin = FilesPlugin::with_home(config_for(&[dir.path()]), MockPlatform::empty(), None);
        assert_eq!(plugin.index_size(), Some(0));
        plugin.refresh().unwrap();
        assert_eq!(plugin.index_size(), Some(2));
    }

    #[test]
    fn files_offer_reveal_and_copy_path() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("report.txt"));
        let plugin = indexed(config_for(&[dir.path()]), None);
        let item = plugin.query("report").remove(0);
        let path = dir.path().join("report.txt");

        assert_eq!(item.secondary.len(), 2);
        assert_eq!(item.secondary[0].label, "Show in folder");
        assert_eq!(item.secondary[0].modifier, Some(Modifier::Ctrl));
        assert_eq!(
            item.secondary[0].action,
            Action::RevealPath { path: path.clone() }
        );
        assert_eq!(item.secondary[1].label, "Copy path");
        assert_eq!(item.secondary[1].modifier, Some(Modifier::Shift));
        assert_eq!(
            item.secondary[1].action,
            Action::CopyText {
                text: path.to_string_lossy().into_owned()
            }
        );
        assert_eq!(item.copy_text(), Some(path.to_string_lossy().into_owned()));
    }

    #[test]
    fn revealing_goes_through_the_platform() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("report.txt"));
        let platform = MockPlatform::empty();
        let plugin = FilesPlugin::with_home(config_for(&[dir.path()]), platform.clone(), None);
        plugin.refresh().unwrap();
        let mut item = plugin.query("report").remove(0);
        item.action = item.secondary[0].action.clone();
        plugin.execute(&item).unwrap();
        assert_eq!(
            *platform.revealed.lock().unwrap(),
            vec![dir.path().join("report.txt")]
        );
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
    fn resolve_rebuilds_a_result_from_its_id() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes.txt");
        touch(&file);
        let platform = MockPlatform::empty();
        // Not indexed: the file is outside the configured directories.
        let plugin = FilesPlugin::with_home(FilesConfig::default(), platform.clone(), None);

        let id = format!("files:{}", file.display());
        let item = plugin.resolve(&id).expect("existing file");
        assert_eq!(item.id, id);
        assert_eq!(item.title, "notes.txt");
        plugin.execute(&item).unwrap();
        assert_eq!(*platform.opened_paths.lock().unwrap(), vec![file]);

        let dir_item = plugin
            .resolve(&format!("files:{}", dir.path().display()))
            .expect("existing folder");
        assert!(matches!(dir_item.action, Action::OpenPath { .. }));

        assert!(plugin
            .resolve(&format!("files:{}", dir.path().join("gone").display()))
            .is_none());
        assert!(plugin.resolve("apps:notes.txt").is_none());
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
                    ..FileEntry::default()
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

    /// A plugin that browses `root`, referring to it as `~` (the home directory).
    fn browsing(root: &Path, include_hidden: bool) -> FilesPlugin {
        let mut config = config_for(&[]);
        config.include_hidden = include_hidden;
        FilesPlugin::with_home(config, MockPlatform::empty(), Some(root.to_path_buf()))
    }

    fn sample_tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("Documents/report.txt"));
        touch(&dir.path().join("Documents/reports/q1.txt"));
        touch(&dir.path().join("Downloads/setup.exe"));
        touch(&dir.path().join("notes.md"));
        touch(&dir.path().join(".hidden/x"));
        touch(&dir.path().join(".bashrc"));
        dir
    }

    #[test]
    fn typed_home_path_lists_folders_first_then_files() {
        let dir = sample_tree();
        let plugin = browsing(dir.path(), false);
        let found = plugin.query("~/");
        let names: Vec<_> = found.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(names, ["Documents", "Downloads", "notes.md"]);
        assert!(found.iter().all(|r| r.score >= score::KEYWORD));
        assert!(found[0].score > found[2].score);
        assert_eq!(
            found[0].action,
            Action::OpenPath {
                path: dir.path().join("Documents")
            }
        );
        assert_eq!(found[0].subtitle, "~");
    }

    #[test]
    fn typed_path_filters_by_the_last_segment() {
        let dir = sample_tree();
        let plugin = browsing(dir.path(), false);
        let found = plugin.query("~/Documents/rep");
        let names: Vec<_> = found.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(names, ["reports", "report.txt"]);
        assert!(plugin.query("~/Documents/zzz").is_empty());
        // Fuzzy, like the rest of the search.
        assert_eq!(titles(&plugin, "~/Dow"), ["Downloads"]);
        assert_eq!(titles(&plugin, "~/dwn"), ["Downloads"]);
        // A missing directory is simply empty; browsing never falls back to the index.
        assert!(plugin.query("~/Nowhere/").is_empty());
    }

    #[test]
    fn hidden_entries_follow_the_setting_or_a_typed_dot() {
        let dir = sample_tree();
        let shown = |plugin: &FilesPlugin, query: &str| titles(plugin, query);

        let plugin = browsing(dir.path(), false);
        assert!(!shown(&plugin, "~/").contains(&".bashrc".to_owned()));
        assert_eq!(shown(&plugin, "~/.")[..2], [".hidden", ".bashrc"]);

        let plugin = browsing(dir.path(), true);
        let all = shown(&plugin, "~/");
        assert!(all.contains(&".hidden".to_owned()) && all.contains(&".bashrc".to_owned()));
    }

    #[test]
    fn tab_completion_drills_into_folders_with_the_typed_separator() {
        let dir = sample_tree();
        let plugin = browsing(dir.path(), false);
        let found = plugin.query("~/Doc");
        assert_eq!(found[0].title, "Documents");
        assert_eq!(found[0].autocomplete.as_deref(), Some("~/Documents/"));

        let found = plugin.query("~/Documents/reports/q");
        assert_eq!(
            found[0].autocomplete.as_deref(),
            Some("~/Documents/reports/q1.txt"),
            "files complete to their name"
        );
        // The completion browses straight into the folder.
        let inside = plugin.query("~/Documents/");
        assert_eq!(inside[0].title, "reports");
        assert_eq!(inside[1].title, "report.txt");
    }

    #[cfg(windows)]
    #[test]
    fn backslash_paths_keep_their_separator() {
        let dir = sample_tree();
        let plugin = browsing(dir.path(), false);
        let found = plugin.query("~\\Doc");
        assert_eq!(found[0].autocomplete.as_deref(), Some("~\\Documents\\"));
        let absolute = format!("{}\\Dow", dir.path().display());
        let found = plugin.query(&absolute);
        assert_eq!(found[0].title, "Downloads");
        assert_eq!(
            found[0].autocomplete.as_deref(),
            Some(format!("{}\\Downloads\\", dir.path().display()).as_str())
        );
    }

    #[test]
    fn absolute_paths_work_and_plain_queries_still_use_the_index() {
        let dir = sample_tree();
        let mut config = config_for(&[dir.path()]);
        config.include_hidden = false;
        let plugin = indexed(config, Some(dir.path().to_path_buf()));
        let typed = format!("{}{}Docu", dir.path().display(), std::path::MAIN_SEPARATOR);
        let found = plugin.query(&typed);
        assert_eq!(found[0].title, "Documents");
        assert_eq!(found.len(), 1);
        // No separator-led input: unchanged index search.
        assert!(titles(&plugin, "setup").contains(&"setup.exe".to_owned()));
        assert!(plugin.query("1/2").is_empty());
    }

    #[test]
    fn browsed_rows_offer_reveal_and_copy_path_too() {
        let dir = sample_tree();
        let plugin = browsing(dir.path(), false);
        let found = plugin.query("~/notes");
        let path = dir.path().join("notes.md");
        assert_eq!(found[0].secondary.len(), 2);
        assert_eq!(
            found[0].secondary[0].action,
            Action::RevealPath { path: path.clone() }
        );
        assert_eq!(
            found[0].copy_text(),
            Some(path.to_string_lossy().into_owned())
        );
    }

    #[test]
    fn path_browsing_does_not_need_the_index() {
        let dir = sample_tree();
        // Never refreshed.
        let plugin = browsing(dir.path(), false);
        assert!(plugin.snapshot().is_empty());
        assert_eq!(titles(&plugin, "~/notes"), ["notes.md"]);
    }

    /// Typed network paths (Windows): one row, and nothing is listed or
    /// stat'ed. The default configuration has `allow_network_paths = false`.
    #[cfg(windows)]
    #[test]
    fn typed_network_paths_get_one_refusal_row_and_no_io() {
        let plugin = FilesPlugin::with_home(FilesConfig::default(), MockPlatform::empty(), None);
        assert!(!FilesConfig::default().allow_network_paths);
        for typed in [
            r"\\server\share\dir\na",
            "//server/share/dir/na",
            r"\\?\UNC\server\share\x\y",
            r"\\.\UNC\server\share\x\y",
            r"\\host@SSL\share\x\y",
            r"\\server",
            r"\\server\share",
        ] {
            let rows = plugin.query(typed);
            assert_eq!(rows.len(), 1, "{typed}: {rows:?}");
            assert_eq!(
                rows[0].title,
                "Network paths are turned off (Settings \u{2192} Files)"
            );
            assert_eq!(rows[0].plugin_id, "files");
        }
        // A device path is refused whatever the setting says.
        let config = FilesConfig {
            allow_network_paths: true,
            ..FilesConfig::default()
        };
        let allowed = FilesPlugin::with_home(config, MockPlatform::empty(), None);
        let rows = allowed.query(r"\\.\pipe\name");
        assert_eq!(rows.len(), 1);
        assert!(rows[0].title.contains("device path"), "{:?}", rows[0].title);
    }

    #[cfg(windows)]
    #[test]
    fn a_network_path_is_not_resolved_or_indexed() {
        let config = FilesConfig {
            directories: vec![
                r"\\server\share\docs".to_owned(),
                "//server/share".to_owned(),
            ],
            ..FilesConfig::default()
        };
        let plugin = FilesPlugin::with_home(config, MockPlatform::empty(), None);
        // `resolve` would otherwise ask the file system, and so the network.
        assert!(plugin.resolve(r"files:\\server\share\x.txt").is_none());
        assert!(plugin.resolve("files://server/share/x.txt").is_none());
        // The configured shares are skipped, not scanned.
        plugin.refresh().unwrap();
        assert!(plugin.snapshot().is_empty());
    }

    /// Where these spellings are ordinary file names nothing is refused.
    #[cfg(not(windows))]
    #[test]
    fn other_systems_do_not_treat_unc_spellings_as_network_paths() {
        let plugin = FilesPlugin::with_home(FilesConfig::default(), MockPlatform::empty(), None);
        let rows = plugin.query(r"\\server\share");
        assert!(rows.iter().all(|row| row.id != "path-refused"));
    }

    /// A home folder with a few kinds of files, sizes and ages, indexed.
    fn filter_tree() -> (tempfile::TempDir, FilesPlugin) {
        let home = tempfile::tempdir().unwrap();
        let root = home.path();
        let write = |rel: &str, bytes: usize| {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, vec![b'x'; bytes]).unwrap();
        };
        write("docs/report.pdf", 20 * 1024);
        write("docs/report.txt", 10);
        write("docs/old-report.pdf", 2048);
        write("docs/reports/q1.md", 10);
        write("photos/holiday.jpg", 3000);
        write("photos/holiday.png", 3000);
        write("Projects/app/src/main.rs", 100);
        write("archive/report.zip", 500);
        write("tmp/report.tmp", 1);
        // 2020-01-01T00:00:00Z
        let old = UNIX_EPOCH + std::time::Duration::from_secs(1_577_836_800);
        fs::File::options()
            .write(true)
            .open(root.join("docs/old-report.pdf"))
            .unwrap()
            .set_modified(old)
            .unwrap();

        let config = FilesConfig {
            directories: vec!["~".into()],
            ..config_for(&[])
        };
        let plugin = indexed(config, Some(root.to_path_buf()));
        (home, plugin)
    }

    fn sorted_titles(plugin: &FilesPlugin, query: &str) -> Vec<String> {
        let mut found = titles(plugin, query);
        found.sort();
        found
    }

    #[test]
    fn filters_narrow_the_index_search() {
        let (_home, plugin) = filter_tree();
        // (query, the titles found, sorted)
        let cases: &[(&str, &[&str])] = &[
            ("report ext:pdf", &["old-report.pdf", "report.pdf"]),
            ("report EXT:.PDF", &["old-report.pdf", "report.pdf"]),
            (
                "report -ext:pdf -ext:tmp",
                &["report.txt", "report.zip", "reports"],
            ),
            ("ext:jpg,png", &["holiday.jpg", "holiday.png"]),
            ("type:image", &["holiday.jpg", "holiday.png"]),
            ("type:archive", &["report.zip"]),
            ("size:>10kb", &["report.pdf"]),
            ("report size:<1kb ext:txt", &["report.txt"]),
            ("ext:pdf modified:<2021-01-01", &["old-report.pdf"]),
            ("ext:pdf modified:today", &["report.pdf"]),
            ("ext:pdf modified:>365d", &["old-report.pdf"]),
            ("in:~/Projects", &["app", "main.rs", "src"]),
            ("in:src", &["main.rs"]),
            ("in:\"~/Projects/app\" is:file", &["main.rs"]),
            ("report is:folder", &["reports"]),
            (
                "rep in:docs is:file -ext:md",
                &["old-report.pdf", "report.pdf", "report.txt"],
            ),
            ("\"old-report\"", &["old-report.pdf"]),
            ("report -\"old\" ext:pdf", &["report.pdf"]),
            ("size:empty", &[]),
            ("-report", &[]),
        ];
        for (query, expected) in cases {
            assert_eq!(sorted_titles(&plugin, query), *expected, "{query:?}");
        }
    }

    #[test]
    fn filter_only_queries_list_newest_first() {
        let (_home, plugin) = filter_tree();
        let found = plugin.query("ext:pdf");
        assert_eq!(
            found.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
            ["report.pdf", "old-report.pdf"]
        );
        assert!(found[0].score > found[1].score);
        assert!(found[0].score <= FILTER_ONLY_SCORE);
    }

    #[test]
    fn plain_queries_are_unchanged_by_filter_support() {
        let (_home, plugin) = filter_tree();
        let found = plugin.query("report");
        assert!(found.len() >= 5);
        // No filter: the subtitle is the folder alone.
        let pdf = found.iter().find(|r| r.title == "report.pdf").unwrap();
        assert_eq!(
            pdf.subtitle,
            Path::new("~").join("docs").display().to_string()
        );
    }

    #[test]
    fn the_subtitle_shows_active_filters_and_what_they_looked_at() {
        let (_home, plugin) = filter_tree();
        let found = plugin.query("report ext:pdf size:>10kb");
        assert_eq!(found.len(), 1);
        let docs = Path::new("~").join("docs").display().to_string();
        assert_eq!(
            found[0].subtitle,
            format!("{docs} \u{b7} 20 KB \u{b7} ext:pdf size:>10kb")
        );

        let found = plugin.query("old-report modified:<2021-01-01");
        assert_eq!(found.len(), 1);
        assert!(
            found[0].subtitle.contains("modified 2019-12-31")
                || found[0].subtitle.contains("modified 2020-01-01"),
            "{}",
            found[0].subtitle
        );
        assert!(found[0].subtitle.ends_with("modified:<2021-01-01"));
    }

    #[test]
    fn an_invalid_filter_is_one_row_that_says_what_works() {
        let (_home, plugin) = filter_tree();
        for (query, needle) in [
            ("report size:huge", "size:>10mb"),
            ("type:spreadsheet", "image, video, audio"),
            ("report modified:someday", "2026-01-01"),
            ("ext:", "ext:pdf"),
        ] {
            let rows = plugin.query(query);
            assert_eq!(rows.len(), 1, "{query:?}: {rows:?}");
            assert_eq!(rows[0].id, "files:filter-invalid");
            assert!(
                rows[0].title.starts_with("Cannot use the filter"),
                "{query:?}"
            );
            assert!(
                rows[0].subtitle.contains(needle),
                "{query:?}: {}",
                rows[0].subtitle
            );
            assert!(plugin.execute(&rows[0]).is_ok());
        }
    }

    #[test]
    fn creation_times_are_used_where_the_file_system_records_them() {
        let (home, plugin) = filter_tree();
        let recorded = fs::metadata(home.path().join("docs/report.pdf"))
            .and_then(|m| m.created())
            .is_ok();
        let found = sorted_titles(&plugin, "report created:7d ext:pdf");
        if recorded {
            assert!(found.contains(&"report.pdf".to_owned()), "{found:?}");
        } else {
            assert!(found.is_empty(), "{found:?}");
        }
    }

    #[test]
    fn the_index_records_sizes_and_times() {
        let (_home, plugin) = filter_tree();
        let snapshot = plugin.snapshot();
        let entry = |name: &str| snapshot.iter().find(|e| e.name == name).unwrap().clone();
        assert_eq!(entry("report.pdf").size, 20 * 1024);
        assert_eq!(entry("reports").size, 0);
        assert_eq!(entry("old-report.pdf").modified, Some(1_577_836_800));
        assert!(entry("report.pdf").modified.unwrap() > 1_577_836_800);
    }

    /// Filtered queries over the 100k cap; prints timings like the test above.
    #[test]
    fn filtered_query_timing_over_100k_entries() {
        let entries: Vec<FileEntry> = (0..MAX_INDEX_ENTRIES)
            .map(|i| {
                let ext = ["pdf", "txt", "png", "rs"][i % 4];
                let name = format!("file_{i}.{ext}");
                FileEntry {
                    path: PathBuf::from(format!("/home/u/Documents/dir{}/{name}", i % 500)),
                    name,
                    is_dir: false,
                    size: (i as u64 % 1000) * 1024,
                    modified: Some(1_700_000_000 + i as i64),
                    created: None,
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
            "file ext:pdf",
            "ext:pdf",
            "type:image size:>500kb",
            "in:dir7 modified:>2026-01-01",
            "file_9 -ext:txt \"_9\"",
        ] {
            let _ = plugin.query(query);
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
