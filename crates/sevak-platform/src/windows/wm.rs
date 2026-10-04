//! Windows: list, focus, move and resize top-level windows, and read the
//! displays.
//!
//! - **Windows are found** with `EnumWindows` (which walks the z-order from the
//!   top, so the list is most recently used first) and filtered the way
//!   Alt+Tab does: visible, not cloaked (other virtual desktops and suspended
//!   Store apps), no tool windows, no owned dialogs, with a title.
//! - **The target** is the window `remember_foreground_app` noted when the
//!   launcher opened.
//! - **Invisible borders**: since Windows 10 a window's frame rectangle
//!   (`GetWindowRect`) includes about 7 transparent pixels on the left, right
//!   and bottom. Layouts are computed for the *visible* rectangle, which DWM
//!   reports as `DWMWA_EXTENDED_FRAME_BOUNDS`; the difference is added back
//!   before calling `SetWindowPos`.
//! - **Maximized and minimized windows** are restored first, because
//!   `SetWindowPos` on a maximized window moves it without leaving the
//!   maximized state.
//! - **DPI**: positions are given in physical pixels of the virtual desktop. The
//!   calls run with the thread in per-monitor-v2 awareness, so the numbers mean
//!   the same whatever the process manifest says. A window moved to a display
//!   with another scale resizes itself on arrival, so the placement is verified
//!   and, if it is off, repeated once.
//! - **Elevated windows** cannot be moved by a process that is not elevated
//!   (UIPI); the error says so.

use std::ffi::c_void;

use sevak_core::window_layout::{frame_insets, Monitor, Rect};
use windows::core::BOOL;
use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT,
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindow, GetWindowLongPtrW, GetWindowPlacement, GetWindowRect,
    GetWindowTextW, IsIconic, IsWindow, IsWindowVisible, IsZoomed, SetWindowPos, ShowWindow,
    GWL_EXSTYLE, GW_OWNER, SWP_NOACTIVATE, SWP_NOZORDER, SW_RESTORE, WINDOWPLACEMENT,
    WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
};

use super::paste::{self, app_of, hwnd_to_int, int_to_hwnd, is_own_window};
use crate::error::{PlatformError, Result};
use crate::window_manager::{WindowId, WindowInfo, WindowState, WindowSupport};

/// Windows with these classes are the desktop and the taskbar, never something
/// to switch to or arrange.
const SHELL_CLASSES: [&str; 5] = [
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "NotifyIconOverflowWindow",
];
/// Longest title read.
const TITLE_CHARS: usize = 512;
/// How far a placement may be off (pixels) and still count as exact.
const PLACEMENT_TOLERANCE: i32 = 1;

fn failure(operation: &'static str, message: impl Into<String>) -> PlatformError {
    PlatformError::Os {
        operation,
        message: message.into(),
    }
}

/// Runs the enclosed calls with the thread in per-monitor-v2 DPI awareness.
struct DpiScope(DPI_AWARENESS_CONTEXT);

impl DpiScope {
    fn enter() -> Self {
        // SAFETY: a plain call that changes the calling thread's awareness; the
        // previous value is put back by `drop`.
        Self(unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) })
    }
}

impl Drop for DpiScope {
    fn drop(&mut self) {
        // SAFETY: restores the value `SetThreadDpiAwarenessContext` returned.
        unsafe { SetThreadDpiAwarenessContext(self.0) };
    }
}

pub(crate) fn window_support() -> WindowSupport {
    WindowSupport::Available
}

fn rect_of(rect: RECT) -> Rect {
    Rect::new(
        rect.left,
        rect.top,
        rect.right - rect.left,
        rect.bottom - rect.top,
    )
}

fn class_name(hwnd: HWND) -> String {
    let mut buffer = [0u16; 256];
    // SAFETY: `buffer` is valid for the call; a stale handle returns 0.
    let len = unsafe { GetClassNameW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..usize::try_from(len).unwrap_or(0)])
}

fn title(hwnd: HWND) -> String {
    let mut buffer = [0u16; TITLE_CHARS];
    // SAFETY: `buffer` is valid for the call; a stale handle returns 0.
    let len = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..usize::try_from(len).unwrap_or(0).min(TITLE_CHARS)])
}

fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked = 0u32;
    // SAFETY: the pointer and size describe `cloaked`.
    let read = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            (&raw mut cloaked).cast::<c_void>(),
            size_of::<u32>() as u32,
        )
    };
    read.is_ok() && cloaked != 0
}

fn is_shell_window(hwnd: HWND) -> bool {
    let class = class_name(hwnd);
    SHELL_CLASSES.iter().any(|shell| *shell == class)
}

/// Whether Alt+Tab would list the window.
fn is_switchable(hwnd: HWND) -> bool {
    // SAFETY: plain queries on a window handle; a stale handle gives "no".
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            return false;
        }
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let app_window = style & WS_EX_APPWINDOW.0 != 0;
        if style & WS_EX_TOOLWINDOW.0 != 0 && !app_window {
            return false;
        }
        let owned = GetWindow(hwnd, GW_OWNER).is_ok_and(|owner| !owner.0.is_null());
        if owned && !app_window {
            return false;
        }
    }
    !is_own_window(hwnd) && !is_cloaked(hwnd) && !is_shell_window(hwnd) && !title(hwnd).is_empty()
}

unsafe extern "system" fn collect_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: `lparam` is the address of the `Vec` that `list_windows` keeps
    // alive for the whole `EnumWindows` call.
    let windows = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
    if is_switchable(hwnd) {
        windows.push(hwnd);
    }
    BOOL(1)
}

pub(crate) fn list_windows() -> Result<Vec<WindowInfo>> {
    let mut handles: Vec<HWND> = Vec::new();
    // SAFETY: the callback only touches the vector behind `lparam`, which
    // outlives the call.
    unsafe {
        EnumWindows(Some(collect_window), LPARAM((&raw mut handles) as isize))
            .map_err(|err| failure("EnumWindows", err.to_string()))?;
    }
    Ok(handles
        .into_iter()
        .filter_map(|hwnd| {
            let id = WindowId::new(&hwnd_to_int(hwnd).to_string())?;
            let app = app_of(hwnd).map(|app| app.name).unwrap_or_default();
            // SAFETY: a plain query.
            let minimized = unsafe { IsIconic(hwnd).as_bool() };
            Some(WindowInfo {
                id,
                title: title(hwnd),
                app,
                minimized,
            })
        })
        .collect())
}

/// The window behind `id`, if it still exists.
fn window_of(id: &WindowId) -> Result<HWND> {
    let value: isize = id
        .as_str()
        .parse()
        .ok()
        .filter(|value| *value != 0)
        .ok_or_else(|| failure("window", format!("{id} is not a window of this system")))?;
    let hwnd = int_to_hwnd(value);
    // SAFETY: a plain query; a stale or invalid handle gives "no".
    if unsafe { IsWindow(Some(hwnd)).as_bool() } {
        Ok(hwnd)
    } else {
        Err(PlatformError::Message("That window has closed".to_owned()))
    }
}

pub(crate) fn focus_window(id: &WindowId) -> Result<()> {
    let hwnd = window_of(id)?;
    paste::focus(hwnd).map_err(PlatformError::Message)
}

/// The rectangle the OS positions (frame including invisible borders).
fn outer_rect(hwnd: HWND) -> Option<Rect> {
    let mut rect = RECT::default();
    // SAFETY: `rect` is valid for the call.
    unsafe { GetWindowRect(hwnd, &mut rect) }
        .ok()
        .map(|()| rect_of(rect))
}

/// The rectangle the user sees: DWM's extended frame bounds, else the frame.
fn visible_rect(hwnd: HWND) -> Option<Rect> {
    let mut rect = RECT::default();
    // SAFETY: the pointer and size describe `rect`.
    let read = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&raw mut rect).cast::<c_void>(),
            size_of::<RECT>() as u32,
        )
    };
    if read.is_ok() {
        let visible = rect_of(rect);
        if !visible.is_empty() {
            return Some(visible);
        }
    }
    outer_rect(hwnd)
}

fn state_of(hwnd: HWND, id: WindowId) -> Result<WindowState> {
    // SAFETY: plain queries on a window handle.
    let (minimized, maximized) = unsafe { (IsIconic(hwnd).as_bool(), IsZoomed(hwnd).as_bool()) };
    let rect = if minimized {
        // A minimized window sits at (-32000, -32000); where it will come back
        // to is its restored placement.
        let mut placement = WINDOWPLACEMENT {
            length: size_of::<WINDOWPLACEMENT>() as u32,
            ..WINDOWPLACEMENT::default()
        };
        // SAFETY: `placement` is valid for the call.
        unsafe { GetWindowPlacement(hwnd, &mut placement) }
            .ok()
            .map(|()| rect_of(placement.rcNormalPosition))
    } else {
        visible_rect(hwnd)
    }
    .ok_or_else(|| failure("window", "could not read the window's position"))?;
    let app = app_of(hwnd).map(|app| app.name).unwrap_or_default();
    Ok(WindowState {
        id,
        title: title(hwnd),
        app,
        rect,
        maximized,
        minimized,
    })
}

pub(crate) fn window_state(id: &WindowId) -> Result<WindowState> {
    let _dpi = DpiScope::enter();
    state_of(window_of(id)?, id.clone())
}

pub(crate) fn target_window() -> Result<WindowState> {
    let _dpi = DpiScope::enter();
    let hwnd = paste::remembered_window().ok_or_else(|| {
        PlatformError::Message("There is no previous window to arrange".to_owned())
    })?;
    let id = WindowId::new(&hwnd_to_int(hwnd).to_string())
        .ok_or_else(|| failure("window", "invalid window handle"))?;
    let hwnd = window_of(&id)?;
    if is_shell_window(hwnd) {
        return Err(PlatformError::Message(
            "The desktop and the taskbar cannot be arranged".to_owned(),
        ));
    }
    state_of(hwnd, id)
}

pub(crate) fn set_window_rect(id: &WindowId, visible: Rect) -> Result<()> {
    let _dpi = DpiScope::enter();
    let hwnd = window_of(id)?;
    // SAFETY: plain calls on a window handle.
    unsafe {
        if IsIconic(hwnd).as_bool() || IsZoomed(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
    }
    // Twice at most: the first placement can change the window's DPI, and the
    // window then resizes itself; the second one corrects that.
    for _ in 0..2 {
        let insets = match (outer_rect(hwnd), visible_rect(hwnd)) {
            (Some(outer), Some(seen)) => frame_insets(outer, seen),
            _ => sevak_core::window_layout::Insets::ZERO,
        };
        let outer = visible.expand(insets);
        // SAFETY: a plain call; failure is reported, not undefined.
        let placed = unsafe {
            SetWindowPos(
                hwnd,
                None,
                outer.x,
                outer.y,
                outer.width,
                outer.height,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
        if let Err(err) = placed {
            return Err(if err.code() == ERROR_ACCESS_DENIED.to_hresult() {
                PlatformError::Message(
                    "Windows does not let Sevak move that window (it is running as administrator)"
                        .to_owned(),
                )
            } else {
                failure("SetWindowPos", err.to_string())
            });
        }
        if visible_rect(hwnd).is_some_and(|now| now.approx_eq(visible, PLACEMENT_TOLERANCE)) {
            break;
        }
    }
    Ok(())
}

unsafe extern "system" fn collect_monitor(
    monitor: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    // SAFETY: `lparam` is the address of the `Vec` that `list_monitors` keeps
    // alive for the whole `EnumDisplayMonitors` call.
    let monitors = unsafe { &mut *(lparam.0 as *mut Vec<Monitor>) };
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    // SAFETY: `info` is a MONITORINFOEXW with its size set, which the call
    // accepts through a pointer to the MONITORINFO it starts with.
    let read = unsafe { GetMonitorInfoW(monitor, (&raw mut info).cast::<MONITORINFO>()) };
    if read.as_bool() {
        let name_len = info
            .szDevice
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(info.szDevice.len());
        let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
        // SAFETY: the pointers are valid for the call. On failure the 96 DPI
        // defaults stand.
        let _ = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
        monitors.push(Monitor {
            name: String::from_utf16_lossy(&info.szDevice[..name_len]),
            bounds: rect_of(info.monitorInfo.rcMonitor),
            work_area: rect_of(info.monitorInfo.rcWork),
            scale_percent: (dpi_x * 100 / 96).max(100),
            // MONITORINFOF_PRIMARY
            primary: info.monitorInfo.dwFlags & 1 != 0,
        });
    }
    BOOL(1)
}

pub(crate) fn list_monitors() -> Result<Vec<Monitor>> {
    let _dpi = DpiScope::enter();
    let mut monitors: Vec<Monitor> = Vec::new();
    // SAFETY: the callback only touches the vector behind `lparam`, which
    // outlives the call.
    let ok = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(collect_monitor),
            LPARAM((&raw mut monitors) as isize),
        )
    };
    if !ok.as_bool() || monitors.is_empty() {
        return Err(failure("EnumDisplayMonitors", "no display was found"));
    }
    Ok(monitors)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads the real desktop and changes nothing.
    #[test]
    fn lists_real_monitors_and_windows_without_touching_them() {
        let monitors = list_monitors().expect("at least one display");
        assert!(monitors.iter().any(|monitor| monitor.primary));
        for monitor in &monitors {
            assert!(!monitor.bounds.is_empty(), "{monitor:?}");
            assert!(monitor.bounds.contains(monitor.work_area), "{monitor:?}");
            assert!(monitor.scale_percent >= 100);
        }
        // A CI runner may have no windows at all; whatever is listed is sane.
        for window in list_windows().expect("window list") {
            assert!(!window.title.is_empty());
            assert!(window.id.as_str().parse::<isize>().is_ok());
        }
    }

    #[test]
    fn unknown_or_malformed_window_ids_are_refused_without_a_side_effect() {
        for bad in ["0", "abc", "-0x"] {
            let id = WindowId::new(bad).unwrap();
            assert!(window_state(&id).is_err(), "{bad}");
            assert!(focus_window(&id).is_err(), "{bad}");
            assert!(
                set_window_rect(&id, Rect::new(0, 0, 10, 10)).is_err(),
                "{bad}"
            );
        }
    }

    /// Moves and resizes a window this test creates itself (never anyone
    /// else's), including one that is maximized, and checks that the *visible*
    /// rectangle ends up exactly where it was asked to. It flashes a small
    /// window for a moment, so it only runs on request:
    /// `cargo test -p sevak-platform windows::wm -- --ignored`.
    #[test]
    #[ignore = "shows a small test window for a moment; run manually"]
    fn places_a_window_of_its_own_exactly() {
        use windows::core::w;
        use windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext;
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, SW_MAXIMIZE, SW_SHOWNOACTIVATE, WS_EX_NOACTIVATE,
            WS_EX_TOOLWINDOW, WS_OVERLAPPEDWINDOW,
        };

        // Like Sevak itself (its manifest), the test process is per-monitor DPI
        // aware; an unaware window would be scaled by the system and its sizes
        // would not be comparable.
        // SAFETY: a plain call that changes the awareness of this test process.
        let _ =
            unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        // SAFETY: a window of the built-in STATIC class, owned by this thread
        // and destroyed below; nothing else is touched.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                w!("STATIC"),
                w!("Sevak window test"),
                WS_OVERLAPPEDWINDOW,
                300,
                300,
                400,
                300,
                None,
                None,
                None,
                None,
            )
        }
        .expect("a test window");
        // SAFETY: plain call on the window created above.
        let _ = unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
        let id = WindowId::new(&hwnd_to_int(hwnd).to_string()).unwrap();

        let monitors = list_monitors().unwrap();
        let work = monitors.iter().find(|m| m.primary).unwrap().work_area;
        let targets = [
            Rect::new(work.x + 40, work.y + 40, 520, 380),
            Rect::new(work.x, work.y, work.width / 2, work.height),
            Rect::new(
                work.x + work.width / 2,
                work.y,
                work.width / 2,
                work.height / 2,
            ),
        ];
        for target in targets {
            set_window_rect(&id, target).unwrap();
            let state = window_state(&id).unwrap();
            assert!(!state.maximized && !state.minimized);
            assert!(
                state.rect.approx_eq(target, 1),
                "asked for {target:?}, the window shows {:?}",
                state.rect
            );
        }

        // Maximized windows are restored first, then placed.
        // SAFETY: plain call on the window created above.
        let _ = unsafe { ShowWindow(hwnd, SW_MAXIMIZE) };
        assert!(window_state(&id).unwrap().maximized);
        let target = Rect::new(work.x + 100, work.y + 100, 600, 400);
        set_window_rect(&id, target).unwrap();
        let state = window_state(&id).unwrap();
        assert!(!state.maximized);
        assert!(state.rect.approx_eq(target, 1), "{:?}", state.rect);

        // SAFETY: destroys the window created above, from its own thread.
        let _ = unsafe { DestroyWindow(hwnd) };
        assert!(window_state(&id).is_err(), "a destroyed window is gone");
    }
}
