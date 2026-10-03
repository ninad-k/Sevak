//! Windows power and session commands.

use std::path::PathBuf;

use windows::core::PCWSTR;
use windows::Win32::System::Power::{
    GetPwrCapabilities, SetSuspendState, SYSTEM_POWER_CAPABILITIES,
};
use windows::Win32::System::Shutdown::LockWorkStation;
use windows::Win32::UI::Shell::{
    SHEmptyRecycleBinW, SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND,
};

use crate::error::{PlatformError, Result};
use crate::process::run_checked;
use crate::system::GRACE;

fn os_error(operation: &'static str, err: impl std::fmt::Display) -> PlatformError {
    PlatformError::Os {
        operation,
        message: err.to_string(),
    }
}

/// Whether hibernation is switched on (it needs the hibernation file, which
/// `powercfg /hibernate on` creates).
pub(crate) fn can_hibernate() -> bool {
    let mut caps = SYSTEM_POWER_CAPABILITIES::default();
    // SAFETY: `caps` is a valid, writable SYSTEM_POWER_CAPABILITIES.
    let ok = unsafe { GetPwrCapabilities(&mut caps) };
    ok && caps.SystemS4 && caps.HiberFilePresent
}

pub(crate) fn lock() -> Result<()> {
    // SAFETY: plain Win32 call without arguments.
    unsafe { LockWorkStation() }.map_err(|err| os_error("LockWorkStation", err))
}

pub(crate) fn sleep() -> Result<()> {
    // Suspend (not hibernate), without forcing apps and with wake events on.
    // SAFETY: plain Win32 call taking three flags.
    if unsafe { SetSuspendState(false, false, false) } {
        Ok(())
    } else {
        Err(os_error(
            "SetSuspendState",
            windows::core::Error::from_thread(),
        ))
    }
}

pub(crate) fn empty_recycle_bin() -> Result<()> {
    // SAFETY: no window, no drive filter (all drives), flags only.
    unsafe {
        SHEmptyRecycleBinW(
            None,
            PCWSTR::null(),
            SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND,
        )
    }
    .map_err(|err| os_error("SHEmptyRecycleBinW", err))
}

/// Runs the system's `shutdown.exe` (by absolute path, so nothing in the
/// working directory or `PATH` can stand in for it).
pub(crate) fn run_shutdown(args: &[&str]) -> Result<()> {
    run_checked(&shutdown_exe(), args, GRACE)
}

fn shutdown_exe() -> String {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let path: PathBuf = [
        root.as_os_str(),
        "System32".as_ref(),
        "shutdown.exe".as_ref(),
    ]
    .iter()
    .collect();
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutdown_exe_is_the_system32_one_and_exists() {
        let path = shutdown_exe();
        assert!(path
            .to_ascii_lowercase()
            .ends_with(r"system32\shutdown.exe"));
        assert!(std::path::Path::new(&path).is_file());
    }
}
