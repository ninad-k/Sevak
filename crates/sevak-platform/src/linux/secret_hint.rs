//! Linux: ask the clipboard owner whether the content is marked secret (see
//! [`crate::secret_hint`]).
//!
//! - **X11**: the CLIPBOARD selection's `TARGETS` are requested (a conversion
//!   into a property of a small unmapped window of ours) and, if the hint is
//!   among them, its value is requested the same way. Each request waits at
//!   most [`REPLY_TIMEOUT`] for the owner.
//! - **Wayland**: only if `wl-paste` (wl-clipboard) is installed, it is run
//!   without a shell, with a timeout, to list the types and to read the hint.
//!   Its answer is remembered for as long as the text is the same, because a
//!   process start per poll would be too much.
//! - Anything else, or a failure: not marked (best effort).

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::thread::sleep;
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, Window, WindowClass,
};
use x11rb::protocol::Event;
use x11rb::{COPY_FROM_PARENT, CURRENT_TIME};

use crate::secret_hint::{self, ClipboardTargets, HINT_TARGET};
use crate::session::DisplayServer;

use super::paste::X;

/// How long the selection owner gets to answer one request.
const REPLY_TIMEOUT: Duration = Duration::from_millis(150);
/// How long a `wl-paste` run may take.
const WL_PASTE_TIMEOUT: Duration = Duration::from_millis(400);
/// The most output read from `wl-paste` (the hint and the type list are tiny).
const WL_PASTE_MAX_OUTPUT: u64 = 16 * 1024;
/// The most data read back for a target over X11.
const MAX_TARGET_BYTES: u32 = 4096;

/// True if the content on the clipboard, whose text is `text`, is marked
/// secret by the app that copied it. Never fails: when nothing can be asked the
/// answer is "not marked".
pub(crate) fn clipboard_marked_secret(text: Option<&str>) -> bool {
    match DisplayServer::detect() {
        DisplayServer::X11 => {
            X11Selection::open().is_some_and(|selection| secret_hint::marked_secret(&selection))
        }
        DisplayServer::Wayland => text.is_some_and(wayland_marked_secret),
        _ => false,
    }
}

/// The CLIPBOARD selection of an X server.
struct X11Selection {
    x: X,
    window: Window,
    clipboard: Atom,
    property: Atom,
}

impl X11Selection {
    fn open() -> Option<Self> {
        let x = X::connect().ok()?;
        let clipboard = x
            .conn
            .intern_atom(false, b"CLIPBOARD")
            .ok()?
            .reply()
            .ok()?
            .atom;
        let property = x
            .conn
            .intern_atom(false, b"SEVAK_CLIPBOARD_PROBE")
            .ok()?
            .reply()
            .ok()?
            .atom;
        let window = x.conn.generate_id().ok()?;
        x.conn
            .create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                window,
                x.root,
                0,
                0,
                1,
                1,
                0,
                WindowClass::INPUT_OUTPUT,
                COPY_FROM_PARENT,
                &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
            )
            .ok()?;
        Some(Self {
            x,
            window,
            clipboard,
            property,
        })
    }

    /// Asks the selection owner to convert the selection to `target` and
    /// returns the property it wrote: `(type, format, data)`. `None` if the
    /// owner refuses, is slow, or the answer is not a plain property.
    fn convert(&self, target: Atom) -> Option<(Atom, u8, Vec<u8>)> {
        let conn = &self.x.conn;
        conn.convert_selection(
            self.window,
            self.clipboard,
            target,
            self.property,
            CURRENT_TIME,
        )
        .ok()?;
        conn.flush().ok()?;

        let deadline = Instant::now() + REPLY_TIMEOUT;
        loop {
            match conn.poll_for_event().ok()? {
                Some(Event::SelectionNotify(event)) if event.requestor == self.window => {
                    if event.property == x11rb::NONE {
                        return None;
                    }
                    let reply = conn
                        .get_property(
                            true,
                            self.window,
                            event.property,
                            AtomEnum::ANY,
                            0,
                            MAX_TARGET_BYTES / 4,
                        )
                        .ok()?
                        .reply()
                        .ok()?;
                    // A transfer in pieces (INCR) is never a hint or a target list.
                    if reply.bytes_after != 0 {
                        return None;
                    }
                    return Some((reply.type_, reply.format, reply.value));
                }
                Some(_) => {}
                None => {
                    if Instant::now() >= deadline {
                        return None;
                    }
                    sleep(Duration::from_millis(2));
                }
            }
        }
    }

    fn atom_named(&self, name: &str) -> Option<Atom> {
        let atom = self
            .x
            .conn
            .intern_atom(true, name.as_bytes())
            .ok()?
            .reply()
            .ok()?
            .atom;
        (atom != x11rb::NONE).then_some(atom)
    }
}

impl Drop for X11Selection {
    fn drop(&mut self) {
        let _ = self.x.conn.destroy_window(self.window);
        let _ = self.x.conn.flush();
    }
}

impl ClipboardTargets for X11Selection {
    fn has_target(&self, target: &str) -> bool {
        // An atom nobody ever created cannot be a target.
        let Some(wanted) = self.atom_named(target) else {
            return false;
        };
        let Some(targets) = self.atom_named("TARGETS") else {
            return false;
        };
        let Some((_, 32, data)) = self.convert(targets) else {
            return false;
        };
        data.as_chunks::<4>()
            .0
            .iter()
            .any(|chunk| Atom::from_ne_bytes(*chunk) == wanted)
    }

    fn read_target(&self, target: &str) -> Option<Vec<u8>> {
        let atom = self.atom_named(target)?;
        self.convert(atom).map(|(_, _, data)| data)
    }
}

/// The last text `wl-paste` was asked about and the answer.
static WAYLAND_ANSWER: Mutex<Option<(u64, bool)>> = Mutex::new(None);

fn wayland_marked_secret(text: &str) -> bool {
    let Some(program) = wl_paste() else {
        return false;
    };
    let key = fingerprint(text);
    let mut remembered = WAYLAND_ANSWER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((known, answer)) = *remembered {
        if known == key {
            return answer;
        }
    }
    let answer = secret_hint::marked_secret(&WlPaste { program });
    *remembered = Some((key, answer));
    answer
}

fn fingerprint(text: &str) -> u64 {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// `wl-paste`, if wl-clipboard is installed (looked for once).
fn wl_paste() -> Option<PathBuf> {
    static PROGRAM: OnceLock<Option<PathBuf>> = OnceLock::new();
    PROGRAM
        .get_or_init(|| crate::process::find_in_path("wl-paste"))
        .clone()
}

struct WlPaste {
    program: PathBuf,
}

impl WlPaste {
    /// Runs `wl-paste` with `args` and returns its output, or `None` if it fails
    /// or does not finish in time.
    fn run(&self, args: &[&str]) -> Option<Vec<u8>> {
        let mut command = Command::new(&self.program);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        crate::process::configure_helper_command(&mut command);
        let mut child = command.spawn().ok()?;

        let deadline = Instant::now() + WL_PASTE_TIMEOUT;
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => break,
                Ok(Some(_)) | Err(_) => return None,
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    tracing::debug!("wl-paste did not answer in time");
                    return None;
                }
                Ok(None) => sleep(Duration::from_millis(5)),
            }
        }
        let mut output = Vec::new();
        child
            .stdout
            .take()?
            .take(WL_PASTE_MAX_OUTPUT)
            .read_to_end(&mut output)
            .ok()?;
        Some(output)
    }
}

impl ClipboardTargets for WlPaste {
    fn has_target(&self, target: &str) -> bool {
        debug_assert_eq!(target, HINT_TARGET);
        self.run(&["--list-types"])
            .is_some_and(|out| secret_hint::lists_hint(&String::from_utf8_lossy(&out)))
    }

    fn read_target(&self, target: &str) -> Option<Vec<u8>> {
        self.run(&["--no-newline", "--type", target])
    }
}
