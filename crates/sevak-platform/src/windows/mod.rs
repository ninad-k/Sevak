//! Win32 calls used by the rest of the crate. Only compiled on Windows.

mod capture;
mod com;
mod icons;
pub(crate) mod media;
mod packaged;
mod paste;
mod provider;
mod shortcuts;
pub(crate) mod system;
pub(crate) mod tasks;

pub(crate) use provider::WindowsProvider;

use std::ffi::OsStr;
use std::fmt;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
use windows::Win32::UI::Shell::{
    ShellExecuteExW, ShellExecuteW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY, SW_SHOWNORMAL};

use crate::error::PlatformError;

/// `SE_ERR_NOASSOC`: no application is associated with the file type.
const SE_ERR_NOASSOC: isize = 31;

pub(crate) fn allow_foreground_handoff() {
    // SAFETY: plain Win32 call taking a process id (or ASFW_ANY); no pointers.
    if let Err(err) = unsafe { AllowSetForegroundWindow(ASFW_ANY) } {
        tracing::debug!(%err, "AllowSetForegroundWindow failed");
    }
}

pub(crate) fn attach_parent_console() {
    // SAFETY: plain Win32 call taking a process id sentinel; no pointers.
    // Failure is normal (no parent console, or a console is already attached),
    // so the result is intentionally ignored.
    let _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

/// The failure code `ShellExecuteW` returned (a value `<= 32`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ShellExecuteError {
    pub(crate) code: isize,
}

impl ShellExecuteError {
    /// True when the target exists but no program is registered to open it.
    pub(crate) fn is_no_association(&self) -> bool {
        self.code == SE_ERR_NOASSOC
    }

    fn describe(&self) -> String {
        match self.code {
            0 => "out of memory or resources".to_owned(),
            2 => "file not found".to_owned(),
            3 => "path not found".to_owned(),
            5 => "access denied".to_owned(),
            SE_ERR_NOASSOC => "no application is associated with this file type".to_owned(),
            code => format!("error code {code}"),
        }
    }
}

impl fmt::Display for ShellExecuteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.describe())
    }
}

impl std::error::Error for ShellExecuteError {}

impl From<ShellExecuteError> for PlatformError {
    fn from(err: ShellExecuteError) -> Self {
        PlatformError::Os {
            operation: "ShellExecuteW",
            message: err.describe(),
        }
    }
}

/// Runs `verb` (e.g. `"open"`) on `target` through the shell, honouring file
/// associations, protocol handlers and `App Paths`.
pub(crate) fn shell_execute(
    verb: &str,
    target: &OsStr,
    parameters: Option<&OsStr>,
) -> Result<(), ShellExecuteError> {
    let verb = HSTRING::from(verb);
    let target = HSTRING::from(target);
    let parameters = parameters.map(HSTRING::from);
    let parameters = parameters
        .as_ref()
        .map_or(PCWSTR::null(), |p| PCWSTR(p.as_ptr()));

    // SAFETY: every string is a NUL-terminated UTF-16 buffer (HSTRING) that
    // outlives the call; null is allowed for the window handle, parameters and
    // working directory.
    let result = unsafe {
        ShellExecuteW(
            None,
            &verb,
            &target,
            parameters,
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };

    // Values above 32 mean success; the HINSTANCE is a legacy status code.
    let code = result.0 as isize;
    if code > 32 {
        Ok(())
    } else {
        Err(ShellExecuteError { code })
    }
}

/// Like [`shell_execute`], but with a working directory and without any shell
/// error dialogs (`SEE_MASK_FLAG_NO_UI`), so a broken shortcut cannot pop a
/// modal box over the launcher. COM must be initialized on the calling thread.
pub(crate) fn shell_execute_in(
    verb: &str,
    target: &OsStr,
    parameters: Option<&OsStr>,
    directory: Option<&OsStr>,
) -> Result<(), ShellExecuteError> {
    let verb = HSTRING::from(verb);
    let target = HSTRING::from(target);
    let parameters = parameters.map(HSTRING::from);
    let directory = directory.map(HSTRING::from);
    let as_pcwstr = |s: &Option<HSTRING>| s.as_ref().map_or(PCWSTR::null(), |s| PCWSTR(s.as_ptr()));

    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_FLAG_NO_UI | SEE_MASK_NOASYNC,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(target.as_ptr()),
        lpParameters: as_pcwstr(&parameters),
        lpDirectory: as_pcwstr(&directory),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };

    // SAFETY: `info` is fully initialized with the correct `cbSize`; every
    // string pointer refers to a NUL-terminated HSTRING that outlives the call.
    let result = unsafe { ShellExecuteExW(&mut info) };
    match result {
        Ok(()) => Ok(()),
        Err(_) => {
            // On failure `hInstApp` carries the legacy SE_ERR_* / Win32 code.
            let code = info.hInstApp.0 as isize;
            Err(ShellExecuteError {
                code: if code > 32 { 0 } else { code },
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_association_is_code_31_only() {
        assert!(ShellExecuteError { code: 31 }.is_no_association());
        assert!(!ShellExecuteError { code: 2 }.is_no_association());
    }

    #[test]
    fn converts_to_os_error_with_readable_message() {
        let err: PlatformError = ShellExecuteError { code: 5 }.into();
        assert!(err.to_string().contains("access denied"));
    }
}
