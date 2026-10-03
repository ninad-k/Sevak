//! Process-level helpers: spawning detached children, console attachment and
//! foreground-window hand-off.

use std::env;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::error::{PlatformError, Result};

/// Returns the first executable named `program` on `PATH`.
///
/// On Windows a bare name without an extension also matches `<name>.exe`, since
/// users type `notepad` rather than `notepad.exe`.
pub fn find_in_path(program: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    let try_exe = cfg!(windows) && !program.contains('.');
    env::split_paths(&path).find_map(|dir| {
        let candidate = dir.join(program);
        if candidate.is_file() {
            return Some(candidate);
        }
        if try_exe {
            let with_exe = dir.join(format!("{program}.exe"));
            if with_exe.is_file() {
                return Some(with_exe);
            }
        }
        None
    })
}

fn detached_command<S: AsRef<OsStr>>(program: &str, args: &[S]) -> Command {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

fn spawn_error(program: &str, err: std::io::Error) -> PlatformError {
    PlatformError::CommandFailed {
        command: program.to_owned(),
        message: err.to_string(),
    }
}

/// Spawns `program` fully detached: null stdio, its own process group (so a
/// Ctrl+C or SIGHUP aimed at Sevak does not reach it), and a reaper thread that
/// waits on the child so it never lingers as a zombie.
#[cfg(unix)]
pub fn spawn_detached<S: AsRef<OsStr>>(program: &str, args: &[S]) -> Result<()> {
    use std::os::unix::process::CommandExt;

    let mut child = detached_command(program, args)
        .process_group(0)
        .spawn()
        .map_err(|err| spawn_error(program, err))?;

    let name = program.to_owned();
    let reaper = std::thread::Builder::new()
        .name("sevak-reaper".into())
        .spawn(move || match child.wait() {
            Ok(status) if status.success() => {}
            Ok(status) => tracing::debug!(program = %name, %status, "detached child exited"),
            Err(err) => tracing::debug!(program = %name, %err, "waiting on detached child failed"),
        });
    if let Err(err) = reaper {
        // The child is already running; losing the reaper only risks a zombie.
        tracing::debug!(program, %err, "could not start reaper thread");
    }
    Ok(())
}

/// Spawns `program` fully detached: null stdio, no inherited console and its own
/// process group. Windows has no zombies, so the child is not waited on.
#[cfg(windows)]
pub fn spawn_detached<S: AsRef<OsStr>>(program: &str, args: &[S]) -> Result<()> {
    use std::os::windows::process::CommandExt;

    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

    detached_command(program, args)
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .map(drop)
        .map_err(|err| spawn_error(program, err))
}

/// Lets the already-running Sevak instance bring its window to the foreground
/// when this (second) process forwards it `--toggle`.
///
/// Windows only lets the foreground process activate windows; a freshly launched
/// second instance has that right and passes it on. No-op elsewhere.
pub fn allow_foreground_handoff() {
    #[cfg(windows)]
    crate::windows::allow_foreground_handoff();
}

/// Attaches to the parent console so release builds (GUI subsystem, no console)
/// can print `--help` output. No-op elsewhere.
pub fn attach_parent_console() {
    #[cfg(windows)]
    crate::windows::attach_parent_console();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_nothing_for_nonsense_program() {
        assert!(find_in_path("sevak-definitely-not-a-real-program").is_none());
    }

    #[cfg(windows)]
    #[test]
    fn finds_cmd_without_extension() {
        assert!(find_in_path("cmd").is_some());
    }

    #[cfg(unix)]
    #[test]
    fn finds_sh() {
        assert!(find_in_path("sh").is_some());
    }

    #[test]
    fn spawn_of_missing_program_is_an_error() {
        let result = spawn_detached("sevak-definitely-not-a-real-program", &[] as &[&str]);
        assert!(matches!(result, Err(PlatformError::CommandFailed { .. })));
    }
}
