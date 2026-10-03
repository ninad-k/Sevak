//! End-to-end tests of script plugins: real child processes, the real search
//! engine, a recording platform.
//!
//! The script is the `sevak-script-fixture` binary (a small compiled program), so the
//! tests need no Python or Node and run the same on Windows, macOS and Linux.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sevak_core::{
    Action, Config, EngineOptions, IconData, IconSource, LaunchTarget, ResultItem, ResultsNotifier,
    SearchEngine, UsageStore,
};
use sevak_platform::{PlatformError, PlatformProvider, Result as PlatformResult};
use sevak_plugins::ScriptPluginHost;

const WAIT: Duration = Duration::from_secs(10);

#[derive(Default)]
struct RecordingPlatform {
    clipboard: Mutex<Vec<String>>,
    urls: Mutex<Vec<String>>,
    paths: Mutex<Vec<PathBuf>>,
}

impl PlatformProvider for RecordingPlatform {
    fn list_applications(&self) -> PlatformResult<Vec<sevak_core::AppEntry>> {
        Ok(Vec::new())
    }
    fn launch(&self, _target: &LaunchTarget) -> PlatformResult<()> {
        Ok(())
    }
    fn open_path(&self, path: &Path) -> PlatformResult<()> {
        self.paths.lock().unwrap().push(path.to_path_buf());
        Ok(())
    }
    fn open_url(&self, url: &str) -> PlatformResult<()> {
        self.urls.lock().unwrap().push(url.to_owned());
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

/// The `sevak-script-fixture` binary (`tests/fixtures/script_fixture.rs`),
/// which cargo builds before running this test.
fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_sevak-script-fixture"))
}

struct World {
    root: tempfile::TempDir,
    host: ScriptPluginHost,
    platform: Arc<RecordingPlatform>,
}

impl World {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let host = ScriptPluginHost::new(
            root.path().join("config").join("plugins"),
            root.path().join("data").join("plugins"),
            root.path().join("data").join("approvals.json"),
        );
        Self {
            root,
            host,
            platform: Arc::new(RecordingPlatform::default()),
        }
    }

    fn plugin_dir(&self, folder: &str) -> PathBuf {
        self.root.path().join("config").join("plugins").join(folder)
    }

    fn data_dir(&self, folder: &str) -> PathBuf {
        self.root.path().join("data").join("plugins").join(folder)
    }

    /// Creates a plugin folder whose script is the fixture, started with `args`.
    fn add(&self, folder: &str, keyword: &str, args: &[&str], extra: &str) {
        let dir = self.plugin_dir(folder);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("icon.png"), b"not really a png").unwrap();
        std::fs::write(
            self.root
                .path()
                .join("config")
                .join("plugins")
                .join("icon.png"),
            b"x",
        )
        .unwrap();
        let mut command = vec![format!("'{}'", fixture().display())];
        command.extend(args.iter().map(|arg| format!("'{arg}'")));
        let manifest = format!(
            "protocol = 1\nid = \"script:{folder}\"\nkeyword = \"{keyword}\"\n\
             command = [{}]\n{extra}\n",
            command.join(", ")
        );
        std::fs::write(dir.join("plugin.toml"), manifest).unwrap();
    }

    fn approve_all(&self) {
        for candidate in self.host.pending(&Config::default()) {
            self.host.approve(&candidate).unwrap();
        }
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

/// Runs `query`, re-running it after each "results updated" notification until
/// it has results, like the shell does.
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
        // Either a notification arrives or we poll again shortly (a restart
        // after a crash waits out a backoff and sends no notification).
        let _ = rx.recv_timeout(left.min(Duration::from_millis(200)));
    }
}

fn titles(items: &[ResultItem]) -> Vec<&str> {
    items.iter().map(|item| item.title.as_str()).collect()
}

fn pid_of(items: &[ResultItem]) -> String {
    let echo = items
        .iter()
        .find(|item| item.id.ends_with(":echo"))
        .unwrap();
    echo.subtitle.rsplit(' ').next().unwrap().to_owned()
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
fn persistent_plugin_answers_queries_and_runs_actions() {
    let world = World::new();
    world.add("fx", "fx", &[], "");
    world.approve_all();
    let (engine, rx) = world.engine();

    let items = query_until_results(&engine, &rx, "fx hello");
    assert_eq!(
        titles(&items),
        [
            "echo: hello",
            "custom: hello",
            "icon outside the plugin folder"
        ]
    );
    assert_eq!(items[0].id, "script:fx:echo");
    assert_eq!(items[0].plugin_id, "script:fx");
    assert!(
        items[0].subtitle.starts_with("script:fx pid "),
        "{}",
        items[0].subtitle
    );
    // The script's scores are honoured but stay below the keyword score.
    assert!(items[0].score > items[1].score && items[1].score > items[2].score);

    // Icons come from the plugin folder; one that escapes it is dropped.
    assert_eq!(
        items[0].icon,
        Some(IconSource::File {
            path: world.plugin_dir("fx").join("icon.png")
        })
    );
    assert_eq!(items[2].icon, None);

    // A standard action is performed by Sevak, not the script.
    engine.execute(&items[0], "fx hello").unwrap();
    assert_eq!(*world.platform.clipboard.lock().unwrap(), ["hello"]);

    // A custom action is sent back to the script.
    engine.execute(&items[1], "fx hello").unwrap();
    let note = wait_for_file(&world.data_dir("fx").join("executed.txt"));
    assert_eq!(note, "custom hello");

    // The keyword is required: the plugin never answers global queries.
    assert!(engine
        .query("hello")
        .iter()
        .all(|item| item.plugin_id != "script:fx"));
}

#[test]
fn a_slow_script_never_blocks_typing_and_its_late_answer_is_announced() {
    let world = World::new();
    world.add("fx", "fx", &[], "");
    world.approve_all();
    let (engine, rx) = world.engine();
    query_until_results(&engine, &rx, "fx warm");
    while rx.try_recv().is_ok() {}

    let started = Instant::now();
    let items = engine.query("fx slow");
    let waited = started.elapsed();
    assert!(
        items.is_empty(),
        "the script needs 800 ms, got {:?}",
        titles(&items)
    );
    assert!(
        waited < Duration::from_millis(500),
        "typing stalled for {waited:?}"
    );

    // The late answer triggers a notification...
    assert_eq!(rx.recv_timeout(WAIT).unwrap(), "script:fx");
    // ...and re-running the same query now returns it from the cache at once.
    let started = Instant::now();
    let items = engine.query("fx slow");
    assert!(started.elapsed() < Duration::from_millis(200));
    assert_eq!(items[0].title, "echo: slow");
}

#[test]
fn answers_for_queries_the_user_typed_past_are_dropped() {
    let world = World::new();
    world.add("fx", "fx", &[], "");
    world.approve_all();
    let (engine, rx) = world.engine();
    query_until_results(&engine, &rx, "fx warm");
    while rx.try_recv().is_ok() {}

    // Two slow queries back to back: the first answer (for "slow") is stale by
    // the time it arrives and must not be announced or served.
    assert!(engine.query("fx slow").is_empty());
    assert!(engine.query("fx slowx").is_empty());
    assert_eq!(rx.recv_timeout(WAIT).unwrap(), "script:fx");
    assert!(
        rx.recv_timeout(Duration::from_millis(1500)).is_err(),
        "the stale answer must not notify"
    );
    assert_eq!(engine.query("fx slowx")[0].title, "echo: slowx");
}

#[test]
fn a_crashing_script_is_restarted_after_a_backoff() {
    let world = World::new();
    world.add("fx", "fx", &[], "");
    world.approve_all();
    let (engine, rx) = world.engine();
    let first_pid = pid_of(&query_until_results(&engine, &rx, "fx hello"));

    assert!(engine.query("fx crash").is_empty());
    std::thread::sleep(Duration::from_millis(100));
    // Backing off: nothing, and no new process yet.
    assert!(engine.query("fx other").is_empty());

    std::thread::sleep(Duration::from_millis(700));
    let items = query_until_results(&engine, &rx, "fx zebra");
    assert_ne!(pid_of(&items), first_pid, "expected a new process");
}

#[test]
fn stdout_noise_does_not_break_the_protocol() {
    let world = World::new();
    world.add("fx", "fx", &[], "");
    world.approve_all();
    let (engine, rx) = world.engine();
    let items = query_until_results(&engine, &rx, "fx garbage");
    assert_eq!(items[0].title, "echo: garbage");
}

#[test]
fn a_script_that_never_answers_is_stopped_and_restarted() {
    let world = World::new();
    world.add("fx", "fx", &[], "hard_timeout_ms = 600");
    world.approve_all();
    let (engine, rx) = world.engine();
    let first_pid = pid_of(&query_until_results(&engine, &rx, "fx hello"));

    assert!(engine.query("fx hang").is_empty());
    std::thread::sleep(Duration::from_millis(900));
    // This query finds the script hung and stops it; later ones restart it.
    assert!(engine.query("fx other").is_empty());
    let items = query_until_results(&engine, &rx, "fx zebra");
    assert_ne!(pid_of(&items), first_pid);
}

#[test]
fn an_idle_script_is_stopped_and_started_again_on_demand() {
    let world = World::new();
    world.add("fx", "fx", &[], "idle_timeout_secs = 1");
    world.approve_all();
    let (engine, rx) = world.engine();
    let first_pid = pid_of(&query_until_results(&engine, &rx, "fx hello"));

    std::thread::sleep(Duration::from_millis(2500));
    let items = query_until_results(&engine, &rx, "fx again");
    assert_ne!(
        pid_of(&items),
        first_pid,
        "the idle script should have been stopped"
    );
}

#[test]
fn shutdown_tells_the_script_to_exit() {
    let world = World::new();
    world.add("fx", "fx", &[], "");
    world.approve_all();
    let (engine, rx) = world.engine();
    query_until_results(&engine, &rx, "fx hello");
    engine.shutdown();
    assert!(world.data_dir("fx").exists());
    wait_for_file(&world.data_dir("fx").join("shutdown.txt"));
}

#[test]
fn oneshot_scripts_run_per_query_with_the_query_as_an_argument() {
    let world = World::new();
    world.add("shot", "os", &["oneshot-sevak"], "mode = \"oneshot\"");
    world.approve_all();
    let (engine, rx) = world.engine();

    let items = query_until_results(&engine, &rx, "os hello world");
    assert_eq!(items[0].title, "echo: hello world");
    // A different query is a different process.
    let first_pid = pid_of(&items);
    let items = query_until_results(&engine, &rx, "os other");
    assert_eq!(items[0].title, "echo: other");
    assert_ne!(pid_of(&items), first_pid);
    // Custom actions need a persistent process, so one-shot output drops them.
    assert!(items
        .iter()
        .all(|item| !matches!(item.action, Action::Custom { .. })));
}

#[test]
fn alfred_script_filter_output_is_mapped_to_actions() {
    let world = World::new();
    world.add(
        "alfred",
        "al",
        &["oneshot-alfred"],
        "mode = \"oneshot\"\nformat = \"alfred\"",
    );
    world.approve_all();
    let (engine, rx) = world.engine();

    let items = query_until_results(&engine, &rx, "al rust");
    assert_eq!(titles(&items), ["alfred: rust", "plain text"]);
    assert_eq!(items[0].id, "script:alfred:link");
    assert_eq!(
        items[0].action,
        Action::OpenUrl {
            url: "https://example.com/?q=rust".into()
        }
    );
    assert_eq!(
        items[1].action,
        Action::CopyText {
            text: "just text".into()
        }
    );
    assert!(matches!(items[0].icon, Some(IconSource::File { .. })));

    // `autocomplete` is what Tab inserts; the engine puts the keyword back.
    assert_eq!(items[0].autocomplete.as_deref(), Some("al alfred rust "));
    assert_eq!(items[1].autocomplete, None);
    // `mods` become secondary actions: `alt` on Alt+Enter, the combination in
    // the action panel only.
    let secondary: Vec<_> = items[0]
        .secondary
        .iter()
        .map(|s| (s.label.as_str(), s.modifier))
        .collect();
    assert_eq!(
        secondary,
        [
            ("Copy the text", Some(sevak_core::Modifier::Alt)),
            ("Open both", None)
        ]
    );
    assert!(items[1].secondary.is_empty());

    engine.execute(&items[0], "al rust").unwrap();
    assert_eq!(
        *world.platform.urls.lock().unwrap(),
        ["https://example.com/?q=rust"]
    );
    engine.execute(&items[1], "al rust").unwrap();
    assert_eq!(*world.platform.clipboard.lock().unwrap(), ["just text"]);

    // Alt+Enter runs the mod's own action through the same plugin.
    engine.execute_secondary(&items[0], 0, "al rust").unwrap();
    assert_eq!(
        *world.platform.clipboard.lock().unwrap(),
        ["just text", "copy rust"]
    );
    engine.execute_secondary(&items[0], 1, "al rust").unwrap();
    assert_eq!(
        *world.platform.urls.lock().unwrap(),
        ["https://example.com/?q=rust", "https://example.com/both"]
    );
}

#[test]
fn a_one_shot_script_that_is_too_slow_is_announced_later() {
    let world = World::new();
    world.add("shot", "os", &["oneshot-sevak"], "mode = \"oneshot\"");
    world.approve_all();
    let (engine, rx) = world.engine();
    let started = Instant::now();
    assert!(engine.query("os slow").is_empty());
    assert!(started.elapsed() < Duration::from_millis(500));
    assert_eq!(rx.recv_timeout(WAIT).unwrap(), "script:shot");
    assert_eq!(engine.query("os slow")[0].title, "echo: slow");
}

#[test]
fn plugins_are_not_loaded_until_approved_or_when_disabled() {
    let world = World::new();
    world.add("fx", "fx", &[], "");
    let (engine, _rx) = world.engine();
    assert!(
        engine.plugins().is_empty(),
        "an unapproved plugin must not load"
    );
    assert_eq!(world.host.pending(&Config::default()).len(), 1);

    world.approve_all();
    let (engine, _rx) = world.engine();
    assert_eq!(engine.plugins().len(), 1);

    let mut config = Config::default();
    config.plugins.disabled = vec!["script:fx".into()];
    let platform: Arc<dyn PlatformProvider> = world.platform.clone();
    assert!(world.host.plugins(&config, &platform).is_empty());
}

/// The plugins under `examples/plugins` are documentation people copy, so they
/// must keep working. Each runs only where its interpreter is installed.
#[test]
fn the_bundled_examples_work_where_their_interpreter_is_installed() {
    use sevak_platform::process::{script_runner, ScriptRunner};

    let installed =
        |extension: &str| matches!(script_runner(extension), ScriptRunner::Interpreter(_));
    let root = tempfile::tempdir().unwrap();
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/plugins");
    let host = ScriptPluginHost::new(
        examples,
        root.path().join("data").join("plugins"),
        root.path().join("data").join("approvals.json"),
    );
    for candidate in host.pending(&Config::default()) {
        host.approve(&candidate).unwrap();
    }
    let platform = Arc::new(RecordingPlatform::default());
    let provider: Arc<dyn PlatformProvider> = platform;
    let engine = SearchEngine::new(
        host.plugins(&Config::default(), &provider),
        UsageStore::default(),
        EngineOptions::default(),
    );
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let notifier: ResultsNotifier = Arc::new(move |id: &str| {
        let _ = tx.lock().unwrap().send(id.to_owned());
    });
    engine.attach_notifier(&notifier);
    assert_eq!(engine.plugins().len(), 3, "the three bundled examples");

    if installed("py") {
        let items = query_until_results(&engine, &rx, "hello Ada");
        assert_eq!(items[0].title, "Hello, Ada!");
        assert_eq!(
            items[0].action,
            Action::CopyText {
                text: "Hello, Ada!".into()
            }
        );
        let remember = items
            .iter()
            .find(|item| item.id.ends_with(":remember"))
            .unwrap();
        engine.execute(remember, "hello Ada").unwrap();
        let names = root.path().join("data/plugins/hello-python/names.txt");
        assert_eq!(wait_for_file(&names).trim(), "Ada");
    } else {
        eprintln!("skipping the Python example: no interpreter");
    }

    if installed("ps1") {
        let items = query_until_results(&engine, &rx, "ts 1700000000");
        assert_eq!(items[0].title, "2023-11-14T22:13:20Z");
        assert_eq!(items[2].title, "1700000000");
    } else {
        eprintln!("skipping the PowerShell example: no interpreter");
    }

    if installed("js") {
        let items = query_until_results(&engine, &rx, "case user account id");
        let titles = titles(&items);
        assert!(titles.contains(&"userAccountId"), "{titles:?}");
        assert!(titles.contains(&"user_account_id"), "{titles:?}");
        assert!(titles.contains(&"USER_ACCOUNT_ID"), "{titles:?}");
    } else {
        eprintln!("skipping the Node example: no interpreter");
    }
    engine.shutdown();
}
