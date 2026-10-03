//! End-to-end tests of workflows: real child processes for the script nodes,
//! the real search engine, a recording platform.
//!
//! The programs the nodes run are modes of the `sevak-script-fixture` binary
//! (`tests/fixtures/script_fixture.rs`), so the tests need no Python or Node and
//! run the same on Windows, macOS and Linux.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sevak_core::{
    Action, Config, EngineOptions, IconData, IconSource, LaunchTarget, ResultItem, ResultsNotifier,
    SearchEngine, Selection, UsageStore,
};
use sevak_platform::{PlatformError, PlatformProvider, Result as PlatformResult};
use sevak_plugins::workflow::{
    Ctx, NoSink, OutputSink, RunReport, Runtime, Workflow, WorkflowHost,
};

const WAIT: Duration = Duration::from_secs(15);

#[derive(Default)]
struct RecordingPlatform {
    clipboard: Mutex<Vec<String>>,
    urls: Mutex<Vec<String>>,
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

#[derive(Default)]
struct Notes(Mutex<Vec<String>>);

impl OutputSink for Notes {
    fn notify(&self, _heading: &str, body: &str) {
        self.0.lock().unwrap().push(body.to_owned());
    }
    fn large_type(&self, _text: &str) {}
    fn text_view(&self, _heading: &str, _text: &str) {}
}

/// The `sevak-script-fixture` binary, which cargo builds before this test.
fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_sevak-script-fixture"))
}

/// `['<fixture>', 'wf-echo', ...]` as a TOML array (literal strings keep
/// Windows paths intact).
fn program(mode: &str, args: &[&str]) -> String {
    let mut items = vec![format!("'{}'", fixture().display()), format!("'{mode}'")];
    items.extend(args.iter().map(|arg| format!("'{arg}'")));
    format!("[{}]", items.join(", "))
}

struct World {
    root: tempfile::TempDir,
    host: WorkflowHost,
    platform: Arc<RecordingPlatform>,
    notes: Arc<Notes>,
}

impl World {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let notes = Arc::new(Notes::default());
        let host = WorkflowHost::new(
            root.path().join("config").join("workflows"),
            root.path().join("data").join("workflows"),
            root.path().join("data").join("approvals.json"),
            notes.clone(),
        );
        Self {
            root,
            host,
            platform: Arc::new(RecordingPlatform::default()),
            notes,
        }
    }

    fn dir(&self, folder: &str) -> PathBuf {
        self.root
            .path()
            .join("config")
            .join("workflows")
            .join(folder)
    }

    fn data(&self, folder: &str) -> PathBuf {
        self.root.path().join("data").join("workflows").join(folder)
    }

    fn add(&self, folder: &str, toml: &str) {
        let dir = self.dir(folder);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("workflow.toml"), toml).unwrap();
    }

    fn clipboard(&self) -> Vec<String> {
        self.platform.clipboard.lock().unwrap().clone()
    }

    fn platform(&self) -> Arc<dyn PlatformProvider> {
        self.platform.clone()
    }

    fn approve_all(&self) {
        for candidate in self.host.pending(&Config::default()) {
            self.host.approve(&candidate).unwrap();
        }
    }

    /// A runtime for a workflow that is not installed anywhere (no approval
    /// is involved), to read its report.
    fn runtime(&self, folder: &str, toml: &str) -> Arc<Runtime> {
        let workflow = Workflow::from_toml(toml).unwrap();
        assert!(workflow.is_valid(), "{:?}", workflow.validate());
        let dir = self.dir(folder);
        std::fs::create_dir_all(&dir).unwrap();
        Runtime::new(
            folder,
            dir,
            self.data(folder),
            workflow,
            self.platform(),
            self.notes.clone(),
            &Config::default(),
        )
    }

    fn engine(&self) -> (SearchEngine, Receiver<String>) {
        let plugins = self.host.plugins(&Config::default(), &self.platform());
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

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
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
        let _ = rx.recv_timeout(left.min(Duration::from_millis(200)));
    }
}

fn run(runtime: &Runtime, start: &str, arg: &str) -> RunReport {
    runtime.run_blocking(start, Ctx::with_arg(arg))
}

// ---- script nodes ----------------------------------------------------------

#[test]
fn a_script_node_gets_arguments_stdin_and_environment_and_its_output_flows_on() {
    let w = World::new();
    let toml = format!(
        r#"
        name = "Scripts"
        [variables]
        GREETING = "hello"
        site = "example"

        [[node]]
        id = "go"
        type = "external"

        [[node]]
        id = "echo"
        type = "run_script"
        command = {echo}
        [[node]]
        id = "echo-copy"
        type = "copy"
        text = "echo={{query}}"

        [[node]]
        id = "env"
        type = "run_script"
        command = {env}
        [[node]]
        id = "env-copy"
        type = "copy"
        text = "env={{query}}"

        [[node]]
        id = "stdin"
        type = "run_script"
        command = {stdin}
        stdin = "shout {{query}}"
        [[node]]
        id = "stdin-copy"
        type = "copy"
        text = "stdin={{query}}"

        [[node]]
        id = "node-env"
        type = "run_script"
        command = {node_env}
        env = {{ EXTRA = "extra-{{query}}" }}
        [[node]]
        id = "node-env-copy"
        type = "copy"
        text = "extra={{query}}"

        [[node]]
        id = "id-env"
        type = "run_script"
        command = {id_env}
        [[node]]
        id = "id-env-copy"
        type = "copy"
        text = "id={{query}}"

        [[node]]
        id = "evil"
        type = "set_variable"
        name = "PATH"
        value = "/evil"
        [[node]]
        id = "path"
        type = "run_script"
        command = {path_env}
        [[node]]
        id = "path-copy"
        type = "copy"
        text = "path={{query}}"

        [[node]]
        id = "envelope"
        type = "run_script"
        command = {envelope}
        [[node]]
        id = "envelope-copy"
        type = "copy"
        text = "{{query}}/{{var:picked}}/{{var:n}}/{{var:site}}"

        [[connection]]
        from = "go"
        to = "echo"
        [[connection]]
        from = "echo"
        to = "echo-copy"
        [[connection]]
        from = "go"
        to = "env"
        [[connection]]
        from = "env"
        to = "env-copy"
        [[connection]]
        from = "go"
        to = "stdin"
        [[connection]]
        from = "stdin"
        to = "stdin-copy"
        [[connection]]
        from = "go"
        to = "node-env"
        [[connection]]
        from = "node-env"
        to = "node-env-copy"
        [[connection]]
        from = "go"
        to = "id-env"
        [[connection]]
        from = "id-env"
        to = "id-env-copy"
        [[connection]]
        from = "go"
        to = "evil"
        [[connection]]
        from = "evil"
        to = "path"
        [[connection]]
        from = "path"
        to = "path-copy"
        [[connection]]
        from = "go"
        to = "envelope"
        [[connection]]
        from = "envelope"
        to = "envelope-copy"
        "#,
        echo = program("wf-echo", &["{query}", "{var:GREETING}", "fixed"]),
        env = program("wf-env", &["GREETING"]),
        stdin = program("wf-stdin", &[]),
        node_env = program("wf-env", &["EXTRA"]),
        id_env = program("wf-env", &["SEVAK_WORKFLOW_ID"]),
        path_env = program("wf-env", &["PATH"]),
        envelope = program("wf-envelope", &[]),
    );
    let runtime = w.runtime("scripts", &toml);
    let report = run(&runtime, "go", "typed text");
    assert!(report.errors.is_empty(), "{report:?}");
    let mut clipboard = w.clipboard();
    // A variable named PATH never replaces the real one.
    let path = clipboard.remove(5);
    assert!(path.starts_with("path="), "{path}");
    assert_ne!(path, "path=/evil");
    assert_ne!(path, "path=<unset>");
    assert_eq!(
        clipboard,
        [
            // Arguments are separate, in order, with placeholders filled in.
            "echo=typed text|hello|fixed",
            // The workflow's variables are environment variables.
            "env=hello",
            "stdin=SHOUT TYPED TEXT",
            // A node's own environment is templated too.
            "extra=extra-typed text",
            "id=workflow:scripts",
            // Alfred's envelope sets the argument and variables, which flow on.
            "from-envelope/yes/2/example",
        ],
        "{clipboard:#?}"
    );
}

#[test]
fn arguments_with_spaces_and_quotes_arrive_as_one_argument() {
    let w = World::new();
    let nasty = "a \"quoted\" ; $(whoami) `id` 'x' && echo hi";
    let toml = format!(
        r#"
        name = "Quoting"
        [[node]]
        id = "go"
        type = "external"
        [[node]]
        id = "echo"
        type = "run_script"
        command = {echo}
        [[node]]
        id = "copy"
        type = "copy"
        text = "{{query}}"
        [[connection]]
        from = "go"
        to = "echo"
        [[connection]]
        from = "echo"
        to = "copy"
        "#,
        echo = program("wf-echo", &["{query}"]),
    );
    let runtime = w.runtime("quoting", &toml);
    let report = run(&runtime, "go", nasty);
    assert!(report.errors.is_empty(), "{report:?}");
    // No shell is involved: the text is one argument, untouched.
    assert_eq!(w.clipboard(), [nasty]);
}

#[test]
fn a_failing_or_slow_script_ends_its_branch_and_leaks_nothing() {
    let w = World::new();
    let toml = format!(
        r#"
        name = "Failures"
        [[node]]
        id = "go"
        type = "external"
        [[node]]
        id = "exit"
        type = "run_script"
        command = {exit}
        [[node]]
        id = "after-exit"
        type = "copy"
        text = "never 1"
        [[node]]
        id = "slow"
        type = "run_script"
        command = {slow}
        timeout_ms = 300
        [[node]]
        id = "after-slow"
        type = "copy"
        text = "never 2"
        [[node]]
        id = "missing"
        type = "run_script"
        command = ["sevak-no-such-program-anywhere"]
        [[node]]
        id = "ok"
        type = "copy"
        text = "the others still run"
        [[connection]]
        from = "go"
        to = "exit"
        [[connection]]
        from = "exit"
        to = "after-exit"
        [[connection]]
        from = "go"
        to = "slow"
        [[connection]]
        from = "slow"
        to = "after-slow"
        [[connection]]
        from = "go"
        to = "missing"
        [[connection]]
        from = "go"
        to = "ok"
        "#,
        exit = program("wf-exit", &["3"]),
        slow = program("wf-sleep", &["30000"]),
    );
    let runtime = w.runtime("failures", &toml);
    let started = Instant::now();
    let report = run(&runtime, "go", "");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the slow script was not stopped: {:?}",
        started.elapsed()
    );
    assert_eq!(w.clipboard(), ["the others still run"]);
    let failed: Vec<&str> = report.errors.iter().map(|e| e.node.as_str()).collect();
    assert_eq!(failed, ["exit", "slow", "missing"]);
    assert!(report.errors[0].message.contains("exited with code 3"));
    assert!(report.errors[1].message.contains("did not finish"));
    assert!(report.errors[2].message.contains("could not start"));
    // The program's own error output is not part of what is reported.
    assert!(report
        .errors
        .iter()
        .all(|e| !e.message.contains("private output")));
}

#[test]
fn a_script_runs_in_the_workflow_folder_and_may_keep_files_in_its_data_folder() {
    let w = World::new();
    let toml = format!(
        r#"
        name = "Record"
        [[node]]
        id = "go"
        type = "external"
        [[node]]
        id = "record"
        type = "run_script"
        command = {record}
        [[connection]]
        from = "go"
        to = "record"
        "#,
        record = program("wf-record", &["{query}"]),
    );
    let runtime = w.runtime("record", &toml);
    let report = run(&runtime, "go", "first");
    assert!(report.errors.is_empty(), "{report:?}");
    let note = std::fs::read_to_string(w.data("record").join("record.txt")).unwrap();
    assert_eq!(note, "first\n");
}

// ---- script filters and the engine ------------------------------------------

#[test]
fn a_script_filter_shows_rows_with_autocomplete_and_mods_and_picking_runs_the_chain() {
    let w = World::new();
    let toml = format!(
        r#"
        name = "Filter"
        [variables]
        site = "workflow"
        [[node]]
        id = "filter"
        type = "script_filter"
        keyword = "fl"
        command = {filter}
        [[node]]
        id = "copy"
        type = "copy"
        text = "{{query}}|{{var:from}}|{{var:site}}|{{var:mod}}"
        [[connection]]
        from = "filter"
        to = "copy"
        "#,
        filter = program("wf-filter", &[]),
    );
    w.add("filter", &toml);
    // It runs a program, so it waits for the user.
    assert!(w.host.plugins(&Config::default(), &w.platform()).is_empty());
    w.approve_all();

    let (engine, rx) = w.engine();
    let items = query_until_results(&engine, &rx, "fl abc");
    assert_eq!(items.len(), 2);
    let row = &items[0];
    assert_eq!(row.title, "one: abc");
    assert_eq!(row.plugin_id, "workflow:filter:filter");
    // The script sees the workflow's id in its environment.
    assert_eq!(row.subtitle, "workflow:filter");
    // `autocomplete` is what Tab inserts; the engine puts the keyword back.
    assert_eq!(row.autocomplete.as_deref(), Some("fl one abc"));
    // `mods`: alt is a secondary action on Alt+Enter; the invalid cmd one is gone.
    assert_eq!(row.secondary.len(), 1);
    assert_eq!(row.secondary[0].label, "The alt way");
    assert_eq!(row.secondary[0].modifier, Some(sevak_core::Modifier::Alt));
    assert!(matches!(row.action, Action::Custom { .. }));

    // Enter: the row's arg and variables start the nodes after the filter.
    engine.execute(row, "fl abc").unwrap();
    wait_until("the first pick", || w.clipboard().len() == 1);
    // The row's variable beats the workflow's; `mod` is not set.
    assert_eq!(w.clipboard(), ["arg-abc|filter|row|"]);

    // Alt+Enter: the mod's arg, and the modifier as a variable.
    engine.execute_secondary(row, 0, "fl abc").unwrap();
    wait_until("the alt pick", || w.clipboard().len() == 2);
    assert_eq!(w.clipboard()[1], "alt-arg|filter|row|alt");

    // A row that only informs cannot be run.
    assert!(engine.execute(&items[1], "fl abc").is_err());
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(w.clipboard().len(), 2);
}

// ---- approval ---------------------------------------------------------------

#[test]
fn nothing_runs_before_the_user_allows_it_and_a_changed_workflow_asks_again() {
    let w = World::new();
    let toml = |args: &str| {
        format!(
            r#"
            name = "Guarded"
            [[node]]
            id = "go"
            type = "external"
            [[node]]
            id = "hk"
            type = "hotkey"
            key = "Ctrl+Alt+G"
            [[node]]
            id = "record"
            type = "run_script"
            command = {record}
            [[connection]]
            from = "go"
            to = "record"
            [[connection]]
            from = "hk"
            to = "record"
            "#,
            record = program("wf-record", &[args]),
        )
    };
    w.add("guarded", &toml("one"));
    let config = Config::default();
    let record = w.data("guarded").join("record.txt");

    // Not allowed yet: no plugins, no hotkey binding, no external trigger.
    assert!(w.host.plugins(&config, &w.platform()).is_empty());
    assert!(w.host.hotkey_bindings(&config).is_empty());
    assert!(w.host.trigger("guarded", "go", "").is_err());
    std::thread::sleep(Duration::from_millis(100));
    assert!(!record.exists(), "the script ran without approval");
    let pending = w.host.pending(&config);
    assert_eq!(pending.len(), 1);
    let shown = pending[0].describe();
    assert!(shown.contains("runs "), "{shown}");
    assert!(shown.contains("sevak --trigger go"), "{shown}");

    // Declining keeps it off.
    w.host.decline(&pending[0]);
    assert!(w.host.pending(&config).is_empty());
    assert!(w.host.plugins(&config, &w.platform()).is_empty());

    // Allowing starts it.
    w.host.approve(&pending[0]).unwrap();
    let plugins = w.host.plugins(&config, &w.platform());
    assert_eq!(plugins.len(), 1);
    assert_eq!(w.host.hotkey_bindings(&config).len(), 1);
    w.host.trigger("guarded", "go", "").unwrap();
    wait_until("the script", || record.exists());
    assert_eq!(std::fs::read_to_string(&record).unwrap(), "one\n");

    // Editing what runs asks again, and the old plugin set no longer applies.
    w.add("guarded", &toml("two"));
    assert_eq!(w.host.pending(&config).len(), 1);
    assert!(w.host.plugins(&config, &w.platform()).is_empty());
    assert!(w.host.trigger("guarded", "go", "").is_err());
}

// ---- triggers through the engine ---------------------------------------------

#[test]
fn a_keyword_workflow_runs_from_the_engine_without_approval_when_it_runs_no_code() {
    let w = World::new();
    w.add(
        "search",
        r#"
        name = "Search"
        [[node]]
        id = "kw"
        type = "keyword"
        keyword = "wiki"
        argument = "required"
        title = "Search for {query}"
        [[node]]
        id = "open"
        type = "open_url"
        url = "https://example.com/?q={query}"
        [[connection]]
        from = "kw"
        to = "open"
        "#,
    );
    assert!(w.host.pending(&Config::default()).is_empty());
    let (engine, _rx) = w.engine();
    let items = engine.query("wiki rust & go");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].title, "Search for rust & go");
    engine.execute(&items[0], "wiki rust & go").unwrap();
    wait_until("the link", || !w.platform.urls.lock().unwrap().is_empty());
    assert_eq!(
        *w.platform.urls.lock().unwrap(),
        ["https://example.com/?q=rust%20%26%20go"]
    );

    // With the argument missing, the row is shown but does nothing.
    let bare = engine.query("wiki ");
    assert_eq!(bare[0].subtitle, "Type some text after the keyword");
    assert!(engine.execute(&bare[0], "wiki ").is_err());
}

#[test]
fn universal_actions_reach_workflows_and_leave_no_trace() {
    let w = World::new();
    w.add(
        "shout",
        r#"
        name = "Shout"
        description = "Upper-cases the selection"
        [[node]]
        id = "sel"
        type = "selection"
        title = "Shout it"
        accepts = ["text"]
        [[node]]
        id = "up"
        type = "transform"
        op = "upper"
        [[node]]
        id = "copy"
        type = "copy"
        [[connection]]
        from = "sel"
        to = "up"
        [[connection]]
        from = "up"
        to = "copy"
        "#,
    );
    let (engine, _rx) = w.engine();
    let selection = Selection::from_text("my secret words").unwrap();
    let rows = engine.selection_actions(&selection);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, "Shout it");
    assert!(!rows[0].id.contains("secret"));

    engine.execute(&rows[0], "").unwrap();
    wait_until("the copy", || !w.clipboard().is_empty());
    assert_eq!(w.clipboard(), ["MY SECRET WORDS"]);
    // Nothing about the selection reaches the usage statistics or history.
    assert!(engine.history().is_empty());
    assert_eq!(engine.usage_snapshot().len(), 0);

    // Files do not match a text trigger.
    let files = Selection::from_files(vec!["/tmp/a.txt".into()]).unwrap();
    assert!(engine.selection_actions(&files).is_empty());
}

#[test]
fn hotkey_bindings_resolve_to_a_runnable_result() {
    let w = World::new();
    w.add(
        "keys",
        r#"
        name = "Keys"
        [[node]]
        id = "hk"
        type = "hotkey"
        key = "Ctrl+Alt+K"
        [[node]]
        id = "copy"
        type = "copy"
        text = "pressed"
        [[connection]]
        from = "hk"
        to = "copy"
        "#,
    );
    let bindings = w.host.hotkey_bindings(&Config::default());
    assert_eq!(bindings.len(), 1);
    let id = bindings[0].run.clone().unwrap();
    let (engine, _rx) = w.engine();
    // What the hotkey code does: resolve the id and execute it.
    let item = engine.resolve(&id).expect("the engine resolves the id");
    engine.execute(&item, "").unwrap();
    wait_until("the copy", || !w.clipboard().is_empty());
    assert_eq!(w.clipboard(), ["pressed"]);
}

#[test]
fn failures_are_reported_through_the_sink() {
    let w = World::new();
    let toml = format!(
        r#"
        name = "Reports"
        [[node]]
        id = "go"
        type = "external"
        [[node]]
        id = "boom"
        title = "The script"
        type = "run_script"
        command = {exit}
        [[connection]]
        from = "go"
        to = "boom"
        "#,
        exit = program("wf-exit", &["7"]),
    );
    let runtime = w.runtime("reports", &toml);
    runtime.start("go", Ctx::default()).unwrap();
    wait_until("the notification", || !w.notes.0.lock().unwrap().is_empty());
    let notes = w.notes.0.lock().unwrap().clone();
    assert_eq!(
        notes,
        ["\"The script\" failed: the program exited with code 7"]
    );
}

#[test]
fn a_sink_can_be_ignored() {
    // Workflows built without a shell still run; their outputs just go nowhere.
    let w = World::new();
    let workflow = Workflow::from_toml(
        r#"
        name = "Quiet"
        [[node]]
        id = "go"
        type = "external"
        [[node]]
        id = "n"
        type = "notification"
        body = "hi"
        [[connection]]
        from = "go"
        to = "n"
        "#,
    )
    .unwrap();
    let runtime = Runtime::new(
        "quiet",
        w.dir("quiet"),
        w.data("quiet"),
        workflow,
        w.platform(),
        Arc::new(NoSink),
        &Config::default(),
    );
    let report = run(&runtime, "go", "");
    assert_eq!(report.executed, ["n"]);
    assert!(report.errors.is_empty());
}
