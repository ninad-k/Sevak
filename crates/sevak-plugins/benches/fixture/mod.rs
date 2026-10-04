//! A large, deterministic, offline index for the latency benchmark
//! (`benches/engine_query.rs`) and the latency budget test
//! (`tests/latency.rs`), which both include this file with `#[path]`.
//!
//! It builds the real default search engine (every built-in plugin, through the
//! real registry) over a mock platform and generated data:
//!
//! | Source | Size | How it gets into the plugin |
//! |---|---|---|
//! | applications | 5 000 | the mock platform's `list_applications`, read by `refresh` |
//! | files and folders | 100 000 (the plugin's own cap) | `FilesPlugin::load_entries`, no disk walk |
//! | bookmarks | 10 000 | a Chromium `Bookmarks` file in a temp profile, read by `refresh` |
//! | clipboard history | 200 | a history file in a temp folder, read by `refresh` |
//! | workflows | the two examples | copied into a temp workflows folder, approved |
//!
//! Nothing touches the user's real data: the platform answers every OS question
//! itself (no real clipboard, process list, OS file index or browsers) and the
//! home folder, history and workflows live in a temp directory.

#![allow(dead_code)] // each includer uses a different part

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use sevak_core::{
    AppEntry, Config, EngineOptions, IconData, LaunchTarget, Plugin, SearchEngine, UsageStore,
};
use sevak_platform::{
    BrowserFamily, BrowserRoot, ClipboardMedia, ClipboardRead, MediaCommand, MediaRequest, OsHit,
    OsSearchError, OsSearchRequest, PlatformError, PlatformProvider, ProcessInfo,
    Result as PlatformResult, SettingsPage, SystemCommand, TaskKind,
};
use sevak_plugins::files::{FileEntry, FilesPlugin};
use sevak_plugins::workflow::{NoSink, WorkflowHost};
use sevak_plugins::{BookmarksPlugin, ClipboardPlugin, PluginRegistry};

/// How much data the fixture holds.
#[derive(Debug, Clone, Copy)]
pub struct Sizes {
    pub apps: usize,
    /// Entries put straight into the files index.
    pub files: usize,
    /// Files created on disk for the files plugin to scan (for the startup
    /// measurement; creating 100 000 files per run would dwarf what it measures).
    pub files_on_disk: usize,
    pub bookmarks: usize,
    pub clipboard: usize,
}

impl Sizes {
    /// The sizes the budget and the benchmark share.
    pub const LARGE: Self = Self {
        apps: 5_000,
        files: 100_000,
        files_on_disk: 0,
        bookmarks: 10_000,
        clipboard: 200,
    };
}

/// A platform that answers from memory: the benchmark must not read the real
/// clipboard, process list, OS file index or browsers of the machine it runs on.
pub struct FixturePlatform {
    apps: Vec<AppEntry>,
}

impl PlatformProvider for FixturePlatform {
    fn list_applications(&self) -> PlatformResult<Vec<AppEntry>> {
        Ok(self.apps.clone())
    }
    fn launch(&self, _target: &LaunchTarget) -> PlatformResult<()> {
        Ok(())
    }
    fn open_url(&self, _url: &str) -> PlatformResult<()> {
        Ok(())
    }
    fn open_path(&self, _path: &Path) -> PlatformResult<()> {
        Ok(())
    }
    fn load_icon(&self, _source: &sevak_core::IconSource, _size: u32) -> PlatformResult<IconData> {
        Err(PlatformError::Unsupported("icons in the benchmark"))
    }
    fn set_clipboard_text(&self, _text: &str) -> PlatformResult<()> {
        Ok(())
    }
    fn supported_system_commands(&self) -> Vec<SystemCommand> {
        SystemCommand::ALL.to_vec()
    }
    fn supported_settings_pages(&self) -> Vec<SettingsPage> {
        Vec::new()
    }
    fn supported_tasks(&self) -> Vec<TaskKind> {
        TaskKind::ALL.to_vec()
    }
    fn list_processes(&self) -> PlatformResult<Vec<ProcessInfo>> {
        Ok((0..300)
            .map(|i| ProcessInfo {
                name: format!("process-{i}.exe"),
                pid: 1000 + i,
                cpu_percent: 0.0,
                memory_bytes: 1 << 20,
            })
            .collect())
    }
    fn list_running_apps(&self) -> PlatformResult<Vec<sevak_platform::RunningApp>> {
        Ok(Vec::new())
    }
    fn list_removable_drives(&self) -> PlatformResult<Vec<sevak_platform::Drive>> {
        Ok(Vec::new())
    }
    fn supported_media_commands(&self) -> Vec<MediaCommand> {
        Vec::new()
    }
    fn now_playing_available(&self) -> bool {
        false
    }
    fn browser_roots(&self) -> Vec<BrowserRoot> {
        Vec::new()
    }
    fn clipboard_text(&self) -> PlatformResult<Option<String>> {
        Ok(None)
    }
    fn read_clipboard(&self) -> PlatformResult<ClipboardRead> {
        Err(PlatformError::Unsupported("the clipboard in the benchmark"))
    }
    fn read_clipboard_media(&self, _request: MediaRequest) -> ClipboardMedia {
        ClipboardMedia::default()
    }
    fn os_search(&self, _request: &OsSearchRequest) -> Result<Vec<OsHit>, OsSearchError> {
        Err(OsSearchError::Unavailable(
            "no OS index in the benchmark".into(),
        ))
    }
}

/// A small deterministic generator (the benchmark needs the same data every
/// run, and no `rand` dependency).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn pick<'a>(&mut self, words: &[&'a str]) -> &'a str {
        words[(self.next() as usize) % words.len()]
    }
}

const WORDS: &[&str] = &[
    "report",
    "invoice",
    "budget",
    "meeting",
    "notes",
    "draft",
    "final",
    "backup",
    "photo",
    "holiday",
    "project",
    "design",
    "review",
    "summary",
    "contract",
    "resume",
    "travel",
    "taxes",
    "receipt",
    "slides",
    "proposal",
    "schedule",
    "archive",
    "readme",
    "config",
    "server",
    "client",
    "module",
    "release",
    "changelog",
    "screenshot",
    "recording",
    "music",
    "podcast",
    "lecture",
    "thesis",
    "dataset",
    "export",
    "import",
    "migration",
];
const EXTENSIONS: &[&str] = &[
    "pdf", "docx", "xlsx", "txt", "md", "png", "jpg", "rs", "ts", "svelte", "json", "toml", "csv",
    "zip", "mp3", "mp4", "log", "html",
];
const APP_BASES: &[&str] = &[
    "Firefox",
    "Chrome",
    "Visual Studio Code",
    "Notepad",
    "Terminal",
    "Calculator",
    "Photos",
    "Slack",
    "Spotify",
    "Discord",
    "Zoom",
    "Word",
    "Excel",
    "PowerPoint",
    "Outlook",
    "Teams",
    "Paint",
    "Settings",
    "File Explorer",
    "Task Manager",
    "Docker Desktop",
    "Postman",
    "IntelliJ IDEA",
    "Blender",
    "Inkscape",
    "GIMP",
    "VLC",
    "OBS Studio",
    "Steam",
    "Git Bash",
];
const APP_SUFFIXES: &[&str] = &[
    "Pro",
    "Lite",
    "Studio",
    "Suite",
    "Manager",
    "Viewer",
    "Editor",
    "Tools",
    "Helper",
    "Center",
    "Updater",
    "Launcher",
    "Companion",
    "Assistant",
];

pub fn gen_apps(count: usize) -> Vec<AppEntry> {
    let mut rng = Lcg(1);
    (0..count)
        .map(|i| {
            let base = if i < APP_BASES.len() {
                APP_BASES[i].to_owned()
            } else {
                format!("{} {} {}", rng.pick(APP_BASES), rng.pick(APP_SUFFIXES), i)
            };
            AppEntry {
                id: format!("app-{i}"),
                name: base.clone(),
                description: Some(format!("{base} application")),
                keywords: vec![rng.pick(WORDS).to_owned()],
                icon: None,
                target: LaunchTarget::Shortcut {
                    path: PathBuf::from(format!("C:/ProgramData/Start Menu/{base}.lnk")),
                },
            }
        })
        .collect()
}

pub fn gen_files(home: &Path, count: usize) -> Vec<FileEntry> {
    let mut rng = Lcg(2);
    (0..count)
        .map(|i| {
            let dir = home
                .join("Documents")
                .join(format!("{}-{}", rng.pick(WORDS), i % 997))
                .join(rng.pick(WORDS));
            if i % 20 == 0 {
                let name = format!("{}-{}", rng.pick(WORDS), i);
                FileEntry {
                    path: dir.join(&name),
                    name,
                    is_dir: true,
                }
            } else {
                let name = format!(
                    "{}-{}-{}.{}",
                    rng.pick(WORDS),
                    rng.pick(WORDS),
                    2015 + i % 11,
                    rng.pick(EXTENSIONS)
                );
                FileEntry {
                    path: dir.join(&name),
                    name,
                    is_dir: false,
                }
            }
        })
        .collect()
}

/// A Chromium `Bookmarks` file with `count` bookmarks in folders of 100.
pub fn gen_bookmarks_json(count: usize) -> String {
    let mut rng = Lcg(3);
    let mut folders = Vec::new();
    for folder in 0..count.div_ceil(100) {
        let children: Vec<String> = (0..100.min(count - folder * 100))
            .map(|j| {
                let n = folder * 100 + j;
                let title = format!("{} {} {}", rng.pick(WORDS), rng.pick(WORDS), n);
                format!(
                    r#"{{"type":"url","name":"{title}","url":"https://example.org/{}/{}/{n}"}}"#,
                    rng.pick(WORDS),
                    rng.pick(WORDS)
                )
            })
            .collect();
        folders.push(format!(
            r#"{{"type":"folder","name":"Folder {folder}","children":[{}]}}"#,
            children.join(",")
        ));
    }
    format!(
        r#"{{"roots":{{"bookmark_bar":{{"type":"folder","name":"Bookmarks bar","children":[{}]}},"other":{{"type":"folder","name":"Other bookmarks","children":[]}}}}}}"#,
        folders.join(",")
    )
}

fn gen_history_json(count: usize) -> String {
    let mut rng = Lcg(4);
    let items: Vec<String> = (0..count)
        .map(|i| {
            format!(
                r#"{{"text":"{} {} {} copied text {i}","copied_at":{}}}"#,
                rng.pick(WORDS),
                rng.pick(WORDS),
                rng.pick(WORDS),
                1_700_000_000 + i
            )
        })
        .collect();
    format!(r#"{{"version":1,"items":[{}]}}"#, items.join(","))
}

/// The data on disk plus everything needed to build engines from it.
pub struct Fixture {
    dir: tempfile::TempDir,
    sizes: Sizes,
    platform: Arc<FixturePlatform>,
    config: Config,
    files_plugin: Arc<FilesPlugin>,
    pub engine: SearchEngine,
}

impl Fixture {
    pub fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }

    /// The default engine over the fixture, *not refreshed yet*.
    pub fn new(sizes: Sizes) -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let home = dir.path().join("home");
        // A real folder for path browsing (`~/Documents/rep`).
        let documents = home.join("Documents");
        fs::create_dir_all(&documents).unwrap();
        for i in 0..300 {
            fs::write(documents.join(format!("report-{i:03}.txt")), b"").unwrap();
        }
        fs::create_dir_all(documents.join("Reports")).unwrap();
        // The tree the files plugin scans, when asked to (startup measurement).
        let scan_root = home.join("Projects");
        for i in 0..sizes.files_on_disk {
            let sub = scan_root
                .join(format!("p{}", i / 200))
                .join(format!("m{}", i % 7));
            fs::create_dir_all(&sub).unwrap();
            fs::write(sub.join(format!("file-{i}.txt")), b"").unwrap();
        }

        // Bookmarks: one Chromium profile.
        let profile = dir.path().join("chrome").join("Default");
        fs::create_dir_all(&profile).unwrap();
        fs::write(
            profile.join("Bookmarks"),
            gen_bookmarks_json(sizes.bookmarks),
        )
        .unwrap();

        // Clipboard history.
        let history = dir.path().join("data").join("clipboard-history.json");
        fs::create_dir_all(history.parent().unwrap()).unwrap();
        fs::write(&history, gen_history_json(sizes.clipboard)).unwrap();

        // Workflows: the two shipped examples, approved.
        let workflows = dir.path().join("config").join("workflows");
        for (folder, text) in [
            (
                "duckduckgo",
                include_str!("../../../../examples/workflows/duckduckgo/workflow.toml"),
            ),
            (
                "tidy-text",
                include_str!("../../../../examples/workflows/tidy-text/workflow.toml"),
            ),
        ] {
            fs::create_dir_all(workflows.join(folder)).unwrap();
            fs::write(workflows.join(folder).join("workflow.toml"), text).unwrap();
        }

        let mut config = Config::default();
        config.clipboard.enabled = true;
        config.files.directories = if sizes.files_on_disk > 0 {
            vec![scan_root.display().to_string()]
        } else {
            Vec::new()
        };
        config.files.max_depth = 4;

        let platform = Arc::new(FixturePlatform {
            apps: gen_apps(sizes.apps),
        });
        let dyn_platform: Arc<dyn PlatformProvider> = platform.clone();

        // Every built-in plugin through the registry, except the three whose
        // data comes from somewhere the fixture controls.
        let mut plugins: Vec<Arc<dyn Plugin>> = PluginRegistry::builtin()
            .instantiate(&config, dyn_platform.clone())
            .into_iter()
            .filter(|p| !matches!(p.id(), "files" | "bookmarks" | "clipboard"))
            .collect();
        let files_plugin = Arc::new(FilesPlugin::with_home(
            config.files.clone(),
            dyn_platform.clone(),
            Some(home.clone()),
        ));
        plugins.push(files_plugin.clone());
        plugins.push(Arc::new(BookmarksPlugin::with_roots(
            config.bookmarks.clone(),
            dyn_platform.clone(),
            vec![BrowserRoot {
                id: "chrome",
                name: "Chrome",
                family: BrowserFamily::Chromium,
                dir: dir.path().join("chrome"),
            }],
        )));
        plugins.push(Arc::new(ClipboardPlugin::new(
            &config.clipboard,
            &config.paste,
            dyn_platform.clone(),
            Some(history),
        )));

        let host = WorkflowHost::new(
            workflows,
            dir.path().join("data").join("workflows"),
            dir.path().join("data").join("workflow-approvals.json"),
            Arc::new(NoSink),
        );
        for pending in host.pending(&config) {
            host.approve(&pending).expect("approve example workflow");
        }
        plugins.extend(host.plugins(&config, &dyn_platform));

        let engine = SearchEngine::new(plugins, UsageStore::default(), EngineOptions::default());
        Self {
            dir,
            sizes,
            platform,
            config,
            files_plugin,
            engine,
        }
    }

    /// Refreshes every plugin (the startup work) and returns how long it took.
    /// Then fills the files index with the generated entries, if any.
    pub fn refresh(&self) -> Duration {
        let started = Instant::now();
        let failures = self.engine.refresh_all();
        let elapsed = started.elapsed();
        assert!(
            failures.is_empty(),
            "a plugin failed to refresh: {failures:?}"
        );
        if self.sizes.files > 0 {
            self.files_plugin
                .load_entries(gen_files(&self.home(), self.sizes.files));
        }
        elapsed
    }

    /// A refreshed fixture, ready to query.
    pub fn ready(sizes: Sizes) -> Self {
        let fixture = Self::new(sizes);
        fixture.refresh();
        fixture
    }
}

/// The queries the benchmark and the budget test run: `(name, input, answers)`; `answers` is false for the one query that matches nothing.
pub const QUERIES: &[(&str, &str, bool)] = &[
    ("single letter", "a", true),
    ("short prefix", "fir", true),
    ("app name", "visual studio", true),
    ("multi-word", "budget report meeting", true),
    ("no match anywhere", "qzxvkj", false),
    ("keyword: web", "g rust lifetimes", true),
    ("keyword: bookmarks", "b budget", true),
    ("keyword: clipboard", "cb copied", true),
    ("keyword: files", "f invoice", true),
    ("keyword: workflow", "ddg rust", true),
    ("path browse", "~/Documents/rep", true),
    ("calculator", "(12 + 8) * 3 / 4", true),
    ("unit conversion", "12 km in miles", true),
];

impl Fixture {
    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn platform(&self) -> &Arc<FixturePlatform> {
        &self.platform
    }
}
