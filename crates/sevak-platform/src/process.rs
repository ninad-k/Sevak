//! Process-level helpers: spawning detached children, console attachment and
//! foreground-window hand-off.

use std::env;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
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
