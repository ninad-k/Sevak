//! Linux (X11): watch typing with the RECORD extension, replace typed text with
//! XTest Backspaces and a Ctrl+V.
//!
//! Wayland lets applications neither read other apps' key presses nor inject
//! their own, so there the feature reports itself unavailable.
//!
//! RECORD is the X server's own "tell me about input" facility (it is what
//! `xinput test-xi2` style tools and screen-reader helpers use). It is passive:
//! it sees events without grabbing or changing them.
//!
//! Like the other backends this turns raw input into [`KeyEvent`]s and nothing
//! else: a key is reported as the character the current keyboard mapping gives
//! it, and anything that is not plain typing is a [`KeyEvent::Reset`]. Dead keys
//! are not followed (they reset). Nothing about a key is logged.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{sleep, JoinHandle};
use std::time::{Duration, Instant};

use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::record::{self, ConnectionExt as _, Range, Range8, CS};
use x11rb::protocol::xproto::{ConnectionExt as _, Window};
use x11rb::rust_connection::RustConnection;

use crate::error::{PlatformError, Result};
use crate::expand::{self, ExpandDriver, SystemExpandClipboard};
use crate::keyboard::{KeyEvent, KeyListener, KeyListenerSupport, KeySink, TypingTarget};
use crate::session::DisplayServer;

use super::capture::wait_for_modifier_release;
use super::paste::{x_error, X};

const KEYSYM_BACKSPACE: u32 = 0xff08;
const KEYSYM_CONTROL_L: u32 = 0xffe3;
const KEYSYM_V: u32 = 0x76;
const FALLBACK_KEYCODE_BACKSPACE: u8 = 22;
const FALLBACK_KEYCODE_CONTROL_L: u8 = 37;
const FALLBACK_KEYCODE_V: u8 = 55;

/// Core protocol event codes the recording asks for.
const KEY_PRESS: u8 = 2;
const BUTTON_PRESS: u8 = 4;
/// Size of a core protocol event on the wire.
const EVENT_LEN: usize = 32;

/// `state` bits (core protocol modifier mask).
const SHIFT: u16 = 1 << 0;
const LOCK: u16 = 1 << 1;
const CONTROL: u16 = 1 << 2;
const MOD1: u16 = 1 << 3;
const MOD2: u16 = 1 << 4;
const MOD4: u16 = 1 << 6;
const MOD5: u16 = 1 << 7;

/// How long the keyboard mapping is reused before it is read again (layouts can
/// be switched at runtime).
const MAPPING_TTL: Duration = Duration::from_secs(1);

pub(crate) fn key_listener_support() -> KeyListenerSupport {
    match DisplayServer::detect() {
        DisplayServer::X11 => {}
        DisplayServer::Wayland => {
            return KeyListenerSupport::Unavailable(
                "Wayland does not let apps watch typing in other apps".to_owned(),
            )
        }
        _ => {
            return KeyListenerSupport::Unavailable(
                "Watching typing needs an X11 session".to_owned(),
            )
        }
    }
    let Ok(x) = X::connect() else {
        return KeyListenerSupport::Unavailable("Cannot reach the X server".to_owned());
    };
    match x.conn.extension_information(record::X11_EXTENSION_NAME) {
        Ok(Some(_)) => KeyListenerSupport::Available,
        _ => KeyListenerSupport::Unavailable("The X server has no RECORD extension".to_owned()),
    }
}

pub(crate) fn start(sink: KeySink) -> Result<KeyListener> {
    if let KeyListenerSupport::Unavailable(reason) | KeyListenerSupport::NeedsPermission(reason) =
        key_listener_support()
    {
        return Err(PlatformError::Os {
            operation: "RECORD",
            message: reason,
        });
    }

    // One connection to give the orders (create, and later stop, the recording)
    // and one that is taken over by the stream of recorded events.
    let control = X::connect()?;
    let (data, _) = RustConnection::connect(None).map_err(|e| x_error("X11", e))?;

    let context = control.conn.generate_id().map_err(|e| x_error("X11", e))?;
    let range = Range {
        device_events: Range8 {
            first: KEY_PRESS,
            last: BUTTON_PRESS,
        },
        ..Range::default()
    };
    let everyone = u32::from(u8::from(CS::ALL_CLIENTS));
    control
        .conn
        .record_create_context(context, 0, &[everyone], &[range])
        .map_err(|e| x_error("RECORD", e))?
        .check()
        .map_err(|e| x_error("RECORD", e))?;

    let done = Arc::new(AtomicBool::new(false));
    let thread_done = done.clone();
    let spawned = std::thread::Builder::new()
        .name("sevak-record".to_owned())
        .spawn(move || {
            record_loop(&data, context, sink);
            thread_done.store(true, Ordering::SeqCst);
        });
    let handle = match spawned {
        Ok(handle) => handle,
        Err(err) => {
            let _ = control.conn.record_free_context(context);
            let _ = control.conn.flush();
            return Err(PlatformError::Io(err));
        }
    };

    Ok(KeyListener::new(move || {
        stop(&control, context, handle, &done);
    }))
}

fn stop(control: &X, context: record::Context, handle: JoinHandle<()>, done: &AtomicBool) {
    // Ends the stream of replies on the other connection.
    let _ = control.conn.record_disable_context(context);
    let _ = control.conn.flush();
    // The loop only ends once the server has sent the end-of-data reply.
    let deadline = Instant::now() + Duration::from_secs(2);
    while !done.load(Ordering::SeqCst) && Instant::now() < deadline {
        sleep(Duration::from_millis(10));
    }
    if done.load(Ordering::SeqCst) {
        let _ = handle.join();
    }
    let _ = control.conn.record_free_context(context);
    let _ = control.conn.flush();
}

/// Reads the recorded events until the recording is disabled.
fn record_loop(data: &RustConnection, context: record::Context, sink: KeySink) {
    let Ok(mut watcher) = Watcher::new(sink) else {
        tracing::warn!("typing cannot be followed: no second X connection");
        return;
    };
    let Ok(stream) = data.record_enable_context(context) else {
        return;
    };
    for reply in stream {
        let Ok(reply) = reply else { break };
        // Category 0 is "from server": a run of 32-byte core events.
        if reply.category != 0 {
            continue;
        }
        for event in reply.data.as_chunks::<EVENT_LEN>().0 {
            watcher.on_event(event);
        }
    }
}

/// Turns recorded events into [`KeyEvent`]s.
struct Watcher {
    sink: KeySink,
    x: X,
    mapping: Mapping,
    mapping_read: Instant,
    active_window: Option<Window>,
}

impl Watcher {
    fn new(sink: KeySink) -> Result<Self> {
        let x = X::connect()?;
        let mapping = Mapping::read(&x).unwrap_or_default();
        let active_window = x.active_window();
        Ok(Self {
            sink,
            x,
            mapping,
            mapping_read: Instant::now(),
            active_window,
        })
    }

    fn emit(&self, event: KeyEvent) {
        (self.sink)(event);
    }

    fn on_event(&mut self, bytes: &[u8]) {
        // Bit 7 marks events other programs sent with SendEvent; XTest input
        // (Sevak's own) arrives as an ordinary event, so what we inject is
        // told apart by the expansion's own bookkeeping instead (it wipes the
        // buffer after each expansion).
        let kind = bytes[0] & 0x7f;
        match kind {
            BUTTON_PRESS => self.emit(KeyEvent::Reset),
            KEY_PRESS => {
                let state = u16::from_ne_bytes([bytes[28], bytes[29]]);
                self.on_key(bytes[1], state);
            }
            _ => {}
        }
    }

    fn on_key(&mut self, keycode: u8, state: u16) {
        let active = self.x.active_window();
        if active != self.active_window {
            self.active_window = active;
            self.emit(KeyEvent::Reset);
        }
        if self.mapping_read.elapsed() >= MAPPING_TTL {
            if let Some(mapping) = Mapping::read(&self.x) {
                self.mapping = mapping;
            }
            self.mapping_read = Instant::now();
        }
        let keysym = self.mapping.keysym(keycode, state);
        match classify(keysym, state) {
            Class::Ignore => {}
            Class::Reset => self.emit(KeyEvent::Reset),
            Class::Backspace => self.emit(KeyEvent::Backspace),
            Class::Char(c) => self.emit(KeyEvent::Char(c)),
        }
    }
}

/// The keyboard mapping: which keysyms each keycode types.
#[derive(Default)]
struct Mapping {
    min_keycode: u8,
    per_keycode: usize,
    keysyms: Vec<u32>,
}

impl Mapping {
    fn read(x: &X) -> Option<Self> {
        let setup = x.conn.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let reply = x
            .conn
            .get_keyboard_mapping(min, max - min + 1)
            .ok()?
            .reply()
            .ok()?;
        Some(Self {
            min_keycode: min,
            per_keycode: usize::from(reply.keysyms_per_keycode).max(1),
            keysyms: reply.keysyms,
        })
    }

    /// The keysym `keycode` types with the modifiers in `state`.
    fn keysym(&self, keycode: u8, state: u16) -> u32 {
        let Some(index) = keycode.checked_sub(self.min_keycode) else {
            return 0;
        };
        let start = usize::from(index) * self.per_keycode;
        let Some(syms) = self.keysyms.get(start..start + self.per_keycode) else {
            return 0;
        };
        pick_keysym(syms, state)
    }
}

/// Chooses among the keysyms of one key: columns are group 1 level 1 and 2,
/// group 2 level 1 and 2, then levels 3 and 4 of group 1 (AltGr).
fn pick_keysym(syms: &[u32], state: u16) -> u32 {
    let at = |column: usize| syms.get(column).copied().unwrap_or(0);
    let group = usize::from((state >> 13) & 0x3);
    let level3 = state & MOD5 != 0;
    let shifted = state & SHIFT != 0;
    let base = if level3 && syms.len() > 4 {
        4
    } else {
        (group * 2).min(2)
    };
    let (lower, upper) = (at(base), at(base + 1));
    let lower = if lower == 0 { at(0) } else { lower };

    // Keypad digits follow Num Lock, not Shift.
    if (0xff80..=0xffbf).contains(&lower) {
        let numlock = state & MOD2 != 0;
        return if numlock != shifted && upper != 0 {
            upper
        } else {
            lower
        };
    }

    let letter = char::from_u32(lower).filter(|c| c.is_alphabetic());
    let caps = state & LOCK != 0 && letter.is_some();
    match (shifted != caps, upper) {
        (false, _) => lower,
        // Shift on a key with only one keysym: letters have an upper case.
        (true, 0) => letter
            .and_then(|c| c.to_uppercase().next())
            .map_or(lower, |c| c as u32),
        (true, upper) => upper,
    }
}

enum Class {
    /// A modifier or lock key by itself.
    Ignore,
    Reset,
    Backspace,
    Char(char),
}

fn classify(keysym: u32, state: u16) -> Class {
    match keysym {
        // Shift, Control, Caps Lock, Meta/Alt, Super, Hyper and the level-3
        // and mode switches: pressed alone they type nothing and change nothing.
        0xffe1..=0xffee | 0xfe01..=0xfe13 | 0xff7e | 0xff7f => return Class::Ignore,
        _ => {}
    }
    // Control, Alt and Super make shortcuts (AltGr is Mod5, which types).
    if state & (CONTROL | MOD1 | MOD4) != 0 {
        return Class::Reset;
    }
    if keysym == KEYSYM_BACKSPACE {
        return Class::Backspace;
    }
    match keysym_to_char(keysym) {
        Some(c) if !c.is_control() => Class::Char(c),
        _ => Class::Reset,
    }
}

/// The character a keysym types, for the keysyms that type one.
fn keysym_to_char(keysym: u32) -> Option<char> {
    match keysym {
        0x20..=0x7e | 0xa0..=0xff => char::from_u32(keysym),
        // Unicode keysyms.
        0x0100_0000..=0x0110_ffff => char::from_u32(keysym - 0x0100_0000),
        // Keypad: space, equals, operators, decimal and digits.
        0xff80 => Some(' '),
        0xffbd => Some('='),
        0xffaa..=0xffb9 => char::from_u32(keysym - 0xffaa + u32::from(b'*')),
        _ => None,
    }
}

pub(crate) fn typing_target() -> TypingTarget {
    let Ok(x) = X::connect() else {
        return TypingTarget::default();
    };
    let Some(window) = x.active_window() else {
        return TypingTarget::default();
    };
    TypingTarget {
        app: x.app_of(window),
        own_window: x.is_own(window),
        // X11 offers no way to ask whether the focused field hides its input.
        private: false,
    }
}

pub(crate) fn replace_typed_text(
    delete: usize,
    text: &str,
    still_current: &dyn Fn() -> bool,
) -> Result<bool> {
    expand::replace_typed_text(
        delete,
        text,
        still_current,
        &SystemExpandClipboard,
        &X11Expand,
        true,
    )
}

struct X11Expand;

impl ExpandDriver for X11Expand {
    fn foreground_token(&self) -> Option<u64> {
        let x = X::connect().ok()?;
        x.active_window().map(u64::from)
    }

    fn release_modifiers(&self) {
        wait_for_modifier_release();
    }

    fn press_backspaces(&self, count: usize) -> Result<()> {
        let x = X::connect()?;
        let key = x
            .keycode_for(KEYSYM_BACKSPACE)
            .unwrap_or(FALLBACK_KEYCODE_BACKSPACE);
        for _ in 0..count {
            x.fake_key(key, true)?;
            x.fake_key(key, false)?;
        }
        // A round trip, so every event above has been processed before we return.
        x.conn
            .get_input_focus()
            .map_err(|e| x_error("X11", e))?
            .reply()
            .map_err(|e| x_error("X11", e))?;
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

    // US layout columns for a few keys: [lower, upper].
    const A: [u32; 2] = [0x61, 0x41];
    const ONE: [u32; 2] = [0x31, 0x21];
    // A German-style key with AltGr levels: e E (group 2) (group 2) euro cent.
    const E_ALTGR: [u32; 6] = [0x65, 0x45, 0, 0, 0x20ac, 0xa2];
    const KP_1: [u32; 2] = [0xff9c, 0xffb1]; // KP_End, KP_1

    #[test]
    fn plain_and_shifted_keys() {
        assert_eq!(pick_keysym(&A, 0), 0x61);
        assert_eq!(pick_keysym(&A, SHIFT), 0x41);
        assert_eq!(pick_keysym(&ONE, SHIFT), 0x21);
    }

    #[test]
    fn caps_lock_flips_letters_but_not_digits() {
        assert_eq!(pick_keysym(&A, LOCK), 0x41);
        assert_eq!(pick_keysym(&A, LOCK | SHIFT), 0x61);
        assert_eq!(pick_keysym(&ONE, LOCK), 0x31);
    }

    #[test]
    fn a_key_with_one_keysym_still_has_an_upper_case() {
        assert_eq!(pick_keysym(&[0x61], SHIFT), 0x41);
        assert_eq!(pick_keysym(&[0x61, 0], SHIFT), 0x41);
        assert_eq!(pick_keysym(&[0x31], SHIFT), 0x31);
    }

    #[test]
    fn altgr_selects_the_third_level() {
        assert_eq!(pick_keysym(&E_ALTGR, MOD5), 0x20ac);
        assert_eq!(pick_keysym(&E_ALTGR, MOD5 | SHIFT), 0xa2);
        // A key without a third level falls back to the first.
        assert_eq!(pick_keysym(&A, MOD5), 0x61);
    }

    #[test]
    fn the_keypad_follows_num_lock() {
        assert_eq!(pick_keysym(&KP_1, MOD2), 0xffb1);
        assert_eq!(pick_keysym(&KP_1, 0), 0xff9c);
        assert_eq!(pick_keysym(&KP_1, MOD2 | SHIFT), 0xff9c);
    }

    #[test]
    fn keysyms_become_characters() {
        assert_eq!(keysym_to_char(0x61), Some('a'));
        assert_eq!(keysym_to_char(0x20), Some(' '));
        assert_eq!(keysym_to_char(0xe9), Some('é'));
        assert_eq!(keysym_to_char(0x0100_20ac), Some('€'));
        assert_eq!(keysym_to_char(0xffb1), Some('1'));
        assert_eq!(keysym_to_char(0xffaa), Some('*'));
        assert_eq!(keysym_to_char(0xff0d), None); // Return
    }

    fn class(keysym: u32, state: u16) -> &'static str {
        match classify(keysym, state) {
            Class::Ignore => "ignore",
            Class::Reset => "reset",
            Class::Backspace => "backspace",
            Class::Char(_) => "char",
        }
    }

    #[test]
    fn typing_is_told_from_everything_else() {
        assert_eq!(class(0x61, 0), "char");
        assert_eq!(class(0x41, SHIFT), "char");
        assert_eq!(class(0x20, 0), "char");
        assert_eq!(class(KEYSYM_BACKSPACE, 0), "backspace");
        assert_eq!(class(KEYSYM_BACKSPACE, CONTROL), "reset");
        assert_eq!(class(0xff0d, 0), "reset"); // Return
        assert_eq!(class(0xff1b, 0), "reset"); // Escape
        assert_eq!(class(0xff09, 0), "reset"); // Tab
        assert_eq!(class(0xff51, 0), "reset"); // Left
        assert_eq!(class(0xffff, 0), "reset"); // Delete
        assert_eq!(class(0, 0), "reset");
        assert_eq!(class(0xfe50, 0), "reset"); // a dead key
    }

    #[test]
    fn shortcuts_reset_and_modifiers_alone_are_ignored() {
        assert_eq!(class(0x63, CONTROL), "reset");
        assert_eq!(class(0x63, MOD1), "reset");
        assert_eq!(class(0x63, MOD4), "reset");
        assert_eq!(class(0xffe1, 0), "ignore"); // Shift_L
        assert_eq!(class(0xffe3, 0), "ignore"); // Control_L
        assert_eq!(class(0xffe5, 0), "ignore"); // Caps_Lock
        assert_eq!(class(0xfe03, 0), "ignore"); // ISO_Level3_Shift
                                                // AltGr + e types a euro sign.
        assert_eq!(class(0x0100_20ac, MOD5), "char");
    }

    #[test]
    fn without_an_x_server_the_listener_is_not_available() {
        // CI has no display; a developer machine may. Either way this must not
        // panic, and a session that is not X11 must never claim support.
        let support = key_listener_support();
        if DisplayServer::detect() != DisplayServer::X11 {
            assert!(!support.is_available());
        }
    }
}
