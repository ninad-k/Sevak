//! The archive: writing a backup, and reading one that may have come from
//! anywhere.
//!
//! A backup is a zip file (`.sevakbackup`) holding `manifest.json` and the
//! files the manifest lists. [`read`] trusts nothing in it:
//!
//! - the whole file, every entry and the unpacked total are capped, and entries
//!   are read with a hard limit, whatever size the zip claims;
//! - every path must pass the allowlist ([`classify`]): no `..`, absolute or
//!   drive paths, backslashes, hidden or reserved names, links or devices;
//! - the manifest must be ours and not newer, list every file exactly once and
//!   match each file's size and SHA-256;
//! - settings, snippets and web searches are parsed against the real schema,
//!   themes, plugin manifests and workflows are validated like their own
//!   loaders do, and secrets-looking keys and unknown sections are dropped.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::Path;

use serde::Serialize;
use sevak_core::config::{Snippet, WebSearchEngine};
use sevak_core::Config;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::category::{
    classify, split_folder, Category, NEVER_INCLUDED_TEXT, SNIPPET_KEY, WEB_SEARCH_KEY,
};
use crate::collect::{fragment, parse_config};
use crate::error::{Error, Result};
use crate::item::{validate_file, validate_folder, Item};
use crate::limits::{
    MAX_ARCHIVE_BYTES, MAX_CONFIG_BYTES, MAX_ENTRIES, MAX_FILE_BYTES, MAX_FOLDER_FILES,
    MAX_MANIFEST_BYTES, MAX_TOTAL_BYTES,
};
use crate::manifest::{FileEntry, Kind, Manifest, FORMAT_NAME, FORMAT_VERSION, MANIFEST_PATH};
use crate::util::{platform_name, sha256_hex, Stamp};

/// A backup that passed every check.
#[derive(Debug, Clone)]
pub struct Backup {
    pub manifest: Manifest,
    /// The files, sorted by path.
    pub items: Vec<Item>,
    /// SHA-256 of the archive file, hex. A restore passes it back to be sure it
    /// applies the file that was previewed.
    pub sha256: String,
    /// Size of the archive file.
    pub size: u64,
    /// Things the restore will ignore (unknown sections, secret-looking keys).
    pub warnings: Vec<String>,
    /// The settings, snippets and web search parts, parsed, filtered to their
    /// own sections and scrubbed.
    pub fragments: BTreeMap<Category, toml::Table>,
}

impl Backup {
    pub fn has(&self, category: Category) -> bool {
        self.manifest.categories.contains(&category)
    }

    pub fn of(&self, category: Category) -> impl Iterator<Item = &Item> {
        self.items
            .iter()
            .filter(move |item| item.category == category)
    }
}

/// What goes into a new manifest besides the files.
pub struct Meta<'a> {
    pub kind: Kind,
    pub categories: &'a [Category],
    pub restore_of: &'a [Category],
    pub stamp: &'a Stamp,
}

fn write_error(err: impl std::fmt::Display) -> Error {
    Error::Current(format!("cannot write the backup: {err}"))
}

/// Builds the archive for `items`.
pub fn build(items: &[Item], meta: &Meta<'_>) -> Result<Vec<u8>> {
    let mut sorted: Vec<&Item> = items.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    let mut categories = meta.categories.to_vec();
    categories.sort();
    categories.dedup();

    let manifest = Manifest {
        format: FORMAT_NAME.to_owned(),
        version: FORMAT_VERSION,
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        created_unix: meta.stamp.unix,
        created: meta.stamp.iso.clone(),
        platform: platform_name().to_owned(),
        kind: meta.kind,
        categories,
        files: sorted
            .iter()
            .map(|item| FileEntry {
                path: item.path.clone(),
                category: item.category,
                size: item.data.len() as u64,
                sha256: sha256_hex(&item.data),
            })
            .collect(),
        restore_of: meta.restore_of.to_vec(),
        never_included: NEVER_INCLUDED_TEXT
            .iter()
            .map(|s| (*s).to_owned())
            .collect(),
    };

    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let base = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    writer
        .start_file(MANIFEST_PATH, base.unix_permissions(0o644))
        .map_err(write_error)?;
    let json = serde_json::to_vec_pretty(&manifest).map_err(write_error)?;
    writer.write_all(&json).map_err(write_error)?;
    for item in sorted {
        let mode = if item.executable { 0o755 } else { 0o644 };
        writer
            .start_file(item.path.as_str(), base.unix_permissions(mode))
            .map_err(write_error)?;
        writer.write_all(&item.data).map_err(write_error)?;
    }
    let cursor = writer.finish().map_err(write_error)?;
    Ok(cursor.into_inner())
}

fn damaged(err: impl std::fmt::Display) -> Error {
    Error::invalid(format!("the backup file is damaged ({err})"))
}

/// Opens a backup file. The size is checked before anything is read.
pub fn read_file(path: &Path) -> Result<Backup> {
    let file = fs::File::open(path).map_err(|err| Error::io("cannot open the backup", err))?;
    let meta = file
        .metadata()
        .map_err(|err| Error::io("cannot read the backup", err))?;
    if !meta.is_file() {
        return Err(Error::invalid("that is not a file"));
    }
    if meta.len() > MAX_ARCHIVE_BYTES {
        return Err(Error::invalid(format!(
            "the file is larger than {} MiB, so it is not a Sevak backup",
            MAX_ARCHIVE_BYTES / (1024 * 1024)
        )));
    }
    let mut bytes = Vec::new();
    file.take(MAX_ARCHIVE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| Error::io("cannot read the backup", err))?;
    read(&bytes)
}

/// Checks and unpacks the bytes of a backup. See the module documentation.
pub fn read(bytes: &[u8]) -> Result<Backup> {
    if bytes.len() as u64 > MAX_ARCHIVE_BYTES {
        return Err(Error::invalid("the file is too large to be a Sevak backup"));
    }
    let mut zip = ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| Error::invalid("this is not a Sevak backup, or the file is damaged"))?;
    if zip.len() > MAX_ENTRIES {
        return Err(Error::invalid("the backup has too many entries"));
    }

    // Pass 1: bytes out of the zip, under hard caps.
    let mut manifest_bytes: Option<Vec<u8>> = None;
    let mut files: BTreeMap<String, (Vec<u8>, bool)> = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut total: u64 = 0;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(damaged)?;
        if entry.encrypted() {
            return Err(Error::invalid(
                "the backup is encrypted, which Sevak does not support",
            ));
        }
        let name = entry.name().to_owned();
        let kind = entry.unix_mode().map(|mode| mode & 0o170_000);
        match kind {
            // Regular file or no Unix mode at all (made on Windows).
            None | Some(0o100_000) | Some(0) => {}
            Some(0o040_000) => continue,
            Some(0o120_000) => {
                return Err(Error::invalid(format!(
                    "the backup contains a link ({name}), which is not allowed"
                )))
            }
            Some(_) => {
                return Err(Error::invalid(format!(
                    "the backup contains a special file ({name}), which is not allowed"
                )))
            }
        }
        if entry.is_dir() {
            continue;
        }
        if !seen.insert(name.to_lowercase()) {
            return Err(Error::invalid(format!("the backup lists {name} twice")));
        }
        let cap = if name == MANIFEST_PATH {
            MAX_MANIFEST_BYTES
        } else {
            MAX_FILE_BYTES
        };
        if entry.size() > cap {
            return Err(Error::invalid(format!("{name} is too large")));
        }
        let executable = entry.unix_mode().is_some_and(|mode| mode & 0o111 != 0);
        let mut data = Vec::new();
        (&mut entry)
            .take(cap + 1)
            .read_to_end(&mut data)
            .map_err(damaged)?;
        if data.len() as u64 > cap {
            return Err(Error::invalid(format!("{name} is too large")));
        }
        total += data.len() as u64;
        if total > MAX_TOTAL_BYTES {
            return Err(Error::invalid(
                "the backup unpacks to far more data than it should",
            ));
        }
        if name == MANIFEST_PATH {
            manifest_bytes = Some(data);
        } else {
            files.insert(name, (data, executable));
        }
    }

    // Pass 2: the manifest, and that it describes exactly these files.
    let manifest_bytes =
        manifest_bytes.ok_or_else(|| Error::invalid("this is not a Sevak backup (no manifest)"))?;
    let manifest = Manifest::parse(&manifest_bytes)?;
    if manifest.files.len() > MAX_ENTRIES {
        return Err(Error::invalid("the backup lists too many files"));
    }
    let mut listed = BTreeSet::new();
    let mut items = Vec::with_capacity(files.len());
    for entry in &manifest.files {
        if !listed.insert(entry.path.as_str()) {
            return Err(Error::invalid(format!(
                "the manifest lists {} twice",
                entry.path
            )));
        }
        match classify(&entry.path) {
            Some(category) if category == entry.category => {}
            _ => {
                return Err(Error::invalid(format!(
                    "the backup contains a file that does not belong in a backup ({})",
                    entry.path
                )))
            }
        }
        if !manifest.categories.contains(&entry.category) {
            return Err(Error::invalid(format!(
                "{} is in a category the backup does not declare",
                entry.path
            )));
        }
        let (data, executable) = files.remove(&entry.path).ok_or_else(|| {
            Error::invalid(format!(
                "the backup is incomplete: {} is missing",
                entry.path
            ))
        })?;
        if data.len() as u64 != entry.size || sha256_hex(&data) != entry.sha256 {
            return Err(Error::invalid(format!(
                "{} does not match its checksum; the backup is damaged or was changed",
                entry.path
            )));
        }
        items.push(Item {
            path: entry.path.clone(),
            category: entry.category,
            data,
            executable,
        });
    }
    if let Some(extra) = files.keys().next() {
        return Err(Error::invalid(format!(
            "the backup contains a file its manifest does not list ({extra})"
        )));
    }
    items.sort_by(|a, b| a.path.cmp(&b.path));

    // Pass 3: what the files are.
    let mut warnings = Vec::new();
    let fragments = validate_items(&items, &mut warnings)?;

    Ok(Backup {
        manifest,
        items,
        sha256: sha256_hex(bytes),
        size: bytes.len() as u64,
        warnings,
        fragments,
    })
}

fn validate_items(
    items: &[Item],
    warnings: &mut Vec<String>,
) -> Result<BTreeMap<Category, toml::Table>> {
    let mut fragments = BTreeMap::new();
    let mut folders: BTreeMap<(Category, &str), BTreeMap<String, &[u8]>> = BTreeMap::new();

    for item in items {
        match item.category {
            Category::Settings | Category::Snippets | Category::WebSearch => {
                if item.data.len() as u64 > MAX_CONFIG_BYTES {
                    return Err(Error::invalid(format!("{} is too large", item.path)));
                }
                let part = config_fragment(item, warnings)?;
                fragments.insert(item.category, part);
            }
            Category::Themes => {
                validate_file(&item.path, item.category, &item.data).map_err(|why| {
                    Error::invalid(format!("{} in the backup is not valid: {why}", item.path))
                })?;
            }
            Category::Plugins | Category::Workflows => {
                if let Some((folder, rest)) = split_folder(&item.path) {
                    folders
                        .entry((item.category, folder))
                        .or_default()
                        .insert(rest.to_owned(), item.data.as_slice());
                }
            }
        }
    }
    for ((category, folder), files) in &folders {
        if files.len() > MAX_FOLDER_FILES {
            return Err(Error::invalid(format!("{folder} has too many files")));
        }
        validate_folder(*category, folder, files).map_err(|why| {
            Error::invalid(format!("\"{folder}\" in the backup cannot be used: {why}"))
        })?;
    }
    Ok(fragments)
}

/// Parses a settings, snippets or web search file against the real schema and
/// keeps only its own, allowlisted, scrubbed sections.
fn config_fragment(item: &Item, warnings: &mut Vec<String>) -> Result<toml::Table> {
    let text = std::str::from_utf8(&item.data)
        .map_err(|_| Error::invalid(format!("{} is not text", item.path)))?;
    let table = parse_config(text)
        .map_err(|_| Error::invalid(format!("{} in the backup is not valid TOML", item.path)))?;
    let mut removed = Vec::new();
    let mut unknown = Vec::new();
    let part = fragment(&table, item.category, &mut removed, &mut unknown);
    for key in unknown {
        warnings.push(format!(
            "Ignored the section [{key}] of the backup: this version does not restore it."
        ));
    }
    for key in removed {
        warnings.push(format!(
            "Ignored {key} of the backup: its name suggests it holds a secret."
        ));
    }

    // Types: the same schema `config.toml` is read with.
    let rendered = toml::to_string(&part)
        .map_err(|err| Error::invalid(format!("{} is not valid: {err}", item.path)))?;
    Config::from_toml_str(&rendered).map_err(|err| {
        Error::invalid(format!(
            "{} in the backup is not valid: {}",
            item.path,
            err.message()
        ))
    })?;
    if let Some(value) = part.get(WEB_SEARCH_KEY) {
        let engines: Vec<WebSearchEngine> =
            value.clone().try_into().map_err(|err: toml::de::Error| {
                Error::invalid(format!(
                    "the web searches in the backup are not valid: {}",
                    err.message()
                ))
            })?;
        if engines
            .iter()
            .any(|e| e.keyword.trim().is_empty() || !e.url.contains("{query}"))
        {
            return Err(Error::invalid(
                "a web search in the backup has no keyword or no {query} in its address",
            ));
        }
    }
    if let Some(value) = part.get(SNIPPET_KEY) {
        let _: Vec<Snippet> = value.clone().try_into().map_err(|err: toml::de::Error| {
            Error::invalid(format!(
                "the snippets in the backup are not valid: {}",
                err.message()
            ))
        })?;
    }
    Ok(part)
}

/// A small, serializable description of a backup for the UI.
#[derive(Debug, Clone, Serialize)]
pub struct ArchiveInfo {
    pub app_version: String,
    pub created: String,
    pub created_unix: i64,
    pub platform: String,
    pub kind: Kind,
    pub categories: Vec<Category>,
    pub files: usize,
    pub size: u64,
    /// Identifies the exact file that was checked.
    pub sha256: String,
}

impl Backup {
    pub fn info(&self) -> ArchiveInfo {
        ArchiveInfo {
            app_version: self.manifest.app_version.clone(),
            created: self.manifest.created.clone(),
            created_unix: self.manifest.created_unix,
            platform: self.manifest.platform.clone(),
            kind: self.manifest.kind,
            categories: self.manifest.categories.clone(),
            files: self.items.len(),
            size: self.size,
            sha256: self.sha256.clone(),
        }
    }
}
