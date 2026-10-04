//! Asking the system whether the focused control is a password field, within a
//! time budget and without asking too often.
//!
//! The question is answered by an accessibility API (Windows: UI Automation's
//! `IsPassword` of the focused element), which talks to the other app and can
//! take long, or hang if that app does. So:
//!
//! - the question is asked on a thread of its own ([`BudgetedProbe`]), which
//!   also owns the COM objects that must stay on one thread, and the caller
//!   waits at most the budget for the answer;
//! - while a question is still being worked on, no second one is queued: the
//!   answer is "unknown" at once;
//! - an answer is reused for a few hundred milliseconds for the same window
//!   ([`PasswordFieldDetector`]), since typing asks once per keystroke.
//!
//! "Unknown" (too slow, busy, no accessibility support in the app, an error)
//! counts as "not a password field": the other guards (the app rules, the
//! `ES_PASSWORD` check, the browser rule) are what covers those cases. Both
//! types are generic over the question so the logic is tested with fakes.

// Used by the Windows backend only; the logic is built (and tested) everywhere.
#![cfg_attr(not(windows), allow(dead_code))]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The platform's way of asking. Created and used on the probe's own thread.
pub(crate) trait FocusProbe {
    /// Whether the control with keyboard focus is a password field, or `None`
    /// if that could not be told.
    fn focused_is_password(&mut self) -> Option<bool>;
}

/// Something that can be asked "is the focus in a password field?" within its
/// own time budget.
pub(crate) trait Ask {
    fn ask(&self) -> Option<bool>;
}

struct Request {
    reply: mpsc::Sender<Option<bool>>,
}

/// A [`FocusProbe`] on a thread of its own, with a time budget per question.
pub(crate) struct BudgetedProbe {
    requests: mpsc::Sender<Request>,
    /// A question is being worked on (possibly one the caller gave up on).
    busy: Arc<AtomicBool>,
    budget_ms: AtomicU64,
}

impl BudgetedProbe {
    /// Starts the thread. `make` runs on it and builds the probe there (COM
    /// objects belong to the thread that made them); `None` means the system
    /// has no such API, and every question is then unknown.
    pub(crate) fn spawn(
        budget: Duration,
        make: impl FnOnce() -> Option<Box<dyn FocusProbe>> + Send + 'static,
    ) -> Self {
        let (requests, queue) = mpsc::channel::<Request>();
        let busy = Arc::new(AtomicBool::new(false));
        let worker_busy = busy.clone();
        let spawned = std::thread::Builder::new()
            .name("sevak-focus-probe".to_owned())
            .spawn(move || {
                let mut probe = make();
                for request in queue {
                    let answer = probe.as_mut().and_then(|probe| probe.focused_is_password());
                    // Before replying: a caller that gave up has long gone.
                    worker_busy.store(false, Ordering::SeqCst);
                    let _ = request.reply.send(answer);
                }
            });
        if let Err(err) = spawned {
            tracing::debug!("the password field probe could not start: {err}");
        }
        Self {
            requests,
            busy,
            budget_ms: AtomicU64::new(millis(budget)),
        }
    }

    /// Changes the time budget (tests lengthen it once they are past the
    /// question they wanted to see time out).
    #[cfg(test)]
    fn set_budget(&self, budget: Duration) {
        self.budget_ms.store(millis(budget), Ordering::SeqCst);
    }
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

impl Ask for BudgetedProbe {
    fn ask(&self) -> Option<bool> {
        if self.busy.swap(true, Ordering::SeqCst) {
            // The last question has not come back yet.
            return None;
        }
        let (reply, answer) = mpsc::channel();
        if self.requests.send(Request { reply }).is_err() {
            self.busy.store(false, Ordering::SeqCst);
            return None;
        }
        // A timeout leaves `busy` set: the thread clears it when it is done.
        let budget = Duration::from_millis(self.budget_ms.load(Ordering::SeqCst));
        answer.recv_timeout(budget).ok().flatten()
    }
}

#[derive(Clone, Copy)]
struct Cached {
    window: isize,
    at_ms: u64,
    password: bool,
}

/// Remembers the answer for a window for a short time.
pub(crate) struct PasswordFieldDetector<A: Ask> {
    asker: A,
    ttl_ms: u64,
    cache: Mutex<Option<Cached>>,
}

impl<A: Ask> PasswordFieldDetector<A> {
    pub(crate) fn new(asker: A, ttl: Duration) -> Self {
        Self {
            asker,
            ttl_ms: millis(ttl),
            cache: Mutex::new(None),
        }
    }

    /// Whether the focus in `window` (any stable number for it) is a password
    /// field. `now_ms` is a monotonic clock. An unknown answer is "no" and is
    /// not remembered, so the next keystroke asks again.
    pub(crate) fn is_password(&self, window: isize, now_ms: u64) -> bool {
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(cached) = *cache {
            if cached.window == window && now_ms.saturating_sub(cached.at_ms) < self.ttl_ms {
                return cached.password;
            }
        }
        match self.asker.ask() {
            Some(password) => {
                *cache = Some(Cached {
                    window,
                    at_ms: now_ms,
                    password,
                });
                password
            }
            None => {
                *cache = None;
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::sync::mpsc::{channel, Receiver, Sender};

    use super::*;

    struct Scripted {
        answers: Vec<Option<bool>>,
        asked: Cell<usize>,
    }

    impl Scripted {
        fn new(answers: &[Option<bool>]) -> Self {
            Self {
                answers: answers.to_vec(),
                asked: Cell::new(0),
            }
        }
    }

    impl Ask for Scripted {
        fn ask(&self) -> Option<bool> {
            let index = self.asked.get();
            self.asked.set(index + 1);
            self.answers.get(index).copied().flatten()
        }
    }

    fn detector(answers: &[Option<bool>]) -> PasswordFieldDetector<Scripted> {
        PasswordFieldDetector::new(Scripted::new(answers), Duration::from_millis(300))
    }

    #[test]
    fn an_answer_is_reused_for_the_same_window_within_the_time_limit() {
        let detector = detector(&[Some(true), Some(false)]);
        assert!(detector.is_password(7, 1_000));
        assert!(detector.is_password(7, 1_299));
        assert_eq!(detector.asker.asked.get(), 1);
        // After the limit it is asked again, and the new answer stands.
        assert!(!detector.is_password(7, 1_300));
        assert_eq!(detector.asker.asked.get(), 2);
        assert!(!detector.is_password(7, 1_400));
        assert_eq!(detector.asker.asked.get(), 2);
    }

    #[test]
    fn another_window_is_another_question() {
        let detector = detector(&[Some(true), Some(false)]);
        assert!(detector.is_password(1, 0));
        assert!(!detector.is_password(2, 10));
        assert_eq!(detector.asker.asked.get(), 2);
        // Back to the first window: the cache holds only the latest.
        assert!(detector.asker.answers[0] == Some(true));
        assert!(!detector.is_password(1, 20));
    }

    #[test]
    fn an_unknown_answer_means_no_and_is_asked_again_next_time() {
        let detector = detector(&[None, Some(true)]);
        assert!(!detector.is_password(1, 0));
        assert!(detector.is_password(1, 1));
        assert_eq!(detector.asker.asked.get(), 2);
    }

    #[test]
    fn a_clock_that_stepped_back_does_not_trust_the_cache_forever() {
        let detector = detector(&[Some(true), Some(false)]);
        assert!(detector.is_password(1, 5_000));
        // saturating_sub: an earlier time reads as "just now" (a hit), never a panic.
        assert!(detector.is_password(1, 4_000));
    }

    /// A probe that answers when told to, so a test controls the time without
    /// sleeping: it waits for a value on `release`.
    struct Gated {
        release: Receiver<Option<bool>>,
        started: Sender<()>,
    }

    impl FocusProbe for Gated {
        fn focused_is_password(&mut self) -> Option<bool> {
            let _ = self.started.send(());
            self.release.recv().ok().flatten()
        }
    }

    fn gated(budget: Duration) -> (BudgetedProbe, Sender<Option<bool>>, Receiver<()>) {
        let (release_tx, release_rx) = channel();
        let (started_tx, started_rx) = channel();
        let probe = BudgetedProbe::spawn(budget, move || {
            Some(Box::new(Gated {
                release: release_rx,
                started: started_tx,
            }))
        });
        (probe, release_tx, started_rx)
    }

    #[test]
    fn a_prompt_probe_answers_within_the_budget() {
        let (probe, release, _started) = gated(Duration::from_secs(10));
        release.send(Some(true)).unwrap();
        assert_eq!(probe.ask(), Some(true));
        release.send(Some(false)).unwrap();
        assert_eq!(probe.ask(), Some(false));
    }

    #[test]
    fn a_slow_probe_is_given_up_on_and_not_queued_behind() {
        // A budget no real answer can meet: the probe is blocked until released.
        let (probe, release, started) = gated(Duration::from_millis(1));
        assert_eq!(probe.ask(), None, "over the budget");
        started.recv().unwrap();
        // Still working on the first question: the second is refused at once.
        assert_eq!(probe.ask(), None);
        // Release it; once it has finished, questions are answered again.
        probe.set_budget(Duration::from_secs(10));
        release.send(Some(true)).unwrap();
        release.send(Some(true)).unwrap();
        let mut answered = None;
        for _ in 0..10_000_000 {
            answered = probe.ask();
            if answered.is_some() {
                break;
            }
            std::thread::yield_now();
        }
        // The answer to the abandoned question was thrown away; this one is new.
        assert_eq!(answered, Some(true));
    }

    #[test]
    fn a_system_without_the_api_answers_unknown() {
        let probe = BudgetedProbe::spawn(Duration::from_secs(10), || None);
        assert_eq!(probe.ask(), None);
        assert_eq!(probe.ask(), None);
    }
}
