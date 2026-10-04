//! Making a backup.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::archive::{build, Meta};
use crate::category::{split_folder, Category};
use crate::collect::{collect, Collected};
use crate::error::{Error, Result};
use crate::item::Item;
use crate::manifest::{Kind, EXTENSION};
use crate::state::{LastBackup, State};
use crate::util::{write_unique, Roots, Stamp};

/// What is (or would be) in one category of a backup, for people to read.
#[derive(Debug, Clone, Serialize)]
pub struct CategoryContents {
    pub category: Category,
    pub label: &'static str,
    /// Sections, theme files, or plugin and workflow folders.
    pub entries: Vec<String>,
    pub files: usize,
    pub bytes: u64,
}

/// What a backup of some categories would hold.
#[derive(Debug, Clone, Serialize)]
pub struct Contents {
    pub categories: Vec<CategoryContents>,
    pub warnings: Vec<String>,
    /// `config.toml` sections that stay out on purpose.
    pub left_out: Vec<String>,
    pub bytes: u64,
}

fn entries_of(category: Category, items: &[&Item]) -> Vec<String> {
    match category {
        Category::Settings | Category::Snippets | Category::WebSearch => items
            .iter()
            .flat_map(|item| {
                std::str::from_utf8(&item.data)
                    .ok()
                    .and_then(|text| text.parse::<toml::Table>().ok())
                    .map(|table| {
                        table
                            .iter()
                            .map(|(key, value)| match value {
                                toml::Value::Array(list) => format!("[[{key}]] ({})", list.len()),
                                _ => format!("[{key}]"),
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            })
            .collect(),
        Category::Themes => items
            .iter()
            .map(|item| {
                item.path
                    .split_once('/')
                    .map_or(item.path.clone(), |(_, name)| name.to_owned())
            })
            .collect(),
        Category::Plugins | Category::Workflows => {
            let mut folders: BTreeMap<&str, usize> = BTreeMap::new();
            for item in items {
                if let Some((folder, _)) = split_folder(&item.path) {
                    *folders.entry(folder).or_default() += 1;
                }
            }
            folders
                .into_iter()
                .map(|(folder, files)| {
                    format!(
                        "{folder} ({files} file{})",
                        if files == 1 { "" } else { "s" }
                    )
                })
                .collect()
        }
    }
}

fn describe(collected: &Collected, categories: &[Category]) -> Contents {
    let mut out = Vec::new();
    let mut total = 0;
    for category in categories {
        let items: Vec<&Item> = collected.of(*category).collect();
        let bytes: u64 = items.iter().map(|item| item.data.len() as u64).sum();
        total += bytes;
        out.push(CategoryContents {
            category: *category,
            label: category.label(),
            entries: entries_of(*category, &items),
            files: items.len(),
            bytes,
        });
    }
    Contents {
        categories: out,
        warnings: collected.warnings.clone(),
        left_out: collected.left_out.clone(),
        bytes: total,
    }
}

/// What a backup of `categories` would contain right now. Writes nothing.
pub fn contents(roots: &Roots, categories: &[Category]) -> Result<Contents> {
    let collected = collect(roots, categories)?;
    Ok(describe(&collected, categories))
}

/// Where a new backup goes.
#[derive(Debug, Clone, Copy)]
pub enum Destination<'a> {
    /// Exactly this file (`.sevakbackup` is added when there is no extension).
    File(&'a Path),
    /// A new file in this folder, named `<prefix>-<date>-<time>.sevakbackup`.
    Folder { dir: &'a Path, prefix: &'a str },
}

/// The result of making a backup.
#[derive(Debug, Clone, Serialize)]
pub struct BackupReport {
    pub path: PathBuf,
    pub bytes: u64,
    pub created: String,
    pub created_unix: i64,
    pub kind: Kind,
    pub contents: Contents,
}

/// The default name of a manual backup: `sevak-backup-20261004-153045.sevakbackup`.
pub fn default_file_name(stamp: &Stamp) -> String {
    format!("sevak-backup-{}.{EXTENSION}", stamp.compact)
}

/// Makes a backup of `categories` and writes it. The file is written whole or
/// not at all, and is readable by its owner only on Unix. The backup is
/// plaintext: it is not encrypted.
pub fn create(
    roots: &Roots,
    categories: &[Category],
    kind: Kind,
    stamp: &Stamp,
    destination: Destination<'_>,
) -> Result<BackupReport> {
    if categories.is_empty() {
        return Err(Error::Nothing(
            "choose at least one thing to back up".to_owned(),
        ));
    }
    let collected = collect(roots, categories)?;
    let bytes = build(
        &collected.items,
        &Meta {
            kind,
            categories,
            restore_of: &[],
            stamp,
        },
    )?;
    let contents = describe(&collected, categories);

    let path = match destination {
        Destination::File(path) => {
            let mut path = path.to_path_buf();
            if path.extension().is_none() {
                path.set_extension(EXTENSION);
            }
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)
                    .map_err(|err| Error::io("cannot create the backup folder", err))?;
            }
            sevak_platform::private_file::write_atomic(&path, &bytes)
                .map_err(|err| Error::io("cannot write the backup", err))?;
            path
        }
        Destination::Folder { dir, prefix } => write_unique(
            dir,
            &format!("{prefix}-{}", stamp.compact),
            EXTENSION,
            &bytes,
        )
        .map_err(|err| Error::io("cannot write the backup", err))?,
    };

    if kind != Kind::Snapshot {
        let mut state = State::load(roots);
        state.last = Some(LastBackup {
            path: path.display().to_string(),
            unix: stamp.unix,
            created: stamp.iso.clone(),
            kind,
            categories: categories.to_vec(),
        });
        if kind == Kind::Auto {
            state.last_auto_unix = Some(stamp.unix);
        }
        if let Err(err) = state.save(roots) {
            tracing::warn!("the backup was written but its record was not: {err}");
        }
    }
    Ok(BackupReport {
        path,
        bytes: bytes.len() as u64,
        created: stamp.iso.clone(),
        created_unix: stamp.unix,
        kind,
        contents,
    })
}
