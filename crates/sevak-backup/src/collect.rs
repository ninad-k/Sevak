//! Reading the current configuration into backup items.
//!
//! Only what [`crate::category::classify`] allows is ever read into an item.
//! Anything else in the folders (and every section of `config.toml` that is not
//! allowlisted) is left where it is.

use std::fs;
use std::path::Path;

use serde::Serialize;
use sevak_core::config::WebSearchEngine;
use sevak_core::theme_store::MAX_THEME_BYTES;
use sevak_core::Config;
use sevak_plugins::workflow::model::valid_folder_name;

use crate::category::{
    classify, config_key_category, is_sensitive_key, is_sensitive_name, Category, CSS_DIR,
    LEFT_OUT_TABLES, SETTINGS_FILE, SKIPPED_DIRS, SNIPPETS_FILE, THEMES_DIR, WEB_SEARCH_FILE,
};
use crate::error::{Error, Result};
use crate::item::{dir_of, validate_folder, Item};
use crate::limits::{MAX_FILE_BYTES, MAX_FOLDER_FILES, MAX_TOTAL_BYTES};
use crate::util::Roots;

/// What [`collect`] found.
#[derive(Debug, Default)]
pub struct Collected {
    pub items: Vec<Item>,
    /// Things worth telling the user: a plugin that was skipped and why.
    pub warnings: Vec<String>,
    /// `config.toml` sections that are not in the backup on purpose.
    pub left_out: Vec<String>,
}

impl Collected {
    pub fn of(&self, category: Category) -> impl Iterator<Item = &Item> {
        self.items
            .iter()
            .filter(move |item| item.category == category)
    }
}

/// The text of `config.toml`, or `None` when there is no such file yet.
pub fn read_config_text(roots: &Roots) -> Result<Option<String>> {
    match fs::read_to_string(&roots.config_file) {
        Ok(text) => Ok(Some(text)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(Error::io("cannot read config.toml", err)),
    }
}

/// Parses `config.toml` text. A mistake in it is reported plainly: nothing
/// that touches the configuration can go on until the file is fixed.
pub fn parse_config(text: &str) -> Result<toml::Table> {
    text.parse::<toml::Table>().map_err(|err| {
        Error::Current(format!(
            "your config.toml has a mistake ({}). Fix it first, or leave Settings, Snippets \
             and Web search engines out.",
            err.message()
        ))
    })
}

/// Drops every key that looks like a secret, at any depth. What was dropped is
/// added to `removed` (as `section.key`).
pub fn scrub(value: &mut toml::Value, path: &str, removed: &mut Vec<String>) {
    match value {
        toml::Value::Table(table) => {
            let keys: Vec<String> = table.keys().cloned().collect();
            for key in keys {
                let here = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                if is_sensitive_key(&key) {
                    table.remove(&key);
                    removed.push(here);
                } else if let Some(child) = table.get_mut(&key) {
                    scrub(child, &here, removed);
                }
            }
        }
        toml::Value::Array(items) => {
            for item in items {
                scrub(item, path, removed);
            }
        }
        _ => {}
    }
}

/// The keys of `table` that belong to `category`, scrubbed of anything that
/// looks like a secret. Keys of other categories and keys nobody listed are
/// left out; the latter are returned in `unknown`.
pub fn fragment(
    table: &toml::Table,
    category: Category,
    removed: &mut Vec<String>,
    unknown: &mut Vec<String>,
) -> toml::Table {
    let mut out = toml::Table::new();
    for (key, value) in table {
        match config_key_category(key) {
            Some(owner) if owner == category => {
                let mut value = value.clone();
                scrub(&mut value, key, removed);
                out.insert(key.clone(), value);
            }
            Some(_) => {}
            None => unknown.push(key.clone()),
        }
    }
    out
}

#[derive(Serialize)]
struct WebSearchOut<'a> {
    web_search: &'a [WebSearchEngine],
}

/// The web search engines that are in effect (the defaults when the file does
/// not list any), written out explicitly.
fn web_search_fragment(text: Option<&str>) -> Result<toml::Table> {
    let config = match text {
        Some(text) => Config::from_toml_str(text).map_err(|err| {
            Error::Current(format!(
                "your config.toml has a mistake ({}). Fix it first, or leave Settings, Snippets \
                 and Web search engines out.",
                err.message()
            ))
        })?,
        None => Config::default(),
    };
    toml::Table::try_from(WebSearchOut {
        web_search: &config.web_search,
    })
    .map_err(|err| Error::Current(format!("cannot write the web search engines: {err}")))
}

fn table_text(table: &toml::Table) -> Result<Vec<u8>> {
    toml::to_string_pretty(table)
        .map(String::into_bytes)
        .map_err(|err| Error::Current(format!("cannot write the settings: {err}")))
}

/// Reads the current state of `categories` into items.
pub fn collect(roots: &Roots, categories: &[Category]) -> Result<Collected> {
    let mut out = Collected::default();
    let wants = |category: Category| categories.contains(&category);

    if wants(Category::Settings) || wants(Category::Snippets) || wants(Category::WebSearch) {
        let text = read_config_text(roots)?;
        let table = match &text {
            Some(text) => parse_config(text)?,
            None => toml::Table::new(),
        };
        let mut removed = Vec::new();
        let mut unknown = Vec::new();
        for (category, file) in [
            (Category::Settings, SETTINGS_FILE),
            (Category::Snippets, SNIPPETS_FILE),
        ] {
            if !wants(category) {
                continue;
            }
            let part = fragment(&table, category, &mut removed, &mut unknown);
            if !part.is_empty() {
                out.items.push(Item {
                    path: file.to_owned(),
                    category,
                    data: table_text(&part)?,
                    executable: false,
                });
            }
        }
        if wants(Category::WebSearch) {
            let part = web_search_fragment(text.as_deref())?;
            out.items.push(Item {
                path: WEB_SEARCH_FILE.to_owned(),
                category: Category::WebSearch,
                data: table_text(&part)?,
                executable: false,
            });
        }
        // Reported once, not once per category.
        unknown.sort();
        unknown.dedup();
        out.left_out = LEFT_OUT_TABLES
            .iter()
            .map(|name| (*name).to_owned())
            .filter(|name| table.contains_key(name))
            .chain(unknown)
            .map(|name| format!("[{name}]"))
            .collect();
        for key in removed {
            out.warnings.push(format!(
                "Left out {key}: its name suggests it holds a secret."
            ));
        }
    }

    if wants(Category::Themes) {
        collect_themes(roots, &mut out)?;
    }
    for category in [Category::Plugins, Category::Workflows] {
        if wants(category) {
            let root = if category == Category::Plugins {
                roots.plugins_dir()
            } else {
                roots.workflows_dir()
            };
            collect_folders(&root, category, &mut out)?;
        }
    }

    out.items.sort_by(|a, b| a.path.cmp(&b.path));
    let total: u64 = out.items.iter().map(|item| item.data.len() as u64).sum();
    if total > MAX_TOTAL_BYTES {
        return Err(Error::Current(format!(
            "the files to back up add up to more than {} MiB; leave some categories out",
            MAX_TOTAL_BYTES / (1024 * 1024)
        )));
    }
    Ok(out)
}

#[cfg(unix)]
fn is_executable(meta: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_meta: &fs::Metadata) -> bool {
    false
}

/// Reads a regular file (never through a link) of at most `limit` bytes.
fn read_regular(path: &Path, limit: u64) -> std::result::Result<Option<(Vec<u8>, bool)>, String> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        // A missing file is the user's business, not a problem to report.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.to_string()),
    };
    if !meta.file_type().is_file() {
        return Ok(None);
    }
    if meta.len() > limit {
        return Err(format!("it is larger than {} KiB", limit / 1024));
    }
    let data = fs::read(path).map_err(|err| err.to_string())?;
    Ok(Some((data, is_executable(&meta))))
}

fn collect_themes(roots: &Roots, out: &mut Collected) -> Result<()> {
    let dir = roots.themes_dir();
    if let Ok(entries) = fs::read_dir(&dir) {
        let mut names: Vec<String> = entries
            .filter_map(std::result::Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect();
        names.sort();
        for name in names {
            if !name.ends_with(".toml") {
                continue;
            }
            let path = format!("{THEMES_DIR}/{name}");
            if classify(&path) != Some(Category::Themes) {
                out.warnings.push(format!(
                    "The theme file {name} was not included: its name is not allowed."
                ));
                continue;
            }
            match read_regular(&dir.join(&name), MAX_THEME_BYTES) {
                Ok(Some((data, _))) => {
                    if let Err(why) = crate::item::validate_file(&path, Category::Themes, &data) {
                        out.warnings
                            .push(format!("The theme file {name} was not included: {why}."));
                    } else {
                        out.items.push(Item {
                            path,
                            category: Category::Themes,
                            data,
                            executable: false,
                        });
                    }
                }
                Ok(None) => {}
                Err(why) => out
                    .warnings
                    .push(format!("The theme file {name} was not included: {why}.")),
            }
        }
    }

    // The custom stylesheet the settings name, when it is a plain file name.
    let css = match read_config_text(roots) {
        Ok(Some(text)) => parse_config(&text).ok().and_then(|table| {
            table
                .get("appearance")
                .and_then(|a| a.get("custom_css"))
                .and_then(toml::Value::as_str)
                .map(|name| name.trim().to_owned())
        }),
        _ => None,
    };
    if let Some(name) = css.filter(|name| !name.is_empty()) {
        let path = format!("{CSS_DIR}/{name}");
        if name.contains('/') || name.contains('\\') || classify(&path) != Some(Category::Themes) {
            out.warnings.push(format!(
                "The stylesheet {name} was not included: only a plain .css file in the config \
                 folder is backed up."
            ));
        } else {
            match read_regular(&roots.config_dir.join(&name), MAX_THEME_BYTES) {
                Ok(Some((data, _))) => {
                    if crate::item::validate_file(&path, Category::Themes, &data).is_ok() {
                        out.items.push(Item {
                            path,
                            category: Category::Themes,
                            data,
                            executable: false,
                        });
                    } else {
                        out.warnings.push(format!(
                            "The stylesheet {name} was not included: it is not text."
                        ));
                    }
                }
                Ok(None) => {}
                Err(why) => out
                    .warnings
                    .push(format!("The stylesheet {name} was not included: {why}.")),
            }
        }
    }
    Ok(())
}

/// One walk over a plugin or workflow folder.
struct Walk<'a> {
    category: Category,
    folder: &'a str,
    files: Vec<Item>,
    /// A reason the whole folder cannot be included.
    fatal: Option<String>,
    skipped: Vec<String>,
}

fn walk(dir: &Path, prefix: &str, depth: usize, state: &mut Walk<'_>) {
    if state.fatal.is_some() {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        state.fatal = Some("it cannot be read".to_owned());
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(std::result::Result::ok).collect();
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let Ok(name) = entry.file_name().into_string() else {
            state
                .skipped
                .push("a file with an unreadable name".to_owned());
            continue;
        };
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        let kind = meta.file_type();
        if kind.is_symlink() {
            state.skipped.push(format!("{rel} (a link)"));
        } else if kind.is_dir() {
            if name.starts_with('.') || SKIPPED_DIRS.contains(&name.as_str()) {
                continue;
            }
            if depth >= 6 {
                state.fatal = Some("its folders are nested too deeply".to_owned());
                return;
            }
            walk(&path, &rel, depth + 1, state);
        } else if kind.is_file() {
            if name.ends_with(".pyc") || name.ends_with(".pyo") {
                continue;
            }
            if is_sensitive_name(&name) {
                state
                    .skipped
                    .push(format!("{rel} (its name suggests it holds a secret)"));
                continue;
            }
            if meta.len() > MAX_FILE_BYTES {
                state.fatal = Some(format!(
                    "{rel} is larger than {} MiB",
                    MAX_FILE_BYTES / (1024 * 1024)
                ));
                return;
            }
            if state.files.len() >= MAX_FOLDER_FILES {
                state.fatal = Some(format!("it has more than {MAX_FOLDER_FILES} files"));
                return;
            }
            let item_path = format!("{}/{}/{rel}", dir_of(state.category), state.folder);
            if classify(&item_path) != Some(state.category) {
                state
                    .skipped
                    .push(format!("{rel} (its name is not allowed)"));
                continue;
            }
            match fs::read(&path) {
                Ok(data) => state.files.push(Item {
                    path: item_path,
                    category: state.category,
                    data,
                    executable: is_executable(&meta),
                }),
                Err(err) => {
                    state.fatal = Some(format!("{rel} cannot be read: {err}"));
                    return;
                }
            }
        }
    }
}

fn collect_folders(root: &Path, category: Category, out: &mut Collected) -> Result<()> {
    let Ok(entries) = fs::read_dir(root) else {
        return Ok(());
    };
    let what = if category == Category::Plugins {
        "The script plugin"
    } else {
        "The workflow"
    };
    let mut folders: Vec<String> = entries
        .filter_map(std::result::Result::ok)
        .filter(|entry| {
            fs::symlink_metadata(entry.path()).is_ok_and(|meta| meta.file_type().is_dir())
        })
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    folders.sort();
    for folder in folders {
        if folder.starts_with('.') {
            continue;
        }
        if !valid_folder_name(&folder) {
            out.warnings.push(format!(
                "{what} folder \"{folder}\" was not included: its name may only use letters, \
                 digits, spaces and - _ ."
            ));
            continue;
        }
        let mut state = Walk {
            category,
            folder: &folder,
            files: Vec::new(),
            fatal: None,
            skipped: Vec::new(),
        };
        walk(&root.join(&folder), "", 0, &mut state);
        let Walk {
            files,
            fatal,
            skipped,
            ..
        } = state;
        if let Some(why) = fatal {
            out.warnings
                .push(format!("{what} \"{folder}\" was not included: {why}."));
            continue;
        }
        let prefix = format!("{}/{folder}/", dir_of(category));
        let map = files
            .iter()
            .map(|item| (item.path[prefix.len()..].to_owned(), item.data.as_slice()))
            .collect();
        if let Err(why) = validate_folder(category, &folder, &map) {
            out.warnings
                .push(format!("{what} \"{folder}\" was not included: {why}."));
            continue;
        }
        if !skipped.is_empty() {
            out.warnings.push(format!(
                "{what} \"{folder}\": left out {}.",
                skipped.join(", ")
            ));
        }
        out.items.extend(files);
    }
    Ok(())
}
