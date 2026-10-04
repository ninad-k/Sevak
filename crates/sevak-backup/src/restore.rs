//! Restoring a backup: preview, apply with a safety snapshot, undo.
//!
//! The order of a restore is fixed so that a failure never leaves a mix:
//!
//! 1. the backup was already checked by [`crate::archive::read`];
//! 2. [`crate::plan::build`] works out the changes and the configuration that
//!    would result, and the caller's validator checks it;
//! 3. a **safety snapshot** of the current state of the chosen categories is
//!    written to the data folder (if that fails, nothing is touched);
//! 4. approvals of the scripts that are being replaced are dropped, so they ask
//!    again (the approval file itself is never part of a backup);
//! 5. [`crate::apply::execute`] swaps the files in, all or nothing.

use std::fs;
use std::path::PathBuf;

use serde::Serialize;
use sevak_core::config::Snippet;
use sevak_core::config::DEFAULT_CONFIG_TOML;
use sevak_core::Config;
use sevak_plugins::script::ApprovalStore;
use toml_edit::{DocumentMut, Item as EditItem};

use crate::apply::{execute, Hooks, Op};
use crate::archive::{build, read_file, ArchiveInfo, Backup, Meta};
use crate::category::{Category, Mode, SNIPPET_KEY};
use crate::collect::collect;
use crate::error::{Error, Result};
use crate::limits::KEEP_SNAPSHOTS;
use crate::manifest::{Kind, EXTENSION};
use crate::plan::{self, CategoryPreview, Plan};
use crate::util::{write_unique, Roots, Stamp};

/// Checks a configuration before it is written (the shell passes the same
/// checks the Settings window applies).
pub type Validator<'a> = &'a dyn Fn(&Config) -> std::result::Result<(), String>;

/// What the user is told before a restore.
#[derive(Debug, Clone, Serialize)]
pub struct Preview {
    pub archive: ArchiveInfo,
    pub mode: Mode,
    pub categories: Vec<CategoryPreview>,
    pub warnings: Vec<String>,
    /// Scripts, plugins and workflows that will ask for approval again.
    pub needs_approval: Vec<String>,
    /// Why the restore would be refused, if it would be.
    pub problem: Option<String>,
    pub nothing_to_change: bool,
}

fn chosen(backup: &Backup, categories: &[Category]) -> Vec<Category> {
    Category::ALL
        .into_iter()
        .filter(|c| categories.contains(c) && backup.has(*c))
        .collect()
}

/// What restoring `categories` of `backup` in `mode` would change. Writes
/// nothing.
pub fn preview(
    roots: &Roots,
    backup: &Backup,
    categories: &[Category],
    mode: Mode,
    validate: Validator<'_>,
) -> Result<Preview> {
    let selected = chosen(backup, categories);
    let plan = plan::build(roots, backup, &selected, mode, validate)?;
    let problem = plan.validation_error.clone().or_else(|| {
        plan.previews
            .iter()
            .filter(|p| p.selected)
            .find_map(|p| p.problem.clone())
    });
    let nothing_to_change = !plan
        .previews
        .iter()
        .filter(|p| p.selected)
        .any(CategoryPreview::changes_something);
    Ok(Preview {
        archive: backup.info(),
        mode,
        categories: plan.previews,
        warnings: plan.warnings,
        needs_approval: plan.needs_approval,
        problem,
        nothing_to_change,
    })
}

/// How to apply a restore.
pub struct RestoreOptions<'a> {
    pub validate: Validator<'a>,
    pub stamp: Stamp,
    /// The `sha256` the preview reported; refuses a file that changed since.
    pub expect_sha256: Option<&'a str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RestoreReport {
    pub mode: Mode,
    /// What was applied (the selected categories).
    pub categories: Vec<CategoryPreview>,
    /// The safety copy "Undo restore" uses.
    pub snapshot: Option<PathBuf>,
    pub needs_approval: Vec<String>,
    pub warnings: Vec<String>,
    /// False when the backup held nothing that differed.
    pub changed: bool,
}

/// Applies the chosen categories of `backup`.
pub fn restore(
    roots: &Roots,
    backup: &Backup,
    categories: &[Category],
    mode: Mode,
    options: &RestoreOptions<'_>,
) -> Result<RestoreReport> {
    restore_with(
        roots,
        backup,
        categories,
        mode,
        options,
        Hooks::default(),
        true,
    )
}

pub(crate) fn restore_with(
    roots: &Roots,
    backup: &Backup,
    categories: &[Category],
    mode: Mode,
    options: &RestoreOptions<'_>,
    hooks: Hooks,
    take_snapshot: bool,
) -> Result<RestoreReport> {
    if let Some(expected) = options.expect_sha256 {
        if expected != backup.sha256 {
            return Err(Error::Changed);
        }
    }
    let selected = chosen(backup, categories);
    if selected.is_empty() {
        return Err(Error::Nothing(
            "choose at least one category that is in the backup".to_owned(),
        ));
    }

    let plan = plan::build(roots, backup, &selected, mode, options.validate)?;
    if let Some(problem) = plan
        .previews
        .iter()
        .filter(|p| p.selected)
        .find_map(|p| p.problem.clone())
    {
        return Err(Error::Current(problem));
    }
    if let Some(why) = &plan.validation_error {
        return Err(Error::Rejected(why.clone()));
    }

    let mut ops = plan.ops.clone();
    if let Some(text) = render_config(roots, &plan)? {
        ops.push(Op::WriteFile {
            target: roots.config_file.clone(),
            data: text.into_bytes(),
        });
    }
    let applied: Vec<CategoryPreview> = plan
        .previews
        .iter()
        .filter(|p| p.selected)
        .cloned()
        .collect();
    if ops.is_empty() {
        return Ok(RestoreReport {
            mode,
            categories: applied,
            snapshot: None,
            needs_approval: Vec::new(),
            warnings: plan.warnings,
            changed: false,
        });
    }

    let snapshot = if take_snapshot {
        Some(create_snapshot(roots, &selected, &options.stamp)?)
    } else {
        None
    };

    if !plan.revoke.is_empty() {
        let mut ids = plan.revoke.clone();
        ids.sort();
        ids.dedup();
        ApprovalStore::new(roots.approvals_file())
            .revoke(&ids)
            .map_err(|err| Error::Apply {
                message: format!(
                    "could not reset the script approvals, so nothing was changed: {err}"
                ),
                rolled_back: true,
            })?;
    }

    execute(&roots.config_dir, &ops, hooks)?;
    if snapshot.is_some() {
        prune_snapshots(roots);
    }
    Ok(RestoreReport {
        mode,
        categories: applied,
        snapshot,
        needs_approval: plan.needs_approval,
        warnings: plan.warnings,
        changed: true,
    })
}

#[derive(serde::Serialize)]
struct SnippetsOut<'a> {
    snippet: &'a [Snippet],
}

/// `text` with its `[[snippet]]` entries replaced.
fn set_snippets(text: &str, snippets: &[Snippet]) -> Result<String> {
    let rejected =
        |err: &dyn std::fmt::Display| Error::Rejected(format!("cannot write the snippets: {err}"));
    let mut document: DocumentMut = text.parse().map_err(|e| rejected(&e))?;
    document.remove(SNIPPET_KEY);
    if !snippets.is_empty() {
        let rendered = toml_edit::ser::to_document(&SnippetsOut { snippet: snippets })
            .map_err(|e| rejected(&e))?;
        if let Some(item) = rendered.get(SNIPPET_KEY) {
            let tables = item
                .clone()
                .into_array_of_tables()
                .map_err(|_| rejected(&"unexpected snippet layout"))?;
            document.insert(SNIPPET_KEY, EditItem::ArrayOfTables(tables));
        }
    }
    let mut out = document.to_string();
    if text.contains("\r\n") {
        out = out.replace("\r\n", "\n").replace('\n', "\r\n");
    }
    Ok(out)
}

/// The new text of `config.toml`, or `None` when it stays as it is. The change
/// is made on a copy next to the configuration by the same code the Settings
/// window saves with, so comments and key order survive and keys Sevak does not
/// know are left alone.
fn render_config(roots: &Roots, plan: &Plan) -> Result<Option<String>> {
    let (Some(state), Some(after)) = (&plan.config_state, &plan.after) else {
        return Ok(None);
    };
    let base = state
        .text
        .clone()
        .unwrap_or_else(|| DEFAULT_CONFIG_TOML.to_owned());
    let mut text = base;
    if let Some(snippets) = &plan.snippets_after {
        text = set_snippets(&text, snippets)?;
    }

    let scratch = roots
        .config_dir
        .join(format!(".sevak-render-{}", std::process::id()));
    let file = scratch.join("config.toml");
    let rendered = (|| -> std::io::Result<String> {
        fs::create_dir_all(&scratch)?;
        fs::write(&file, &text)?;
        after
            .save_to(&file)
            .map_err(|err| std::io::Error::other(err.to_string()))?;
        fs::read_to_string(&file)
    })();
    let _ = fs::remove_dir_all(&scratch);
    let rendered =
        rendered.map_err(|err| Error::Rejected(format!("cannot prepare config.toml: {err}")))?;
    if state.text.as_deref() == Some(rendered.as_str()) {
        Ok(None)
    } else {
        Ok(Some(rendered))
    }
}

// ---------------------------------------------------------------------------
// Safety snapshots and undo
// ---------------------------------------------------------------------------

const SNAPSHOT_PREFIX: &str = "pre-restore-";

/// A safety copy waiting for "Undo restore".
#[derive(Debug, Clone, Serialize)]
pub struct SnapshotInfo {
    pub path: PathBuf,
    pub created: String,
    pub created_unix: i64,
    pub categories: Vec<Category>,
}

/// Saves the current state of `categories` as a snapshot.
pub fn create_snapshot(roots: &Roots, categories: &[Category], stamp: &Stamp) -> Result<PathBuf> {
    let collected = collect(roots, categories).map_err(|err| Error::Snapshot(err.to_string()))?;
    let bytes = build(
        &collected.items,
        &Meta {
            kind: Kind::Snapshot,
            categories,
            restore_of: categories,
            stamp,
        },
    )
    .map_err(|err| Error::Snapshot(err.to_string()))?;
    write_unique(
        &roots.snapshots_dir(),
        &format!("{SNAPSHOT_PREFIX}{}", stamp.compact),
        EXTENSION,
        &bytes,
    )
    .map_err(|err| Error::Snapshot(err.to_string()))
}

fn snapshot_files(roots: &Roots) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(roots.snapshots_dir()) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with(SNAPSHOT_PREFIX) && name.ends_with(&format!(".{EXTENSION}"))
                })
        })
        .collect();
    // Names carry the date and time, so the newest sorts last.
    files.sort();
    files
}

fn prune_snapshots(roots: &Roots) {
    let files = snapshot_files(roots);
    if files.len() > KEEP_SNAPSHOTS {
        for old in &files[..files.len() - KEEP_SNAPSHOTS] {
            let _ = fs::remove_file(old);
        }
    }
}

/// The newest safety copy, if "Undo restore" has anything to undo.
pub fn latest_snapshot(roots: &Roots) -> Option<SnapshotInfo> {
    snapshot_files(roots).into_iter().rev().find_map(|path| {
        let backup = read_file(&path).ok()?;
        Some(SnapshotInfo {
            path,
            created: backup.manifest.created.clone(),
            created_unix: backup.manifest.created_unix,
            categories: backup.manifest.categories.clone(),
        })
    })
}

/// Puts back the state the newest snapshot holds, for the categories that
/// restore touched, and drops the snapshot.
pub fn undo_restore(roots: &Roots, stamp: Stamp) -> Result<RestoreReport> {
    undo_with(roots, stamp, Hooks::default())
}

pub(crate) fn undo_with(roots: &Roots, stamp: Stamp, hooks: Hooks) -> Result<RestoreReport> {
    let info = latest_snapshot(roots)
        .ok_or_else(|| Error::Nothing("there is no restore to undo".to_owned()))?;
    let backup = read_file(&info.path)?;
    let categories = backup.manifest.categories.clone();
    // What the snapshot holds was in use before, so it needs no new checks.
    let options = RestoreOptions {
        validate: &|_| Ok(()),
        stamp,
        expect_sha256: None,
    };
    let report = restore_with(
        roots,
        &backup,
        &categories,
        Mode::Replace,
        &options,
        hooks,
        false,
    )?;
    let _ = fs::remove_file(&info.path);
    Ok(report)
}
