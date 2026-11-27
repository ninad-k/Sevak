//! Script Filter `cache` and `rerun`: the bookkeeping, apart from any process.
//!
//! Time comes in as a [`Duration`] since some fixed start (a [`Clock`]), so
//! the tests move a fake clock instead of sleeping.
//!
//! * [`FilterCache`] keeps the rows of recent answers by query (at most
//!   [`MAX_ENTRIES`], least recently used out first). An answer that says
//!   `"cache": {"seconds": 30}` is *fresh* for 30 seconds: the script is not
//!   run again for that query. With `"loosereload": true` an expired answer is
//!   *stale*: shown at once while the script runs again.
//! * [`Rerun`] remembers that the script asked to run again after an interval
//!   for the query on screen. Whatever changes the query, or hides the
//!   launcher, disarms it.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use sevak_core::ResultItem;

use super::alfred::CacheSpec;

/// Cached queries kept per script filter.
pub const MAX_ENTRIES: usize = 64;

/// A monotonic clock, as the time since an arbitrary start.
pub trait Clock: Send + Sync {
    fn now(&self) -> Duration;
}

/// The real clock.
pub struct SystemClock {
    start: Instant,
}

impl Default for SystemClock {
    fn default() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.start.elapsed()
    }
}

/// What the cache knows about a query.
#[derive(Debug, PartialEq)]
pub enum Lookup {
    /// Nothing usable: run the script.
    Miss,
    /// A cached answer that has not expired: use it, run nothing.
    Fresh(Vec<ResultItem>),
    /// An expired answer of a `loosereload` filter: show it and run the script.
    Stale(Vec<ResultItem>),
}

#[derive(Debug)]
struct Entry {
    query: String,
    items: Vec<ResultItem>,
    stored: Duration,
    spec: CacheSpec,
}

/// Recent answers by query, least recently used first.
#[derive(Debug, Default)]
pub struct FilterCache {
    entries: VecDeque<Entry>,
}

impl FilterCache {
    /// The answer for `query` at time `now`. A hit counts as a use.
    pub fn get(&mut self, query: &str, now: Duration) -> Lookup {
        let Some(position) = self.entries.iter().position(|entry| entry.query == query) else {
            return Lookup::Miss;
        };
        let Some(entry) = self.entries.remove(position) else {
            return Lookup::Miss;
        };
        let age = now.saturating_sub(entry.stored);
        let fresh = age < Duration::from_secs(entry.spec.seconds);
        let lookup = if fresh {
            Lookup::Fresh(entry.items.clone())
        } else if entry.spec.loose_reload {
            Lookup::Stale(entry.items.clone())
        } else {
            // Expired for good.
            return Lookup::Miss;
        };
        self.entries.push_back(entry);
        lookup
    }

    /// Remembers the answer for `query`.
    pub fn put(&mut self, query: &str, items: Vec<ResultItem>, spec: CacheSpec, now: Duration) {
        self.remove(query);
        self.entries.push_back(Entry {
            query: query.to_owned(),
            items,
            stored: now,
            spec,
        });
        while self.entries.len() > MAX_ENTRIES {
            self.entries.pop_front();
        }
    }

    /// Forgets `query` (its newest answer asked for no caching).
    pub fn remove(&mut self, query: &str) {
        self.entries.retain(|entry| entry.query != query);
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Debug)]
struct Armed {
    token: u64,
    input: String,
    due: Duration,
}

/// The pending re-run of the script for the query on screen.
#[derive(Debug, Default)]
pub struct Rerun {
    next_token: AtomicU64,
    armed: Mutex<Option<Armed>>,
}

impl Rerun {
    fn armed(&self) -> std::sync::MutexGuard<'_, Option<Armed>> {
        self.armed
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Plans a re-run of `input` after `interval`, replacing any earlier plan.
    /// The token names this plan to [`Rerun::remaining`] and [`Rerun::take_due`].
    pub fn arm(&self, input: &str, interval: Duration, now: Duration) -> u64 {
        let token = self.next_token.fetch_add(1, Ordering::SeqCst) + 1;
        *self.armed() = Some(Armed {
            token,
            input: input.to_owned(),
            due: now + interval,
        });
        token
    }

    /// Drops the plan (the query changed, or the launcher was hidden).
    pub fn cancel(&self) {
        *self.armed() = None;
    }

    /// How long until plan `token` is due: `None` when it was replaced or
    /// cancelled, zero when it is due now.
    pub fn remaining(&self, token: u64, now: Duration) -> Option<Duration> {
        self.armed()
            .as_ref()
            .filter(|armed| armed.token == token)
            .map(|armed| armed.due.saturating_sub(now))
    }

    /// The input to run again, once, if plan `token` is due.
    pub fn take_due(&self, token: u64, now: Duration) -> Option<String> {
        let mut armed = self.armed();
        let due = armed
            .as_ref()
            .is_some_and(|plan| plan.token == token && plan.due <= now);
        if due {
            armed.take().map(|plan| plan.input)
        } else {
            None
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Arc;

    use sevak_core::Action;

    use super::*;

    /// A clock the test moves by hand.
    #[derive(Default)]
    pub struct FakeClock(Mutex<Duration>);

    impl FakeClock {
        pub fn advance(&self, by: Duration) {
            *self.0.lock().unwrap() += by;
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> Duration {
            *self.0.lock().unwrap()
        }
    }

    pub fn arc_clock() -> (Arc<FakeClock>, Arc<dyn Clock>) {
        let clock = Arc::new(FakeClock::default());
        let dynamic: Arc<dyn Clock> = clock.clone();
        (clock, dynamic)
    }

    fn rows(title: &str) -> Vec<ResultItem> {
        vec![ResultItem::new(
            "script:t",
            title,
            title,
            Action::CopyText { text: title.into() },
        )]
    }

    const fn secs(seconds: u64) -> Duration {
        Duration::from_secs(seconds)
    }

    fn spec(seconds: u64, loose_reload: bool) -> CacheSpec {
        CacheSpec {
            seconds,
            loose_reload,
        }
    }

    #[test]
    fn an_answer_is_fresh_until_its_seconds_pass_then_gone() {
        let mut cache = FilterCache::default();
        assert_eq!(cache.get("q", secs(0)), Lookup::Miss);
        cache.put("q", rows("a"), spec(30, false), secs(100));
        assert_eq!(cache.get("q", secs(100)), Lookup::Fresh(rows("a")));
        assert_eq!(cache.get("q", secs(129)), Lookup::Fresh(rows("a")));
        assert_eq!(cache.get("q", secs(130)), Lookup::Miss);
        assert!(cache.is_empty(), "an expired answer is dropped");
        assert_eq!(cache.get("other", secs(100)), Lookup::Miss);
    }

    #[test]
    fn loose_reload_keeps_showing_the_expired_answer() {
        let mut cache = FilterCache::default();
        cache.put("q", rows("a"), spec(10, true), secs(0));
        assert_eq!(cache.get("q", secs(9)), Lookup::Fresh(rows("a")));
        assert_eq!(cache.get("q", secs(10)), Lookup::Stale(rows("a")));
        assert_eq!(cache.get("q", secs(5_000)), Lookup::Stale(rows("a")));
        // The refreshed answer is fresh again.
        cache.put("q", rows("b"), spec(10, true), secs(5_001));
        assert_eq!(cache.get("q", secs(5_002)), Lookup::Fresh(rows("b")));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn an_answer_without_caching_clears_the_old_one() {
        let mut cache = FilterCache::default();
        cache.put("q", rows("a"), spec(30, false), secs(0));
        cache.remove("q");
        assert_eq!(cache.get("q", secs(1)), Lookup::Miss);
    }

    #[test]
    fn the_cache_is_bounded_and_drops_the_least_recently_used() {
        let mut cache = FilterCache::default();
        for index in 0..MAX_ENTRIES {
            cache.put(&format!("q{index}"), rows("x"), spec(60, false), secs(0));
        }
        // Using q0 makes q1 the oldest.
        assert!(matches!(cache.get("q0", secs(1)), Lookup::Fresh(_)));
        cache.put("new", rows("y"), spec(60, false), secs(1));
        assert_eq!(cache.len(), MAX_ENTRIES);
        assert!(matches!(cache.get("q0", secs(2)), Lookup::Fresh(_)));
        assert_eq!(cache.get("q1", secs(2)), Lookup::Miss);
        assert!(matches!(cache.get("new", secs(2)), Lookup::Fresh(_)));
    }

    #[test]
    fn a_rerun_is_due_after_its_interval_and_only_once() {
        let rerun = Rerun::default();
        let token = rerun.arm("q", Duration::from_millis(500), secs(10));
        assert_eq!(
            rerun.remaining(token, secs(10)),
            Some(Duration::from_millis(500))
        );
        assert_eq!(rerun.take_due(token, Duration::from_millis(10_499)), None);
        assert_eq!(
            rerun.take_due(token, Duration::from_millis(10_500)),
            Some("q".to_owned())
        );
        assert_eq!(rerun.take_due(token, secs(11)), None, "it runs once");
        assert_eq!(rerun.remaining(token, secs(11)), None);
    }

    #[test]
    fn cancelling_or_replacing_a_rerun_stops_the_old_plan() {
        let rerun = Rerun::default();
        let first = rerun.arm("a", secs(1), secs(0));
        let second = rerun.arm("ab", secs(1), secs(0));
        assert_eq!(rerun.take_due(first, secs(5)), None);
        assert_eq!(rerun.take_due(second, secs(5)), Some("ab".to_owned()));
        let third = rerun.arm("abc", secs(1), secs(0));
        rerun.cancel();
        assert_eq!(rerun.take_due(third, secs(5)), None);
    }
}
