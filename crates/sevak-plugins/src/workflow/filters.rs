//! The input nodes that list rows without running a script.
//!
//! | Plugin | Node | Rows |
//! |---|---|---|
//! | [`ListFilterPlugin`] | `list_filter` | the fixed items written in the builder, fuzzy-filtered by the typed text |
//! | [`FileFilterPlugin`] | `file_filter` | file and folder names from a bounded scan kept in memory |
//! | [`RunningAppsPlugin`] | `running_apps` | the apps that have a window, from the platform |
//!
//! Like a script filter, picking a row starts the nodes after the filter with
//! the row's argument. None of them touches the disk or the OS in `query`:
//! the file scan runs in `refresh`, the running apps in a cache that refreshes
//! in the background.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sevak_core::config::FilesConfig;
use sevak_core::model::score;
use sevak_core::{
    Action, FuzzyQuery, IconSource, Plugin, PluginError, PluginResult, ResultItem, ResultsNotifier,
};
use sevak_platform::{netpath, PlatformProvider, RunningApp};

use super::exec::{Ctx, Runtime};
use super::model::{AppOutput, FileScope, ListItem, Node, NodeKind};
use super::plugins::node_plugin_id;
use crate::files::{expand_home, home_dir, scan, FileEntry};
use crate::live::Cache;

/// Most rows one of these filters shows.
const MAX_ROWS: usize = 30;
/// Most files one `file_filter` keeps in memory.
const MAX_FILE_ENTRIES: usize = 20_000;
/// How long the list of running apps is reused.
const APPS_TTL: Duration = Duration::from_secs(3);

/// What picking a row starts the workflow with.
#[derive(Debug, Serialize, Deserialize)]
struct Pick {
    arg: String,
}

fn custom(arg: String) -> Action {
    Action::Custom {
        payload: serde_json::to_string(&Pick { arg }).unwrap_or_default(),
    }
}

fn start_pick(runtime: &Arc<Runtime>, node_id: &str, item: &ResultItem) -> PluginResult<()> {
    let Action::Custom { payload } = &item.action else {
        return Err(PluginError::Unsupported(item.id.clone()));
    };
    let pick: Pick =
        serde_json::from_str(payload).map_err(|_| PluginError::Unsupported(item.id.clone()))?;
    runtime
        .start(node_id, Ctx::with_arg(pick.arg))
        .map_err(PluginError::Message)
}

/// The rows to show for `names` (each with the text to match) against the
/// typed `input`: best first, or in list order when nothing is typed. Returns
/// `(score, index)` pairs.
fn rank(input: &str, names: &[(String, Option<String>)]) -> Vec<(f64, usize)> {
    let mut query = FuzzyQuery::new(input.trim());
    let mut scored: Vec<(f64, usize)> = if query.is_empty() {
        (0..names.len())
            .map(|i| (score::KEYWORD + (names.len() - i) as f64, i))
            .collect()
    } else {
        names
            .iter()
            .enumerate()
            .filter_map(|(i, (title, subtitle))| {
                let best = query.score(title).map(f64::from).or_else(|| {
                    // A match in the subtitle counts for less.
                    subtitle
                        .as_deref()
                        .and_then(|text| query.score(text))
                        .map(|s| f64::from(s) / 2.0)
                })?;
                Some((score::KEYWORD + best, i))
            })
            .collect()
    };
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored.truncate(MAX_ROWS);
    scored
}

fn icon() -> IconSource {
    IconSource::builtin("plugin")
}

/// The common identity of the three filters.
struct Base {
    id: String,
    keyword: String,
    node_id: String,
    runtime: Arc<Runtime>,
}

impl Base {
    fn new(runtime: Arc<Runtime>, node: &Node, keyword: &str) -> Self {
        Self {
            id: node_plugin_id(&runtime.folder, &node.id),
            keyword: keyword.trim().to_owned(),
            node_id: node.id.clone(),
            runtime,
        }
    }

    fn row(&self, id: String, title: String, subtitle: String, arg: String) -> ResultItem {
        ResultItem::new(&self.id, id, title, custom(arg))
            .with_subtitle(subtitle)
            .with_icon(icon())
    }
}

macro_rules! filter_identity {
    () => {
        fn id(&self) -> &str {
            &self.base.id
        }

        fn name(&self) -> &str {
            &self.base.runtime.workflow.name
        }

        fn description(&self) -> &str {
            &self.base.runtime.workflow.description
        }

        fn keyword(&self) -> Option<&str> {
            Some(&self.base.keyword)
        }

        fn global(&self) -> bool {
            false
        }

        fn execute(&self, item: &ResultItem) -> PluginResult<()> {
            start_pick(&self.base.runtime, &self.base.node_id, item)
        }

        fn shutdown(&self) {
            self.base.runtime.shutdown();
        }
    };
}

// ---- list filter ---------------------------------------------------------

/// A keyword over a fixed list of rows.
pub struct ListFilterPlugin {
    base: Base,
    items: Vec<ListItem>,
    haystacks: Vec<(String, Option<String>)>,
}

impl ListFilterPlugin {
    /// `None` when `node` is not a list filter.
    pub fn new(runtime: Arc<Runtime>, node: &Node) -> Option<Self> {
        let NodeKind::ListFilter { keyword, items } = &node.kind else {
            return None;
        };
        let haystacks = items
            .iter()
            .map(|item| {
                (
                    item.title.clone(),
                    Some(item.subtitle.clone()).filter(|s| !s.is_empty()),
                )
            })
            .collect();
        Some(Self {
            base: Base::new(runtime, node, keyword),
            items: items.clone(),
            haystacks,
        })
    }
}

impl Plugin for ListFilterPlugin {
    filter_identity!();

    fn query(&self, input: &str) -> Vec<ResultItem> {
        rank(input, &self.haystacks)
            .into_iter()
            .map(|(points, index)| {
                let item = &self.items[index];
                let arg = if item.arg.is_empty() {
                    &item.title
                } else {
                    &item.arg
                };
                self.base
                    .row(
                        format!("item:{index}"),
                        item.title.clone(),
                        item.subtitle.clone(),
                        arg.clone(),
                    )
                    .with_score(points)
            })
            .collect()
    }
}

// ---- file filter ---------------------------------------------------------

/// A keyword over file and folder names.
pub struct FileFilterPlugin {
    base: Base,
    roots: Vec<String>,
    extensions: Vec<String>,
    scope: FileScope,
    files: FilesConfig,
    home: Option<PathBuf>,
    index: RwLock<Arc<Vec<FileEntry>>>,
}

impl FileFilterPlugin {
    /// `None` when `node` is not a file filter. `files` is the `[files]`
    /// config: its directories are searched when the node names none, and its
    /// depth, hidden-file and network-path settings always apply.
    pub fn new(runtime: Arc<Runtime>, node: &Node, files: &FilesConfig) -> Option<Self> {
        let NodeKind::FileFilter {
            keyword,
            folders,
            extensions,
            scope,
        } = &node.kind
        else {
            return None;
        };
        let roots = if folders.is_empty() {
            files.directories.clone()
        } else {
            folders.clone()
        };
        Some(Self {
            base: Base::new(runtime, node, keyword),
            roots,
            extensions: extensions
                .iter()
                .map(|ext| ext.trim().trim_start_matches('.').to_lowercase())
                .filter(|ext| !ext.is_empty())
                .collect(),
            scope: *scope,
            files: files.clone(),
            home: home_dir(),
            index: RwLock::new(Arc::new(Vec::new())),
        })
    }

    fn snapshot(&self) -> Arc<Vec<FileEntry>> {
        self.index
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn keeps(&self, entry: &FileEntry) -> bool {
        match self.scope {
            FileScope::Files if entry.is_dir => return false,
            FileScope::Folders if !entry.is_dir => return false,
            _ => {}
        }
        if self.extensions.is_empty() {
            return true;
        }
        // An extension filter is about files: a folder has none.
        !entry.is_dir
            && entry
                .path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| {
                    self.extensions
                        .iter()
                        .any(|ok| ext.eq_ignore_ascii_case(ok))
                })
    }

    #[cfg(test)]
    fn load(&self, entries: Vec<FileEntry>) {
        let kept: Vec<FileEntry> = entries.into_iter().filter(|e| self.keeps(e)).collect();
        *self
            .index
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Arc::new(kept);
    }
}

impl Plugin for FileFilterPlugin {
    filter_identity!();

    fn query(&self, input: &str) -> Vec<ResultItem> {
        let entries = self.snapshot();
        let mut query = FuzzyQuery::for_paths(input.trim());
        let mut scored: Vec<(f64, &FileEntry)> = if query.is_empty() {
            entries
                .iter()
                .take(MAX_ROWS)
                .enumerate()
                .map(|(i, entry)| (score::KEYWORD + (MAX_ROWS - i) as f64, entry))
                .collect()
        } else {
            entries
                .iter()
                .filter_map(|entry| {
                    query
                        .score(&entry.name)
                        .map(|s| (score::KEYWORD + f64::from(s), entry))
                })
                .collect()
        };
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        scored.truncate(MAX_ROWS);
        scored
            .into_iter()
            .map(|(points, entry)| {
                let path = entry.path.to_string_lossy().into_owned();
                self.base
                    .row(
                        format!("file:{path}"),
                        entry.name.clone(),
                        shorten(&path, self.home.as_deref()),
                        path,
                    )
                    .with_score(points)
                    .with_icon(IconSource::builtin(if entry.is_dir {
                        "folder"
                    } else {
                        "file"
                    }))
            })
            .collect()
    }

    fn index_size(&self) -> Option<usize> {
        Some(self.snapshot().len())
    }

    fn refresh(&self) -> PluginResult<()> {
        let mut roots = Vec::new();
        for folder in &self.roots {
            let root = expand_home(folder, self.home.as_deref());
            if netpath::refusal(&root.to_string_lossy(), self.files.allow_network_paths).is_some() {
                tracing::warn!(
                    workflow = self.base.runtime.folder,
                    node = self.base.node_id,
                    "a file filter folder is on the network and was skipped"
                );
                continue;
            }
            if root.is_dir() {
                roots.push(root);
            }
        }
        let (entries, capped) = scan(
            &roots,
            self.files.max_depth,
            self.files.include_hidden,
            MAX_FILE_ENTRIES,
        );
        if capped {
            tracing::warn!(
                workflow = self.base.runtime.folder,
                node = self.base.node_id,
                limit = MAX_FILE_ENTRIES,
                "a file filter reached its size limit; remaining files are not listed"
            );
        }
        let mut kept: Vec<FileEntry> = entries.into_iter().filter(|e| self.keeps(e)).collect();
        // With nothing typed the newest come first.
        kept.sort_by_key(|entry| std::cmp::Reverse(entry.modified));
        *self
            .index
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Arc::new(kept);
        Ok(())
    }
}

/// `path` with the home folder written as `~`.
fn shorten(path: &str, home: Option<&std::path::Path>) -> String {
    if let Some(home) = home {
        if let Ok(rest) = std::path::Path::new(path).strip_prefix(home) {
            return std::path::Path::new("~").join(rest).display().to_string();
        }
    }
    path.to_owned()
}

// ---- running apps --------------------------------------------------------

/// A keyword over the running apps.
pub struct RunningAppsPlugin {
    base: Base,
    output: AppOutput,
    platform: Arc<dyn PlatformProvider>,
    apps: Arc<Cache<Vec<RunningApp>>>,
    /// Lower-case app name to its bundle or executable path, from `refresh`.
    paths: RwLock<HashMap<String, String>>,
    notifier: OnceLock<ResultsNotifier>,
}

impl RunningAppsPlugin {
    /// `None` when `node` is not a running-apps node.
    pub fn new(
        runtime: Arc<Runtime>,
        node: &Node,
        platform: Arc<dyn PlatformProvider>,
    ) -> Option<Self> {
        let NodeKind::RunningApps { keyword, output } = &node.kind else {
            return None;
        };
        Some(Self {
            base: Base::new(runtime, node, keyword),
            output: *output,
            platform,
            apps: Cache::new(),
            paths: RwLock::new(HashMap::new()),
            notifier: OnceLock::new(),
        })
    }

    fn arg_for(&self, app: &RunningApp) -> String {
        match self.output {
            AppOutput::Name => app.name.clone(),
            AppOutput::Pid => app
                .pids
                .first()
                .map_or_else(|| app.name.clone(), |pid| pid.to_string()),
            AppOutput::Path => self
                .paths
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get(&app_key(&app.name))
                .cloned()
                .unwrap_or_else(|| app.name.clone()),
        }
    }
}

/// `Chrome.EXE` and `chrome` are the same app.
fn app_key(name: &str) -> String {
    let lower = name.to_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_owned()
}

impl Plugin for RunningAppsPlugin {
    filter_identity!();

    fn query(&self, input: &str) -> Vec<ResultItem> {
        let platform = Arc::clone(&self.platform);
        self.apps.refresh_if_stale(
            APPS_TTL,
            move || platform.list_running_apps().unwrap_or_default(),
            self.notifier.get().cloned(),
            &self.base.id,
        );
        let apps = self.apps.get();
        let names: Vec<(String, Option<String>)> = apps
            .iter()
            .map(|app| (display(&app.name).to_owned(), None))
            .collect();
        rank(input, &names)
            .into_iter()
            .map(|(points, index)| {
                let app = &apps[index];
                let subtitle = match app.pids.len() {
                    0 | 1 => String::new(),
                    n => format!("{n} processes"),
                };
                self.base
                    .row(
                        format!("app:{}", app.name),
                        display(&app.name).to_owned(),
                        subtitle,
                        self.arg_for(app),
                    )
                    .with_score(points)
            })
            .collect()
    }

    fn attach_notifier(&self, notifier: ResultsNotifier) {
        let _ = self.notifier.set(notifier);
    }

    fn refresh(&self) -> PluginResult<()> {
        if self.output != AppOutput::Path {
            return Ok(());
        }
        let apps = self
            .platform
            .list_applications()
            .map_err(|err| PluginError::Message(err.to_string()))?;
        let mut paths = HashMap::new();
        for entry in apps {
            if let Some(path) = entry.target.path() {
                paths
                    .entry(app_key(&entry.name))
                    .or_insert_with(|| path.to_string_lossy().into_owned());
            }
        }
        *self
            .paths
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = paths;
        Ok(())
    }
}

/// `chrome.exe` shown as `chrome`.
fn display(name: &str) -> &str {
    match name.len().checked_sub(4) {
        Some(at) if name.is_char_boundary(at) && name[at..].eq_ignore_ascii_case(".exe") => {
            &name[..at]
        }
        _ => name,
    }
}

#[cfg(test)]
mod tests {
    use sevak_core::config::FilesConfig;

    use super::*;
    use crate::test_util::MockPlatform;
    use crate::workflow::testing::{copy, fixture, node, wire};

    fn picked(item: &ResultItem) -> String {
        let Action::Custom { payload } = &item.action else {
            panic!("not a custom action");
        };
        serde_json::from_str::<Pick>(payload).unwrap().arg
    }

    fn list_node(items: &[(&str, &str, &str)]) -> Node {
        node(
            "list",
            NodeKind::ListFilter {
                keyword: "pick".into(),
                items: items
                    .iter()
                    .map(|(title, subtitle, arg)| ListItem {
                        title: (*title).into(),
                        subtitle: (*subtitle).into(),
                        arg: (*arg).into(),
                    })
                    .collect(),
            },
        )
    }

    #[test]
    fn a_list_filter_ranks_by_the_typed_text_and_passes_the_arg() {
        let list = list_node(&[
            ("Apple", "fruit", "a-1"),
            ("Banana", "fruit", ""),
            ("Cherry", "berry", "c-3"),
        ]);
        let f = fixture(
            vec![list.clone(), copy("c", "{query}")],
            vec![wire("list", "c")],
        );
        let plugin = ListFilterPlugin::new(f.runtime.clone(), &list).unwrap();
        assert_eq!(plugin.keyword(), Some("pick"));

        let all = plugin.query("");
        let titles: Vec<&str> = all.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles, ["Apple", "Banana", "Cherry"]);

        let some = plugin.query("ban");
        assert_eq!(some.len(), 1);
        assert_eq!(some[0].title, "Banana");
        // An empty arg falls back to the title.
        assert_eq!(picked(&some[0]), "Banana");
        assert_eq!(picked(&plugin.query("cher")[0]), "c-3");
        // The subtitle matches too, for less.
        assert_eq!(plugin.query("berry")[0].title, "Cherry");
        assert!(plugin.query("zzzz").is_empty());
    }

    #[test]
    fn picking_a_list_row_runs_the_chain_with_its_arg() {
        let list = list_node(&[("Apple", "", "a-1")]);
        let f = fixture(
            vec![list.clone(), copy("c", "got {query}")],
            vec![wire("list", "c")],
        );
        let plugin = ListFilterPlugin::new(f.runtime.clone(), &list).unwrap();
        let row = plugin.query("app").remove(0);
        let report = f.runtime.run_blocking("list", Ctx::with_arg(picked(&row)));
        assert_eq!(report.executed, ["c"]);
        assert_eq!(super::super::testing::clipboard(&f), ["got a-1"]);
    }

    fn file_node(folders: &[&str], extensions: &[&str], scope: FileScope) -> Node {
        node(
            "files",
            NodeKind::FileFilter {
                keyword: "ff".into(),
                folders: folders.iter().map(|s| (*s).into()).collect(),
                extensions: extensions.iter().map(|s| (*s).into()).collect(),
                scope,
            },
        )
    }

    fn entry(path: &str, is_dir: bool) -> FileEntry {
        let path = PathBuf::from(path);
        FileEntry {
            name: path.file_name().unwrap().to_string_lossy().into_owned(),
            path,
            is_dir,
            ..FileEntry::default()
        }
    }

    #[test]
    fn a_file_filter_applies_extensions_and_scope() {
        let entries = vec![
            entry("/x/report.PDF", false),
            entry("/x/notes.txt", false),
            entry("/x/reports", true),
        ];
        let make = |extensions: &[&str], scope| {
            let node = file_node(&["/x"], extensions, scope);
            let f = fixture(
                vec![node.clone(), copy("c", "{query}")],
                vec![wire("files", "c")],
            );
            let plugin =
                FileFilterPlugin::new(f.runtime.clone(), &node, &FilesConfig::default()).unwrap();
            plugin.load(entries.clone());
            (f, plugin)
        };
        let names = |plugin: &FileFilterPlugin, input: &str| -> Vec<String> {
            plugin.query(input).into_iter().map(|r| r.title).collect()
        };

        let (_f, any) = make(&[], FileScope::Any);
        assert_eq!(names(&any, "").len(), 3);
        assert_eq!(names(&any, "rep").len(), 2);

        let (_f, pdf) = make(&[".pdf"], FileScope::Any);
        assert_eq!(names(&pdf, ""), ["report.PDF"]);

        let (_f, folders) = make(&[], FileScope::Folders);
        assert_eq!(names(&folders, ""), ["reports"]);

        let (_f, files) = make(&[], FileScope::Files);
        assert_eq!(names(&files, "rep"), ["report.PDF"]);
        let row = files.query("rep").remove(0);
        assert_eq!(picked(&row), "/x/report.PDF");
    }

    #[test]
    fn a_file_filter_scans_its_folders_in_refresh_not_in_query() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("alpha.md"), "x").unwrap();
        std::fs::write(dir.path().join("beta.txt"), "x").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let node = file_node(&[&dir.path().to_string_lossy()], &["md"], FileScope::Files);
        let f = fixture(
            vec![node.clone(), copy("c", "{query}")],
            vec![wire("files", "c")],
        );
        let plugin =
            FileFilterPlugin::new(f.runtime.clone(), &node, &FilesConfig::default()).unwrap();
        assert!(
            plugin.query("").is_empty(),
            "nothing is read before refresh"
        );
        plugin.refresh().unwrap();
        assert_eq!(plugin.index_size(), Some(1));
        let rows = plugin.query("alp");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "alpha.md");
    }

    #[test]
    fn a_file_filter_without_folders_uses_the_files_config() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("gamma.txt"), "x").unwrap();
        let node = file_node(&[], &[], FileScope::Any);
        let f = fixture(
            vec![node.clone(), copy("c", "{query}")],
            vec![wire("files", "c")],
        );
        let config = FilesConfig {
            directories: vec![dir.path().to_string_lossy().into_owned()],
            ..FilesConfig::default()
        };
        let plugin = FileFilterPlugin::new(f.runtime.clone(), &node, &config).unwrap();
        plugin.refresh().unwrap();
        assert_eq!(plugin.query("gam")[0].title, "gamma.txt");
    }

    fn apps_node(output: AppOutput) -> Node {
        node(
            "apps",
            NodeKind::RunningApps {
                keyword: "apps".into(),
                output,
            },
        )
    }

    fn app(name: &str, pids: &[u32]) -> RunningApp {
        RunningApp {
            name: name.into(),
            pids: pids.to_vec(),
        }
    }

    fn apps_plugin(output: AppOutput) -> (RunningAppsPlugin, Arc<MockPlatform>) {
        let node = apps_node(output);
        let f = fixture(
            vec![node.clone(), copy("c", "{query}")],
            vec![wire("apps", "c")],
        );
        let platform = MockPlatform::empty();
        let plugin = RunningAppsPlugin::new(f.runtime.clone(), &node, platform.clone()).unwrap();
        plugin.apps.store(vec![
            app("Slack", &[300, 301]),
            app("chrome.exe", &[42]),
            app("Notes", &[7]),
        ]);
        (plugin, platform)
    }

    #[test]
    fn running_apps_are_filtered_and_pass_name_pid_or_path() {
        let (name, _) = apps_plugin(AppOutput::Name);
        let rows = name.query("chr");
        assert_eq!(rows.len(), 1);
        // The `.exe` is not shown, but the name passed on is the real one.
        assert_eq!(rows[0].title, "chrome");
        assert_eq!(picked(&rows[0]), "chrome.exe");
        assert_eq!(name.query("slack")[0].subtitle, "2 processes");

        let (pid, _) = apps_plugin(AppOutput::Pid);
        assert_eq!(picked(&pid.query("slack")[0]), "300");

        let (path, platform) = apps_plugin(AppOutput::Path);
        // Before `refresh` the path is not known: the name stands in.
        assert_eq!(picked(&path.query("notes")[0]), "Notes");
        platform.apps.lock().unwrap().push(sevak_core::AppEntry {
            id: "notes".into(),
            name: "Notes".into(),
            description: None,
            keywords: Vec::new(),
            icon: None,
            target: sevak_core::LaunchTarget::Executable {
                path: PathBuf::from("/Applications/Notes.app"),
                args: Vec::new(),
                working_dir: None,
            },
        });
        path.refresh().unwrap();
        assert_eq!(picked(&path.query("notes")[0]), "/Applications/Notes.app");
    }

    #[test]
    fn the_filters_are_not_global_and_are_not_built_for_other_nodes() {
        let list = list_node(&[("A", "", "")]);
        let f = fixture(vec![list.clone(), copy("c", "x")], vec![wire("list", "c")]);
        let plugin = ListFilterPlugin::new(f.runtime.clone(), &list).unwrap();
        assert!(!plugin.global());
        assert!(ListFilterPlugin::new(f.runtime.clone(), &copy("c", "x")).is_none());
    }
}
