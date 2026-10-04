//! Linux: list, focus, move and resize windows on X11 through EWMH, and read
//! the monitors with RandR.
//!
//! # What is used
//!
//! - **Windows**: `_NET_CLIENT_LIST_STACKING` (top of the stack last, so it is
//!   reversed to put the most recently used first), `_NET_WM_NAME`,
//!   `_NET_WM_WINDOW_TYPE` and `_NET_WM_STATE` to leave out panels, desktops and
//!   skip-taskbar windows.
//! - **Focus**: a `_NET_ACTIVE_WINDOW` client message from a "pager", which
//!   window managers honour (and which also un-minimizes and switches desktop).
//! - **Placement**: `_NET_MOVERESIZE_WINDOW` with static gravity, so the numbers
//!   are those of the client area; `_NET_FRAME_EXTENTS` (the decorations the
//!   window manager draws) and `_GTK_FRAME_EXTENTS` (the transparent shadow of
//!   client-side-decorated GTK windows) convert between that and the visible
//!   rectangle. A maximized or fullscreen window is un-maximized first through
//!   `_NET_WM_STATE`. A window manager without `_NET_MOVERESIZE_WINDOW` gets a
//!   plain `ConfigureWindow`.
//! - **Monitors**: RandR 1.5 `GetMonitors`. X11 has no per-monitor work area; it
//!   is derived from the panels' `_NET_WM_STRUT_PARTIAL`, with `_NET_WORKAREA`
//!   as the fallback (see [`crate::window_manager::x11_geometry`]).
//!
//! # Wayland
//!
//! A Wayland client can neither list nor move other applications' windows, and
//! no compositor-neutral protocol exists for it, so [`window_support`] says so
//! and nothing here runs. (GNOME Shell and KWin have their own scripting
//! interfaces; Sevak does not use them.)

use std::thread::sleep;
use std::time::{Duration, Instant};

use sevak_core::window_layout::{Insets, Monitor, Rect};
use x11rb::connection::Connection;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ClientMessageEvent, ConfigureWindowAux, ConnectionExt as _, EventMask, Window,
};

use super::paste::{remembered_window, x_error, X};
use crate::error::{PlatformError, Result};
use crate::session::DisplayServer;
use crate::window_manager::x11_geometry::{
    choose_work_area, client_rect_for, is_switchable_type, moveresize_data, visible_rect,
    work_area_from_struts, Strut,
};
use crate::window_manager::{WindowId, WindowInfo, WindowState, WindowSupport};

/// How long to wait for the window manager to carry out a request.
const SETTLE: Duration = Duration::from_millis(400);
const POLL: Duration = Duration::from_millis(10);
/// Pixels a placement may be off and still count as done (window managers and
/// clients round to size increments).
const TOLERANCE: i32 = 2;
/// Most windows read from the client list.
const MAX_WINDOWS: u32 = 4096;

const WAYLAND_REASON: &str = "Window management does not work on Wayland: the protocol lets an app neither list nor move other apps' windows. Use your desktop's own tiling shortcuts, or log in to an X11 session";

pub(crate) fn window_support() -> WindowSupport {
    match DisplayServer::detect() {
        DisplayServer::X11 => {}
        DisplayServer::Wayland => return WindowSupport::Unavailable(WAYLAND_REASON.to_owned()),
        _ => {
            return WindowSupport::Unavailable(
                "Window management needs a graphical X11 session".to_owned(),
            )
        }
    }
    match X::connect() {
        Ok(x) if x.has_window_manager() => WindowSupport::Available,
        Ok(_) => WindowSupport::Unavailable(
            "Your window manager does not support the EWMH standard Sevak uses to move windows"
                .to_owned(),
        ),
        Err(_) => WindowSupport::Unavailable("Cannot reach the X server".to_owned()),
    }
}

/// An X11 failure as a platform error.
fn xe(err: impl std::fmt::Display) -> PlatformError {
    x_error("X11", err)
}

/// The cardinals (or window ids, atoms) of one property; empty when absent.
fn values(x: &X, window: Window, property: &str, kind: AtomEnum, max: u32) -> Vec<u32> {
    let Ok(atom) = x.atom(property) else {
        return Vec::new();
    };
    x.conn
        .get_property(false, window, atom, kind, 0, max)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .and_then(|reply| reply.value32().map(Iterator::collect))
        .unwrap_or_default()
}

fn extents(x: &X, window: Window, property: &str) -> Insets {
    // Left, right, top, bottom.
    match values(x, window, property, AtomEnum::CARDINAL, 4)[..] {
        [left, right, top, bottom] => Insets {
            left: i32::try_from(left).unwrap_or(0),
            right: i32::try_from(right).unwrap_or(0),
            top: i32::try_from(top).unwrap_or(0),
            bottom: i32::try_from(bottom).unwrap_or(0),
        },
        _ => Insets::ZERO,
    }
}

impl X {
    fn has_window_manager(&self) -> bool {
        !values(
            self,
            self.root,
            "_NET_SUPPORTING_WM_CHECK",
            AtomEnum::WINDOW,
            1,
        )
        .is_empty()
    }

    fn atom_names(&self, atoms: &[u32]) -> Vec<String> {
        atoms
            .iter()
            .filter_map(|atom| {
                let reply = self.conn.get_atom_name(*atom).ok()?.reply().ok()?;
                String::from_utf8(reply.name).ok()
            })
            .collect()
    }

    fn wm_state(&self, window: Window) -> Vec<String> {
        let atoms = values(self, window, "_NET_WM_STATE", AtomEnum::ATOM, 32);
        self.atom_names(&atoms)
    }

    fn window_types(&self, window: Window) -> Vec<String> {
        let atoms = values(self, window, "_NET_WM_WINDOW_TYPE", AtomEnum::ATOM, 32);
        self.atom_names(&atoms)
    }

    /// `_NET_WM_NAME` (UTF-8), else the legacy `WM_NAME`.
    fn title(&self, window: Window) -> String {
        let read = |property: Atom, kind: Atom| -> Option<String> {
            let reply = self
                .conn
                .get_property(false, window, property, kind, 0, 512)
                .ok()?
                .reply()
                .ok()?;
            let text = String::from_utf8_lossy(&reply.value).into_owned();
            (!text.is_empty()).then_some(text)
        };
        let utf8 = self.atom("UTF8_STRING").ok();
        let net_name = self.atom("_NET_WM_NAME").ok();
        if let (Some(kind), Some(property)) = (utf8, net_name) {
            if let Some(title) = read(property, kind) {
                return title;
            }
        }
        read(AtomEnum::WM_NAME.into(), AtomEnum::STRING.into()).unwrap_or_default()
    }

    /// Sends a client message to the root window, as a pager does.
    fn send_to_root(&self, window: Window, message: &str, data: [u32; 5]) -> Result<()> {
        let atom = self.atom(message)?;
        let event = ClientMessageEvent::new(32, window, atom, data);
        self.conn
            .send_event(
                false,
                self.root,
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                event,
            )
            .map_err(xe)?;
        self.conn.flush().map_err(xe)
    }

    /// The window's client area in root coordinates.
    fn client_rect(&self, window: Window) -> Result<Rect> {
        let geometry = self
            .conn
            .get_geometry(window)
            .map_err(xe)?
            .reply()
            .map_err(|_| PlatformError::Message("That window has closed".to_owned()))?;
        let origin = self
            .conn
            .translate_coordinates(window, self.root, 0, 0)
            .map_err(xe)?
            .reply()
            .map_err(xe)?;
        Ok(Rect::new(
            i32::from(origin.dst_x),
            i32::from(origin.dst_y),
            i32::from(geometry.width),
            i32::from(geometry.height),
        ))
    }

    /// The decorations and the shadow around the client area.
    fn frame(&self, window: Window) -> (Insets, Insets) {
        (
            extents(self, window, "_NET_FRAME_EXTENTS"),
            extents(self, window, "_GTK_FRAME_EXTENTS"),
        )
    }

    fn visible(&self, window: Window) -> Result<Rect> {
        let client = self.client_rect(window)?;
        let (frame, shadow) = self.frame(window);
        Ok(visible_rect(client, frame, shadow))
    }

    fn supports(&self, hint: &str) -> bool {
        let (Ok(wanted), supported) = (
            self.atom(hint),
            values(self, self.root, "_NET_SUPPORTED", AtomEnum::ATOM, 4096),
        ) else {
            return false;
        };
        supported.contains(&wanted)
    }

    fn state_of(&self, window: Window) -> Result<WindowState> {
        let id = window_id(window)?;
        let state = self.wm_state(window);
        let has = |name: &str| state.iter().any(|entry| entry == name);
        Ok(WindowState {
            id,
            title: self.title(window),
            app: self.app_of(window).map(|app| app.name).unwrap_or_default(),
            rect: self.visible(window)?,
            maximized: (has("_NET_WM_STATE_MAXIMIZED_HORZ") && has("_NET_WM_STATE_MAXIMIZED_VERT"))
                || has("_NET_WM_STATE_FULLSCREEN"),
            minimized: has("_NET_WM_STATE_HIDDEN"),
        })
    }

    /// Asks the window manager to focus `window` (which also un-minimizes it
    /// and switches to its desktop), and waits for it.
    fn activate(&self, window: Window) -> Result<()> {
        // Source indication 2 is "pager": honoured although Sevak has no focus.
        self.send_to_root(window, "_NET_ACTIVE_WINDOW", [2, 0, 0, 0, 0])
    }

    /// Leaves maximized and fullscreen, and un-minimizes, waiting until done.
    fn normalize(&self, window: Window) -> Result<()> {
        let state = self.wm_state(window);
        let has = |name: &str| state.iter().any(|entry| entry == name);
        if has("_NET_WM_STATE_HIDDEN") {
            self.activate(window)?;
        }
        let fullscreen = has("_NET_WM_STATE_FULLSCREEN");
        let maximized = has("_NET_WM_STATE_MAXIMIZED_HORZ") || has("_NET_WM_STATE_MAXIMIZED_VERT");
        if !(fullscreen || maximized || has("_NET_WM_STATE_HIDDEN")) {
            return Ok(());
        }
        const REMOVE: u32 = 0;
        const SOURCE_PAGER: u32 = 2;
        if fullscreen {
            let fullscreen = self.atom("_NET_WM_STATE_FULLSCREEN")?;
            self.send_to_root(
                window,
                "_NET_WM_STATE",
                [REMOVE, fullscreen, 0, SOURCE_PAGER, 0],
            )?;
        }
        if maximized {
            let horizontal = self.atom("_NET_WM_STATE_MAXIMIZED_HORZ")?;
            let vertical = self.atom("_NET_WM_STATE_MAXIMIZED_VERT")?;
            self.send_to_root(
                window,
                "_NET_WM_STATE",
                [REMOVE, horizontal, vertical, SOURCE_PAGER, 0],
            )?;
        }
        let deadline = Instant::now() + SETTLE;
        while Instant::now() < deadline {
            let now = self.wm_state(window);
            let still = now.iter().any(|entry| {
                matches!(
                    entry.as_str(),
                    "_NET_WM_STATE_MAXIMIZED_HORZ"
                        | "_NET_WM_STATE_MAXIMIZED_VERT"
                        | "_NET_WM_STATE_FULLSCREEN"
                        | "_NET_WM_STATE_HIDDEN"
                )
            });
            if !still {
                break;
            }
            sleep(POLL);
        }
        Ok(())
    }

    /// Puts the window's client area at `client`.
    fn place(&self, window: Window, client: Rect) -> Result<()> {
        if self.supports("_NET_MOVERESIZE_WINDOW") {
            self.send_to_root(window, "_NET_MOVERESIZE_WINDOW", moveresize_data(client))
        } else {
            let aux = ConfigureWindowAux::new()
                .x(client.x)
                .y(client.y)
                .width(u32::try_from(client.width.max(1)).unwrap_or(1))
                .height(u32::try_from(client.height.max(1)).unwrap_or(1));
            self.conn.configure_window(window, &aux).map_err(xe)?;
            self.conn.flush().map_err(xe)
        }
    }
}

fn window_id(window: Window) -> Result<WindowId> {
    WindowId::new(&format!("x11:{window}"))
        .ok_or_else(|| PlatformError::Message("invalid window id".to_owned()))
}

fn window_of(id: &WindowId) -> Result<Window> {
    id.as_str()
        .strip_prefix("x11:")
        .and_then(|number| number.parse::<u32>().ok())
        .filter(|window| *window != 0)
        .ok_or_else(|| PlatformError::Message(format!("{id} is not a window of this system")))
}

pub(crate) fn list_windows() -> Result<Vec<WindowInfo>> {
    let x = X::connect()?;
    let mut windows = values(
        &x,
        x.root,
        "_NET_CLIENT_LIST_STACKING",
        AtomEnum::WINDOW,
        MAX_WINDOWS,
    );
    if windows.is_empty() {
        windows = values(
            &x,
            x.root,
            "_NET_CLIENT_LIST",
            AtomEnum::WINDOW,
            MAX_WINDOWS,
        );
    } else {
        // Bottom to top: most recently used first.
        windows.reverse();
    }
    let mut listed = Vec::new();
    for window in windows {
        if x.is_own(window) {
            continue;
        }
        let types = x.window_types(window);
        let type_names: Vec<&str> = types.iter().map(String::as_str).collect();
        if !is_switchable_type(&type_names) {
            continue;
        }
        let state = x.wm_state(window);
        if state
            .iter()
            .any(|entry| entry == "_NET_WM_STATE_SKIP_TASKBAR")
        {
            continue;
        }
        let title = x.title(window);
        if title.is_empty() {
            continue;
        }
        let Ok(id) = window_id(window) else { continue };
        listed.push(WindowInfo {
            id,
            title,
            app: x.app_of(window).map(|app| app.name).unwrap_or_default(),
            minimized: state.iter().any(|entry| entry == "_NET_WM_STATE_HIDDEN"),
        });
    }
    Ok(listed)
}

pub(crate) fn focus_window(id: &WindowId) -> Result<()> {
    let window = window_of(id)?;
    let x = X::connect()?;
    x.client_rect(window)?;
    x.activate(window)?;
    let deadline = Instant::now() + SETTLE;
    while x.active_window() != Some(window) {
        if Instant::now() >= deadline {
            // Some window managers refuse focus (focus-stealing prevention) but
            // still raise the window; that is the best that can be done.
            break;
        }
        sleep(POLL);
    }
    Ok(())
}

pub(crate) fn target_window() -> Result<WindowState> {
    let window = remembered_window().ok_or_else(|| {
        PlatformError::Message("There is no previous window to arrange".to_owned())
    })?;
    let x = X::connect()?;
    let types = x.window_types(window);
    let type_names: Vec<&str> = types.iter().map(String::as_str).collect();
    if !is_switchable_type(&type_names) {
        return Err(PlatformError::Message(
            "That window (the desktop or a panel) cannot be arranged".to_owned(),
        ));
    }
    x.state_of(window)
}

pub(crate) fn window_state(id: &WindowId) -> Result<WindowState> {
    X::connect()?.state_of(window_of(id)?)
}

pub(crate) fn set_window_rect(id: &WindowId, visible: Rect) -> Result<()> {
    let window = window_of(id)?;
    let x = X::connect()?;
    x.client_rect(window)?;
    x.normalize(window)?;
    // Twice at most: the decorations can change when a window leaves the
    // maximized state, so the extents are read again for the second try.
    for _ in 0..2 {
        let (frame, shadow) = x.frame(window);
        x.place(window, client_rect_for(visible, frame, shadow))?;
        let deadline = Instant::now() + SETTLE;
        loop {
            if x.visible(window)
                .is_ok_and(|now| now.approx_eq(visible, TOLERANCE))
            {
                return Ok(());
            }
            if Instant::now() >= deadline {
                break;
            }
            sleep(POLL);
        }
    }
    // The window manager or the client limited the size (a terminal snaps to
    // its character grid, a dialog has a fixed size); that is not an error.
    Ok(())
}

pub(crate) fn list_monitors() -> Result<Vec<Monitor>> {
    let x = X::connect()?;
    let screen = x
        .conn
        .get_geometry(x.root)
        .map_err(xe)?
        .reply()
        .map_err(xe)?;
    let screen_rect = Rect::new(0, 0, i32::from(screen.width), i32::from(screen.height));

    let mut bounds: Vec<(String, Rect, bool)> = Vec::new();
    if let Ok(cookie) = x.conn.randr_get_monitors(x.root, true) {
        if let Ok(reply) = cookie.reply() {
            for monitor in reply.monitors {
                let name = x
                    .conn
                    .get_atom_name(monitor.name)
                    .ok()
                    .and_then(|cookie| cookie.reply().ok())
                    .and_then(|reply| String::from_utf8(reply.name).ok())
                    .unwrap_or_default();
                let rect = Rect::new(
                    i32::from(monitor.x),
                    i32::from(monitor.y),
                    i32::from(monitor.width),
                    i32::from(monitor.height),
                );
                if !rect.is_empty() {
                    bounds.push((name, rect, monitor.primary));
                }
            }
        }
    }
    if bounds.is_empty() {
        bounds.push(("screen".to_owned(), screen_rect, true));
    }

    let struts = collect_struts(&x);
    let net_workarea = current_workarea(&x);
    Ok(bounds
        .into_iter()
        .map(|(name, rect, primary)| {
            let from_struts = work_area_from_struts(rect, screen_rect, &struts);
            Monitor {
                name,
                bounds: rect,
                work_area: choose_work_area(rect, from_struts, net_workarea),
                // X11 has no per-monitor scale; sizes are real pixels.
                scale_percent: 100,
                primary,
            }
        })
        .collect())
}

/// The struts of every client window (panels, docks).
fn collect_struts(x: &X) -> Vec<Strut> {
    values(x, x.root, "_NET_CLIENT_LIST", AtomEnum::WINDOW, MAX_WINDOWS)
        .into_iter()
        .filter_map(|window| {
            let partial = values(x, window, "_NET_WM_STRUT_PARTIAL", AtomEnum::CARDINAL, 12);
            Strut::from_partial(&partial)
                .or_else(|| {
                    let basic = values(x, window, "_NET_WM_STRUT", AtomEnum::CARDINAL, 4);
                    Strut::from_basic(&basic)
                })
                .filter(|strut| !strut.is_empty())
        })
        .collect()
}

/// `_NET_WORKAREA` of the current desktop.
fn current_workarea(x: &X) -> Option<Rect> {
    let desktop = values(x, x.root, "_NET_CURRENT_DESKTOP", AtomEnum::CARDINAL, 1)
        .first()
        .copied()
        .unwrap_or(0) as usize;
    let all = values(x, x.root, "_NET_WORKAREA", AtomEnum::CARDINAL, 4096);
    let area = all.get(desktop * 4..desktop * 4 + 4)?;
    // The property is signed in practice (a monitor left of the origin) although
    // declared CARDINAL; reinterpret the bits.
    let signed = |value: u32| value as i32;
    let rect = Rect::new(
        signed(area[0]),
        signed(area[1]),
        signed(area[2]),
        signed(area[3]),
    );
    (!rect.is_empty()).then_some(rect)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_ids_name_an_x11_window_and_nothing_else() {
        let id = window_id(0x3a0_0004).unwrap();
        assert_eq!(id.as_str(), "x11:60817412");
        assert_eq!(window_of(&id).unwrap(), 0x3a0_0004);
        for bad in [
            "x11:0",
            "x11:",
            "x11:-5",
            "x11:abc",
            "77",
            "wayland:5",
            "x11:99999999999",
        ] {
            let id = WindowId::new(bad).unwrap();
            assert!(window_of(&id).is_err(), "{bad}");
        }
    }

    #[test]
    fn there_is_a_reason_for_wayland() {
        assert!(WAYLAND_REASON.contains("Wayland"));
        assert!(WAYLAND_REASON.contains("X11"));
    }
}
