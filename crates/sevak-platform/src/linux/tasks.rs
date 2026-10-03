//! Linux: the apps that have a window, for the quit task.
//!
//! X11 (and XWayland) windows are found through the window manager's
//! `_NET_CLIENT_LIST`, each with the `_NET_WM_PID` of its owner. Apps that
//! run as native Wayland clients are invisible to X11, so a Wayland session
//! also gets the user's processes that were started with a display (see
//! [`graphical_by_environment`]), minus a few well-known desktop helpers.

use std::collections::HashSet;

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};
use x11rb::rust_connection::RustConnection;

use crate::error::Result;
use crate::tasks::{group_apps, RunningApp};

/// Pids of the windows the X server's window manager lists. Empty when there
/// is no X server (pure Wayland) or no window manager support.
fn x11_window_pids() -> Vec<u32> {
    let Ok((conn, screen)) = RustConnection::connect(None) else {
        return Vec::new();
    };
    let root = conn.setup().roots[screen].root;
    let atom = |name: &str| {
        conn.intern_atom(false, name.as_bytes())
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .map(|reply| reply.atom)
    };
    let (Some(client_list), Some(wm_pid)) = (atom("_NET_CLIENT_LIST"), atom("_NET_WM_PID")) else {
        return Vec::new();
    };
    let windows: Vec<u32> = conn
        .get_property(false, root, client_list, AtomEnum::WINDOW, 0, u32::MAX)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .and_then(|reply| reply.value32().map(Iterator::collect))
        .unwrap_or_default();
    windows
        .into_iter()
        .filter_map(|window| {
            conn.get_property(false, window, wm_pid, AtomEnum::CARDINAL, 0, 1)
                .ok()?
                .reply()
                .ok()?
                .value32()?
                .next()
        })
        .collect()
}

/// Process names that hold a display connection without being apps.
const DESKTOP_HELPERS: [&str; 12] = [
    "gnome-shell",
    "gnome-session",
    "gnome-keyring",
    "xdg-desktop-portal",
    "xdg-document-portal",
    "xdg-permission-store",
    "gsd-",
    "ibus",
    "at-spi",
    "dbus-",
    "xwayland",
    "kwin",
];

fn is_desktop_helper(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("gnome-shell")
        || DESKTOP_HELPERS
            .iter()
            .any(|helper| lower.starts_with(helper))
        || lower.starts_with("plasma")
        || lower.starts_with("sevak")
}

/// Pids of processes that were started with `DISPLAY` or `WAYLAND_DISPLAY` set:
/// graphical programs, plus the helpers [`is_desktop_helper`] filters out.
fn graphical_by_environment() -> HashSet<u32> {
    let own = std::process::id();
    let mut pids = HashSet::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return pids;
    };
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == own {
            continue;
        }
        // Readable only for the user's own processes, which is the point.
        let Ok(environ) = std::fs::read(entry.path().join("environ")) else {
            continue;
        };
        if environ
            .split(|b| *b == 0)
            .any(|var| var.starts_with(b"WAYLAND_DISPLAY=") || var.starts_with(b"DISPLAY="))
        {
            pids.insert(pid);
        }
    }
    pids
}

pub(crate) fn running_apps() -> Result<Vec<RunningApp>> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    let own = std::process::id();

    let mut pids: HashSet<u32> = x11_window_pids().into_iter().collect();
    if crate::session::DisplayServer::detect() == crate::session::DisplayServer::Wayland {
        pids.extend(graphical_by_environment());
    }
    pids.remove(&own);

    Ok(group_apps(pids.into_iter().filter_map(|pid| {
        let process = system.process(sysinfo::Pid::from_u32(pid))?;
        let name = process.name().to_string_lossy().into_owned();
        (!name.is_empty() && !is_desktop_helper(&name)).then_some((pid, name))
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_helpers_are_not_apps() {
        for name in [
            "gnome-shell",
            "gsd-media-keys",
            "ibus-daemon",
            "Xwayland",
            "dbus-daemon",
            "plasmashell",
            "sevak",
        ] {
            assert!(is_desktop_helper(name), "{name}");
        }
        for name in ["firefox", "code", "nautilus", "gedit"] {
            assert!(!is_desktop_helper(name), "{name}");
        }
    }

    #[test]
    fn listing_apps_is_read_only() {
        // May be empty (no display on a CI box); must not fail.
        let apps = running_apps().expect("app list");
        assert!(apps.iter().all(|app| !app.pids.is_empty()));
    }
}
