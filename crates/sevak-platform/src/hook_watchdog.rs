//! Keeping a low-level keyboard hook alive.
//!
//! Windows removes a low-level hook without telling anyone when its callback
//! takes longer than `LowLevelHooksTimeout` too often, and a hook can also stop
//! working across a session lock, a remote-desktop switch or a display change.
//! Nothing reports it: the hotkeys and snippet expansion just go quiet. "No
//! events for a while" is not a signal (the user may simply not be typing), so
//! the hook is put in again on a schedule, and sooner after the events above.
//!
//! This module is the schedule and the bookkeeping; the Windows module does
//! the actual `SetWindowsHookExW` calls behind [`HookInstaller`]. Times are
//! milliseconds from any monotonic clock the caller passes in, so the logic is
//! tested with a fake one, and nothing here touches an operating system hook.
//!
//! A reinstall is idempotent and safe: the new hook is installed before the old
//! one is removed (on the one thread that serves both, with no message pumped in
//! between, so no key event reaches both), and if installing fails the old
//! hook is left alone.

// Used by the Windows backend only; the logic is built (and tested) everywhere.
#![cfg_attr(not(windows), allow(dead_code))]

/// How often the hook is put in again when nothing else happened.
pub(crate) const REINSTALL_EVERY_MS: u64 = 60_000;
/// After an event that may have broken the hook, wait this long (restarting for
/// every further event) so the session has settled before reinstalling.
pub(crate) const SETTLE_MS: u64 = 1_500;
/// A burst of events (a display change comes as several) cannot delay the
/// reinstall longer than this in total.
pub(crate) const MAX_DELAY_MS: u64 = 10_000;
/// No two reinstalls closer together than this, whatever the events.
pub(crate) const MIN_GAP_MS: u64 = 5_000;
/// After a failed reinstall, try again in this long.
pub(crate) const RETRY_MS: u64 = 5_000;

/// Why the hook is being put in again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reason {
    /// The regular schedule.
    Periodic,
    /// The session was unlocked or reconnected (lock screen, fast user
    /// switching, remote desktop).
    Session,
    /// The display configuration changed.
    Display,
    /// The machine woke from sleep.
    Resume,
}

/// When the next reinstall is due.
#[derive(Debug, Clone)]
pub(crate) struct ReinstallSchedule {
    next_periodic: u64,
    /// An event waiting to be acted on: the reason, when it first arrived and
    /// when it is due.
    pending: Option<(Reason, u64, u64)>,
    last_done: Option<u64>,
}

impl ReinstallSchedule {
    /// The first regular reinstall is a full interval after `now`.
    pub(crate) fn new(now: u64) -> Self {
        Self {
            next_periodic: now.saturating_add(REINSTALL_EVERY_MS),
            pending: None,
            last_done: None,
        }
    }

    /// Something that may have broken the hook happened. Repeated events move
    /// the due time back, up to [`MAX_DELAY_MS`] after the first of them, and
    /// never closer than [`MIN_GAP_MS`] to the last reinstall.
    pub(crate) fn note(&mut self, reason: Reason, now: u64) {
        let first = self.pending.map_or(now, |(_, first, _)| first);
        let settled = now.saturating_add(SETTLE_MS);
        let latest = first.saturating_add(MAX_DELAY_MS);
        let earliest = self
            .last_done
            .map_or(0, |done| done.saturating_add(MIN_GAP_MS));
        let due = settled.min(latest).max(earliest);
        let reason = self
            .pending
            .map_or(reason, |(first_reason, _, _)| first_reason);
        self.pending = Some((reason, first, due));
    }

    /// The reason for a reinstall that is due now, if one is.
    pub(crate) fn due(&self, now: u64) -> Option<Reason> {
        if let Some((reason, _, due)) = self.pending {
            if now >= due {
                return Some(reason);
            }
        }
        (now >= self.next_periodic).then_some(Reason::Periodic)
    }

    /// The reinstall worked: everything pending is covered, and the regular
    /// schedule starts over.
    pub(crate) fn done(&mut self, now: u64) {
        self.pending = None;
        self.last_done = Some(now);
        self.next_periodic = now.saturating_add(REINSTALL_EVERY_MS);
    }

    /// The reinstall failed: try again soon.
    pub(crate) fn failed(&mut self, now: u64) {
        self.pending = None;
        self.next_periodic = now.saturating_add(RETRY_MS);
    }
}

/// The OS half: puts the hook in again.
pub(crate) trait HookInstaller {
    /// Installs a fresh hook and only then removes the old one. On `Err` the old
    /// hook is still in place.
    fn reinstall(&mut self) -> Result<(), String>;
}

/// What a [`HookWatchdog::tick`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Tick {
    /// Nothing was due.
    Idle,
    /// The hook was put in again, for this reason.
    Reinstalled(Reason),
    /// The reinstall failed (this is the first failure in a row, so worth a
    /// log line; later ones are quiet).
    Failed { reason: Reason, message: String },
    /// The reinstall failed again after an earlier failure.
    StillFailing,
}

/// The schedule and the installer together.
pub(crate) struct HookWatchdog<I: HookInstaller> {
    installer: I,
    schedule: ReinstallSchedule,
    failing: bool,
}

impl<I: HookInstaller> HookWatchdog<I> {
    pub(crate) fn new(installer: I, now: u64) -> Self {
        Self {
            installer,
            schedule: ReinstallSchedule::new(now),
            failing: false,
        }
    }

    pub(crate) fn installer_mut(&mut self) -> &mut I {
        &mut self.installer
    }

    /// Gives the installer back (to remove the hooks when the thread ends).
    pub(crate) fn into_installer(self) -> I {
        self.installer
    }

    /// Tells the watchdog about an event that may have broken the hook.
    pub(crate) fn note(&mut self, reason: Reason, now: u64) {
        self.schedule.note(reason, now);
    }

    /// Called regularly (a timer a second is plenty): reinstalls if it is time.
    pub(crate) fn tick(&mut self, now: u64) -> Tick {
        let Some(reason) = self.schedule.due(now) else {
            return Tick::Idle;
        };
        match self.installer.reinstall() {
            Ok(()) => {
                self.failing = false;
                self.schedule.done(now);
                Tick::Reinstalled(reason)
            }
            Err(message) => {
                self.schedule.failed(now);
                if std::mem::replace(&mut self.failing, true) {
                    Tick::StillFailing
                } else {
                    Tick::Failed { reason, message }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Fake {
        installs: u32,
        fail: bool,
    }

    impl HookInstaller for Fake {
        fn reinstall(&mut self) -> Result<(), String> {
            if self.fail {
                return Err("access denied".to_owned());
            }
            self.installs += 1;
            Ok(())
        }
    }

    fn watchdog() -> HookWatchdog<Fake> {
        HookWatchdog::new(Fake::default(), 0)
    }

    #[test]
    fn the_hook_is_put_in_again_every_minute() {
        let mut dog = watchdog();
        assert_eq!(dog.tick(1_000), Tick::Idle);
        assert_eq!(dog.tick(59_999), Tick::Idle);
        assert_eq!(dog.tick(60_000), Tick::Reinstalled(Reason::Periodic));
        // The next one is a minute after this one, not after the start.
        assert_eq!(dog.tick(60_001), Tick::Idle);
        assert_eq!(dog.tick(119_999), Tick::Idle);
        assert_eq!(dog.tick(120_000), Tick::Reinstalled(Reason::Periodic));
        assert_eq!(dog.installer_mut().installs, 2);
    }

    #[test]
    fn a_late_tick_reinstalls_once_not_once_per_missed_minute() {
        // The machine slept for ten minutes.
        let mut dog = watchdog();
        assert_eq!(dog.tick(600_000), Tick::Reinstalled(Reason::Periodic));
        assert_eq!(dog.tick(600_001), Tick::Idle);
        assert_eq!(dog.installer_mut().installs, 1);
    }

    #[test]
    fn an_unlock_reinstalls_after_things_settle() {
        let mut dog = watchdog();
        dog.note(Reason::Session, 10_000);
        assert_eq!(dog.tick(10_000), Tick::Idle);
        assert_eq!(dog.tick(11_499), Tick::Idle);
        assert_eq!(dog.tick(11_500), Tick::Reinstalled(Reason::Session));
        // The regular schedule restarted from here.
        assert_eq!(dog.tick(70_000), Tick::Idle);
        assert_eq!(dog.tick(71_500), Tick::Reinstalled(Reason::Periodic));
    }

    #[test]
    fn a_burst_of_display_changes_reinstalls_once() {
        let mut dog = watchdog();
        for at in [20_000, 20_400, 20_900, 21_300] {
            dog.note(Reason::Display, at);
        }
        assert_eq!(dog.tick(22_000), Tick::Idle, "still settling");
        assert_eq!(dog.tick(22_800), Tick::Reinstalled(Reason::Display));
        assert_eq!(dog.tick(23_000), Tick::Idle);
        assert_eq!(dog.installer_mut().installs, 1);
    }

    #[test]
    fn events_that_never_stop_cannot_postpone_the_reinstall_forever() {
        let mut dog = watchdog();
        let mut reinstalled_at = None;
        // An event every second from the moment the first arrives.
        for second in 0..30u64 {
            let now = 5_000 + second * 1_000;
            dog.note(Reason::Display, now);
            if let Tick::Reinstalled(_) = dog.tick(now) {
                reinstalled_at = Some(now);
                break;
            }
        }
        // No later than 10 s after the first event.
        assert!(
            reinstalled_at.is_some_and(|at| at <= 15_000),
            "{reinstalled_at:?}"
        );
    }

    #[test]
    fn no_two_reinstalls_are_closer_than_the_minimum_gap() {
        let mut dog = watchdog();
        dog.note(Reason::Session, 0);
        assert_eq!(dog.tick(1_500), Tick::Reinstalled(Reason::Session));
        // Another event right after: it waits for the gap.
        dog.note(Reason::Resume, 2_000);
        assert_eq!(dog.tick(3_500), Tick::Idle);
        assert_eq!(dog.tick(6_499), Tick::Idle);
        assert_eq!(dog.tick(6_500), Tick::Reinstalled(Reason::Resume));
    }

    #[test]
    fn a_failure_is_retried_soon_and_logged_once() {
        let mut dog = watchdog();
        dog.installer_mut().fail = true;
        assert_eq!(
            dog.tick(60_000),
            Tick::Failed {
                reason: Reason::Periodic,
                message: "access denied".to_owned()
            }
        );
        // Not another minute: five seconds.
        assert_eq!(dog.tick(64_999), Tick::Idle);
        assert_eq!(dog.tick(65_000), Tick::StillFailing);
        assert_eq!(dog.tick(70_000), Tick::StillFailing);
        // Working again: back to the regular schedule, and a later failure is
        // news again.
        dog.installer_mut().fail = false;
        assert_eq!(dog.tick(75_000), Tick::Reinstalled(Reason::Periodic));
        assert_eq!(dog.tick(134_999), Tick::Idle);
        dog.installer_mut().fail = true;
        assert!(matches!(dog.tick(135_000), Tick::Failed { .. }));
    }

    #[test]
    fn the_first_event_names_the_reason() {
        let mut dog = watchdog();
        dog.note(Reason::Resume, 1_000);
        dog.note(Reason::Display, 1_200);
        assert_eq!(dog.tick(2_699), Tick::Idle);
        assert_eq!(dog.tick(2_700), Tick::Reinstalled(Reason::Resume));
    }

    #[test]
    fn an_event_noted_while_a_failure_is_pending_still_gets_its_retry() {
        let mut dog = watchdog();
        dog.installer_mut().fail = true;
        dog.note(Reason::Session, 1_000);
        assert!(matches!(dog.tick(2_500), Tick::Failed { .. }));
        dog.installer_mut().fail = false;
        assert_eq!(dog.tick(7_500), Tick::Reinstalled(Reason::Periodic));
    }

    #[test]
    fn a_clock_that_goes_back_never_panics_or_fires_early() {
        let mut dog = watchdog();
        assert_eq!(dog.tick(60_000), Tick::Reinstalled(Reason::Periodic));
        assert_eq!(dog.tick(1_000), Tick::Idle);
        dog.note(Reason::Session, 500);
        assert_eq!(dog.tick(600), Tick::Idle);
    }
}
