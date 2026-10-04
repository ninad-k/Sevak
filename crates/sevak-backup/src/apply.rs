//! Applying a restore as one unit.
//!
//! A restore touches several files and folders. None of that can be a single
//! file-system operation, so it is done in two phases:
//!
//! 1. **Stage.** Everything that will be written is written into a staging
//!    folder next to the configuration. If anything fails here, nothing in the
//!    configuration has been touched.
//! 2. **Swap.** Each target is moved aside into the staging folder and the
//!    staged copy is renamed into place (renames within one folder tree are
//!    atomic and cheap). Every step is journalled; if one fails, the journal is
//!    played backwards, which puts the previous files back. The configuration
//!    file goes last, so a failure before it never leaves new settings pointing
//!    at old files.
//!
//! The previous state is also in the safety snapshot taken before this runs,
//! which is what "Undo restore" uses.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{Error, Result};

/// One file to write.
#[derive(Debug, Clone)]
pub(crate) struct StagedFile {
    /// Path inside the folder (forward slashes).
    pub rel: String,
    pub data: Vec<u8>,
    pub executable: bool,
}

/// One change to the configuration folder.
#[derive(Debug, Clone)]
pub(crate) enum Op {
    /// Replace a whole folder by these files.
    ReplaceDir {
        target: PathBuf,
        files: Vec<StagedFile>,
    },
    /// Replace (or create) one file.
    WriteFile {
        target: PathBuf,
        data: Vec<u8>,
    },
    Remove {
        target: PathBuf,
    },
}

impl Op {
    fn target(&self) -> &Path {
        match self {
            Self::ReplaceDir { target, .. }
            | Self::WriteFile { target, .. }
            | Self::Remove { target } => target,
        }
    }
}

/// For tests: make the swap fail after this many steps.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Hooks {
    pub fail_after: Option<usize>,
}

#[derive(Debug)]
enum Step {
    /// `target` was moved to `old`.
    Moved { target: PathBuf, old: PathBuf },
    /// A staged copy was renamed to `target`.
    Placed { target: PathBuf },
}

fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn remove_any(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

fn write_staged(path: &Path, data: &[u8], executable: bool) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Only the owner's execute bit survives: never setuid and friends.
        let mode = if executable { 0o755 } else { 0o644 };
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    let _ = executable;
    Ok(())
}

fn unique_name() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!(".sevak-restore-{}-{nanos}", std::process::id())
}

/// Runs `ops` as described in the module documentation. `ops` must list the
/// configuration file last.
pub(crate) fn execute(config_dir: &Path, ops: &[Op], hooks: Hooks) -> Result<()> {
    if ops.is_empty() {
        return Ok(());
    }
    let staging = config_dir.join(unique_name());
    fs::create_dir_all(&staging).map_err(|err| Error::io("cannot prepare the restore", err))?;
    let new_root = staging.join("new");
    let old_root = staging.join("old");

    // Phase 1: stage.
    let staged = stage(ops, &new_root);
    if let Err(err) = staged.and_then(|()| fs::create_dir_all(&old_root)) {
        let _ = fs::remove_dir_all(&staging);
        return Err(Error::Apply {
            message: format!("could not prepare the restore, so nothing was changed: {err}"),
            rolled_back: true,
        });
    }

    // Phase 2: swap, with a journal.
    let mut journal: Vec<Step> = Vec::new();
    let mut counter = 0usize;
    let swapped = swap(ops, &new_root, &old_root, &mut journal, &mut counter, hooks);
    match swapped {
        Ok(()) => {
            let _ = fs::remove_dir_all(&staging);
            Ok(())
        }
        Err(cause) => {
            let failures = roll_back(&mut journal);
            if failures.is_empty() {
                let _ = fs::remove_dir_all(&staging);
                Err(Error::Apply {
                    message: format!(
                        "the restore failed and your settings are as they were: {cause}"
                    ),
                    rolled_back: true,
                })
            } else {
                // Leave the staging folder: it holds the files that could not be put back.
                Err(Error::Apply {
                    message: format!(
                        "the restore failed ({cause}) and some files could not be put back \
                         ({}). Use \"Undo restore\", or find the originals in {}",
                        failures.join("; "),
                        staging.join("old").display()
                    ),
                    rolled_back: false,
                })
            }
        }
    }
}

fn stage(ops: &[Op], new_root: &Path) -> io::Result<()> {
    for (n, op) in ops.iter().enumerate() {
        let slot = new_root.join(n.to_string());
        match op {
            Op::ReplaceDir { files, .. } => {
                fs::create_dir_all(&slot)?;
                for file in files {
                    write_staged(&slot.join(&file.rel), &file.data, file.executable)?;
                }
            }
            Op::WriteFile { data, .. } => write_staged(&slot, data, false)?,
            Op::Remove { .. } => {}
        }
    }
    Ok(())
}

fn step(counter: &mut usize, hooks: Hooks) -> io::Result<()> {
    *counter += 1;
    if hooks.fail_after == Some(*counter) {
        return Err(io::Error::other("injected failure"));
    }
    Ok(())
}

fn swap(
    ops: &[Op],
    new_root: &Path,
    old_root: &Path,
    journal: &mut Vec<Step>,
    counter: &mut usize,
    hooks: Hooks,
) -> io::Result<()> {
    for (n, op) in ops.iter().enumerate() {
        let target = op.target();
        if exists(target) {
            step(counter, hooks)?;
            let old = old_root.join(n.to_string());
            fs::rename(target, &old)?;
            journal.push(Step::Moved {
                target: target.to_path_buf(),
                old,
            });
        }
        if !matches!(op, Op::Remove { .. }) {
            step(counter, hooks)?;
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::rename(new_root.join(n.to_string()), target)?;
            journal.push(Step::Placed {
                target: target.to_path_buf(),
            });
        }
    }
    Ok(())
}

/// Plays the journal backwards. Returns what could not be undone.
fn roll_back(journal: &mut Vec<Step>) -> Vec<String> {
    let mut failures = Vec::new();
    while let Some(step) = journal.pop() {
        match step {
            Step::Placed { target } => {
                if let Err(err) = remove_any(&target) {
                    failures.push(format!("{}: {err}", target.display()));
                }
            }
            Step::Moved { target, old } => {
                if let Err(err) = fs::rename(&old, &target) {
                    failures.push(format!("{}: {err}", target.display()));
                }
            }
        }
    }
    failures
}
