//! Windows automation tasks: dark mode, shortcuts, volume, Wi-Fi and Bluetooth
//! radios, window listing and closing, removable drives, keep-awake.

use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use windows::core::BOOL;
use windows::core::{w, PCWSTR};
use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioKind, RadioState};
use windows::Win32::Foundation::{E_FAIL, HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW};
use windows::Win32::System::Com::{
    CoCreateInstance, CoIncrementMTAUsage, CoInitializeEx, CoUninitialize, CLSCTX_ALL,
    COINIT_MULTITHREADED,
};
use windows::Win32::System::Power::{
    SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_QUERY_VALUE, KEY_SET_VALUE, REG_DWORD,
};
use windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindow, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsWindowVisible, PostMessageW, SendMessageTimeoutW, GWL_EXSTYLE,
    GW_OWNER, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_CLOSE, WM_SETTINGCHANGE, WS_EX_APPWINDOW,
    WS_EX_TOOLWINDOW,
};

use super::com::ComGuard;
use super::paste::{key_input, send_inputs};
use crate::error::{PlatformError, Result};
use crate::process::{run_checked, spawn_detached};
use crate::system::GRACE;
use crate::tasks::{
    group_apps, windows_eject_args, windows_task_chord, Drive, RunningApp, Task,
    WINDOWS_KILL_EXPLORER, WINDOWS_RECENT_TARGET, WINDOWS_SCREENSHOT_URI,
};

fn os_error(operation: &'static str, err: impl std::fmt::Display) -> PlatformError {
    PlatformError::Os {
        operation,
        message: err.to_string(),
    }
}

/// How long after Sevak's window is told to hide the synthesized shortcuts are
/// sent, so they reach the app the user was in and not Sevak.
const HIDE_DELAY: Duration = Duration::from_millis(250);

/// Volume change of the volume up / down tasks, in percentage points.
const VOLUME_STEP: f32 = 0.10;

pub(crate) fn run(task: &Task) -> Result<()> {
    if let Some(chord) = windows_task_chord(task) {
        send_chord_later(chord);
        return Ok(());
    }
    match task {
        Task::ToggleDarkMode => toggle_dark_mode(),
        Task::Screenshot => open_shell(WINDOWS_SCREENSHOT_URI),
        Task::OpenRecentFiles => open_shell(WINDOWS_RECENT_TARGET),
        Task::RestartShell => restart_explorer(),
        Task::Mute => with_volume(|volume| set_mute(volume, true)),
        Task::Unmute => with_volume(|volume| set_mute(volume, false)),
        Task::VolumeUp => with_volume(|volume| nudge_volume(volume, VOLUME_STEP)),
        Task::VolumeDown => with_volume(|volume| nudge_volume(volume, -VOLUME_STEP)),
        Task::SetVolume(percent) => with_volume(|volume| set_volume(volume, *percent)),
        Task::ToggleWifi => toggle_radio(RadioKind::WiFi),
        Task::ToggleBluetooth => toggle_radio(RadioKind::Bluetooth),
        Task::KeepAwake(minutes) => crate::tasks::start_awake(*minutes, None),
        Task::QuitApp(name) => quit_app(name),
        Task::Eject(drive) => {
            let args = windows_eject_args(drive)
                .ok_or(PlatformError::Unsupported("ejecting this drive"))?;
            run_checked(&powershell_exe(), &args, GRACE)
        }
        _ => Err(PlatformError::Unsupported("this task")),
    }
}

fn open_shell(target: &str) -> Result<()> {
    super::shell_execute("open", std::ffi::OsStr::new(target), None)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Shortcuts
// ---------------------------------------------------------------------------

/// Presses the keys together (in order, released in reverse) once Sevak's
/// window has gone. A thread, because the launcher hides its window only after
/// the task returns.
fn send_chord_later(chord: &'static [u16]) {
    let spawned = std::thread::Builder::new()
        .name("sevak-chord".into())
        .spawn(move || {
            std::thread::sleep(HIDE_DELAY);
            if let Err(err) = send_chord(chord) {
                tracing::warn!(%err, "could not send the shortcut");
            }
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "could not start the shortcut thread");
    }
}

pub(crate) fn send_chord(chord: &[u16]) -> Result<()> {
    let mut inputs = Vec::with_capacity(chord.len() * 2);
    for key in chord {
        inputs.push(key_input(VIRTUAL_KEY(*key), false));
    }
    for key in chord.iter().rev() {
        inputs.push(key_input(VIRTUAL_KEY(*key), true));
    }
    send_inputs(&inputs)
}

// ---------------------------------------------------------------------------
// Dark mode
// ---------------------------------------------------------------------------

const PERSONALIZE_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
const APPS_USE_LIGHT: PCWSTR = w!("AppsUseLightTheme");
const SYSTEM_USES_LIGHT: PCWSTR = w!("SystemUsesLightTheme");

fn open_personalize(access: windows::Win32::System::Registry::REG_SAM_FLAGS) -> Result<HKEY> {
    let mut key = HKEY::default();
    // SAFETY: the key name is a static NUL-terminated string and `key` is a
    // valid out pointer; the handle is closed by the callers.
    unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, PERSONALIZE_KEY, None, access, &mut key) }
        .ok()
        .map_err(|err| os_error("RegOpenKeyExW", err))?;
    Ok(key)
}

fn read_dword(key: HKEY, name: PCWSTR) -> Option<u32> {
    let mut value = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: `value` is valid for `size` bytes and both outlive the call.
    let status = unsafe {
        RegQueryValueExW(
            key,
            name,
            None,
            None,
            Some(&mut value as *mut u32 as *mut u8),
            Some(&mut size),
        )
    };
    status.is_ok().then_some(value)
}

/// Whether apps use the light theme right now (the default when unset).
pub(crate) fn apps_use_light_theme() -> bool {
    let Ok(key) = open_personalize(KEY_QUERY_VALUE) else {
        return true;
    };
    let light = read_dword(key, APPS_USE_LIGHT).unwrap_or(1) != 0;
    // SAFETY: closes the handle opened above.
    let _ = unsafe { RegCloseKey(key) };
    light
}

fn toggle_dark_mode() -> Result<()> {
    let light = u32::from(!apps_use_light_theme());
    let key = open_personalize(KEY_SET_VALUE)?;
    let mut result = Ok(());
    for name in [APPS_USE_LIGHT, SYSTEM_USES_LIGHT] {
        // SAFETY: the data is a four-byte DWORD that outlives the call.
        let status =
            unsafe { RegSetValueExW(key, name, None, REG_DWORD, Some(&light.to_le_bytes())) };
        if status.is_err() && result.is_ok() {
            result = status.ok().map_err(|err| os_error("RegSetValueExW", err));
        }
    }
    // SAFETY: closes the handle opened above.
    let _ = unsafe { RegCloseKey(key) };
    result?;
    // Running apps and the shell pick the change up from this broadcast.
    // SAFETY: the string is static and NUL-terminated; the result is not used.
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(w!("ImmersiveColorSet").as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            1000,
            None,
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Volume
// ---------------------------------------------------------------------------

fn with_volume(
    action: impl FnOnce(&IAudioEndpointVolume) -> windows::core::Result<()>,
) -> Result<()> {
    let _com = ComGuard::new();
    // SAFETY: COM is initialized for this thread by the guard; every interface
    // is released when dropped at the end of the block.
    let volume: IAudioEndpointVolume = unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|err| os_error("audio devices", err))?;
        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .map_err(|err| os_error("default audio device", err))?;
        device
            .Activate(CLSCTX_ALL, None)
            .map_err(|err| os_error("audio volume", err))?
    };
    action(&volume).map_err(|err| os_error("audio volume", err))
}

fn set_mute(volume: &IAudioEndpointVolume, mute: bool) -> windows::core::Result<()> {
    // SAFETY: a null event context is allowed.
    unsafe { volume.SetMute(mute, std::ptr::null()) }
}

fn set_scalar(volume: &IAudioEndpointVolume, level: f32) -> windows::core::Result<()> {
    // SAFETY: a null event context is allowed.
    unsafe { volume.SetMasterVolumeLevelScalar(level.clamp(0.0, 1.0), std::ptr::null()) }
}

fn set_volume(volume: &IAudioEndpointVolume, percent: u8) -> windows::core::Result<()> {
    set_scalar(volume, f32::from(percent.min(100)) / 100.0)?;
    // A level above zero is heard: lift a mute, as the volume keys do.
    if percent > 0 {
        set_mute(volume, false)?;
    }
    Ok(())
}

fn nudge_volume(volume: &IAudioEndpointVolume, delta: f32) -> windows::core::Result<()> {
    // SAFETY: plain getter.
    let current = unsafe { volume.GetMasterVolumeLevelScalar() }?;
    set_scalar(volume, current + delta)?;
    if delta > 0.0 {
        set_mute(volume, false)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Radios
// ---------------------------------------------------------------------------

/// Keeps the multithreaded COM apartment alive for the life of the process.
/// Without it the apartment is torn down when the last thread in it exits, and
/// the WinRT class factories the `windows` crate caches in statics then point
/// at a dead apartment: the second call crashes.
static KEEP_MTA: std::sync::Once = std::sync::Once::new();

/// Runs `work` on a short-lived thread in the multithreaded COM apartment, and
/// returns what it returned. WinRT's blocking `.join()` must not run on a
/// single-threaded apartment (a UI thread would deadlock on it), and the
/// threads Sevak calls platform code from make no promise about theirs.
pub(super) fn on_mta_thread<T: Send>(
    work: impl FnOnce() -> windows::core::Result<T> + Send,
) -> windows::core::Result<T> {
    KEEP_MTA.call_once(|| {
        // SAFETY: plain call without pointers. The cookie is deliberately
        // dropped: the usage count is never released.
        if let Err(err) = unsafe { CoIncrementMTAUsage() } {
            tracing::debug!(%err, "could not pin the multithreaded apartment");
        }
    });
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                // SAFETY: balanced by the `CoUninitialize` below on this thread.
                let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
                let result = work();
                if initialized {
                    // SAFETY: balances the successful call above.
                    unsafe { CoUninitialize() };
                }
                result
            })
            .join()
            .unwrap_or_else(|_| Err(windows::core::Error::from(E_FAIL)))
    })
}

/// Whether the machine has a Wi-Fi radio and a Bluetooth radio Sevak can
/// switch. Blocks on WinRT: call from a background thread.
pub(crate) fn radio_kinds() -> (bool, bool) {
    let kinds = on_mta_thread(|| {
        let radios = Radio::GetRadiosAsync()?.join()?;
        let mut kinds = Vec::new();
        for radio in radios {
            kinds.push(radio.Kind()?);
        }
        Ok(kinds)
    })
    .unwrap_or_default();
    (
        kinds.contains(&RadioKind::WiFi),
        kinds.contains(&RadioKind::Bluetooth),
    )
}

enum RadioOutcome {
    Done,
    Missing,
    Denied,
}

/// Switches every radio of `kind` off if any is on, otherwise on.
fn toggle_radio(kind: RadioKind) -> Result<()> {
    let outcome = on_mta_thread(|| {
        let mut radios = Vec::new();
        for radio in Radio::GetRadiosAsync()?.join()? {
            if radio.Kind()? == kind {
                radios.push(radio);
            }
        }
        if radios.is_empty() {
            return Ok(RadioOutcome::Missing);
        }
        if Radio::RequestAccessAsync()?.join()? != RadioAccessStatus::Allowed {
            return Ok(RadioOutcome::Denied);
        }
        let any_on = radios
            .iter()
            .any(|radio| radio.State().is_ok_and(|state| state == RadioState::On));
        let target = if any_on {
            RadioState::Off
        } else {
            RadioState::On
        };
        for radio in &radios {
            if radio.SetStateAsync(target)?.join()? != RadioAccessStatus::Allowed {
                return Ok(RadioOutcome::Denied);
            }
        }
        Ok(RadioOutcome::Done)
    })
    .map_err(|err| os_error("radio", err))?;
    match outcome {
        RadioOutcome::Done => Ok(()),
        RadioOutcome::Missing => Err(PlatformError::Unsupported("this radio")),
        RadioOutcome::Denied => Err(os_error(
            "radio",
            "Windows did not allow Sevak to change radios (check Settings > Privacy > Radios)",
        )),
    }
}

// ---------------------------------------------------------------------------
// Windows and apps
// ---------------------------------------------------------------------------

struct Collected {
    windows: Vec<(isize, u32)>,
}

/// `EnumWindows` callback: keeps the windows a user would call an app window.
unsafe extern "system" fn collect_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: `lparam` is the `&mut Collected` passed to `EnumWindows` below,
    // which outlives the enumeration.
    let collected = unsafe { &mut *(lparam.0 as *mut Collected) };
    if is_app_window(hwnd) {
        let mut pid = 0u32;
        // SAFETY: `pid` outlives the call.
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if pid != 0 {
            collected.windows.push((hwnd.0 as isize, pid));
        }
    }
    true.into()
}

fn is_app_window(hwnd: HWND) -> bool {
    // SAFETY: only reads window properties; a stale handle makes the calls
    // fail, not misbehave. `title` is valid for its length.
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || GetWindowTextLengthW(hwnd) == 0 {
            return false;
        }
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let owned = GetWindow(hwnd, GW_OWNER).is_ok_and(|owner| !owner.0.is_null());
        // Tool windows and owned dialogs are not apps of their own unless they
        // ask to be (WS_EX_APPWINDOW).
        if (ex_style & WS_EX_TOOLWINDOW.0 != 0 || owned) && ex_style & WS_EX_APPWINDOW.0 == 0 {
            return false;
        }
        // Windows keeps suspended Store apps' windows around, cloaked.
        let mut cloaked = 0u32;
        let queried = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as *mut c_void,
            std::mem::size_of::<u32>() as u32,
        );
        if queried.is_ok() && cloaked != 0 {
            return false;
        }
        // The desktop itself is a visible, titled window.
        let mut title = [0u16; 32];
        let len = GetWindowTextW(hwnd, &mut title).max(0) as usize;
        String::from_utf16_lossy(&title[..len.min(title.len())]) != "Program Manager"
    }
}

fn app_windows() -> Vec<(isize, u32)> {
    let mut collected = Collected {
        windows: Vec::new(),
    };
    // SAFETY: the callback only uses the pointer during this call.
    let _ = unsafe {
        EnumWindows(
            Some(collect_window),
            LPARAM(&mut collected as *mut Collected as isize),
        )
    };
    collected.windows
}

fn process_names() -> std::collections::HashMap<u32, String> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    system
        .processes()
        .iter()
        .map(|(pid, process)| (pid.as_u32(), process.name().to_string_lossy().into_owned()))
        .collect()
}

pub(crate) fn running_apps() -> Result<Vec<RunningApp>> {
    let own = std::process::id();
    let names = process_names();
    Ok(group_apps(
        app_windows()
            .into_iter()
            .filter(|(_, pid)| *pid != own)
            .filter_map(|(_, pid)| names.get(&pid).map(|name| (pid, name.clone())))
            // Store apps (Settings, Calculator) show their windows through this
            // host; it is not an app anyone means to quit.
            .filter(|(_, name)| !name.eq_ignore_ascii_case("ApplicationFrameHost.exe")),
    ))
}

/// Posts WM_CLOSE to every window of the app, the same request as its close
/// button: the app may still ask to save.
fn quit_app(name: &str) -> Result<()> {
    let app = running_apps()?
        .into_iter()
        .find(|app| app.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| os_error("quit", format!("{name} has no open windows")))?;
    let mut posted = 0;
    for (hwnd, pid) in app_windows() {
        if app.pids.contains(&pid) {
            // SAFETY: a plain message post; a stale handle just fails.
            let sent = unsafe {
                PostMessageW(
                    Some(HWND(hwnd as *mut c_void)),
                    WM_CLOSE,
                    WPARAM(0),
                    LPARAM(0),
                )
            };
            if sent.is_ok() {
                posted += 1;
            }
        }
    }
    if posted == 0 {
        return Err(os_error("quit", format!("could not close {name}")));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Explorer, drives, keep awake
// ---------------------------------------------------------------------------

fn system32(program: &str) -> String {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let mut path = PathBuf::from(root);
    path.push("System32");
    path.push(program);
    path.to_string_lossy().into_owned()
}

fn powershell_exe() -> String {
    system32(r"WindowsPowerShell\v1.0\powershell.exe")
}

/// Ends Explorer. Windows restarts it by itself within a moment; if it has not,
/// Sevak starts it.
fn restart_explorer() -> Result<()> {
    run_checked(&system32("taskkill.exe"), &WINDOWS_KILL_EXPLORER, GRACE)?;
    let spawned = std::thread::Builder::new()
        .name("sevak-explorer".into())
        .spawn(|| {
            std::thread::sleep(Duration::from_millis(2500));
            if !process_names()
                .values()
                .any(|name| name.eq_ignore_ascii_case("explorer.exe"))
            {
                let explorer = system32("..\\explorer.exe");
                if let Err(err) = spawn_detached(&explorer, &[] as &[&str]) {
                    tracing::warn!(%err, "could not start Explorer again");
                }
            }
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "could not start the Explorer watcher");
    }
    Ok(())
}

/// USB sticks, SD cards and optical drives (drive letters Explorer offers
/// "Eject" for).
pub(crate) fn removable_drives() -> Vec<Drive> {
    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_CDROM: u32 = 5;
    // SAFETY: plain Win32 call without arguments.
    let mask = unsafe { GetLogicalDrives() };
    let mut drives = Vec::new();
    // A: and B: are floppy letters; nothing to eject there.
    for index in 2..26u32 {
        if mask & (1 << index) == 0 {
            continue;
        }
        let letter = char::from(b'A' + index as u8);
        let root: Vec<u16> = format!("{letter}:\\")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        // SAFETY: `root` is NUL-terminated and outlives the call.
        let kind = unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) };
        if kind != DRIVE_REMOVABLE && kind != DRIVE_CDROM {
            continue;
        }
        let mut name = [0u16; 64];
        // SAFETY: `root` is NUL-terminated; `name` is valid for its length; the
        // other outputs are optional.
        let labelled = unsafe {
            GetVolumeInformationW(
                PCWSTR(root.as_ptr()),
                Some(&mut name),
                None,
                None,
                None,
                None,
            )
        };
        let label = if labelled.is_ok() {
            let len = name.iter().position(|c| *c == 0).unwrap_or(name.len());
            String::from_utf16_lossy(&name[..len])
        } else {
            String::new()
        };
        let label = if label.is_empty() {
            if kind == DRIVE_CDROM {
                "Optical drive"
            } else {
                "Removable disk"
            }
            .to_owned()
        } else {
            label
        };
        drives.push(Drive {
            id: format!("{letter}:"),
            label: format!("{label} ({letter}:)"),
        });
    }
    drives
}

/// Keeps the system and display awake for `duration` from a thread of its own
/// (the power request belongs to the thread that made it). Sending on the
/// returned channel, or dropping it, ends the request early.
pub(crate) fn keep_awake(duration: Duration) -> Result<Sender<()>> {
    let (stop, wait) = mpsc::channel::<()>();
    std::thread::Builder::new()
        .name("sevak-awake".into())
        .spawn(move || {
            // SAFETY: plain Win32 call taking flags.
            unsafe {
                SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED)
            };
            // Returns on timeout, an explicit stop, or the sender going away.
            let _ = wait.recv_timeout(duration);
            // SAFETY: as above; clears the request.
            unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
        })
        .map_err(|err| os_error("keep awake", err))?;
    Ok(stop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system32_programs_exist() {
        for program in ["taskkill.exe", r"WindowsPowerShell\v1.0\powershell.exe"] {
            let path = system32(program);
            assert!(std::path::Path::new(&path).is_file(), "{path}");
        }
        assert!(std::path::Path::new(&system32("..\\explorer.exe")).is_file());
    }

    #[test]
    fn reading_the_theme_does_not_fail() {
        // Read-only: whatever the theme is, the call answers.
        let _ = apps_use_light_theme();
    }

    #[test]
    fn listing_app_windows_and_drives_is_read_only_and_safe() {
        let apps = running_apps().expect("window list");
        assert!(apps
            .iter()
            .all(|app| !app.name.is_empty() && !app.pids.is_empty()));
        for drive in removable_drives() {
            assert!(crate::tasks::valid_drive_id(&drive.id), "{drive:?}");
        }
    }

    #[test]
    fn radio_query_answers() {
        // Read-only: a desktop without radios reports (false, false). Twice,
        // because the second call used to crash once the first call's COM
        // apartment was gone.
        let first = radio_kinds();
        assert_eq!(radio_kinds(), first);
    }
}
