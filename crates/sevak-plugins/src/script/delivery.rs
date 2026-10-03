//! Matching script answers to queries.
//!
//! `Plugin::query` is synchronous, but a script answers when it answers. A
//! [`Delivery`] sits between the two:
//!
//! 1. `query` calls [`Delivery::begin`], which numbers the request (the *query
//!    generation*) and sends nothing itself.
//! 2. It sends the request to the script, then [`Delivery::wait`]s up to the
//!    plugin's soft budget.
//! 3. The script's reader calls [`Delivery::deliver`] with the request number.
//!    An answer is accepted only if no newer request has been issued since, so
//!    answers for queries the user has already typed past are dropped.
//! 4. If the answer arrives after `wait` gave up, the shell is told through the
//!    [`ResultsNotifier`] and re-runs the current query; `begin` then finds the
//!    answer cached for exactly that input and returns it at once.
//!
//! Everything here is plain data behind one mutex, so it is tested without any
//! process.

use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use sevak_core::{ResultItem, ResultsNotifier};

/// A cached answer is reused for the same input for this long. It exists for
/// the re-run after a late answer; it is short so that results which change over
/// time (a clock, a download queue) are fetched again on the next keystroke.
const FRESH_FOR: Duration = Duration::from_secs(2);
/// The previous answer may be shown, while the next one is on its way, for this
/// long (see [`Delivery::wait`]).
const STALE_FOR: Duration = Duration::from_secs(30);

#[derive(Debug)]
struct Answer {
    request_id: u64,
    input: String,
    at: Instant,
    items: Vec<ResultItem>,
}

#[derive(Debug, Default)]
struct State {
    /// The newest request issued; 0 before the first.
    latest: u64,
    /// The input of `latest`.
    input: String,
    /// `latest` has been sent and not answered (or given up on) yet.
    outstanding: bool,
    /// The request a `query` call is blocked on, if any.
    waiting: Option<u64>,
    answered: Option<Answer>,
}

/// What [`Delivery::begin`] decided.
#[derive(Debug, PartialEq)]
pub enum Begin {
    /// A recent answer for exactly this input is cached.
    Cached(Vec<ResultItem>),
    /// A new request `id` was issued; the caller must send it to the script.
    Send(u64),
    /// This input is already in flight as request `id`; just wait for it.
    InFlight(u64),
}

pub struct Delivery {
    plugin_id: String,
    state: Mutex<State>,
    arrived: Condvar,
    notifier: Mutex<Option<ResultsNotifier>>,
}

impl std::fmt::Debug for Delivery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Delivery")
            .field("plugin_id", &self.plugin_id)
            .finish_non_exhaustive()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Delivery {
    pub fn new(plugin_id: &str) -> Self {
        Self {
            plugin_id: plugin_id.to_owned(),
            state: Mutex::new(State::default()),
            arrived: Condvar::new(),
            notifier: Mutex::new(None),
        }
    }

    pub fn attach_notifier(&self, notifier: ResultsNotifier) {
        *lock(&self.notifier) = Some(notifier);
    }

    /// Starts handling `input`; see [`Begin`]. Marks the caller as the one
    /// waiting, so [`Delivery::deliver`] knows nobody needs a notification.
    pub fn begin(&self, input: &str) -> Begin {
        let mut state = lock(&self.state);
        if let Some(answer) = &state.answered {
            if answer.input == input && answer.at.elapsed() < FRESH_FOR {
                return Begin::Cached(answer.items.clone());
            }
        }
        if state.outstanding && state.input == input {
            let id = state.latest;
            state.waiting = Some(id);
            return Begin::InFlight(id);
        }
        state.latest += 1;
        state.input = input.to_owned();
        state.outstanding = true;
        state.waiting = Some(state.latest);
        Begin::Send(state.latest)
    }

    /// Waits up to `budget` for request `id` to be answered and returns its
    /// items. If it is not answered in time (or a newer request replaced it)
    /// this returns the previous answer when that was for a related input (one
    /// the user has since extended or shortened), so the list does not flash
    /// empty on every keystroke, and nothing otherwise.
    pub fn wait(&self, id: u64, budget: Duration) -> Vec<ResultItem> {
        let deadline = Instant::now() + budget;
        let mut state = lock(&self.state);
        loop {
            if let Some(answer) = &state.answered {
                if answer.request_id == id {
                    let items = answer.items.clone();
                    Self::stop_waiting(&mut state, id);
                    return items;
                }
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if state.latest != id || left.is_zero() {
                break;
            }
            state = self
                .arrived
                .wait_timeout(state, left)
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
        }
        let stale = state.answered.as_ref().and_then(|answer| {
            let related =
                answer.input.starts_with(&state.input) || state.input.starts_with(&answer.input);
            (related && answer.at.elapsed() < STALE_FOR).then(|| answer.items.clone())
        });
        Self::stop_waiting(&mut state, id);
        stale.unwrap_or_default()
    }

    /// Gives up on request `id` without waiting (the script could not be
    /// reached); the same input is sent again next time. An answer that still
    /// arrives is accepted, like any other late one.
    pub fn abandon(&self, id: u64) {
        let mut state = lock(&self.state);
        if state.latest == id {
            state.outstanding = false;
        }
        Self::stop_waiting(&mut state, id);
    }

    fn stop_waiting(state: &mut State, id: u64) {
        if state.waiting == Some(id) {
            state.waiting = None;
        }
    }

    /// Whether `id` is still the newest request (nothing has been typed since).
    pub fn is_current(&self, id: u64) -> bool {
        lock(&self.state).latest == id
    }

    /// Records the script's answer to request `id`. Dropped when the request is
    /// stale. If the query that asked is no longer waiting, the shell is told to
    /// run the current query again.
    pub fn deliver(&self, id: u64, items: Vec<ResultItem>) {
        let notify = {
            let mut state = lock(&self.state);
            if state.latest != id {
                tracing::debug!(
                    plugin = self.plugin_id,
                    id,
                    latest = state.latest,
                    "dropped a stale answer"
                );
                return;
            }
            let notify = state.waiting != Some(id);
            state.outstanding = false;
            state.answered = Some(Answer {
                request_id: id,
                input: state.input.clone(),
                at: Instant::now(),
                items,
            });
            self.arrived.notify_all();
            notify
        };
        if notify {
            // Clone the callback out of the lock: it calls back into the shell.
            let notifier = lock(&self.notifier).clone();
            if let Some(notifier) = notifier {
                notifier(&self.plugin_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::thread;

    use sevak_core::Action;

    use super::*;

    const SHORT: Duration = Duration::from_millis(20);
    const LONG: Duration = Duration::from_secs(5);

    fn item(title: &str) -> ResultItem {
        ResultItem::new(
            "script:t",
            title,
            title,
            Action::CopyText { text: title.into() },
        )
    }

    fn titles(items: &[ResultItem]) -> Vec<&str> {
        items.iter().map(|i| i.title.as_str()).collect()
    }

    fn counting_notifier(delivery: &Delivery) -> Arc<AtomicUsize> {
        let count = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&count);
        delivery.attach_notifier(Arc::new(move |plugin_id| {
            assert_eq!(plugin_id, "script:t");
            seen.fetch_add(1, Ordering::SeqCst);
        }));
        count
    }

    #[test]
    fn an_answer_inside_the_budget_is_returned_without_a_notification() {
        let delivery = Arc::new(Delivery::new("script:t"));
        let notified = counting_notifier(&delivery);
        let Begin::Send(id) = delivery.begin("abc") else {
            panic!("first query must send");
        };
        let answerer = {
            let delivery = Arc::clone(&delivery);
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(10));
                delivery.deliver(id, vec![item("one")]);
            })
        };
        assert_eq!(titles(&delivery.wait(id, LONG)), ["one"]);
        answerer.join().unwrap();
        assert_eq!(notified.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_late_answer_notifies_and_is_served_from_the_cache() {
        let delivery = Delivery::new("script:t");
        let notified = counting_notifier(&delivery);
        let Begin::Send(id) = delivery.begin("abc") else {
            panic!()
        };
        assert!(delivery.wait(id, SHORT).is_empty());
        delivery.deliver(id, vec![item("late")]);
        assert_eq!(notified.load(Ordering::SeqCst), 1);
        // The shell re-runs the query: the same input is answered from the cache.
        match delivery.begin("abc") {
            Begin::Cached(items) => assert_eq!(titles(&items), ["late"]),
            other => panic!("expected the cache, got {other:?}"),
        }
        // A different input is a new request.
        assert!(matches!(delivery.begin("abcd"), Begin::Send(_)));
    }

    #[test]
    fn answers_for_superseded_queries_are_dropped() {
        let delivery = Delivery::new("script:t");
        let notified = counting_notifier(&delivery);
        let Begin::Send(first) = delivery.begin("a") else {
            panic!()
        };
        assert!(delivery.wait(first, SHORT).is_empty());
        let Begin::Send(second) = delivery.begin("ab") else {
            panic!()
        };
        assert!(second > first);
        delivery.deliver(first, vec![item("stale")]);
        assert_eq!(
            notified.load(Ordering::SeqCst),
            0,
            "stale answers do not notify"
        );
        assert!(!delivery.is_current(first));
        assert!(delivery.is_current(second));
        delivery.deliver(second, vec![item("fresh")]);
        assert_eq!(titles(&delivery.wait(second, SHORT)), ["fresh"]);
    }

    #[test]
    fn the_same_input_in_flight_is_not_sent_twice() {
        let delivery = Delivery::new("script:t");
        let Begin::Send(id) = delivery.begin("abc") else {
            panic!()
        };
        assert!(delivery.wait(id, SHORT).is_empty());
        assert_eq!(delivery.begin("abc"), Begin::InFlight(id));
    }

    #[test]
    fn a_slow_answer_after_a_second_wait_still_notifies_once() {
        let delivery = Delivery::new("script:t");
        let notified = counting_notifier(&delivery);
        let Begin::Send(id) = delivery.begin("abc") else {
            panic!()
        };
        assert!(delivery.wait(id, SHORT).is_empty());
        assert_eq!(delivery.begin("abc"), Begin::InFlight(id));
        assert!(delivery.wait(id, SHORT).is_empty());
        delivery.deliver(id, vec![item("x")]);
        assert_eq!(notified.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn related_previous_results_bridge_the_gap_but_unrelated_ones_do_not() {
        let delivery = Delivery::new("script:t");
        let Begin::Send(id) = delivery.begin("wor") else {
            panic!()
        };
        delivery.deliver(id, vec![item("world")]);
        assert_eq!(titles(&delivery.wait(id, SHORT)), ["world"]);

        // Typing on: the old list stays until the new one arrives.
        let Begin::Send(next) = delivery.begin("worl") else {
            panic!()
        };
        assert_eq!(titles(&delivery.wait(next, SHORT)), ["world"]);
        // Deleting back works too.
        let Begin::Send(back) = delivery.begin("wo") else {
            panic!()
        };
        assert_eq!(titles(&delivery.wait(back, SHORT)), ["world"]);
        // A different query shows nothing rather than misleading rows.
        let Begin::Send(other) = delivery.begin("xyz") else {
            panic!()
        };
        assert!(delivery.wait(other, SHORT).is_empty());
    }

    #[test]
    fn abandoning_a_request_makes_a_late_answer_notify() {
        let delivery = Delivery::new("script:t");
        let notified = counting_notifier(&delivery);
        let Begin::Send(id) = delivery.begin("abc") else {
            panic!()
        };
        delivery.abandon(id);
        delivery.deliver(id, vec![item("x")]);
        assert_eq!(notified.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn delivery_without_a_notifier_is_harmless() {
        let delivery = Delivery::new("script:t");
        let Begin::Send(id) = delivery.begin("a") else {
            panic!()
        };
        delivery.abandon(id);
        delivery.deliver(id, Vec::new());
    }
}
