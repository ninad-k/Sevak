//! Windows: remember the foreground window, refocus it, send Ctrl+V, and read
//! the clipboard's "do not record" markers.

use std::ffi::c_void;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::thread::sleep;
use std::time::{Duration, Instant};

use windows::core::{w, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, GetClipboardData, GetClipboardSequenceNumber, IsClipboardFormatAvailable,
    OpenClipboard, RegisterClipboardFormatW,
};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId, OpenProcess,
    QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC_EX, VIRTUAL_KEY, VK_CONTROL, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindow, SetForegroundWindow,
    ShowWindow, SW_RESTORE,
};

use crate::error::{PlatformError, Result};
use crate::paste::{
    self, ClipboardRead, ForegroundApp, PasteContent, PasteDriver, PasteOutcome, PasteSupport,
    SystemClipboard,
};

/// The window that had focus when Sevak was shown (an `HWND` as an integer, so
/// the static is `Send`).
static REMEMBERED: Mutex<Option<isize>> = Mutex::new(None);

/// How long to wait for the previous window to come back to the foreground.
const FOCUS_TIMEOUT: Duration = Duration::from_millis(500);

pub(super) fn hwnd_to_int(hwnd: HWND) -> isize {
    hwnd.0 as isize
}

pub(super) fn int_to_hwnd(value: isize) -> HWND {
    HWND(value as *mut c_void)
}

pub(super) fn foreground_window() -> Option<HWND> {
    // SAFETY: plain Win32 call without arguments.
    let hwnd = unsafe { GetForegroundWindow() };
    (!hwnd.0.is_null()).then_some(hwnd)
}

/// Process id and thread id owning `hwnd`.
pub(super) fn window_owner(hwnd: HWND) -> Option<(u32, u32)> {
    let mut pid = 0u32;
    // SAFETY: `pid` outlives the call; `hwnd` is only read.
    let thread = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    (thread != 0 && pid != 0).then_some((pid, thread))
}

/// Full path of the executable of process `pid`.
fn process_path(pid: u32) -> Option<String> {
    // SAFETY: the handle is closed below on every path; `buffer` and `len` are
    // valid for the call, and `len` is the buffer's capacity in characters.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 1024];
        let mut len = buffer.len() as u32;
        let queried = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(process);
        queried.ok()?;
        Some(String::from_utf16_lossy(&buffer[..len as usize]))
    }
}

pub(super) fn app_of(hwnd: HWND) -> Option<ForegroundApp> {
    let (pid, _) = window_owner(hwnd)?;
    let path = process_path(pid)?;
    let path = Path::new(&path);
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let file = path.file_name()?.to_string_lossy().into_owned();
    Some(ForegroundApp::new(stem).with_identifier(file))
}

pub(super) fn is_own_window(hwnd: HWND) -> bool {
    // SAFETY: plain Win32 call without arguments.
    window_owner(hwnd).is_some_and(|(pid, _)| pid == unsafe { GetCurrentProcessId() })
}

pub(crate) fn remember_foreground_app() {
    let current = foreground_window();
    if current.is_some_and(is_own_window) {
        // Sevak is already in front (shown twice); the earlier window stands.
        return;
    }
    *REMEMBERED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = current.map(hwnd_to_int);
}

pub(crate) fn foreground_app() -> Option<ForegroundApp> {
    foreground_window().and_then(app_of)
}

/// The window that had focus when Sevak was last shown (window management acts
/// on it).
pub(super) fn remembered_window() -> Option<HWND> {
    REMEMBERED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .map(int_to_hwnd)
}

pub(crate) fn paste_support() -> PasteSupport {
    PasteSupport::Available
}

pub(crate) fn paste_text(text: &str, restore_clipboard: bool) -> Result<PasteOutcome> {
    paste_content(PasteContent::Text(text), restore_clipboard)
}

pub(crate) fn paste_content(
    content: PasteContent<'_>,
    restore_clipboard: bool,
) -> Result<PasteOutcome> {
    paste::paste(
        content,
        restore_clipboard,
        &SystemClipboard,
        &WindowsDriver,
        true,
    )
}

struct WindowsDriver;

impl PasteDriver for WindowsDriver {
    fn focus_previous(&self) -> std::result::Result<(), String> {
        let remembered = *REMEMBERED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(target) = remembered.map(int_to_hwnd) else {
            // Nothing was remembered; paste wherever focus is, unless that is
            // Sevak itself.
            return match foreground_window() {
                Some(hwnd) if is_own_window(hwnd) => {
                    Err("There is no previous window to paste into".to_owned())
                }
                _ => Ok(()),
            };
        };
        focus(target)
    }

    fn press_paste(&self) -> Result<()> {
        send_inputs(&ctrl_v())
    }
}

/// Ctrl+V as one `SendInput` batch: the system inserts it without any other
/// input in between, so nothing the user types can land between Ctrl and V.
fn ctrl_v() -> [INPUT; 4] {
    [
        key_input(VK_CONTROL, false),
        key_input(VK_V, false),
        key_input(VK_V, true),
        key_input(VK_CONTROL, true),
    ]
}

/// Sends key events to the focused window, all or nothing as far as the OS
/// reports it.
pub(super) fn send_inputs(inputs: &[INPUT]) -> Result<()> {
    // SAFETY: `inputs` is a valid slice of fully initialized INPUT structs
    // and the size passed is that of one element.
    let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err(PlatformError::Os {
            operation: "SendInput",
            message: format!(
                "only {sent} of {} key events were accepted (is the window running as administrator?)",
                inputs.len()
            ),
        })
    }
}

/// A press (or release, `up`) of `key` for [`send_inputs`], with the key's
/// hardware scan code as well as its virtual-key code.
///
/// The scan code matters: an event with `wScan` 0 reaches the app with scan
/// code 0 in its `WM_KEYDOWN`, and apps whose input stack works from scan codes
/// (WinUI/XAML ones such as Windows 11 Notepad and Windows Terminal) then do not
/// recognise the key; a Ctrl they do not see turns Ctrl+V into a typed "v".
/// Keys of the extended set (arrows, right Ctrl, ...) get
/// `KEYEVENTF_EXTENDEDKEY`, as a real keyboard would send them.
pub(super) fn key_input(key: VIRTUAL_KEY, up: bool) -> INPUT {
    // SAFETY: a lookup in the keyboard layout's tables; it has no side effects.
    let scan = unsafe { MapVirtualKeyW(u32::from(key.0), MAPVK_VK_TO_VSC_EX) };
    key_input_with_scan(key, scan, up)
}

/// [`key_input`] for a scan code from `MapVirtualKeyW(.., MAPVK_VK_TO_VSC_EX)`:
/// the low byte is the code, a high byte of `0xE0` or `0xE1` marks an extended
/// key, and 0 means the layout has no such key (the event then carries only the
/// virtual-key code).
fn key_input_with_scan(key: VIRTUAL_KEY, scan: u32, up: bool) -> INPUT {
    let mut flags = if up {
        KEYEVENTF_KEYUP
    } else {
        KEYBD_EVENT_FLAGS(0)
    };
    if matches!(scan >> 8, 0xE0 | 0xE1) {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: (scan & 0xFF) as u16,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Brings `target` to the foreground and waits until it (or another window of
/// its process, such as a dialog it owns) is there.
pub(super) fn focus(target: HWND) -> std::result::Result<(), String> {
    // SAFETY: only plain Win32 calls on window handles; a stale handle makes
    // them fail, not misbehave.
    unsafe {
        if !IsWindow(Some(target)).as_bool() {
            return Err("The previous window has closed".to_owned());
        }
        if IsIconic(target).as_bool() {
            let _ = ShowWindow(target, SW_RESTORE);
        }
        if !is_foreground(target) && !SetForegroundWindow(target).as_bool() {
            // Windows only lets the process that owns the foreground window
            // change it. Sevak was just hidden, so borrow the foreground
            // thread's input queue for the call.
            if let Some((_, other_thread)) = foreground_window().and_then(window_owner) {
                let this_thread = GetCurrentThreadId();
                if other_thread != this_thread {
                    let _ = AttachThreadInput(this_thread, other_thread, true);
                    let _ = SetForegroundWindow(target);
                    let _ = AttachThreadInput(this_thread, other_thread, false);
                }
            }
        }
    }

    let deadline = Instant::now() + FOCUS_TIMEOUT;
    loop {
        if is_foreground(target) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("Could not return to the previous window".to_owned());
        }
        sleep(Duration::from_millis(10));
    }
}

fn is_foreground(target: HWND) -> bool {
    let Some(current) = foreground_window() else {
        return false;
    };
    if current == target {
        return true;
    }
    match (window_owner(current), window_owner(target)) {
        (Some((a, _)), Some((b, _))) => a == b && !is_own_window(current),
        _ => false,
    }
}

pub(crate) fn clipboard_sequence() -> Option<u64> {
    // SAFETY: plain Win32 call without arguments.
    let sequence = unsafe { GetClipboardSequenceNumber() };
    // Zero means the clipboard is not accessible from this window station.
    (sequence != 0).then_some(u64::from(sequence))
}

pub(crate) fn read_clipboard() -> Result<ClipboardRead> {
    if clipboard_marked_secret()? {
        return Ok(ClipboardRead {
            text: None,
            sensitive: true,
        });
    }
    Ok(ClipboardRead {
        text: crate::clipboard::read_text()?,
        sensitive: false,
    })
}

struct SecretFormats {
    /// `ExcludeClipboardContentFromMonitorProcessing`: present means "skip me".
    exclude_from_monitoring: u32,
    /// `CanIncludeInClipboardHistory`: a DWORD, 0 means "not in history".
    can_include_in_history: u32,
    /// `CanUploadToCloudClipboard`: a DWORD, 0 means "do not sync".
    can_upload_to_cloud: u32,
}

fn secret_formats() -> &'static SecretFormats {
    static FORMATS: OnceLock<SecretFormats> = OnceLock::new();
    FORMATS.get_or_init(|| {
        // SAFETY: the wide string literals are NUL-terminated and static.
        unsafe {
            SecretFormats {
                exclude_from_monitoring: RegisterClipboardFormatW(w!(
                    "ExcludeClipboardContentFromMonitorProcessing"
                )),
                can_include_in_history: RegisterClipboardFormatW(w!(
                    "CanIncludeInClipboardHistory"
                )),
                can_upload_to_cloud: RegisterClipboardFormatW(w!("CanUploadToCloudClipboard")),
            }
        }
    })
}

fn format_available(format: u32) -> bool {
    // SAFETY: plain Win32 call; format 0 (registration failed) is simply absent.
    format != 0 && unsafe { IsClipboardFormatAvailable(format) }.is_ok()
}

/// True if the app that filled the clipboard asked for it not to be recorded.
/// Password managers set these formats. `Err` means the clipboard was busy; the
/// caller retries later rather than guessing.
fn clipboard_marked_secret() -> Result<bool> {
    let formats = secret_formats();
    if format_available(formats.exclude_from_monitoring) {
        return Ok(true);
    }
    let has_history_flag = format_available(formats.can_include_in_history);
    let has_cloud_flag = format_available(formats.can_upload_to_cloud);
    if !has_history_flag && !has_cloud_flag {
        return Ok(false);
    }

    // The flags are DWORDs inside the clipboard, which has to be opened to
    // read them. Another program may hold it for a moment.
    // SAFETY: the clipboard is closed on every path below; the memory handle
    // returned by GetClipboardData is owned by the clipboard and only read
    // while it is open.
    unsafe {
        let mut opened = false;
        for _ in 0..5 {
            if OpenClipboard(None).is_ok() {
                opened = true;
                break;
            }
            sleep(Duration::from_millis(5));
        }
        if !opened {
            return Err(PlatformError::Os {
                operation: "OpenClipboard",
                message: "the clipboard is in use by another program".to_owned(),
            });
        }
        let secret = (has_history_flag && read_dword(formats.can_include_in_history) == Some(0))
            || (has_cloud_flag && read_dword(formats.can_upload_to_cloud) == Some(0));
        let _ = CloseClipboard();
        Ok(secret)
    }
}

/// Reads a DWORD clipboard format. The clipboard must be open.
unsafe fn read_dword(format: u32) -> Option<u32> {
    let handle = GetClipboardData(format).ok()?;
    let memory = HGLOBAL(handle.0);
    if GlobalSize(memory) < std::mem::size_of::<u32>() {
        return None;
    }
    let pointer = GlobalLock(memory) as *const u32;
    if pointer.is_null() {
        return None;
    }
    let value = pointer.read_unaligned();
    let _ = GlobalUnlock(memory);
    Some(value)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_BACK, VK_RIGHT};

    /// `(virtual key, scan code, key up, extended)` of a keyboard `INPUT`.
    pub(in crate::windows) fn key_of(input: &INPUT) -> (VIRTUAL_KEY, u16, bool, bool) {
        assert_eq!(input.r#type, INPUT_KEYBOARD);
        // SAFETY: the type says `ki` is the active field.
        let ki = unsafe { input.Anonymous.ki };
        (
            ki.wVk,
            ki.wScan,
            ki.dwFlags.contains(KEYEVENTF_KEYUP),
            ki.dwFlags.contains(KEYEVENTF_EXTENDEDKEY),
        )
    }

    #[test]
    fn key_events_carry_their_scan_codes() {
        // Left Ctrl and Backspace have the same scan code on every layout.
        assert_eq!(
            key_of(&key_input(VK_CONTROL, false)),
            (VK_CONTROL, 0x1D, false, false)
        );
        assert_eq!(
            key_of(&key_input(VK_CONTROL, true)),
            (VK_CONTROL, 0x1D, true, false)
        );
        assert_eq!(
            key_of(&key_input(VK_BACK, true)),
            (VK_BACK, 0x0E, true, false)
        );
        // V moves with the layout (Dvorak), but always has a scan code.
        let (vk, scan, up, _) = key_of(&key_input(VK_V, false));
        assert_eq!((vk, up), (VK_V, false));
        assert_ne!(scan, 0, "V must carry its scan code");
        // Extended keys say so; a key the layout lacks keeps only its VK.
        assert_eq!(
            key_of(&key_input_with_scan(VK_RIGHT, 0xE04D, false)),
            (VK_RIGHT, 0x4D, false, true)
        );
        assert_eq!(
            key_of(&key_input_with_scan(VK_V, 0, true)),
            (VK_V, 0, true, false)
        );
        // SAFETY: `ki` is the active field of a keyboard INPUT.
        assert_eq!(
            unsafe { key_input(VK_V, false).Anonymous.ki.dwExtraInfo },
            0
        );
    }

    #[test]
    fn ctrl_v_holds_ctrl_around_the_v_in_one_batch() {
        let keys: Vec<_> = ctrl_v()
            .iter()
            .map(|input| {
                let (vk, scan, up, _) = key_of(input);
                assert_ne!(scan, 0);
                (vk, up)
            })
            .collect();
        assert_eq!(
            keys,
            [
                (VK_CONTROL, false),
                (VK_V, false),
                (VK_V, true),
                (VK_CONTROL, true)
            ]
        );
    }

    #[test]
    fn the_foreground_window_can_be_described() {
        // Not asserting Some: a headless CI session may have no foreground
        // window. It must not fail either way.
        if let Some(app) = foreground_app() {
            assert!(!app.name.is_empty());
        }
    }

    #[test]
    fn the_secret_formats_register() {
        let formats = secret_formats();
        assert_ne!(formats.exclude_from_monitoring, 0);
        assert_ne!(formats.can_include_in_history, 0);
        assert_ne!(formats.can_upload_to_cloud, 0);
    }

    /// Writes to the real clipboard, so it is a single test (cargo runs tests
    /// in parallel) and gives up quietly where there is no clipboard.
    #[test]
    fn secret_and_ordinary_clipboard_content_are_told_apart() {
        use arboard::SetExtWindows;

        let Ok(mut clipboard) = arboard::Clipboard::new() else {
            return;
        };
        let saved = clipboard.get_text().ok();

        if clipboard.set_text("sevak-test ordinary").is_err() {
            return;
        }
        let ordinary = read_clipboard().unwrap();
        assert!(!ordinary.sensitive);
        assert_eq!(ordinary.text.as_deref(), Some("sevak-test ordinary"));

        let before = clipboard_sequence();
        clipboard
            .set()
            .exclude_from_monitoring()
            .text("sevak-test secret".to_owned())
            .unwrap();
        assert_ne!(clipboard_sequence(), before, "the sequence number advances");
        let secret = read_clipboard().unwrap();
        assert!(secret.sensitive);
        assert_eq!(secret.text, None);

        clipboard
            .set()
            .exclude_from_cloud()
            .text("sevak-test cloud".to_owned())
            .unwrap();
        // `exclude_from_cloud` writes CanUploadToCloudClipboard = 0.
        assert!(read_clipboard().unwrap().sensitive);

        clipboard
            .set()
            .exclude_from_history()
            .text("sevak-test history".to_owned())
            .unwrap();
        assert!(read_clipboard().unwrap().sensitive);

        if let Some(saved) = saved {
            let _ = clipboard.set_text(saved);
        }
    }

    /// Pastes into a real window: two edit-control windows, the first one
    /// remembered, the second one focused, then `paste_text`. Needs an
    /// interactive desktop and steals focus for a moment, so it is run by hand:
    /// `cargo test -p sevak-platform paste_reaches_the_remembered_window -- --ignored`
    #[test]
    #[ignore = "needs an interactive desktop"]
    fn paste_reaches_the_remembered_window() {
        use std::sync::mpsc;
        use windows::core::PCWSTR;
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, DispatchMessageW, GetMessageW, GetWindowTextW,
            PostMessageW, TranslateMessage, MSG, WINDOW_EX_STYLE, WM_CLOSE, WS_OVERLAPPEDWINDOW,
            WS_VISIBLE,
        };

        fn spawn_edit(title: &'static str) -> (isize, std::thread::JoinHandle<()>) {
            let (tx, rx) = mpsc::channel();
            let handle = std::thread::spawn(move || unsafe {
                let title: Vec<u16> = title.encode_utf16().chain([0]).collect();
                let hwnd = CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("EDIT"),
                    PCWSTR(title.as_ptr()),
                    WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                    100,
                    100,
                    400,
                    200,
                    None,
                    None,
                    None,
                    None,
                )
                .expect("create an edit window");
                tx.send(hwnd_to_int(hwnd)).unwrap();
                let mut message = MSG::default();
                while GetMessageW(&mut message, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                let _ = DestroyWindow(hwnd);
            });
            (rx.recv().unwrap(), handle)
        }

        let (target, target_thread) = spawn_edit("sevak paste target");
        let (other, other_thread) = spawn_edit("sevak other window");
        let target_hwnd = int_to_hwnd(target);
        let other_hwnd = int_to_hwnd(other);

        // Focus the target first, remember it, then move focus away.
        assert!(focus(target_hwnd).is_ok(), "could not focus the target");
        *REMEMBERED.lock().unwrap() = Some(target);
        assert!(
            focus(other_hwnd).is_ok(),
            "could not focus the other window"
        );

        let outcome = paste_text("pasted by sevak \u{2713}", false).unwrap();
        sleep(Duration::from_millis(300));

        let mut buffer = [0u16; 128];
        let len = unsafe { GetWindowTextW(target_hwnd, &mut buffer) } as usize;
        let text = String::from_utf16_lossy(&buffer[..len.min(buffer.len())]);
        let other_len = unsafe { GetWindowTextW(other_hwnd, &mut buffer) } as usize;
        let other_text = String::from_utf16_lossy(&buffer[..other_len.min(buffer.len())]);

        for hwnd in [target_hwnd, other_hwnd] {
            unsafe {
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, Default::default(), Default::default());
            }
        }
        drop((target_thread, other_thread));

        assert_eq!(outcome, PasteOutcome::Pasted);
        // Inserted at the caret, in front of the title text the control started with.
        assert_eq!(text, "pasted by sevak \u{2713}sevak paste target");
        assert_eq!(other_text, "sevak other window");
    }
}
