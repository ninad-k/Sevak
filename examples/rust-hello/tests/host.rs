//! The example extension, built and run through Sevak's real script-plugin host:
//! first-run approval bound to the program's bytes, the persistent protocol over
//! real pipes, the closed action vocabulary and a `custom` action coming back.
//!
//! This is what proves `sevak-extension-sdk` and the host agree. The program is
//! the one cargo builds for this package (`CARGO_BIN_EXE_rust-hello`); no
//! network, and everything lives in a temporary folder.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sevak_core::{
    Action, Config, EngineOptions, IconData, IconSource, LaunchTarget, ResultItem, ResultsNotifier,
    SearchEngine, UsageStore,
};
use sevak_platform::{PlatformError, PlatformProvider, Result as PlatformResult};
use sevak_plugins::script::{current_platform, Native};
use sevak_plugins::ScriptPluginHost;

const WAIT: Duration = Duration::from_secs(30);
const FOLDER: &str = "rust-hello";

#[derive(Default)]
struct RecordingPlatform {
    clipboard: Mutex<Vec<String>>,
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

struct World {
    root: tempfile::TempDir,
    host: ScriptPluginHost,
    platform: Arc<RecordingPlatform>,
}

impl World {
    /// Installs the example the way the gallery would: its own `plugin.toml`
    /// and the program under the name the manifest declares for this platform.
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let plugins = root.path().join("config").join("plugins");
        let dir = plugins.join(FOLDER);
        let manifest =
            std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("plugin.toml"))
                .unwrap();
        let native = Native::parse_text(&manifest).unwrap().unwrap();
        let program = native.binary_for(&current_platform()).unwrap().to_owned();
        std::fs::create_dir_all(dir.join(&program).parent().unwrap()).unwrap();
        std::fs::write(dir.join("plugin.toml"), manifest).unwrap();
        std::fs::copy(env!("CARGO_BIN_EXE_rust-hello"), dir.join(&program)).unwrap();
        let host = ScriptPluginHost::new(
            plugins,
            root.path().join("data").join("plugins"),
            root.path().join("data").join("approvals.json"),
        );
        Self {
            root,
            host,
            platform: Arc::new(RecordingPlatform::default()),
        }
    }

    fn program(&self) -> PathBuf {
        let manifest = std::fs::read_to_string(self.plugin_dir().join("plugin.toml")).unwrap();
        let native = Native::parse_text(&manifest).unwrap().unwrap();
        self.plugin_dir()
            .join(native.binary_for(&current_platform()).unwrap())
    }

    fn plugin_dir(&self) -> PathBuf {
        self.root.path().join("config").join("plugins").join(FOLDER)
    }

    fn data_dir(&self) -> PathBuf {
        self.root.path().join("data").join("plugins").join(FOLDER)
    }

    fn engine(&self) -> (SearchEngine, Receiver<String>) {
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

fn query_until_results(
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

fn titles(items: &[ResultItem]) -> Vec<&str> {
    items.iter().map(|item| item.title.as_str()).collect()
}

fn wait_for_file(path: &Path) -> String {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Ok(text) = std::fs::read_to_string(path) {
            return text;
        }
        assert!(
            Instant::now() < deadline,
            "{} never appeared",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn the_extension_waits_for_approval_then_answers_and_acts() {
    let world = World::new();

    // Nothing runs before the user allows it, and the dialog says what it is.
    assert!(world
        .host
        .plugins(
            &Config::default(),
            &(world.platform.clone() as Arc<dyn PlatformProvider>)
        )
        .is_empty());
    let pending = world.host.pending(&Config::default());
    assert_eq!(pending.len(), 1);
    let prompt = pending[0].prompt();
    assert!(prompt.contains("NATIVE EXTENSION"), "{prompt}");
    assert!(prompt.contains("Publisher: Sevak project"), "{prompt}");
    assert!(prompt.contains("Declared permissions: none"), "{prompt}");
    let program_hash = sevak_plugins::net::sha256_hex(&std::fs::read(world.program()).unwrap());
    assert!(prompt.contains(&program_hash), "{prompt}");
    world.host.approve(&pending[0]).unwrap();

    let (engine, rx) = world.engine();
    let items = query_until_results(&engine, &rx, "rh Ada");
    assert_eq!(
        titles(&items),
        ["Hello, Ada!", "HELLO, ADA!", "Remember Ada"]
    );
    assert_eq!(items[0].id, "script:rust-hello:hello");
    // Scores keep the order the extension gave.
    assert!(items[0].score > items[1].score && items[1].score > items[2].score);

    // A standard action is performed by Sevak itself.
    engine.execute(&items[0], "rh Ada").unwrap();
    assert_eq!(*world.platform.clipboard.lock().unwrap(), ["Hello, Ada!"]);
    assert!(matches!(items[2].action, Action::Custom { .. }));

    // A custom action comes back to the program, which saves the name.
    engine.execute(&items[2], "rh Ada").unwrap();
    assert_eq!(wait_for_file(&world.data_dir().join("names.txt")), "Ada\n");

    // The keyword alone lists what was saved.
    let hint = query_until_results(&engine, &rx, "rh ");
    assert!(
        titles(&hint).contains(&"Hello again, Ada!"),
        "{:?}",
        titles(&hint)
    );
}

#[test]
fn a_changed_program_asks_again() {
    let world = World::new();
    world
        .host
        .approve(&world.host.pending(&Config::default())[0])
        .unwrap();
    assert!(world.host.pending(&Config::default()).is_empty());

    // Same manifest, one more byte in the program: not what was allowed.
    let program = world.program();
    let mut bytes = std::fs::read(&program).unwrap();
    bytes.push(0);
    std::fs::write(&program, bytes).unwrap();

    let pending = world.host.pending(&Config::default());
    assert_eq!(pending.len(), 1, "a changed program must be reviewed again");
    assert!(pending[0].reviewed_before);
    assert!(pending[0].prompt().contains("review again"));
}
