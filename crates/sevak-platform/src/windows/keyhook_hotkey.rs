//! Windows: what the global hotkeys take from the shared keyboard hook (see
//! [`keyhook`](super::keyhook)).
//!
//! The decisions are made by the pure [`HookMachine`](crate::hotkey_hook); this
//! module only gives it key events, swallows the ones it says to, and injects
//! the "mask" key it asks for.
//!
//! The mask: Windows opens the Start menu when the Windows key is released with
//! no other key pressed in between, and an app's menu bar when Alt is. Sevak
//! swallowed the other key (Space), so on the release of Win or Alt it first
//! taps an unassigned key (`VK_E8`), which makes the OS treat the modifier as
//! used. That tap is stamped with [`OWN_EXTRA_INFO`], so Sevak's own hook does
//! not mistake it for the user's.
//!
//! Nothing about a key is logged.

use std::sync::OnceLock;
use std::time::Instant;

use windows::Win32::UI::Input::KeyboardAndMouse::{INPUT, VIRTUAL_KEY};
use windows::Win32::UI::WindowsAndMessaging::{
    KBDLLHOOKSTRUCT, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::hotkey_hook::{vk, HookBackend, HotkeyHookService};

use super::keyhook::{self, Client, OWN_EXTRA_INFO};
use super::paste::{key_input, send_inputs};

/// Installs and removes the shared hook for the global hotkeys.
pub(crate) struct WinBackend;

impl HookBackend for WinBackend {
    fn start(&mut self) -> Result<(), String> {
        keyhook::acquire(Client::Hotkeys)
    }

    fn stop(&mut self) {
        keyhook::release(Client::Hotkeys);
    }
}

static SERVICE: OnceLock<HotkeyHookService<WinBackend>> = OnceLock::new();

pub(crate) fn service() -> &'static HotkeyHookService<WinBackend> {
    SERVICE.get_or_init(|| HotkeyHookService::new(WinBackend))
}

static CLOCK: OnceLock<Instant> = OnceLock::new();

/// Milliseconds on a monotonic clock, for the state machine's timeouts.
pub(crate) fn clock_ms() -> u64 {
    let start = CLOCK.get_or_init(Instant::now);
    u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Whether a key event is a press, a release, or neither.
fn press_or_release(message: u32) -> Option<bool> {
    match message {
        WM_KEYDOWN | WM_SYSKEYDOWN => Some(true),
        WM_KEYUP | WM_SYSKEYUP => Some(false),
        _ => None,
    }
}

/// Called by the shared hook for each event that is not Sevak's own. Returns
/// whether the event is to be swallowed.
pub(super) fn on_key(message: u32, info: &KBDLLHOOKSTRUCT) -> bool {
    let Some(down) = press_or_release(message) else {
        return false;
    };
    // Not running (no service yet) means nothing to watch for.
    let Some(service) = SERVICE.get() else {
        return false;
    };
    let outcome = service.on_key(info.vkCode, down, keyhook::current_mods(), clock_ms());
    if outcome.inject_mask {
        send_mask();
    }
    outcome.swallow
}

/// Taps the mask key, stamped as Sevak's own.
fn send_mask() {
    let mut inputs: [INPUT; 2] = [
        key_input(VIRTUAL_KEY(vk::MASK as u16), false),
        key_input(VIRTUAL_KEY(vk::MASK as u16), true),
    ];
    for input in &mut inputs {
        // `key_input` builds a keyboard event, so `ki` is the active field.
        input.Anonymous.ki.dwExtraInfo = OWN_EXTRA_INFO;
    }
    if let Err(err) = send_inputs(&inputs) {
        tracing::debug!(%err, "could not send the mask key");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_key_events_are_read() {
        assert_eq!(press_or_release(WM_KEYDOWN), Some(true));
        assert_eq!(press_or_release(WM_SYSKEYDOWN), Some(true));
        assert_eq!(press_or_release(WM_KEYUP), Some(false));
        assert_eq!(press_or_release(WM_SYSKEYUP), Some(false));
        assert_eq!(press_or_release(0x0200), None);
    }

    #[test]
    fn the_mask_is_an_unassigned_key_stamped_as_sevaks_own() {
        assert_eq!(vk::MASK, 0xE8);
        // `send_mask` stamps both events; checked without sending anything by
        // building the same inputs.
        let input = key_input(VIRTUAL_KEY(vk::MASK as u16), false);
        // SAFETY: `key_input` builds a keyboard event.
        assert_eq!(unsafe { input.Anonymous.ki.wVk }, VIRTUAL_KEY(0xE8));
    }

    #[test]
    fn the_clock_does_not_go_backwards() {
        let first = clock_ms();
        assert!(clock_ms() >= first);
    }
}
