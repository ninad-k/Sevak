//! Windows: the one low-level keyboard hook (`WH_KEYBOARD_LL`) and the thread
//! that serves it, shared by everything in Sevak that watches the keyboard.
//!
//! Two features use it, each through its own module:
//!
//! - the global hotkeys (`keyhook_hotkey`): recognises the configured shortcuts
//!   that `RegisterHotKey` cannot take (Win+Space, keys another app owns) and
//!   *swallows* them;
//! - snippet expansion (`keyhook_expand`, opt-in): watches typing and the mouse
//!   (`WH_MOUSE_LL`, installed only while expansion runs) and never swallows.
//!
//! The hook is installed while at least one of them is a [`Client`] and removed
//! when the last one leaves. A low-level hook is called through the message
//! loop of the thread that installed it, so the hook has a thread of its own.
//! The hook procedure has to be quick (Windows skips or drops a hook that
//! exceeds `LowLevelHooksTimeout`), so both features only translate the event
//! and send it down a channel.
//!
//! Events Sevak injects itself carry [`OWN_EXTRA_INFO`] and are passed straight
//! on, unseen by both features.
//!
//! Windows can remove a low-level hook without a word (a callback that was too
//! slow too often), and a lock, a remote-desktop switch or a display change can
//! leave one dead. So the hook thread puts the hooks in again every minute, and
//! soon after such an event, with the schedule of [`crate::hook_watchdog`]: see
//! [`Hooks`].

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Mutex, MutexGuard};
use std::thread::JoinHandle;

use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_RCONTROL, VK_RMENU, VK_RSHIFT,
    VK_RWIN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, KillTimer, PeekMessageW, PostThreadMessageW,
    SetTimer, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, HC_ACTION, HHOOK,
    KBDLLHOOKSTRUCT, MSG, PM_NOREMOVE, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_APP, WM_QUIT, WM_TIMER,
    WM_USER,
};

use crate::hook_watchdog::{HookInstaller, HookWatchdog, Tick};
use crate::hotkey_hook::Mods;

use super::keyhook_watch::{self, Watch};
use super::{keyhook_expand, keyhook_hotkey};

/// How often the hook thread looks at the watchdog's schedule.
const WATCHDOG_TICK_MS: u32 = 1_000;

/// Stamped on every key event Sevak injects (`dwExtraInfo`), so its own
/// Backspaces, paste and mask keys are never mistaken for the user's.
pub(super) const OWN_EXTRA_INFO: usize = 0x5345_5641;

/// Who is using the hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Client {
    Hotkeys,
    Expansion,
}

struct HookThread {
    id: u32,
    handle: JoinHandle<()>,
}

#[derive(Default)]
struct Host {
    hotkeys: bool,
    expansion: bool,
    thread: Option<HookThread>,
}

impl Host {
    fn set(&mut self, client: Client, on: bool) {
        match client {
            Client::Hotkeys => self.hotkeys = on,
            Client::Expansion => self.expansion = on,
        }
    }

    fn any(&self) -> bool {
        self.hotkeys || self.expansion
    }
}

static HOST: Mutex<Host> = Mutex::new(Host {
    hotkeys: false,
    expansion: false,
    thread: None,
});

/// Whether the mouse hook should be installed (while snippet expansion runs).
static WANT_MOUSE: AtomicBool = AtomicBool::new(false);

/// Posted to the hook thread to make its mouse hook match [`WANT_MOUSE`].
const WM_SYNC_MOUSE: u32 = WM_APP + 1;

/// What the hook thread reports once it has tried to install the hook.
type Started = std::result::Result<u32, String>;

fn host() -> MutexGuard<'static, Host> {
    HOST.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Puts `client` on the hook, installing it (on its own thread) if it was not
/// installed yet.
pub(super) fn acquire(client: Client) -> Result<(), String> {
    let mut host = host();
    host.set(client, true);
    if client == Client::Expansion {
        WANT_MOUSE.store(true, Ordering::SeqCst);
    }
    if let Some(thread) = &host.thread {
        if client == Client::Expansion {
            sync_mouse(thread.id);
        }
        return Ok(());
    }
    match spawn() {
        Ok(thread) => {
            host.thread = Some(thread);
            Ok(())
        }
        Err(message) => {
            host.set(client, false);
            WANT_MOUSE.store(host.expansion, Ordering::SeqCst);
            Err(message)
        }
    }
}

/// Takes `client` off the hook; the hook goes when nobody is left.
pub(super) fn release(client: Client) {
    let mut host = host();
    host.set(client, false);
    if client == Client::Expansion {
        WANT_MOUSE.store(false, Ordering::SeqCst);
    }
    if host.any() {
        if let (Client::Expansion, Some(thread)) = (client, &host.thread) {
            sync_mouse(thread.id);
        }
        return;
    }
    if let Some(thread) = host.thread.take() {
        stop(thread);
    }
}

fn spawn() -> Result<HookThread, String> {
    let (started_tx, started_rx) = mpsc::channel::<Started>();
    let handle = std::thread::Builder::new()
        .name("sevak-keyhook".to_owned())
        .spawn(move || hook_thread(&started_tx))
        .map_err(|err| err.to_string())?;
    match started_rx.recv() {
        Ok(Ok(id)) => Ok(HookThread { id, handle }),
        Ok(Err(message)) => {
            let _ = handle.join();
            Err(message)
        }
        Err(_) => {
            let _ = handle.join();
            Err("the hook thread ended unexpectedly".to_owned())
        }
    }
}

fn sync_mouse(thread_id: u32) {
    // SAFETY: plain Win32 call; the thread has a message queue (it made one
    // before reporting its id).
    unsafe {
        let _ = PostThreadMessageW(thread_id, WM_SYNC_MOUSE, WPARAM(0), LPARAM(0));
    }
}

fn stop(thread: HookThread) {
    // SAFETY: plain Win32 call; the thread has a message queue and WM_QUIT ends
    // its loop.
    unsafe {
        let _ = PostThreadMessageW(thread.id, WM_QUIT, WPARAM(0), LPARAM(0));
    }
    let _ = thread.handle.join();
}

/// The modifier keys the OS says are down right now.
pub(super) fn current_mods() -> Mods {
    let down = |key: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY| {
        // SAFETY: plain Win32 call taking a virtual-key code; the top bit of
        // the result is "currently down".
        let state = unsafe { GetAsyncKeyState(i32::from(key.0)) };
        state as u16 & 0x8000 != 0
    };
    Mods {
        ctrl: down(VK_LCONTROL) || down(VK_RCONTROL),
        alt: down(VK_LMENU) || down(VK_RMENU),
        shift: down(VK_LSHIFT) || down(VK_RSHIFT),
        win: down(VK_LWIN) || down(VK_RWIN),
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && lparam.0 != 0 {
        // SAFETY: for HC_ACTION, `lparam` points to a KBDLLHOOKSTRUCT that is
        // valid for the duration of this call.
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let message = wparam.0 as u32;
        // A panic must not unwind into Windows; a key is passed on if one happens.
        let swallow = catch_unwind(AssertUnwindSafe(|| {
            // Sevak's own injected keys are nobody's business here.
            if info.dwExtraInfo == OWN_EXTRA_INFO {
                return false;
            }
            if keyhook_hotkey::on_key(message, info) {
                return true;
            }
            keyhook_expand::on_key(message, info);
            false
        }))
        .unwrap_or(false);
        if swallow {
            return LRESULT(1);
        }
    }
    // SAFETY: forwarding the arguments we were given.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            keyhook_expand::on_mouse(wparam.0 as u32)
        }));
    }
    // SAFETY: forwarding the arguments we were given.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Makes the mouse hook exist exactly when [`WANT_MOUSE`] says. Clicks only make
/// snippet expansion forget what was typed; without the hook a click that moves
/// the caret would go unnoticed, so a failure is only logged.
///
/// # Safety
/// Must be called on the hook thread; `module` is the executable's handle.
unsafe fn apply_mouse(mouse: &mut Option<HHOOK>, module: Option<HINSTANCE>) {
    let wanted = WANT_MOUSE.load(Ordering::SeqCst);
    match (wanted, mouse.is_some()) {
        (true, false) => {
            // SAFETY: installing a hook whose procedure lives as long as the process.
            *mouse = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0) }.ok();
            if mouse.is_none() {
                tracing::warn!(
                    "the mouse hook could not be installed; clicks will not reset typing"
                );
            }
        }
        (false, true) => {
            if let Some(hook) = mouse.take() {
                // SAFETY: `hook` came from SetWindowsHookExW on this thread.
                let _ = unsafe { UnhookWindowsHookEx(hook) };
            }
        }
        _ => {}
    }
}

/// The hooks of the hook thread, and how to put them in again.
struct Hooks {
    keyboard: HHOOK,
    mouse: Option<HHOOK>,
    module: Option<HINSTANCE>,
}

impl HookInstaller for Hooks {
    /// Installs fresh hooks first and removes the old ones after, so there is
    /// never a moment without one, and a failure leaves the old ones working.
    /// Both calls happen on the hook thread with no message pumped in between,
    /// so no key event is delivered to two copies of the hook.
    fn reinstall(&mut self) -> Result<(), String> {
        // SAFETY: hook installation on the hook thread, which owns these hooks
        // and removes each handle exactly once.
        unsafe {
            let keyboard = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), self.module, 0)
                .map_err(|err| err.to_string())?;
            let _ = UnhookWindowsHookEx(self.keyboard);
            self.keyboard = keyboard;

            if self.mouse.is_some() {
                // If the new mouse hook cannot be made the old one stays.
                if let Ok(mouse) = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), self.module, 0)
                {
                    if let Some(old) = self.mouse.replace(mouse) {
                        let _ = UnhookWindowsHookEx(old);
                    }
                }
            }
        }
        Ok(())
    }
}

fn hook_thread(started: &mpsc::Sender<Started>) {
    // SAFETY: Win32 hook installation and the message loop that serves it, all
    // on this thread; the hooks are removed before the thread ends.
    unsafe {
        let module = GetModuleHandleW(None)
            .ok()
            .map(|module| HINSTANCE(module.0));
        let keyboard = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0) {
            Ok(hook) => hook,
            Err(err) => {
                let _ = started.send(Err(err.to_string()));
                return;
            }
        };
        let mut mouse = None;
        apply_mouse(&mut mouse, module);

        // A thread gets its message queue on first use; `stop` needs it.
        let mut message = MSG::default();
        let _ = PeekMessageW(&mut message, None, WM_USER, WM_USER, PM_NOREMOVE);

        // The watchdog: a once-a-second tick to look at its schedule, and a
        // hidden window that hears about unlocks, display changes and wake-ups.
        let watch = Watch::open(module);
        let timer = SetTimer(None, 0, WATCHDOG_TICK_MS, None);
        let mut watchdog = HookWatchdog::new(
            Hooks {
                keyboard,
                mouse,
                module,
            },
            keyhook_hotkey::clock_ms(),
        );
        let _ = started.send(Ok(GetCurrentThreadId()));

        while GetMessageW(&mut message, None, 0, 0).0 > 0 {
            if message.message == WM_SYNC_MOUSE {
                apply_mouse(&mut watchdog.installer_mut().mouse, module);
                continue;
            }
            let is_tick = timer != 0 && message.message == WM_TIMER && message.wParam.0 == timer;
            if !is_tick {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            // Sent messages (a display change) are served inside GetMessageW,
            // so the flags are read after every message, and the tick makes
            // sure the loop wakes up to read them.
            let now = keyhook_hotkey::clock_ms();
            for reason in keyhook_watch::take_events() {
                watchdog.note(reason, now);
            }
            if is_tick {
                match watchdog.tick(now) {
                    Tick::Idle | Tick::StillFailing => {}
                    Tick::Reinstalled(reason) => {
                        tracing::debug!(?reason, "the keyboard hook was put in again");
                    }
                    Tick::Failed {
                        reason,
                        message: error,
                    } => {
                        tracing::warn!(?reason, %error, "could not put the keyboard hook in again");
                    }
                }
            }
        }

        if timer != 0 {
            let _ = KillTimer(None, timer);
        }
        watch.close();
        let hooks = watchdog.into_installer();
        let _ = UnhookWindowsHookEx(hooks.keyboard);
        if let Some(mouse) = hooks.mouse {
            let _ = UnhookWindowsHookEx(mouse);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hook_stays_while_any_client_is_left() {
        let mut host = Host::default();
        assert!(!host.any());
        host.set(Client::Hotkeys, true);
        host.set(Client::Expansion, true);
        host.set(Client::Hotkeys, false);
        assert!(host.any(), "expansion still needs it");
        host.set(Client::Expansion, false);
        assert!(!host.any());
    }

    #[test]
    fn sevaks_own_keys_carry_the_stamp_expansion_always_used() {
        // "SEVA" in ASCII: changing it would make Sevak mistake its own
        // Backspaces and paste for typing.
        assert_eq!(OWN_EXTRA_INFO, 0x5345_5641);
    }
}
