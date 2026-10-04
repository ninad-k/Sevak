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
    find_in_dirs(env::split_paths(&path), program)
}

/// [`find_in_path`] over an explicit list of folders. An empty entry (a stray
/// `::` or a trailing `:`) or a relative one would mean "the working
/// directory" to a shell; here it is skipped, so a file that happens to sit
/// next to the process can never stand in for a program.
fn find_in_dirs(dirs: impl Iterator<Item = PathBuf>, program: &str) -> Option<PathBuf> {
    let try_exe = cfg!(windows) && !program.contains('.');
    dirs.filter(|dir| dir.is_absolute()).find_map(|dir| {
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

/// Prepares a long-lived helper child that Sevak talks to over pipes (a script
/// plugin): the same child environment as [`spawn_detached`], and on Windows no
/// console window, which would otherwise flash for every console program.
pub fn configure_helper_command(command: &mut Command) {
    apply_child_env(command, crate::session::xwayland_forced());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
}

/// How to run a script file, by its extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptRunner {
    /// Run `<command...> <script path>`; the command is on `PATH`.
    Interpreter(Vec<String>),
    /// No interpreter is needed: start the file itself (an executable, a batch
    /// file, or a script with a shebang line on Unix).
    Direct,
    /// The extension needs an interpreter and none of these is installed.
    Missing(Vec<String>),
}

/// Picks how to run a script file with extension `extension` (no dot, any
/// case). Python runs unbuffered (`-u`), so output reaches Sevak without the
/// script having to flush.
pub fn script_runner(extension: &str) -> ScriptRunner {
    let candidates = interpreter_candidates(extension, cfg!(windows));
    if candidates.is_empty() {
        return ScriptRunner::Direct;
    }
    let names = candidates.iter().map(|c| c[0].to_owned()).collect();
    candidates
        .into_iter()
        .find(|candidate| find_in_path(candidate[0]).is_some())
        .map_or(ScriptRunner::Missing(names), |candidate| {
            ScriptRunner::Interpreter(candidate.into_iter().map(str::to_owned).collect())
        })
}

/// The interpreters to try for `extension`, best first. On Windows `python` and
/// `python3` may be Microsoft Store stubs that open the Store instead of
/// running anything, so the `py` launcher (which only exists for a real Python
/// install) goes first.
fn interpreter_candidates(extension: &str, windows: bool) -> Vec<Vec<&'static str>> {
    match (extension.to_ascii_lowercase().as_str(), windows) {
        ("py", true) => vec![
            vec!["py", "-3", "-u"],
            vec!["python", "-u"],
            vec!["python3", "-u"],
        ],
        ("py", false) => vec![vec!["python3", "-u"], vec!["python", "-u"]],
        ("ps1", true) => vec![
            vec!["pwsh", "-NoProfile", "-NonInteractive", "-File"],
            vec![
                "powershell",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ],
        ],
        ("ps1", false) => vec![vec!["pwsh", "-NoProfile", "-NonInteractive", "-File"]],
        ("js" | "mjs" | "cjs", _) => vec![vec!["node"]],
        ("sh", false) => vec![vec!["sh"]],
        _ => Vec::new(),
    }
}

/// The program to give [`Command::new`].
///
/// On Unix a bare name (no `/`) is replaced by the file [`find_in_path`] finds,
/// and is `NotFound` when `PATH` has none: handed to `exec` as it is, an empty
/// `PATH` entry would make it look in the working directory, where a file of
/// that name could be waiting. A name with a separator is returned as given,
/// and so is everything on Windows (where Rust's own lookup never searches the
/// working directory).
pub fn pin_program(program: &str) -> std::io::Result<String> {
    pin_with(program, cfg!(unix), &find_in_path)
}

fn pin_with(
    program: &str,
    unix: bool,
    find: &dyn Fn(&str) -> Option<PathBuf>,
) -> std::io::Result<String> {
    if !unix || program.contains(['/', '\\']) {
        return Ok(program.to_owned());
    }
    find(program)
        .map(|path| path.to_string_lossy().into_owned())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("`{program}` was not found on PATH"),
            )
        })
}

fn detached_command<S: AsRef<OsStr>>(
    program: &str,
    args: &[S],
    cwd: Option<&Path>,
) -> Result<Command> {
    let mut command = Command::new(pin_program(program).map_err(|err| spawn_error(program, err))?);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    apply_child_env(&mut command, crate::session::xwayland_forced());
    Ok(command)
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

    let mut child = detached_command(program, args, cwd)?
        .process_group(0)
        .spawn()
        .map_err(|err| spawn_error(program, err))?;

    let reaper = std::thread::Builder::new()
        .name("sevak-reaper".into())
        .spawn(move || match child.wait() {
            Ok(status) if status.success() => {}
            Ok(status) => tracing::debug!(%status, "detached child exited"),
            Err(err) => tracing::debug!(%err, "waiting on detached child failed"),
        });
    if let Err(err) = reaper {
        // The child is already running; losing the reaper only risks a zombie.
        tracing::debug!(%err, "could not start reaper thread");
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

    detached_command(program, args, cwd)?
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

    let mut command = Command::new(pin_program(program).map_err(|err| spawn_error(program, err))?);
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
    let waiter = std::thread::Builder::new()
        .name("sevak-run".into())
        .spawn(move || {
            let mut stderr = String::new();
            if let Some(mut pipe) = child.stderr.take() {
                let _ = pipe.read_to_string(&mut stderr);
            }
            let status = child.wait();
            if let Err(err) = &status {
                tracing::debug!(%err, "waiting on child failed");
            }
            let _ = tx.send((status, stderr));
        });
    if let Err(err) = waiter {
        tracing::debug!(%err, "could not start waiter thread");
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

/// Spawns a console program (a shell) in a new console window of its own.
///
/// Unlike [`spawn_detached_in`] the child's standard handles are *not* nulled:
/// the new console supplies them, and a shell with stdin redirected to `NUL`
/// would exit at once. Sevak is a GUI process without a console, so there are no
/// inherited handles to leak. `raw_tail` is appended to the command line as is
/// (see [`crate::terminal::Invocation::raw_tail`]).
#[cfg(windows)]
pub fn spawn_console_in<S: AsRef<OsStr>>(
    program: &str,
    args: &[S],
    raw_tail: Option<&str>,
    cwd: Option<&Path>,
) -> Result<()> {
    use std::os::windows::process::CommandExt;

    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

    let mut command = Command::new(program);
    command.args(args);
    if let Some(raw_tail) = raw_tail {
        command.raw_arg(raw_tail);
    }
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    command
        .creation_flags(CREATE_NEW_CONSOLE | CREATE_NEW_PROCESS_GROUP)
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

    #[test]
    fn bare_program_names_are_pinned_to_the_file_found_on_path_on_unix() {
        let found = |name: &str| (name == "tool").then(|| PathBuf::from("/usr/bin/tool"));
        // Unix: a bare name becomes the file PATH found; none found is an error.
        assert_eq!(pin_with("tool", true, &found).unwrap(), "/usr/bin/tool");
        let err = pin_with("other", true, &found).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        // A path is the caller's own choice, and Windows keeps Rust's lookup.
        assert_eq!(
            pin_with("/opt/x/tool", true, &found).unwrap(),
            "/opt/x/tool"
        );
        assert_eq!(pin_with("./tool", true, &found).unwrap(), "./tool");
        assert_eq!(pin_with("other", false, &found).unwrap(), "other");
    }

    #[test]
    fn empty_and_relative_path_entries_never_match() {
        // `cargo test` runs in the crate folder, so `Cargo.toml` is a file that
        // an empty or relative entry would find.
        assert!(Path::new("Cargo.toml").is_file());
        let relative = [PathBuf::new(), PathBuf::from("."), PathBuf::from("./")];
        assert_eq!(find_in_dirs(relative.into_iter(), "Cargo.toml"), None);
        // An absolute entry still works, wherever the empty one sits.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("sevak-probe"), b"x").unwrap();
        let found = find_in_dirs(
            [PathBuf::new(), dir.path().to_path_buf()].into_iter(),
            "sevak-probe",
        );
        assert_eq!(found, Some(dir.path().join("sevak-probe")));
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
    fn interpreter_by_extension() {
        let first = |ext, windows| interpreter_candidates(ext, windows).remove(0);
        assert_eq!(first("py", true), ["py", "-3", "-u"]);
        assert_eq!(first("PY", false), ["python3", "-u"]);
        assert_eq!(first("js", true), ["node"]);
        assert_eq!(first("ps1", true)[0], "pwsh");
        assert!(interpreter_candidates("sh", true).is_empty());
        assert!(interpreter_candidates("exe", true).is_empty());
        assert!(interpreter_candidates("", false).is_empty());
        assert_eq!(script_runner("exe"), ScriptRunner::Direct);
        // Whatever is found must be a command that exists.
        match script_runner("py") {
            ScriptRunner::Interpreter(argv) => assert!(find_in_path(&argv[0]).is_some()),
            ScriptRunner::Missing(names) => assert!(!names.is_empty()),
            ScriptRunner::Direct => panic!("python scripts need an interpreter"),
        }
    }

    #[test]
    fn spawn_of_missing_program_is_an_error() {
        let result = spawn_detached("sevak-definitely-not-a-real-program", &[] as &[&str]);
        assert!(matches!(result, Err(PlatformError::CommandFailed { .. })));
    }
}
