//! The search engine's lifecycle: construction, background indexing, hot
//! reload and usage persistence.
//!
//! Nothing here runs on the main thread except cheap state access; indexing,
//! saving and reloading happen on dedicated threads.

use std::collections::{HashMap, VecDeque};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use sevak_core::{
    Config, EngineOptions, Plugin, ResultItem, ResultsNotifier, SearchEngine, UsageError,
    UsageStore,
};
use sevak_platform::{native_provider, AppPaths, PlatformProvider};
use sevak_plugins::{builtin_plugins, KeywordOwners, ScriptPluginHost, WorkflowHost};
use tauri::{AppHandle, Emitter, Manager};

use crate::icons::IconStore;
use crate::state::{lock, AppState};
use crate::workflows::TauriSink;
use crate::{script_plugins, window, workflows};

pub const EVENT_INDEX: &str = "sevak:index";
/// A script plugin's late answer is ready: the UI runs its current query again.
pub const EVENT_RESULTS: &str = "sevak:results";

/// Indexes are rebuilt this often so new apps and files show up.
const REFRESH_INTERVAL: Duration = Duration::from_secs(10 * 60);
/// Usage is written this long after the last launch.
const SAVE_DEBOUNCE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Serialize)]
struct IndexEvent {
    state: &'static str,
}

fn engine_options(config: &Config) -> EngineOptions {
    EngineOptions {
        max_results: config.search.max_results,
        fallback_plugins: config
            .search
            .fallback_web_search
            .keywords()
            .iter()
            .map(|keyword| format!("web:{keyword}"))
            .collect(),
        query_history: config.search.query_history,
    }
}

/// The built-in plugins plus the approved script plugins from the config
/// directory's `plugins` folder and the workflows from its `workflows` folder.
fn all_plugins(
    config: &Config,
    platform: &Arc<dyn PlatformProvider>,
    scripts: &ScriptPluginHost,
    workflows: &WorkflowHost,
) -> Vec<Arc<dyn Plugin>> {
    // Everything that opens a path checks this (see `sevak_platform::netpath`).
    sevak_platform::netpath::set_allow_network_paths(config.files.allow_network_paths);
    let mut plugins = builtin_plugins(config, platform.clone());
    plugins.extend(scripts.plugins(config, platform));
    plugins.extend(workflows.plugins(config, platform));
    // A keyword two plugins answer is not an error, but say so once.
    KeywordOwners::collect(config, scripts, workflows).log_shared();
    plugins
}

pub fn build_engine(
    config: &Config,
    platform: Arc<dyn PlatformProvider>,
    usage: UsageStore,
    scripts: &ScriptPluginHost,
    workflows: &WorkflowHost,
) -> SearchEngine {
    SearchEngine::new(
        all_plugins(config, &platform, scripts, workflows),
        usage,
        engine_options(config),
    )
}

/// Tells the UI that a slow plugin has results for the query on screen.
fn results_notifier(app: &AppHandle) -> ResultsNotifier {
    let app = app.clone();
    Arc::new(move |plugin_id: &str| {
        if let Err(err) = app.emit_to(window::MAIN_LABEL, EVENT_RESULTS, plugin_id.to_owned()) {
            tracing::warn!("could not emit {EVENT_RESULTS}: {err}");
        }
    })
}

/// Loads the usage file. A corrupt file is moved aside (not overwritten by the
/// next save) and an empty store is used.
fn load_usage(path: &Path) -> UsageStore {
    match UsageStore::load(path) {
        Ok(usage) => {
            tracing::info!(entries = usage.len(), "usage statistics loaded");
            usage
        }
        Err(err @ UsageError::Parse { .. }) => {
            let mut aside: OsString = path.as_os_str().to_owned();
            aside.push(".corrupt");
            let aside = PathBuf::from(aside);
            match std::fs::rename(path, &aside) {
                Ok(()) => tracing::warn!(
                    "{err}; moved it to {} and starting with empty usage statistics",
                    aside.display()
                ),
                Err(rename_err) => tracing::warn!(
                    "{err}; could not move it aside ({rename_err}); \
                     starting with empty usage statistics"
                ),
            }
            UsageStore::default()
        }
        Err(err) => {
            tracing::warn!("{err}; starting with empty usage statistics");
            UsageStore::default()
        }
    }
}

/// How many recent result sets `execute` can still act on. The UI shows the
/// newest set it has *received*, which can trail the newest one computed while
/// the user types fast, so a few older sets are kept.
const KEPT_RESULT_SETS: usize = 8;

/// One search's results, keyed by id, plus the query that produced them.
struct ResultSet {
    ticket: u64,
    query: String,
    items: HashMap<String, ResultItem>,
}

/// The most recent result sets, oldest first (ascending tickets).
#[derive(Default)]
struct Latest {
    sets: VecDeque<ResultSet>,
}

impl Latest {
    /// Adds a set; searches run concurrently and may finish out of order.
    fn store(&mut self, ticket: u64, query: String, items: Vec<ResultItem>) {
        let set = ResultSet {
            ticket,
            query,
            items: items
                .into_iter()
                .map(|item| (item.id.clone(), item))
                .collect(),
        };
        let at = self.sets.partition_point(|s| s.ticket < ticket);
        self.sets.insert(at, set);
        while self.sets.len() > KEPT_RESULT_SETS {
            self.sets.pop_front();
        }
    }

    fn get(&self, ticket: u64, id: &str) -> Option<(ResultItem, String)> {
        let set = self.sets.iter().find(|set| set.ticket == ticket)?;
        Some((set.items.get(id)?.clone(), set.query.clone()))
    }

    /// Drops the set of `ticket`, if it is still kept.
    fn forget(&mut self, ticket: u64) {
        self.sets.retain(|set| set.ticket != ticket);
    }
}

/// Debounced, serialized persistence of the usage statistics.
pub struct UsageSaver {
    path: PathBuf,
    tx: Sender<()>,
    rx: Mutex<Option<Receiver<()>>>,
    /// Statistics changed since the last successful save.
    dirty: AtomicBool,
    /// Held for the whole of a save: never two writers at once.
    write: Mutex<()>,
}

impl UsageSaver {
    fn new(path: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            path,
            tx,
            rx: Mutex::new(Some(rx)),
            dirty: AtomicBool::new(false),
            write: Mutex::new(()),
        }
    }

    /// Marks the statistics as changed; they are saved after a quiet period.
    pub fn poke(&self) {
        self.dirty.store(true, Ordering::SeqCst);
        // The receiver lives as long as the app; a failed send only means the
        // saver thread was never started (quit still saves synchronously).
        let _ = self.tx.send(());
    }

    /// Saves now if anything changed. Safe to call from any thread.
    pub fn save_now(&self, snapshot: impl FnOnce() -> UsageStore) {
        let _writing = lock(&self.write);
        if !self.dirty.swap(false, Ordering::SeqCst) {
            return;
        }
        let usage = snapshot();
        let started = Instant::now();
        // The file holds the last typed queries: owner-only on Unix.
        match usage.save_with(&self.path, sevak_platform::private_file::write_atomic) {
            Ok(()) => tracing::debug!(
                entries = usage.len(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "usage statistics saved"
            ),
            Err(err) => {
                self.dirty.store(true, Ordering::SeqCst);
                tracing::warn!("could not save usage statistics: {err}");
            }
        }
    }
}

pub struct Search {
    pub platform: Arc<dyn PlatformProvider>,
    /// Script plugins found in `<config dir>/plugins`.
    pub scripts: Arc<ScriptPluginHost>,
    /// Workflows found in `<config dir>/workflows`.
    pub workflows: Arc<WorkflowHost>,
    /// Where workflow output nodes show their results; gets its window handle
    /// in [`start`].
    pub sink: Arc<TauriSink>,
    engine: RwLock<Arc<SearchEngine>>,
    latest: Mutex<Latest>,
    next_ticket: AtomicU64,
    /// The ticket of the Universal Actions results on screen; 0 for none.
    selection_ticket: AtomicU64,
    pub icons: IconStore,
    pub saver: UsageSaver,
    /// Indexing runs in flight that the UI should hear about.
    indexing: AtomicUsize,
    /// Bumped by every reload so a superseded one does not swap in.
    reload_generation: AtomicU64,
    /// The plugins whose last refresh failed, with the error (diagnostics).
    refresh_failures: Mutex<Vec<(String, String)>>,
}

impl Search {
    pub fn new(paths: &AppPaths, config: &Config) -> Self {
        let platform: Arc<dyn PlatformProvider> = Arc::from(native_provider());
        let usage = load_usage(&paths.usage_file);
        let scripts = Arc::new(ScriptPluginHost::new(
            paths.config_dir.join("plugins"),
            paths.data_dir.join("plugins"),
            paths.data_dir.join("script-plugin-approvals.json"),
        ));
        let sink = Arc::new(TauriSink::default());
        let workflows = Arc::new(WorkflowHost::new(
            paths.config_dir.join("workflows"),
            paths.data_dir.join("workflows"),
            // The same file as the script plugins': one list of what is allowed.
            paths.data_dir.join("script-plugin-approvals.json"),
            sink.clone(),
        ));
        let engine = build_engine(config, platform.clone(), usage, &scripts, &workflows);
        Self {
            icons: IconStore::new(platform.clone()),
            platform,
            scripts,
            workflows,
            sink,
            engine: RwLock::new(Arc::new(engine)),
            latest: Mutex::new(Latest::default()),
            next_ticket: AtomicU64::new(1),
            selection_ticket: AtomicU64::new(0),
            saver: UsageSaver::new(paths.usage_file.clone()),
            indexing: AtomicUsize::new(0),
            reload_generation: AtomicU64::new(0),
            refresh_failures: Mutex::new(Vec::new()),
        }
    }

    /// `(plugin id, error)` of the plugins whose last refresh failed.
    pub fn refresh_failures(&self) -> Vec<(String, String)> {
        lock(&self.refresh_failures).clone()
    }

    fn record_failures(&self, failures: Vec<(String, String)>) {
        *lock(&self.refresh_failures) = failures;
    }

    pub fn engine(&self) -> Arc<SearchEngine> {
        self.engine
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn is_indexing(&self) -> bool {
        self.indexing.load(Ordering::SeqCst) > 0
    }

    /// Numbers a search in the order it started.
    pub fn next_ticket(&self) -> u64 {
        self.next_ticket.fetch_add(1, Ordering::SeqCst)
    }

    /// Remembers the results of search `ticket` for `execute`.
    pub fn store_results(&self, ticket: u64, query: String, items: Vec<ResultItem>) {
        lock(&self.latest).store(ticket, query, items);
    }

    /// Remembers the Universal Actions for a selection under `ticket`. They
    /// hold the selected text, so they are dropped as soon as the launcher hides
    /// ([`Search::forget_selection`]) and replace any earlier ones.
    pub fn store_selection(&self, ticket: u64, items: Vec<ResultItem>) {
        self.forget_selection();
        self.selection_ticket.store(ticket, Ordering::SeqCst);
        self.store_results(ticket, String::new(), items);
    }

    /// Drops the stored Universal Actions, if any.
    pub fn forget_selection(&self) {
        let ticket = self.selection_ticket.swap(0, Ordering::SeqCst);
        if ticket != 0 {
            lock(&self.latest).forget(ticket);
        }
    }

    /// The result `id` of search `ticket` (the set the UI is showing) and the
    /// query that produced it, while that set is among the recent ones.
    pub fn result(&self, ticket: u64, id: &str) -> Option<(ResultItem, String)> {
        lock(&self.latest).get(ticket, id)
    }

    /// Swaps in an engine over `plugins` that shares the current engine's usage
    /// statistics. Returns false when a newer reload superseded this one.
    fn install(
        &self,
        generation: u64,
        plugins: Vec<Arc<dyn Plugin>>,
        options: EngineOptions,
    ) -> bool {
        let old = {
            let mut current = self
                .engine
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if self.reload_generation.load(Ordering::SeqCst) != generation {
                return false;
            }
            let rebuilt = Arc::new(current.rebuild(plugins, options));
            std::mem::replace(&mut *current, rebuilt)
        };
        // The replaced plugins' background work (script plugin processes) stops
        // now rather than whenever the last in-flight query lets go of them.
        old.shutdown();
        true
    }
}

/// Tells the UI indexing started, and that it ended when dropped.
struct IndexGuard {
    app: AppHandle,
}

impl IndexGuard {
    fn new(app: &AppHandle) -> Self {
        if let Some(state) = app.try_state::<AppState>() {
            if state.search.indexing.fetch_add(1, Ordering::SeqCst) == 0 {
                emit_index(app, "indexing");
            }
        }
        Self { app: app.clone() }
    }
}

impl Drop for IndexGuard {
    fn drop(&mut self) {
        if let Some(state) = self.app.try_state::<AppState>() {
            if state.search.indexing.fetch_sub(1, Ordering::SeqCst) == 1 {
                emit_index(&self.app, "ready");
            }
        }
    }
}

fn emit_index(app: &AppHandle, state: &'static str) {
    if let Err(err) = app.emit_to(window::MAIN_LABEL, EVENT_INDEX, IndexEvent { state }) {
        tracing::warn!("could not emit {EVENT_INDEX}: {err}");
    }
}

/// Refreshes every plugin; returns `(plugin id, error)` for those that failed.
fn refresh(engine: &SearchEngine, what: &str) -> Vec<(String, String)> {
    let started = Instant::now();
    let failures = engine.refresh_all();
    tracing::info!(
        elapsed_ms = started.elapsed().as_millis() as u64,
        failed_plugins = failures.len(),
        "{what} finished"
    );
    failures
        .into_iter()
        .map(|(id, err)| (id, err.to_string()))
        .collect()
}

fn spawn_thread(name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(err) = std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(work)
    {
        tracing::error!("could not start the {name} thread: {err}");
    }
}

/// Starts the indexing and usage-saver threads. Called once, from `setup`.
pub fn start(app: &AppHandle) {
    let state = app.state::<AppState>();
    state
        .search
        .engine()
        .attach_notifier(&results_notifier(app));
    state.search.sink.attach(app);
    script_plugins::review_new(app);
    workflows::review_new(app);

    let guard = IndexGuard::new(app);
    let indexer = app.clone();
    spawn_thread("sevak-index", move || {
        {
            let _guard = guard;
            let search = &indexer.state::<AppState>().search;
            let failures = refresh(&search.engine(), "initial indexing");
            search.record_failures(failures);
        }
        loop {
            std::thread::sleep(REFRESH_INTERVAL);
            // The engine may have been replaced by a reload meanwhile. These
            // periodic runs are quiet: the old index stays usable throughout.
            let search = &indexer.state::<AppState>().search;
            let failures = refresh(&search.engine(), "periodic indexing");
            search.record_failures(failures);
        }
    });

    let saver_app = app.clone();
    let rx = lock(&app.state::<AppState>().search.saver.rx).take();
    if let Some(rx) = rx {
        spawn_thread("sevak-usage-saver", move || run_saver(&saver_app, &rx));
    }
}

fn run_saver(app: &AppHandle, rx: &Receiver<()>) {
    while rx.recv().is_ok() {
        // Wait for a quiet period; every launch restarts it.
        loop {
            match rx.recv_timeout(SAVE_DEBOUNCE) {
                Ok(()) => {}
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        save_usage(app);
    }
}

/// Writes the usage statistics if they changed (also used when quitting).
pub fn save_usage(app: &AppHandle) {
    let state = app.state::<AppState>();
    state
        .search
        .saver
        .save_now(|| state.search.engine().usage_snapshot());
}

/// Stops the plugins' background work (script plugin processes) when quitting.
pub fn shutdown(app: &AppHandle) {
    app.state::<AppState>().search.engine().shutdown();
}

/// Rebuilds the engine from `config` on a background thread. The new index is
/// built first and swapped in afterwards, so search never goes blank.
pub fn reload(app: &AppHandle, config: &Config) {
    let state = app.state::<AppState>();
    let search = &state.search;
    let generation = search.reload_generation.fetch_add(1, Ordering::SeqCst) + 1;
    let plugins = all_plugins(config, &search.platform, &search.scripts, &search.workflows);
    let notifier = results_notifier(app);
    for plugin in &plugins {
        plugin.attach_notifier(Arc::clone(&notifier));
    }
    let options = engine_options(config);

    script_plugins::review_new(app);
    workflows::review_new(app);

    let guard = IndexGuard::new(app);
    let app = app.clone();
    spawn_thread("sevak-reload", move || {
        let _guard = guard;
        // Usage is irrelevant while warming up; the real snapshot is taken at
        // swap time so launches during indexing are not lost.
        let warm_up = SearchEngine::new(plugins.clone(), UsageStore::default(), options.clone());
        let failures = refresh(&warm_up, "reload indexing");

        if app
            .state::<AppState>()
            .search
            .install(generation, plugins, options)
        {
            app.state::<AppState>().search.record_failures(failures);
            tracing::info!("search engine reloaded");
        } else {
            tracing::info!("reload superseded by a newer one; discarding");
        }
    });
}

#[cfg(test)]
mod tests {
    use sevak_core::Action;

    use super::*;

    fn item(key: &str, text: &str) -> ResultItem {
        ResultItem::new(
            "calc",
            key,
            text,
            Action::CopyText {
                text: text.to_owned(),
            },
        )
    }

    #[test]
    fn executes_the_set_the_ui_shows_not_just_the_newest() {
        let mut latest = Latest::default();
        latest.store(1, "2+2".into(), vec![item("r", "4")]);
        // The next search finished, but the UI still shows ticket 1.
        latest.store(2, "2+23".into(), vec![item("r", "25")]);

        let (shown, query) = latest.get(1, "calc:r").unwrap();
        assert_eq!(shown.title, "4");
        assert_eq!(query, "2+2");
        assert_eq!(latest.get(2, "calc:r").unwrap().0.title, "25");
        assert!(latest.get(2, "calc:missing").is_none());
    }

    #[test]
    fn a_forgotten_set_cannot_be_executed_any_more() {
        let mut latest = Latest::default();
        latest.store(1, "a".into(), vec![item("r", "A")]);
        latest.store(2, String::new(), vec![item("r", "private")]);
        latest.forget(2);
        assert!(latest.get(2, "calc:r").is_none());
        assert!(latest.get(1, "calc:r").is_some());
        // Forgetting what is not there is fine.
        latest.forget(9);
        assert_eq!(latest.sets.len(), 1);
    }

    #[test]
    fn keeps_only_the_newest_sets_even_when_stored_out_of_order() {
        let mut latest = Latest::default();
        let total = KEPT_RESULT_SETS as u64 + 2;
        for ticket in (1..=total).rev() {
            latest.store(ticket, ticket.to_string(), vec![item("r", "x")]);
        }
        assert_eq!(latest.sets.len(), KEPT_RESULT_SETS);
        assert!(latest.get(1, "calc:r").is_none());
        assert!(latest.get(2, "calc:r").is_none());
        assert!(latest.get(3, "calc:r").is_some());
        assert!(latest.get(total, "calc:r").is_some());
        let tickets: Vec<_> = latest.sets.iter().map(|s| s.ticket).collect();
        assert!(tickets.windows(2).all(|w| w[0] < w[1]));
    }
}
