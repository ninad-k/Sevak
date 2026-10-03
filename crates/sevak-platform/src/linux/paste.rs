//! Linux (X11): remember the active window through `_NET_ACTIVE_WINDOW`,
//! refocus it with the same EWMH request a pager sends, and press Ctrl+V with
//! the XTest extension.
//!
//! Wayland offers applications no way to read the focused window or to inject
//! key presses, so there Sevak only copies.

use std::sync::{Mutex, OnceLock};
use std::thread::sleep;
use std::time::{Duration, Instant};

use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::xproto::{
    self, Atom, AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask, Window,
};
use x11rb::protocol::xtest::{self, ConnectionExt as _};
use x11rb::rust_connection::RustConnection;

use crate::error::{PlatformError, Result};
use crate::paste::{self, ForegroundApp, PasteDriver, PasteOutcome, PasteSupport, SystemClipboard};
use crate::session::DisplayServer;

/// The window that was active when Sevak was shown.
static REMEMBERED: Mutex<Option<Window>> = Mutex::new(None);

const FOCUS_TIMEOUT: Duration = Duration::from_millis(500);

/// X11 keysyms for the keys that make up the shortcut.
const KEYSYM_V: u32 = 0x76;
const KEYSYM_CONTROL_L: u32 = 0xffe3;
/// Keycodes of `v` and Left Ctrl on a standard PC layout, used only if the
/// keyboard mapping cannot be searched.
const FALLBACK_KEYCODE_V: u8 = 55;
const FALLBACK_KEYCODE_CONTROL_L: u8 = 37;

pub(super) fn x_error(operation: &'static str, err: impl std::fmt::Display) -> PlatformError {
    PlatformError::Os {
        operation,
        message: err.to_string(),
    }
}

pub(super) struct X {
    pub(super) conn: RustConnection,
    pub(super) root: Window,
}

impl X {
    pub(super) fn connect() -> Result<Self> {
        let (conn, screen) = RustConnection::connect(None).map_err(|e| x_error("X11", e))?;
        let root = conn.setup().roots[screen].root;
        Ok(Self { conn, root })
    }

    fn atom(&self, name: &str) -> Result<Atom> {
        self.conn
            .intern_atom(false, name.as_bytes())
            .map_err(|e| x_error("X11", e))?
            .reply()
            .map(|reply| reply.atom)
            .map_err(|e| x_error("X11", e))
    }

    pub(super) fn active_window(&self) -> Option<Window> {
        let atom = self.atom("_NET_ACTIVE_WINDOW").ok()?;
        let reply = self
            .conn
            .get_property(false, self.root, atom, AtomEnum::WINDOW, 0, 1)
            .ok()?
            .reply()
            .ok()?;
        let window = reply.value32()?.next();
        window.filter(|window| *window != 0)
    }

    fn window_pid(&self, window: Window) -> Option<u32> {
        let atom = self.atom("_NET_WM_PID").ok()?;
        let reply = self
            .conn
            .get_property(false, window, atom, AtomEnum::CARDINAL, 0, 1)
            .ok()?
            .reply()
            .ok()?;
        let pid = reply.value32()?.next();
        pid
    }

    pub(super) fn app_of(&self, window: Window) -> Option<ForegroundApp> {
        let reply = self
            .conn
            .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)
            .ok()?
            .reply()
            .ok()?;
        // "instance\0Class\0"
        let mut parts = reply
            .value
            .split(|byte| *byte == 0)
            .map(|part| String::from_utf8_lossy(part).into_owned());
        let instance = parts.next().unwrap_or_default();
        let class = parts.next().unwrap_or_default();
        let program = self
            .window_pid(window)
            .and_then(|pid| std::fs::read_to_string(format!("/proc/{pid}/comm")).ok())
            .map(|comm| comm.trim().to_owned())
            .unwrap_or_default();

        let name = [&class, &program, &instance]
            .into_iter()
            .find(|name| !name.is_empty())?
            .clone();
        Some(
            ForegroundApp::new(name)
                .with_identifier(class)
                .with_identifier(program)
                .with_identifier(instance),
        )
    }

    pub(super) fn is_own(&self, window: Window) -> bool {
        self.window_pid(window) == Some(std::process::id())
    }

    /// The keycode that produces `keysym`, from the current keyboard mapping.
    pub(super) fn keycode_for(&self, keysym: u32) -> Option<u8> {
        let setup = self.conn.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let reply = self
            .conn
            .get_keyboard_mapping(min, max - min + 1)
            .ok()?
            .reply()
            .ok()?;
        let per_keycode = usize::from(reply.keysyms_per_keycode).max(1);
        reply
            .keysyms
            .chunks(per_keycode)
            .position(|keysyms| keysyms.contains(&keysym))
            .and_then(|index| u8::try_from(index).ok())
            .map(|index| min + index)
    }

    pub(super) fn fake_key(&self, keycode: u8, press: bool) -> Result<()> {
        let kind = if press {
            xproto::KEY_PRESS_EVENT
        } else {
            xproto::KEY_RELEASE_EVENT
        };
        self.conn
            .xtest_fake_input(kind, keycode, 0, self.root, 0, 0, 0)
            .map_err(|e| x_error("XTest", e))?;
        Ok(())
    }
}

fn session_is_x11() -> bool {
    DisplayServer::detect() == DisplayServer::X11
}

pub(crate) fn remember_foreground_app() {
    if !session_is_x11() {
        return;
    }
    let Ok(x) = X::connect() else { return };
    let active = x.active_window();
    if active.is_some_and(|window| x.is_own(window)) {
        return;
    }
    *REMEMBERED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = active;
}

pub(crate) fn foreground_app() -> Option<ForegroundApp> {
    if !session_is_x11() {
        return None;
    }
    let x = X::connect().ok()?;
    let window = x.active_window()?;
    x.app_of(window)
}

pub(crate) fn paste_support() -> PasteSupport {
    static SUPPORT: OnceLock<PasteSupport> = OnceLock::new();
    SUPPORT.get_or_init(detect_support).clone()
}

fn detect_support() -> PasteSupport {
    match DisplayServer::detect() {
        DisplayServer::X11 => {}
        DisplayServer::Wayland => {
            return PasteSupport::CopyOnly(
                "Pasting into other apps is not possible on Wayland".to_owned(),
            )
        }
        _ => return PasteSupport::CopyOnly(paste::UNSUPPORTED_REASON.to_owned()),
    }
    let Ok(x) = X::connect() else {
        return PasteSupport::CopyOnly("Cannot reach the X server".to_owned());
    };
    match x.conn.extension_information(xtest::X11_EXTENSION_NAME) {
        Ok(Some(_)) => PasteSupport::Available,
        _ => PasteSupport::CopyOnly("The X server has no XTest extension".to_owned()),
    }
}

pub(crate) fn paste_text(text: &str, restore_clipboard: bool) -> Result<PasteOutcome> {
    if let PasteSupport::CopyOnly(reason) = paste_support() {
        crate::clipboard::set_text(text)?;
        return Ok(PasteOutcome::CopiedOnly(reason));
    }
    paste::paste(text, restore_clipboard, &SystemClipboard, &X11Driver, true)
}

struct X11Driver;

impl PasteDriver for X11Driver {
    fn focus_previous(&self) -> std::result::Result<(), String> {
        let x = X::connect().map_err(|e| e.to_string())?;
        let Some(target) = *REMEMBERED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
        else {
            return Err("There is no previous window to paste into".to_owned());
        };
        if x.active_window() != Some(target) {
            let atom = x.atom("_NET_ACTIVE_WINDOW").map_err(|e| e.to_string())?;
            // Source indication 2 is "pager": the window manager honours it
            // even though Sevak no longer has focus.
            let event = ClientMessageEvent::new(32, target, atom, [2u32, 0, 0, 0, 0]);
            x.conn
                .send_event(
                    false,
                    x.root,
                    EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                    event,
                )
                .map_err(|e| e.to_string())?;
            x.conn.flush().map_err(|e| e.to_string())?;
        }

        let deadline = Instant::now() + FOCUS_TIMEOUT;
        while x.active_window() != Some(target) {
            if Instant::now() >= deadline {
                return Err("Could not return to the previous window".to_owned());
            }
            sleep(Duration::from_millis(10));
        }
        Ok(())
    }

    fn press_paste(&self) -> Result<()> {
        let x = X::connect()?;
        let control = x
            .keycode_for(KEYSYM_CONTROL_L)
            .unwrap_or(FALLBACK_KEYCODE_CONTROL_L);
        let v = x.keycode_for(KEYSYM_V).unwrap_or(FALLBACK_KEYCODE_V);
        x.fake_key(control, true)?;
        x.fake_key(v, true)?;
        x.fake_key(v, false)?;
        x.fake_key(control, false)?;
        // A round trip, so every event above has been processed before we return.
        x.conn
            .get_input_focus()
            .map_err(|e| x_error("X11", e))?
            .reply()
            .map_err(|e| x_error("X11", e))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_an_x_server_pasting_is_copy_only() {
        // CI has no display; a developer machine may. Either way this must not
        // panic, and a session that is not X11 must never claim support.
        let support = paste_support();
        if !session_is_x11() {
            assert!(!support.is_available());
        }
    }
}
