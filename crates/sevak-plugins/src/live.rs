//! A cache for answers that cost an OS round trip (the process list, the track
//! that is playing), kept off the typing path.
//!
//! [`Plugin::query`](sevak_core::Plugin::query) must never block, so it only
//! reads the last answer ([`Cache::get`]). When that answer is older than its
//! time to live, the query also starts one background refresh
//! ([`Cache::refresh_if_stale`]); when the refresh finishes with a different
//! answer it tells the shell through the plugin's notifier, which re-runs the
//! current query against the fresh cache. Stale answers are therefore shown
//! for a moment instead of making the user wait.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sevak_core::ResultsNotifier;

struct State<T> {
    /// When the value was last loaded; `None` before the first load.
    loaded: Option<Instant>,
    value: Arc<T>,
}

pub(crate) struct Cache<T> {
    state: Mutex<State<T>>,
    /// A refresh is running; only one at a time.
    busy: AtomicBool,
}

impl<T: Default + PartialEq + Send + Sync + 'static> Cache<T> {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                loaded: None,
                value: Arc::new(T::default()),
            }),
            busy: AtomicBool::new(false),
        })
    }

    /// The last answer (the default value before the first load).
    pub(crate) fn get(&self) -> Arc<T> {
        self.lock().value.clone()
    }

    /// Whether the first load has finished.
    #[cfg(test)]
    pub(crate) fn is_loaded(&self) -> bool {
        self.lock().loaded.is_some()
    }

    /// Stores `value` as if it had just been loaded. Returns whether it differs
    /// from the previous answer (or is the first one).
    pub(crate) fn store(&self, value: T) -> bool {
        let mut state = self.lock();
        let changed = state.loaded.is_none() || *state.value != value;
        state.loaded = Some(Instant::now());
        if changed {
            state.value = Arc::new(value);
        }
        changed
    }

    /// Starts `load` on a background thread unless the answer is younger than
    /// `ttl` or a refresh is already running. When it finishes with a new
    /// answer, `notify` runs (with the cache's lock released).
    pub(crate) fn refresh_if_stale(
        self: &Arc<Self>,
        ttl: Duration,
        load: impl FnOnce() -> T + Send + 'static,
        notify: Option<ResultsNotifier>,
        plugin_id: &'static str,
    ) {
        let fresh = self
            .lock()
            .loaded
            .is_some_and(|loaded| loaded.elapsed() < ttl);
        if fresh || self.busy.swap(true, Ordering::AcqRel) {
            return;
        }
        let cache = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name(format!("sevak-{plugin_id}-live"))
            .spawn(move || {
                // A panicking loader must not leave the cache "busy" forever.
                let loaded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(load));
                let changed = loaded.map(|value| cache.store(value));
                cache.busy.store(false, Ordering::Release);
                if let (Ok(true), Some(notify)) = (changed, notify) {
                    notify(plugin_id);
                }
            });
        if let Err(err) = spawned {
            tracing::debug!(plugin = plugin_id, %err, "could not start a refresh thread");
            self.busy.store(false, Ordering::Release);
        }
    }

    /// Waits for a running refresh to finish.
    #[cfg(test)]
    fn wait_idle(&self) {
        for _ in 0..1000 {
            if !self.busy.load(Ordering::Acquire) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the refresh did not finish");
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State<T>> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    fn notifier() -> (ResultsNotifier, mpsc::Receiver<String>) {
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let notify: ResultsNotifier = Arc::new(move |id: &str| {
            let _ = tx.lock().unwrap().send(id.to_owned());
        });
        (notify, rx)
    }

    const WAIT: Duration = Duration::from_secs(5);

    #[test]
    fn starts_empty_and_unloaded() {
        let cache = Cache::<Vec<u32>>::new();
        assert!(cache.get().is_empty());
        assert!(!cache.is_loaded());
    }

    #[test]
    fn a_refresh_loads_off_thread_and_notifies_once_per_change() {
        let cache = Cache::<Vec<u32>>::new();
        let (notify, rx) = notifier();
        let caller = std::thread::current().id();
        cache.refresh_if_stale(
            Duration::ZERO,
            move || {
                assert_ne!(std::thread::current().id(), caller);
                vec![1, 2]
            },
            Some(notify.clone()),
            "tasks",
        );
        assert_eq!(rx.recv_timeout(WAIT).unwrap(), "tasks");
        assert_eq!(*cache.get(), vec![1, 2]);
        assert!(cache.is_loaded());

        // Same answer again: stored, but nobody is told.
        cache.refresh_if_stale(Duration::ZERO, || vec![1, 2], Some(notify.clone()), "tasks");
        cache.wait_idle();
        cache.refresh_if_stale(Duration::ZERO, || vec![3], Some(notify), "tasks");
        assert_eq!(rx.recv_timeout(WAIT).unwrap(), "tasks");
        assert_eq!(*cache.get(), vec![3]);
    }

    #[test]
    fn a_fresh_answer_is_not_loaded_again() {
        let cache = Cache::<u32>::new();
        assert!(cache.store(7));
        let (notify, rx) = notifier();
        cache.refresh_if_stale(
            Duration::from_secs(60),
            || panic!("the answer is still fresh"),
            Some(notify),
            "tasks",
        );
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
        assert_eq!(*cache.get(), 7);
        assert!(!cache.store(7));
        assert!(cache.store(8));
    }

    #[test]
    fn only_one_refresh_runs_at_a_time() {
        let cache = Cache::<u32>::new();
        let (release, gate) = mpsc::channel::<()>();
        let (started, running) = mpsc::channel::<()>();
        cache.refresh_if_stale(
            Duration::ZERO,
            move || {
                started.send(()).unwrap();
                gate.recv_timeout(WAIT).unwrap();
                1
            },
            None,
            "tasks",
        );
        running.recv_timeout(WAIT).unwrap();
        // A second request while the first is in flight is dropped.
        cache.refresh_if_stale(Duration::ZERO, || panic!("second refresh"), None, "tasks");
        release.send(()).unwrap();
        for _ in 0..500 {
            if cache.is_loaded() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(*cache.get(), 1);
    }

    #[test]
    fn a_panicking_loader_does_not_wedge_the_cache() {
        let cache = Cache::<u32>::new();
        cache.refresh_if_stale(Duration::ZERO, || panic!("loader failed"), None, "tasks");
        // The busy flag clears; a later refresh works.
        let (notify, rx) = notifier();
        for _ in 0..500 {
            cache.refresh_if_stale(Duration::ZERO, || 5, Some(notify.clone()), "tasks");
            if rx.recv_timeout(Duration::from_millis(10)).is_ok() {
                break;
            }
        }
        assert_eq!(*cache.get(), 5);
    }
}
