//! Global hotkeys the normal API cannot register, taken with a keyboard hook.
//!
//! On Windows `RegisterHotKey` refuses every Windows-key combination (the OS
//! owns Win+Space, the input-language switcher) and any key another app has
//! registered. A low-level keyboard hook (`WH_KEYBOARD_LL`) sees every key
//! before either of them does: it recognises the configured shortcuts, tells
//! Sevak, and swallows the key so neither Windows nor the other app reacts.
//!
//! This module holds everything except the Win32 calls, so it compiles and is
//! tested everywhere:
//!
//! - [`HookMachine`]: the pure state machine. It is told about every key event
//!   (with the modifiers held) and answers whether to swallow it, which
//!   shortcut fired, and whether a "mask" key must be injected so releasing the
//!   Windows or Alt key does not open the Start menu or an app's menu bar.
//! - [`HotkeyHookService`]: owns a machine, starts the hook (through a
//!   [`HookBackend`]) while there is something to watch for and stops it when
//!   there is not.
//!
//! The Windows backend lives in `windows::keyhook` (one hook thread shared
//! with snippet expansion) and `windows::keyhook_hotkey`. Nothing about a key
//! is logged or kept beyond the state machine's few flags; the hook only ever
//! reports which configured shortcut was pressed.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::accelerator::Combo;

/// Whether the hook may act on key events another program injected
/// (`SendInput`: AutoHotkey, PowerToys remaps, remote-control tools). Off by
/// default: a program on the desktop could otherwise press Sevak's shortcut for
/// the user (open the launcher, make Universal Actions copy the foreground
/// app's selection). `[general] accept_injected_hotkeys` turns it on.
static ACCEPT_INJECTED: AtomicBool = AtomicBool::new(false);

/// Sets whether injected key events count; see [`accepts_injected`].
pub fn set_accept_injected(accept: bool) {
    ACCEPT_INJECTED.store(accept, Ordering::SeqCst);
}

/// Whether injected key events count as the user pressing the key.
pub fn accepts_injected() -> bool {
    ACCEPT_INJECTED.load(Ordering::SeqCst)
}

/// Whether the hook should look at an event at all: always for a real key
/// press, and for an injected one only when `accept` says so. Sevak's own
/// injected keys (the mask) never reach this check: the shared hook passes them
/// straight on, whatever the setting.
pub const fn event_wanted(injected: bool, accept: bool) -> bool {
    accept || !injected
}

/// How long a swallowed key counts as still held without another event for it.
/// Auto-repeat starts within 1 second of the press and then repeats every few
/// tens of milliseconds, so a longer silence means the key-up was missed and the
/// next key-down is a new press, not a repeat.
const STALE_AFTER_MS: u64 = 2_000;

/// How long recording listens for a shortcut before giving up.
pub const RECORD_TIMEOUT_MS: u64 = 30_000;

/// The virtual-key codes that are modifiers.
pub mod vk {
    pub const SHIFT: u32 = 0x10;
    pub const CONTROL: u32 = 0x11;
    pub const MENU: u32 = 0x12;
    pub const LWIN: u32 = 0x5B;
    pub const RWIN: u32 = 0x5C;
    pub const LSHIFT: u32 = 0xA0;
    pub const RSHIFT: u32 = 0xA1;
    pub const LCONTROL: u32 = 0xA2;
    pub const RCONTROL: u32 = 0xA3;
    pub const LMENU: u32 = 0xA4;
    pub const RMENU: u32 = 0xA5;
    pub const ESCAPE: u32 = 0x1B;
    /// An unassigned key: tapping it while Win or Alt is held makes the OS treat
    /// the modifier as used, so its release does nothing on its own.
    pub const MASK: u32 = 0xE8;

    pub const fn is_win(vk: u32) -> bool {
        vk == LWIN || vk == RWIN
    }

    pub const fn is_alt(vk: u32) -> bool {
        vk == MENU || vk == LMENU || vk == RMENU
    }

    pub const fn is_modifier(vk: u32) -> bool {
        is_win(vk)
            || is_alt(vk)
            || matches!(vk, SHIFT | CONTROL | LSHIFT | RSHIFT | LCONTROL | RCONTROL)
    }
}

/// Which modifiers are held (from the OS, at the moment of the event).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}

impl Mods {
    fn matches(self, combo: &Combo) -> bool {
        self.ctrl == combo.ctrl
            && self.alt == combo.alt
            && self.shift == combo.shift
            && self.win == combo.win
    }

    fn is_empty(self) -> bool {
        self == Self::default()
    }
}

/// A shortcut to watch for. `id` is whatever the caller wants to be told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub id: u32,
    pub combo: Combo,
}

/// What the machine reports about a key event, besides swallowing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fire {
    /// The shortcut with this id was pressed.
    Binding(u32),
    /// Recording: this is the shortcut that was pressed.
    Recorded(Combo),
    /// Recording: Escape was pressed, so nothing was chosen.
    RecordCancelled,
}

/// What to do with one key event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Outcome {
    /// Do not pass the event on: neither the OS nor any app sees it.
    pub swallow: bool,
    pub fire: Option<Fire>,
    /// Inject the mask key now (before the modifier's release goes through).
    pub inject_mask: bool,
}

const PASS: Outcome = Outcome {
    swallow: false,
    fire: None,
    inject_mask: false,
};

const SWALLOW: Outcome = Outcome {
    swallow: true,
    fire: None,
    inject_mask: false,
};

/// A key the machine swallowed the press of, so it also swallows its repeats
/// and its release.
#[derive(Debug, Clone, Copy)]
struct Held {
    vk: u32,
    last_seen_ms: u64,
}

/// Recognises the configured shortcuts in a stream of key events.
///
/// Only an exact match is swallowed: `Win+Space` is not `Win+Shift+Space`
/// (which still switches input language backwards), and a modifier key is never
/// swallowed, so Win and Alt keep working for everything else.
#[derive(Debug, Default)]
pub struct HookMachine {
    bindings: Vec<Binding>,
    /// While suspended the bindings do not fire (the settings recorder is open).
    suspended: bool,
    /// `Some(deadline)` while recording the next shortcut.
    recording_until_ms: Option<u64>,
    held: Vec<Held>,
    /// A swallowed shortcut used the Windows key / Alt: its release needs a mask.
    mask_win: bool,
    mask_alt: bool,
}

impl HookMachine {
    pub fn set_bindings(&mut self, bindings: Vec<Binding>) {
        self.bindings = bindings;
        self.reset_keys();
    }

    pub fn set_suspended(&mut self, suspended: bool) {
        self.suspended = suspended;
        self.reset_keys();
    }

    /// Listens for the next shortcut instead of firing the bindings.
    pub fn start_recording(&mut self, now_ms: u64) {
        self.recording_until_ms = Some(now_ms.saturating_add(RECORD_TIMEOUT_MS));
    }

    pub fn stop_recording(&mut self) {
        self.recording_until_ms = None;
    }

    pub fn is_recording(&self) -> bool {
        self.recording_until_ms.is_some()
    }

    /// Whether the hook has any reason to be installed.
    pub fn wants_hook(&self) -> bool {
        self.is_recording() || (!self.suspended && !self.bindings.is_empty())
    }

    /// Forgets what is held. Not the masks: a modifier that is still down will
    /// still be released.
    fn reset_keys(&mut self) {
        self.held.clear();
    }

    /// Processes one key event. `mods` is what the OS says is held (it is not
    /// consulted for the modifier events themselves); `now_ms` a monotonic clock.
    pub fn on_key(&mut self, vk: u32, down: bool, mods: Mods, now_ms: u64) -> Outcome {
        if vk::is_modifier(vk) {
            return self.on_modifier(vk, down);
        }
        if let Some(deadline) = self.recording_until_ms {
            if now_ms > deadline {
                self.recording_until_ms = None;
            }
        }
        if down {
            self.on_key_down(vk, mods, now_ms)
        } else {
            self.on_key_up(vk)
        }
    }

    /// Modifiers always pass; releasing one that a swallowed shortcut used asks
    /// for the mask key first.
    fn on_modifier(&mut self, vk: u32, down: bool) -> Outcome {
        if down {
            return PASS;
        }
        let mask = if vk::is_win(vk) {
            std::mem::take(&mut self.mask_win)
        } else if vk::is_alt(vk) {
            std::mem::take(&mut self.mask_alt)
        } else {
            false
        };
        Outcome {
            inject_mask: mask,
            ..PASS
        }
    }

    fn on_key_up(&mut self, vk: u32) -> Outcome {
        match self.held.iter().position(|held| held.vk == vk) {
            Some(index) => {
                self.held.swap_remove(index);
                SWALLOW
            }
            None => PASS,
        }
    }

    fn on_key_down(&mut self, vk: u32, mods: Mods, now_ms: u64) -> Outcome {
        // Recording takes the first real shortcut (a modifier plus a key, or a
        // function key); other keys are the user typing elsewhere.
        if self.recording_until_ms.is_some() {
            if vk == vk::ESCAPE && mods.is_empty() {
                self.recording_until_ms = None;
                self.hold(vk, now_ms);
                return Outcome {
                    swallow: true,
                    fire: Some(Fire::RecordCancelled),
                    inject_mask: false,
                };
            }
            let function_key = (0x70..=0x87).contains(&vk);
            if !mods.is_empty() || function_key {
                self.recording_until_ms = None;
                self.hold(vk, now_ms);
                self.note_masks(mods);
                let combo = Combo {
                    ctrl: mods.ctrl,
                    alt: mods.alt,
                    shift: mods.shift,
                    win: mods.win,
                    vk,
                };
                return Outcome {
                    swallow: true,
                    fire: Some(Fire::Recorded(combo)),
                    inject_mask: false,
                };
            }
            return PASS;
        }

        // A repeat (or a second press after a missed release that is not stale
        // yet) of a key whose press was swallowed.
        if let Some(held) = self.held.iter_mut().find(|held| held.vk == vk) {
            if now_ms.saturating_sub(held.last_seen_ms) <= STALE_AFTER_MS {
                held.last_seen_ms = now_ms;
                return SWALLOW;
            }
            self.held.retain(|held| held.vk != vk);
        }

        if self.suspended {
            return PASS;
        }
        let Some(binding) = self
            .bindings
            .iter()
            .find(|binding| binding.combo.vk == vk && mods.matches(&binding.combo))
        else {
            return PASS;
        };
        let id = binding.id;
        self.hold(vk, now_ms);
        self.note_masks(mods);
        Outcome {
            swallow: true,
            fire: Some(Fire::Binding(id)),
            inject_mask: false,
        }
    }

    fn hold(&mut self, vk: u32, now_ms: u64) {
        self.held.retain(|held| held.vk != vk);
        self.held.push(Held {
            vk,
            last_seen_ms: now_ms,
        });
    }

    fn note_masks(&mut self, mods: Mods) {
        self.mask_win |= mods.win;
        self.mask_alt |= mods.alt;
    }
}

/// What the hook tells Sevak.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookEvent {
    /// The binding with this id was pressed.
    Pressed(u32),
    /// Recording: the user pressed this shortcut (in the plugin's syntax).
    Recorded(String),
    /// Recording: the user pressed Escape.
    RecordCancelled,
}

/// Receives [`HookEvent`]s on the hook's thread. It must return at once (the OS
/// drops a hook that is slow), so it should only send the event down a channel.
pub type HookSink = Arc<dyn Fn(HookEvent) + Send + Sync>;

/// Installs and removes the real OS hook.
pub trait HookBackend: Send {
    /// Installs the hook; key events must then reach
    /// [`HotkeyHookService::on_key`]. Fails with the reason.
    fn start(&mut self) -> Result<(), String>;
    /// Removes the hook; no event may reach the service afterwards.
    fn stop(&mut self);
}

#[derive(Default)]
struct Core {
    machine: HookMachine,
    sink: Option<HookSink>,
}

/// Runs a [`HookMachine`] and keeps the OS hook installed exactly while it has
/// something to watch for.
pub struct HotkeyHookService<B: HookBackend> {
    core: Mutex<Core>,
    /// The backend, and whether it is started. Locked on its own, never while
    /// `core` is held: the hook thread takes `core` for every key press, and
    /// stopping the backend waits for that thread.
    backend: Mutex<(B, bool)>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl<B: HookBackend> HotkeyHookService<B> {
    pub fn new(backend: B) -> Self {
        Self {
            core: Mutex::new(Core::default()),
            backend: Mutex::new((backend, false)),
        }
    }

    /// Replaces the shortcuts to watch for and where to report them, and ends a
    /// suspension. An empty list removes the hook (unless a recording is going on).
    pub fn configure(&self, bindings: Vec<Binding>, sink: HookSink) -> Result<(), String> {
        {
            let mut core = lock(&self.core);
            core.machine.set_suspended(false);
            core.machine.set_bindings(bindings);
            core.sink = Some(sink);
        }
        self.sync()
    }

    /// While suspended no shortcut fires (the settings recorder is open); the
    /// hook stays installed only if a recording needs it.
    pub fn set_suspended(&self, suspended: bool) -> Result<(), String> {
        lock(&self.core).machine.set_suspended(suspended);
        self.sync()
    }

    /// Reports the next shortcut the user presses, once.
    pub fn start_recording(&self, now_ms: u64) -> Result<(), String> {
        lock(&self.core).machine.start_recording(now_ms);
        self.sync().inspect_err(|_| {
            lock(&self.core).machine.stop_recording();
        })
    }

    pub fn stop_recording(&self) -> Result<(), String> {
        lock(&self.core).machine.stop_recording();
        self.sync()
    }

    /// Removes the hook and forgets everything.
    pub fn shutdown(&self) {
        {
            let mut core = lock(&self.core);
            core.machine = HookMachine::default();
            core.sink = None;
        }
        let _ = self.sync();
    }

    /// Whether the hook is installed right now.
    pub fn is_running(&self) -> bool {
        lock(&self.backend).1
    }

    /// Starts or stops the backend to match what the machine wants.
    fn sync(&self) -> Result<(), String> {
        let wanted = lock(&self.core).machine.wants_hook();
        let mut backend = lock(&self.backend);
        match (wanted, backend.1) {
            (true, false) => {
                backend.0.start()?;
                backend.1 = true;
            }
            (false, true) => {
                backend.0.stop();
                backend.1 = false;
            }
            _ => {}
        }
        Ok(())
    }

    /// Called by the backend for each key event. Returns the outcome; the
    /// caller swallows the event and injects the mask as it says. Never blocks
    /// beyond the short `core` lock.
    pub fn on_key(&self, vk: u32, down: bool, mods: Mods, now_ms: u64) -> Outcome {
        let (outcome, sink) = {
            let mut core = lock(&self.core);
            let outcome = core.machine.on_key(vk, down, mods, now_ms);
            (outcome, core.sink.clone())
        };
        if let (Some(fire), Some(sink)) = (outcome.fire, sink) {
            sink(match fire {
                Fire::Binding(id) => HookEvent::Pressed(id),
                Fire::Recorded(combo) => HookEvent::Recorded(combo.accelerator()),
                Fire::RecordCancelled => HookEvent::RecordCancelled,
            });
        }
        outcome
    }
}

/// Why the hook cannot be used here.
#[cfg(not(windows))]
const UNSUPPORTED: &str = "the keyboard hook is only available on Windows";

/// Whether this OS has a keyboard hook Sevak can take shortcuts with.
pub const fn supported() -> bool {
    cfg!(windows)
}

/// Starts watching for `bindings`, reporting to `sink`. See
/// [`HotkeyHookService::configure`].
pub fn configure(bindings: Vec<Binding>, sink: HookSink) -> Result<(), String> {
    #[cfg(windows)]
    {
        crate::windows::hotkey_hook_service().configure(bindings, sink)
    }
    #[cfg(not(windows))]
    {
        let _ = (bindings, sink);
        Err(UNSUPPORTED.to_owned())
    }
}

/// See [`HotkeyHookService::set_suspended`].
pub fn set_suspended(suspended: bool) {
    #[cfg(windows)]
    if let Err(err) = crate::windows::hotkey_hook_service().set_suspended(suspended) {
        tracing::warn!("could not change the keyboard hook: {err}");
    }
    #[cfg(not(windows))]
    let _ = suspended;
}

/// Reports the next shortcut pressed (even one the OS reserves) to the sink
/// given to [`configure`], within [`RECORD_TIMEOUT_MS`].
pub fn start_recording() -> Result<(), String> {
    #[cfg(windows)]
    {
        crate::windows::hotkey_hook_service().start_recording(crate::windows::hook_clock_ms())
    }
    #[cfg(not(windows))]
    {
        Err(UNSUPPORTED.to_owned())
    }
}

pub fn stop_recording() {
    #[cfg(windows)]
    if let Err(err) = crate::windows::hotkey_hook_service().stop_recording() {
        tracing::warn!("could not change the keyboard hook: {err}");
    }
}

/// Removes the hook (on quit).
pub fn shutdown() {
    #[cfg(windows)]
    crate::windows::hotkey_hook_service().shutdown();
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPACE: u32 = 0x20;
    const K: u32 = 0x4B;

    const NONE: Mods = Mods {
        ctrl: false,
        alt: false,
        shift: false,
        win: false,
    };
    const WIN: Mods = Mods { win: true, ..NONE };
    const ALT: Mods = Mods { alt: true, ..NONE };

    fn bind(id: u32, text: &str) -> Binding {
        Binding {
            id,
            combo: Combo::parse(text).unwrap(),
        }
    }

    fn machine(bindings: &[(u32, &str)]) -> HookMachine {
        let mut machine = HookMachine::default();
        machine.set_bindings(bindings.iter().map(|(id, text)| bind(*id, text)).collect());
        machine
    }

    fn fired(id: u32) -> Option<Fire> {
        Some(Fire::Binding(id))
    }

    #[test]
    fn win_space_fires_once_and_is_swallowed_with_its_release() {
        let mut m = machine(&[(1, "Super+Space")]);
        // The Windows key itself always passes.
        assert_eq!(m.on_key(vk::LWIN, true, WIN, 0), PASS);
        let press = m.on_key(SPACE, true, WIN, 10);
        assert!(press.swallow);
        assert_eq!(press.fire, fired(1));
        // Auto-repeat is swallowed and does not fire again.
        for t in [500, 530, 560] {
            assert_eq!(m.on_key(SPACE, true, WIN, t), SWALLOW);
        }
        assert_eq!(m.on_key(SPACE, false, WIN, 600), SWALLOW);
        // Releasing Win asks for the mask so the Start menu stays closed.
        let release = m.on_key(vk::LWIN, false, NONE, 650);
        assert!(!release.swallow && release.inject_mask);
        // Once only.
        assert_eq!(m.on_key(vk::LWIN, false, NONE, 700), PASS);
    }

    #[test]
    fn the_right_windows_key_counts_too() {
        let mut m = machine(&[(1, "Win+Space")]);
        assert_eq!(m.on_key(SPACE, true, WIN, 0).fire, fired(1));
        assert!(m.on_key(vk::RWIN, false, NONE, 5).inject_mask);
    }

    #[test]
    fn releasing_space_after_win_is_still_swallowed() {
        let mut m = machine(&[(1, "Super+Space")]);
        m.on_key(SPACE, true, WIN, 0);
        let release_win = m.on_key(vk::LWIN, false, NONE, 10);
        assert!(release_win.inject_mask);
        // Repeats after Win is up must not type spaces.
        assert_eq!(m.on_key(SPACE, true, NONE, 500), SWALLOW);
        assert_eq!(m.on_key(SPACE, false, NONE, 520), SWALLOW);
        // And the next plain Space is a plain Space.
        assert_eq!(m.on_key(SPACE, true, NONE, 900), PASS);
        assert_eq!(m.on_key(SPACE, false, NONE, 910), PASS);
    }

    #[test]
    fn only_an_exact_match_is_swallowed() {
        let mut m = machine(&[(1, "Super+Space")]);
        // Plain space, Win+Shift+Space (language back), Ctrl+Win+Space, Win+K.
        assert_eq!(m.on_key(SPACE, true, NONE, 0), PASS);
        assert_eq!(m.on_key(SPACE, false, NONE, 1), PASS);
        let shifted = Mods { shift: true, ..WIN };
        assert_eq!(m.on_key(SPACE, true, shifted, 2), PASS);
        assert_eq!(m.on_key(SPACE, false, shifted, 3), PASS);
        let ctrl = Mods { ctrl: true, ..WIN };
        assert_eq!(m.on_key(SPACE, true, ctrl, 4), PASS);
        assert_eq!(m.on_key(K, true, WIN, 5), PASS);
        // Nothing was swallowed, so no mask is owed.
        assert_eq!(m.on_key(vk::LWIN, false, NONE, 6), PASS);
    }

    #[test]
    fn modifier_keys_are_never_swallowed() {
        let mut m = machine(&[(1, "Super+Space"), (2, "Alt+Space")]);
        for key in [
            vk::SHIFT,
            vk::CONTROL,
            vk::MENU,
            vk::LWIN,
            vk::RWIN,
            vk::LSHIFT,
            vk::RSHIFT,
            vk::LCONTROL,
            vk::RCONTROL,
            vk::LMENU,
            vk::RMENU,
        ] {
            assert_eq!(m.on_key(key, true, NONE, 0), PASS, "{key:#x}");
        }
    }

    #[test]
    fn several_shortcuts_are_told_apart() {
        let mut m = machine(&[(1, "Super+Space"), (2, "Ctrl+Alt+K"), (3, "F9")]);
        let ctrl_alt = Mods {
            ctrl: true,
            alt: true,
            ..NONE
        };
        assert_eq!(m.on_key(K, true, ctrl_alt, 0).fire, fired(2));
        assert_eq!(m.on_key(K, false, ctrl_alt, 1), SWALLOW);
        assert_eq!(m.on_key(0x78, true, NONE, 2).fire, fired(3));
        assert_eq!(m.on_key(0x78, false, NONE, 3), SWALLOW);
        assert_eq!(m.on_key(SPACE, true, WIN, 4).fire, fired(1));
    }

    #[test]
    fn one_key_can_carry_several_shortcuts_with_different_modifiers() {
        let mut m = machine(&[(1, "Super+K"), (2, "Alt+K"), (3, "K")]);
        let shifted = Mods {
            shift: true,
            ..NONE
        };
        assert_eq!(m.on_key(K, true, WIN, 0).fire, fired(1));
        m.on_key(K, false, WIN, 1);
        assert_eq!(m.on_key(K, true, ALT, 2).fire, fired(2));
        m.on_key(K, false, ALT, 3);
        assert_eq!(m.on_key(K, true, NONE, 4).fire, fired(3));
        m.on_key(K, false, NONE, 5);
        // Shift+K is none of them.
        assert_eq!(m.on_key(K, true, shifted, 6), PASS);
    }

    #[test]
    fn a_shortcut_with_alt_masks_the_alt_release() {
        let mut m = machine(&[(1, "Alt+Space")]);
        assert_eq!(m.on_key(SPACE, true, ALT, 0).fire, fired(1));
        assert_eq!(m.on_key(SPACE, false, ALT, 1), SWALLOW);
        assert!(m.on_key(vk::LMENU, false, NONE, 2).inject_mask);
        assert_eq!(m.on_key(vk::LMENU, false, NONE, 3), PASS);
    }

    #[test]
    fn ctrl_and_shift_need_no_mask() {
        let mut m = machine(&[(1, "Ctrl+Shift+K")]);
        let mods = Mods {
            ctrl: true,
            shift: true,
            ..NONE
        };
        assert_eq!(m.on_key(K, true, mods, 0).fire, fired(1));
        m.on_key(K, false, mods, 1);
        assert_eq!(m.on_key(vk::LCONTROL, false, NONE, 2), PASS);
        assert_eq!(m.on_key(vk::LSHIFT, false, NONE, 3), PASS);
    }

    #[test]
    fn a_missed_release_does_not_swallow_the_key_forever() {
        let mut m = machine(&[(1, "Super+Space")]);
        assert_eq!(m.on_key(SPACE, true, WIN, 0).fire, fired(1));
        // The key-up never arrived (the hook was busy). Much later, a new press
        // is a new press: it fires again.
        let later = m.on_key(SPACE, true, WIN, 10_000);
        assert_eq!(later.fire, fired(1));
        // And a plain Space after that missed release is not swallowed.
        let mut m = machine(&[(1, "Super+Space")]);
        m.on_key(SPACE, true, WIN, 0);
        assert_eq!(m.on_key(SPACE, true, NONE, 10_000), PASS);
    }

    #[test]
    fn rebinding_clears_what_is_held() {
        let mut m = machine(&[(1, "Super+Space")]);
        m.on_key(SPACE, true, WIN, 0);
        m.set_bindings(vec![bind(1, "Super+Space")]);
        assert_eq!(m.on_key(SPACE, false, WIN, 1), PASS);
    }

    #[test]
    fn suspended_bindings_do_not_fire() {
        let mut m = machine(&[(1, "Super+Space")]);
        m.set_suspended(true);
        assert!(!m.wants_hook());
        assert_eq!(m.on_key(SPACE, true, WIN, 0), PASS);
        m.set_suspended(false);
        assert!(m.wants_hook());
        assert_eq!(m.on_key(SPACE, true, WIN, 1).fire, fired(1));
    }

    #[test]
    fn the_hook_is_wanted_only_with_something_to_watch() {
        let mut m = HookMachine::default();
        assert!(!m.wants_hook());
        m.start_recording(0);
        assert!(m.wants_hook());
        m.stop_recording();
        assert!(!m.wants_hook());
        m.set_bindings(vec![bind(1, "F9")]);
        assert!(m.wants_hook());
    }

    #[test]
    fn recording_reports_the_next_shortcut_even_win_space() {
        let mut m = machine(&[(1, "Alt+Space")]);
        m.start_recording(0);
        // A bare letter is just typing: it passes and recording goes on.
        assert_eq!(m.on_key(K, true, NONE, 1), PASS);
        assert!(m.is_recording());
        let outcome = m.on_key(SPACE, true, WIN, 2);
        assert!(outcome.swallow);
        assert_eq!(
            outcome.fire,
            Some(Fire::Recorded(Combo::parse("Super+Space").unwrap()))
        );
        assert!(!m.is_recording());
        // Its release is swallowed too and the Start menu is masked.
        assert_eq!(m.on_key(SPACE, false, WIN, 3), SWALLOW);
        assert!(m.on_key(vk::LWIN, false, NONE, 4).inject_mask);
    }

    #[test]
    fn recording_does_not_fire_the_bindings() {
        let mut m = machine(&[(1, "Alt+Space")]);
        m.start_recording(0);
        let outcome = m.on_key(SPACE, true, ALT, 1);
        assert_eq!(
            outcome.fire,
            Some(Fire::Recorded(Combo::parse("Alt+Space").unwrap()))
        );
        // Normal service resumes afterwards.
        m.on_key(SPACE, false, ALT, 2);
        assert_eq!(m.on_key(SPACE, true, ALT, 3).fire, fired(1));
    }

    #[test]
    fn recording_accepts_function_keys_and_cancels_on_escape() {
        let mut m = HookMachine::default();
        m.start_recording(0);
        assert_eq!(
            m.on_key(0x7A, true, NONE, 1).fire,
            Some(Fire::Recorded(Combo::parse("F11").unwrap()))
        );
        m.start_recording(2);
        let cancel = m.on_key(vk::ESCAPE, true, NONE, 3);
        assert!(cancel.swallow);
        assert_eq!(cancel.fire, Some(Fire::RecordCancelled));
        assert!(!m.is_recording());
        // Ctrl+Escape is a shortcut, not a cancel.
        m.start_recording(4);
        let ctrl = Mods { ctrl: true, ..NONE };
        assert_eq!(
            m.on_key(vk::ESCAPE, true, ctrl, 5).fire,
            Some(Fire::Recorded(Combo::parse("Ctrl+Escape").unwrap()))
        );
    }

    #[test]
    fn recording_gives_up_after_its_timeout() {
        let mut m = machine(&[(1, "Alt+Space")]);
        m.start_recording(0);
        let outcome = m.on_key(SPACE, true, ALT, RECORD_TIMEOUT_MS + 1);
        // Too late to be a recording: it is the binding that fires.
        assert_eq!(outcome.fire, fired(1));
        assert!(!m.is_recording());
    }

    // The service, with a fake backend that counts installs.

    #[derive(Default)]
    struct Fake {
        starts: Arc<Mutex<u32>>,
        stops: Arc<Mutex<u32>>,
        fail: bool,
    }

    impl HookBackend for Fake {
        fn start(&mut self) -> Result<(), String> {
            if self.fail {
                return Err("denied".to_owned());
            }
            *self.starts.lock().unwrap() += 1;
            Ok(())
        }

        fn stop(&mut self) {
            *self.stops.lock().unwrap() += 1;
        }
    }

    type Events = Arc<Mutex<Vec<HookEvent>>>;

    fn collecting() -> (HookSink, Events) {
        let events: Events = Arc::default();
        let sink = {
            let events = events.clone();
            Arc::new(move |event| events.lock().unwrap().push(event)) as HookSink
        };
        (sink, events)
    }

    fn service() -> (HotkeyHookService<Fake>, Arc<Mutex<u32>>, Arc<Mutex<u32>>) {
        let fake = Fake::default();
        let (starts, stops) = (fake.starts.clone(), fake.stops.clone());
        (HotkeyHookService::new(fake), starts, stops)
    }

    #[test]
    fn the_hook_runs_exactly_while_there_are_shortcuts() {
        let (service, starts, stops) = service();
        let (sink, _) = collecting();
        assert!(!service.is_running());

        service
            .configure(vec![bind(1, "Super+Space")], sink.clone())
            .unwrap();
        assert!(service.is_running());
        // Reconfiguring does not reinstall it.
        service
            .configure(vec![bind(2, "Super+K")], sink.clone())
            .unwrap();
        assert_eq!((*starts.lock().unwrap(), *stops.lock().unwrap()), (1, 0));

        service.configure(Vec::new(), sink.clone()).unwrap();
        assert!(!service.is_running());
        assert_eq!((*starts.lock().unwrap(), *stops.lock().unwrap()), (1, 1));

        // Recording alone installs it, and ends with it.
        service.start_recording(0).unwrap();
        assert!(service.is_running());
        service.stop_recording().unwrap();
        assert!(!service.is_running());
        assert_eq!((*starts.lock().unwrap(), *stops.lock().unwrap()), (2, 2));
    }

    #[test]
    fn suspending_removes_the_hook_and_resuming_restores_it() {
        let (service, starts, stops) = service();
        let (sink, _) = collecting();
        service
            .configure(vec![bind(1, "Super+Space")], sink)
            .unwrap();
        service.set_suspended(true).unwrap();
        assert!(!service.is_running());
        service.set_suspended(false).unwrap();
        assert!(service.is_running());
        assert_eq!((*starts.lock().unwrap(), *stops.lock().unwrap()), (2, 1));
    }

    #[test]
    fn configuring_ends_a_suspension() {
        let (service, _, _) = service();
        let (sink, events) = collecting();
        service
            .configure(vec![bind(1, "Super+Space")], sink.clone())
            .unwrap();
        service.set_suspended(true).unwrap();
        assert_eq!(service.on_key(SPACE, true, WIN, 0), PASS);
        service
            .configure(vec![bind(1, "Super+Space")], sink)
            .unwrap();
        assert!(service.on_key(SPACE, true, WIN, 1).swallow);
        assert_eq!(*events.lock().unwrap(), [HookEvent::Pressed(1)]);
    }

    #[test]
    fn shutdown_removes_the_hook_and_forgets_the_shortcuts() {
        let (service, _, stops) = service();
        let (sink, events) = collecting();
        service
            .configure(vec![bind(1, "Super+Space")], sink)
            .unwrap();
        service.shutdown();
        assert!(!service.is_running());
        assert_eq!(*stops.lock().unwrap(), 1);
        assert_eq!(service.on_key(SPACE, true, WIN, 0), PASS);
        assert!(events.lock().unwrap().is_empty());
    }

    #[test]
    fn a_hook_that_cannot_be_installed_is_an_error() {
        let service = HotkeyHookService::new(Fake {
            fail: true,
            ..Fake::default()
        });
        let (sink, _) = collecting();
        let err = service
            .configure(vec![bind(1, "Super+Space")], sink)
            .unwrap_err();
        assert_eq!(err, "denied");
        assert!(!service.is_running());
        // A failed recording does not stay armed.
        assert!(service.start_recording(0).is_err());
        assert!(!lock(&service.core).machine.is_recording());
    }

    #[test]
    fn the_sink_hears_which_shortcut_was_pressed() {
        let (service, _, _) = service();
        let (sink, events) = collecting();
        service
            .configure(vec![bind(7, "Super+Space")], sink)
            .unwrap();
        let press = service.on_key(SPACE, true, WIN, 0);
        assert!(press.swallow);
        service.on_key(SPACE, true, WIN, 1); // repeat
        service.on_key(SPACE, false, WIN, 2);
        assert_eq!(*events.lock().unwrap(), [HookEvent::Pressed(7)]);

        service.start_recording(10).unwrap();
        service.on_key(K, true, Mods { ctrl: true, ..NONE }, 11);
        assert_eq!(
            events.lock().unwrap().last(),
            Some(&HookEvent::Recorded("Ctrl+K".to_owned()))
        );
        service.start_recording(20).unwrap();
        service.on_key(vk::ESCAPE, true, NONE, 21);
        assert_eq!(
            events.lock().unwrap().last(),
            Some(&HookEvent::RecordCancelled)
        );
    }

    #[test]
    fn injected_keys_count_only_when_the_setting_says_so() {
        // Real keys always count; injected ones only with the setting.
        assert!(event_wanted(false, false));
        assert!(event_wanted(false, true));
        assert!(!event_wanted(true, false));
        assert!(event_wanted(true, true));
        // The default refuses them. (Tests run in one process, so the global is
        // put back as found.)
        let before = accepts_injected();
        set_accept_injected(false);
        assert!(!accepts_injected());
        set_accept_injected(true);
        assert!(accepts_injected());
        set_accept_injected(before);
    }

    #[test]
    fn supported_only_where_there_is_a_backend() {
        assert_eq!(supported(), cfg!(windows));
        if !supported() {
            let (sink, _) = collecting();
            assert!(configure(Vec::new(), sink).is_err());
            assert!(start_recording().is_err());
        }
    }
}
