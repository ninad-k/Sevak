//! Display-server detection and the hotkey strategy that follows from it.

use std::env;
use std::sync::atomic::{AtomicBool, Ordering};

/// The windowing system Sevak is running under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayServer {
    Windows,
    MacOS,
    X11,
    Wayland,
    /// Linux without a recognizable graphical session (e.g. started from a TTY).
    Unknown,
}

/// How the show/hide shortcut is delivered to Sevak.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyStrategy {
    /// Sevak grabs the key itself (Windows `RegisterHotKey`, macOS Carbon
    /// hotkeys, X11 `XGrabKey`).
    InApp,
    /// The desktop environment owns the key and runs `sevak --toggle`.
    External,
}

impl DisplayServer {
    /// Detects the current display server from the process environment.
    pub fn detect() -> Self {
        if cfg!(windows) {
            return Self::Windows;
        }
        if cfg!(target_os = "macos") {
            return Self::MacOS;
        }
        let session_type = env::var("XDG_SESSION_TYPE").ok();
        Self::classify(
            session_type.as_deref(),
            env_is_set("WAYLAND_DISPLAY"),
            env_is_set("DISPLAY"),
        )
    }

    /// `XDG_SESSION_TYPE` is authoritative when it names a graphical session.
    /// Otherwise (unset, `tty`, ...) fall back to whichever display socket the
    /// environment exposes, preferring Wayland.
    pub fn classify(session_type: Option<&str>, wayland_display: bool, x_display: bool) -> Self {
        match session_type.map(str::to_ascii_lowercase).as_deref() {
            Some("wayland") => Self::Wayland,
            Some("x11") => Self::X11,
            _ if wayland_display => Self::Wayland,
            _ if x_display => Self::X11,
            _ => Self::Unknown,
        }
    }

    pub fn hotkey_strategy(self) -> HotkeyStrategy {
        match self {
            Self::Wayland => HotkeyStrategy::External,
            // Unknown: attempt an in-app grab; failure is logged, not fatal.
            Self::Windows | Self::MacOS | Self::X11 | Self::Unknown => HotkeyStrategy::InApp,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::MacOS => "macos",
            Self::X11 => "x11",
            Self::Wayland => "wayland",
            Self::Unknown => "unknown",
        }
    }
}

/// Set once [`prefer_xwayland`] has put `GDK_BACKEND=x11` into Sevak's own
/// environment (as opposed to the user having set it).
static XWAYLAND_FORCED: AtomicBool = AtomicBool::new(false);

/// Whether Sevak itself set `GDK_BACKEND=x11` to run under XWayland.
///
/// Children must not inherit that variable, or every application launched from
/// Sevak would be pushed onto XWayland too. The spawn helpers in
/// [`crate::process`] remove it when this returns true. A `GDK_BACKEND` the user
/// exported themselves is left alone.
pub fn xwayland_forced() -> bool {
    XWAYLAND_FORCED.load(Ordering::Relaxed)
}

/// On Wayland, asks GTK to use its X11 backend (XWayland) unless the user has
/// already chosen a backend via `GDK_BACKEND`.
///
/// Native Wayland clients cannot place their own windows or read the global
/// cursor position, and compositors may refuse them focus when they are shown
/// from the background. A launcher needs all three. Returns the backend that
/// was forced, if any.
///
/// Must be called before the GUI toolkit initializes and before any other
/// thread is spawned, because it mutates the process environment.
pub fn prefer_xwayland(display: DisplayServer, enabled: bool) -> Option<&'static str> {
    if !cfg!(target_os = "linux") || display != DisplayServer::Wayland || !enabled {
        return None;
    }
    if env_is_set("GDK_BACKEND") {
        return None;
    }
    env::set_var("GDK_BACKEND", "x11");
    XWAYLAND_FORCED.store(true, Ordering::Relaxed);
    Some("x11")
}

/// The desktop environment names from `XDG_CURRENT_DESKTOP`
/// (e.g. `ubuntu:GNOME` becomes `["ubuntu", "GNOME"]`).
pub fn current_desktops() -> Vec<String> {
    env::var("XDG_CURRENT_DESKTOP")
        .map(|value| {
            value
                .split(':')
                .filter(|part| !part.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

pub fn is_gnome() -> bool {
    current_desktops()
        .iter()
        .any(|desktop| desktop.eq_ignore_ascii_case("gnome"))
}

fn env_is_set(name: &str) -> bool {
    env::var_os(name).is_some_and(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_type_wins() {
        assert_eq!(
            DisplayServer::classify(Some("wayland"), false, true),
            DisplayServer::Wayland
        );
        assert_eq!(
            DisplayServer::classify(Some("x11"), true, true),
            DisplayServer::X11
        );
        assert_eq!(
            DisplayServer::classify(Some("Wayland"), false, false),
            DisplayServer::Wayland
        );
    }

    #[test]
    fn falls_back_to_display_sockets() {
        assert_eq!(
            DisplayServer::classify(Some("tty"), true, true),
            DisplayServer::Wayland
        );
        assert_eq!(
            DisplayServer::classify(None, false, true),
            DisplayServer::X11
        );
        assert_eq!(
            DisplayServer::classify(None, false, false),
            DisplayServer::Unknown
        );
    }

    #[test]
    fn only_wayland_uses_an_external_hotkey() {
        assert_eq!(
            DisplayServer::Wayland.hotkey_strategy(),
            HotkeyStrategy::External
        );
        for display in [
            DisplayServer::Windows,
            DisplayServer::MacOS,
            DisplayServer::X11,
            DisplayServer::Unknown,
        ] {
            assert_eq!(display.hotkey_strategy(), HotkeyStrategy::InApp);
        }
    }
}
