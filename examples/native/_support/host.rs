//! Shared by the host tests of the native extensions in this folder
//! (`#[path = "../../_support/host.rs"] mod host;`). Not a crate of its own.
//!
//! It installs the built program the way the gallery would, in a temporary
//! folder, and drives it through Sevak's real script-plugin host: first-run
//! approval bound to the program's bytes, the persistent protocol over real
//! pipes, and the closed action vocabulary. No network.

#![allow(dead_code)] // each test file uses the part it needs

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sevak_core::{
    Config, EngineOptions, IconData, IconSource, LaunchTarget, ResultItem, ResultsNotifier,
    SearchEngine, UsageStore,
};
use sevak_platform::{PlatformError, PlatformProvider, Result as PlatformResult};
use sevak_plugins::script::{current_platform, Native};
use sevak_plugins::ScriptPluginHost;

pub const WAIT: Duration = Duration::from_secs(30);

#[derive(Default)]
pub struct RecordingPlatform {
    pub clipboard: Mutex<Vec<String>>,
}

impl PlatformProvider for RecordingPlatform {
    fn list_applications(&self) -> PlatformResult<Vec<sevak_core::AppEntry>> {
        Ok(Vec::new())
    }
    fn launch(&self, _target: &LaunchTarget) -> PlatformResult<()> {
        Ok(())
    }
    fn open_path(&self, _path: &Path) -> PlatformResult<()> {
        Ok(())
    }
    fn open_url(&self, _url: &str) -> PlatformResult<()> {
        Ok(())
    }
    fn load_icon(&self, _source: &IconSource, _size: u32) -> PlatformResult<IconData> {
        Err(PlatformError::Unsupported("icons in tests"))
    }
    fn set_clipboard_text(&self, text: &str) -> PlatformResult<()> {
        self.clipboard.lock().unwrap().push(text.to_owned());
        Ok(())
    }
}

pub struct World {
    root: tempfile::TempDir,
    folder: String,
    pub host: ScriptPluginHost,
    pub platform: Arc<RecordingPlatform>,
}

impl World {
    /// Installs the extension: its own `plugin.toml` (from `manifest_dir`) and
    /// `program` (the built binary) under the name the manifest declares for
    /// this platform, in a folder named like the extension's id.
    pub fn new(manifest_dir: &str, program: &str, folder: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let plugins = root.path().join("config").join("plugins");
        let dir = plugins.join(folder);
        let manifest =
            std::fs::read_to_string(Path::new(manifest_dir).join("plugin.toml")).unwrap();
        let native = Native::parse_text(&manifest).unwrap().unwrap();
        let name = native.binary_for(&current_platform()).unwrap().to_owned();
        std::fs::create_dir_all(dir.join(&name).parent().unwrap()).unwrap();
        std::fs::write(dir.join("plugin.toml"), manifest).unwrap();
        std::fs::copy(program, dir.join(&name)).unwrap();
        let host = ScriptPluginHost::new(
            plugins,
            root.path().join("data").join("plugins"),
            root.path().join("data").join("approvals.json"),
        );
        Self {
            root,
            folder: folder.to_owned(),
            host,
            platform: Arc::new(RecordingPlatform::default()),
        }
    }

    pub fn plugin_dir(&self) -> PathBuf {
        self.root
            .path()
            .join("config")
            .join("plugins")
            .join(&self.folder)
    }

    pub fn program(&self) -> PathBuf {
        let manifest = std::fs::read_to_string(self.plugin_dir().join("plugin.toml")).unwrap();
        let native = Native::parse_text(&manifest).unwrap().unwrap();
        self.plugin_dir()
            .join(native.binary_for(&current_platform()).unwrap())
    }

    /// What the user sees before anything runs: nothing is loaded and exactly
    /// one native extension is waiting for permission. Returns the dialog text.
    pub fn assert_waits_for_approval(&self) -> String {
        let platform: Arc<dyn PlatformProvider> = self.platform.clone();
        assert!(
            self.host.plugins(&Config::default(), &platform).is_empty(),
            "nothing may run before the user allows it"
        );
        let pending = self.host.pending(&Config::default());
        assert_eq!(pending.len(), 1);
        pending[0].prompt()
    }

    pub fn approve(&self) {
        let pending = self.host.pending(&Config::default());
        assert_eq!(pending.len(), 1);
        self.host.approve(&pending[0]).unwrap();
    }

    pub fn program_sha256(&self) -> String {
        sevak_plugins::net::sha256_hex(&std::fs::read(self.program()).unwrap())
    }

    pub fn engine(&self) -> (SearchEngine, Receiver<String>) {
        let platform: Arc<dyn PlatformProvider> = self.platform.clone();
        let plugins = self.host.plugins(&Config::default(), &platform);
        let engine = SearchEngine::new(plugins, UsageStore::default(), EngineOptions::default());
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let notifier: ResultsNotifier = Arc::new(move |plugin_id: &str| {
            let _ = tx.lock().unwrap().send(plugin_id.to_owned());
        });
        engine.attach_notifier(&notifier);
        (engine, rx)
    }
}

/// Queries until the extension has answered (the first query waits for the
/// program to start), then returns its rows.
pub fn query_until_results(
    engine: &SearchEngine,
    rx: &Receiver<String>,
    query: &str,
) -> Vec<ResultItem> {
    let deadline = Instant::now() + WAIT;
    loop {
        let items = engine.query(query);
        if !items.is_empty() {
            return items;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(!left.is_zero(), "no results for {query:?} within {WAIT:?}");
        let _ = rx.recv_timeout(left.min(Duration::from_millis(200)));
    }
}

/// The extension's keyword is not taken by a built-in, a search engine or a
/// workflow (the gallery requires a unique one).
pub fn assert_keyword_is_free(manifest_dir: &str) {
    let manifest = std::fs::read_to_string(Path::new(manifest_dir).join("plugin.toml")).unwrap();
    let keyword = manifest
        .lines()
        .find_map(|line| line.strip_prefix("keyword"))
        .and_then(|rest| rest.split('"').nth(1))
        .expect("plugin.toml has a keyword")
        .to_lowercase();
    let owners = sevak_plugins::keywords::KeywordOwners::builtin(&Config::default());
    assert!(
        owners.owners_of(&keyword, None).is_empty(),
        "{keyword:?} is taken"
    );
}

pub fn titles(items: &[ResultItem]) -> Vec<&str> {
    items.iter().map(|item| item.title.as_str()).collect()
}
