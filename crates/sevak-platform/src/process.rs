//! Process-level helpers: spawning detached children, console attachment and
//! foreground-window hand-off.

use std::env;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

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

/// Applies the environment children must start with.
///
/// On Wayland, Sevak sets `GDK_BACKEND=x11` in its own process so that its
/// window runs under XWayland (see [`crate::session::prefer_xwayland`]). If
/// children inherited that, every application launched from Sevak would also be
/// forced onto XWayland (blurry on fractional scaling, no native Wayland
/// features). So when Sevak set the variable itself, it is removed here.
fn apply_child_env(command: &mut Command, xwayland_forced: bool) {
    if xwayland_forced {
        command.env_remove("GDK_BACKEND");
    }
}

fn detached_command<S: AsRef<OsStr>>(program: &str, args: &[S], cwd: Option<&Path>) -> Command {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    apply_child_env(&mut command, crate::session::xwayland_forced());
    command
}

fn spawn_error(program: &str, err: std::io::Error) -> PlatformError {
    PlatformError::CommandFailed {
        command: program.to_owned(),
        message: err.to_string(),
    }
}

/// Spawns `program` fully detached; see [`spawn_detached_in`].
pub fn spawn_detached<S: AsRef<OsStr>>(program: &str, args: &[S]) -> Result<()> {
    spawn_detached_in(program, args, None)
}

/// Spawns `program` fully detached: null stdio, its own process group (so a
/// Ctrl+C or SIGHUP aimed at Sevak does not reach it), and a reaper thread that
/// waits on the child so it never lingers as a zombie. `cwd` is the working
/// directory, if not Sevak's own.
#[cfg(unix)]
pub fn spawn_detached_in<S: AsRef<OsStr>>(
    program: &str,
    args: &[S],
    cwd: Option<&Path>,
) -> Result<()> {
    use std::os::unix::process::CommandExt;

    let mut child = detached_command(program, args, cwd)
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
/// process group. Windows has no zombies, so the child is not waited on. `cwd`
/// is the working directory, if not Sevak's own.
#[cfg(windows)]
pub fn spawn_detached_in<S: AsRef<OsStr>>(
    program: &str,
    args: &[S],
    cwd: Option<&Path>,
) -> Result<()> {
    use std::os::windows::process::CommandExt;

    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

    detached_command(program, args, cwd)
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .map(drop)
        .map_err(|err| spawn_error(program, err))
}

/// Runs `program` and reports an immediate failure.
///
/// Waits up to `grace` for the program to exit. A non-zero exit inside that
/// time becomes a [`PlatformError::CommandFailed`] carrying the program's
/// stderr; a program still running afterwards (a slow job, or a window that
/// stays open) is left to finish in the background and counts as started.
pub fn run_checked<S: AsRef<OsStr>>(program: &str, args: &[S], grace: Duration) -> Result<()> {
    use std::io::Read;
    use std::sync::mpsc;

    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    apply_child_env(&mut command, crate::session::xwayland_forced());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Console tools such as shutdown.exe would flash a console window.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().map_err(|err| spawn_error(program, err))?;

    // A thread owns the child so a slow program never blocks the caller and is
    // still reaped; it reports the exit through the channel if anyone listens.
    let (tx, rx) = mpsc::channel();
    let name = program.to_owned();
    let waiter = std::thread::Builder::new()
        .name("sevak-run".into())
        .spawn(move || {
            let mut stderr = String::new();
            if let Some(mut pipe) = child.stderr.take() {
                let _ = pipe.read_to_string(&mut stderr);
            }
            let status = child.wait();
            if let Err(err) = &status {
                tracing::debug!(program = %name, %err, "waiting on child failed");
            }
            let _ = tx.send((status, stderr));
        });
    if let Err(err) = waiter {
        tracing::debug!(program, %err, "could not start waiter thread");
        return Ok(());
    }

    match rx.recv_timeout(grace) {
        Ok((Ok(status), _)) if status.success() => Ok(()),
        Ok((Ok(status), stderr)) => {
            let stderr = stderr.trim();
            Err(PlatformError::CommandFailed {
                command: program.to_owned(),
                message: if stderr.is_empty() {
                    status.to_string()
                } else {
                    format!("{status}: {stderr}")
                },
            })
        }
        Ok((Err(err), _)) => Err(spawn_error(program, err)),
        // Still running, or the waiter vanished: nothing failed yet.
        Err(_) => Ok(()),
    }
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
    fn gdk_backend_is_removed_only_when_sevak_forced_it() {
        let removed = |forced: bool| {
            let mut command = Command::new("x");
            apply_child_env(&mut command, forced);
            command
                .get_envs()
                .any(|(key, value)| key == "GDK_BACKEND" && value.is_none())
        };
        assert!(removed(true));
        assert!(!removed(false));
    }

    fn shell(script: &str) -> (&'static str, Vec<String>) {
        if cfg!(windows) {
            ("cmd", vec!["/c".into(), script.into()])
        } else {
            ("sh", vec!["-c".into(), script.into()])
        }
    }

    #[test]
    fn run_checked_succeeds_for_a_clean_exit() {
        let (program, args) = shell("exit 0");
        assert!(run_checked(program, &args, Duration::from_secs(10)).is_ok());
    }

    #[test]
    fn run_checked_reports_a_failure_with_its_stderr() {
        let (program, args) = shell(if cfg!(windows) {
            "echo boom 1>&2 & exit 3"
        } else {
            "echo boom >&2; exit 3"
        });
        let err = run_checked(program, &args, Duration::from_secs(10)).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("boom"), "{text}");
        assert!(matches!(err, PlatformError::CommandFailed { .. }));
    }

    #[test]
    fn run_checked_does_not_wait_for_a_slow_program() {
        let (program, args) = if cfg!(windows) {
            (
                "cmd",
                vec!["/c".to_owned(), "ping -n 6 127.0.0.1 >nul".to_owned()],
            )
        } else {
            ("sh", vec!["-c".to_owned(), "sleep 5".to_owned()])
        };
        let started = std::time::Instant::now();
        assert!(run_checked(program, &args, Duration::from_millis(100)).is_ok());
        assert!(started.elapsed() < Duration::from_secs(4));
    }

    #[test]
    fn run_checked_of_missing_program_is_an_error() {
        let result = run_checked(
            "sevak-definitely-not-a-real-program",
            &[] as &[&str],
            Duration::from_secs(1),
        );
        assert!(matches!(result, Err(PlatformError::CommandFailed { .. })));
    }

    #[test]
    fn spawn_of_missing_program_is_an_error() {
        let result = spawn_detached("sevak-definitely-not-a-real-program", &[] as &[&str]);
        assert!(matches!(result, Err(PlatformError::CommandFailed { .. })));
    }
}
