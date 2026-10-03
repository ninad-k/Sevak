//! The search engine's lifecycle: construction, background indexing, hot
//! reload and usage persistence.
//!
//! Nothing here runs on the main thread except cheap state access; indexing,
//! saving and reloading happen on dedicated threads.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use sevak_core::{Config, EngineOptions, Plugin, ResultItem, SearchEngine, UsageError, UsageStore};
use sevak_platform::{native_provider, AppPaths, PlatformProvider};
use sevak_plugins::builtin_plugins;
use tauri::{AppHandle, Emitter, Manager};

use crate::icons::IconStore;
use crate::state::{lock, AppState};
use crate::window;

pub const EVENT_INDEX: &str = "sevak:index";

/// Indexes are rebuilt this often so new apps and files show up.
const REFRESH_INTERVAL: Duration = Duration::from_secs(10 * 60);
/// Usage is written this long after the last launch.
const SAVE_DEBOUNCE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Serialize)]
struct IndexEvent {
    state: &'static str,
}

fn engine_options(config: &Config) -> EngineOptions {
    let keyword = &config.search.fallback_web_search;
    EngineOptions {
        max_results: config.search.max_results,
        fallback_plugins: if keyword.is_empty() {
            Vec::new()
        } else {
            vec![format!("web:{keyword}")]
        },
    }
}

pub fn build_engine(
    config: &Config,
    platform: Arc<dyn PlatformProvider>,
    usage: UsageStore,
) -> SearchEngine {
    SearchEngine::new(
        builtin_plugins(config, platform),
        usage,
        engine_options(config),
    )
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

/// Results of the most recent search, keyed by id, so `execute` needs only an id.
#[derive(Default)]
struct Latest {
    /// Ticket of the search that produced `items`.
    ticket: u64,
    items: HashMap<String, ResultItem>,
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
        match usage.save(&self.path) {
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
    engine: RwLock<Arc<SearchEngine>>,
    latest: Mutex<Latest>,
    next_ticket: AtomicU64,
    pub icons: IconStore,
    pub saver: UsageSaver,
    /// Indexing runs in flight that the UI should hear about.
    indexing: AtomicUsize,
    /// Bumped by every reload so a superseded one does not swap in.
    reload_generation: AtomicU64,
}

impl Search {
    pub fn new(paths: &AppPaths, config: &Config) -> Self {
        let platform: Arc<dyn PlatformProvider> = Arc::from(native_provider());
        let usage = load_usage(&paths.usage_file);
        let engine = build_engine(config, platform.clone(), usage);
        Self {
            icons: IconStore::new(platform.clone()),
            platform,
            engine: RwLock::new(Arc::new(engine)),
            latest: Mutex::new(Latest::default()),
            next_ticket: AtomicU64::new(1),
            saver: UsageSaver::new(paths.usage_file.clone()),
            indexing: AtomicUsize::new(0),
            reload_generation: AtomicU64::new(0),
        }
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

    /// Remembers `items` for `execute`, unless a newer search already did
    /// (searches run concurrently and may finish out of order).
    pub fn store_results(&self, ticket: u64, items: Vec<ResultItem>) {
        let mut latest = lock(&self.latest);
        if ticket < latest.ticket {
            return;
        }
        latest.ticket = ticket;
        latest.items = items
            .into_iter()
            .map(|item| (item.id.clone(), item))
            .collect();
    }

    pub fn result(&self, id: &str) -> Option<ResultItem> {
        lock(&self.latest).items.get(id).cloned()
    }

    /// Swaps in an engine over `plugins`, carrying over the usage recorded so
    /// far. Returns false when a newer reload superseded this one.
    fn install(
        &self,
        generation: u64,
        plugins: Vec<Arc<dyn Plugin>>,
        options: EngineOptions,
    ) -> bool {
        let mut current = self
            .engine
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if self.reload_generation.load(Ordering::SeqCst) != generation {
            return false;
        }
        let usage = current.usage_snapshot();
        *current = Arc::new(SearchEngine::new(plugins, usage, options));
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

fn refresh(engine: &SearchEngine, what: &str) {
    let started = Instant::now();
    let failures = engine.refresh_all();
    tracing::info!(
        elapsed_ms = started.elapsed().as_millis() as u64,
        failed_plugins = failures.len(),
        "{what} finished"
    );
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
    let guard = IndexGuard::new(app);
    let indexer = app.clone();
    spawn_thread("sevak-index", move || {
        {
            let _guard = guard;
            let engine = indexer.state::<AppState>().search.engine();
            refresh(&engine, "initial indexing");
        }
        loop {
            std::thread::sleep(REFRESH_INTERVAL);
            // The engine may have been replaced by a reload meanwhile. These
            // periodic runs are quiet: the old index stays usable throughout.
            let engine = indexer.state::<AppState>().search.engine();
            refresh(&engine, "periodic indexing");
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

/// Rebuilds the engine from `config` on a background thread. The new index is
/// built first and swapped in afterwards, so search never goes blank.
pub fn reload(app: &AppHandle, config: &Config) {
    let state = app.state::<AppState>();
    let search = &state.search;
    let generation = search.reload_generation.fetch_add(1, Ordering::SeqCst) + 1;
    let plugins = builtin_plugins(config, search.platform.clone());
    let options = engine_options(config);

    let guard = IndexGuard::new(app);
    let app = app.clone();
    spawn_thread("sevak-reload", move || {
        let _guard = guard;
        // Usage is irrelevant while warming up; the real snapshot is taken at
        // swap time so launches during indexing are not lost.
        let warm_up = SearchEngine::new(plugins.clone(), UsageStore::default(), options.clone());
        refresh(&warm_up, "reload indexing");

        if app
            .state::<AppState>()
            .search
            .install(generation, plugins, options)
        {
            tracing::info!("search engine reloaded");
        } else {
            tracing::info!("reload superseded by a newer one; discarding");
        }
    });
}
