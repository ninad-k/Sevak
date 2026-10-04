//! A file as it travels in an archive, and the checks that depend on what the
//! file is for.

use std::collections::BTreeMap;

use sevak_core::theme_file;
use sevak_core::theme_store::MAX_THEME_BYTES;
use sevak_plugins::script::Manifest;
use sevak_plugins::workflow::model::Workflow;
use sevak_plugins::workflow::validate::error_summary;

use crate::category::{
    split_folder, Category, CSS_DIR, PLUGINS_DIR, PLUGIN_MANIFEST, WORKFLOWS_DIR, WORKFLOW_MANIFEST,
};

/// One file of a backup, in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// Forward-slash path inside the archive (see [`crate::category::classify`]).
    pub path: String,
    pub category: Category,
    pub data: Vec<u8>,
    /// The owner's execute bit (Unix); restored as `0755`, never anything wider.
    pub executable: bool,
}

/// Checks one non-folder file by what it is. `Err` is one sentence.
pub fn validate_file(path: &str, category: Category, data: &[u8]) -> Result<(), String> {
    match category {
        Category::Settings | Category::Snippets | Category::WebSearch => {
            // Parsed against the real schema by the caller (`archive`), which
            // also filters the sections; here it is only checked to be text.
            std::str::from_utf8(data)
                .map(|_| ())
                .map_err(|_| "it is not text".to_owned())
        }
        Category::Themes => {
            let text = std::str::from_utf8(data).map_err(|_| "it is not text".to_owned())?;
            if path.starts_with(CSS_DIR) {
                if data.len() as u64 > MAX_THEME_BYTES {
                    return Err("it is too large for a stylesheet".to_owned());
                }
                Ok(())
            } else {
                theme_file::parse(text).map(|_| ())
            }
        }
        Category::Plugins | Category::Workflows => Ok(()),
    }
}

/// Checks that a plugin or workflow folder is one Sevak can load: it has its
/// manifest and the manifest is valid. `files` maps paths inside the folder to
/// their contents.
pub fn validate_folder(
    category: Category,
    folder: &str,
    files: &BTreeMap<String, &[u8]>,
) -> Result<(), String> {
    let manifest = if category == Category::Plugins {
        PLUGIN_MANIFEST
    } else {
        WORKFLOW_MANIFEST
    };
    let bytes = files
        .get(manifest)
        .ok_or_else(|| format!("it has no {manifest}"))?;
    let text = std::str::from_utf8(bytes).map_err(|_| format!("{manifest} is not text"))?;
    if category == Category::Plugins {
        Manifest::parse(text, folder)
            .map(|_| ())
            .map_err(|err| format!("{manifest} is not valid: {err}"))
    } else {
        let workflow =
            Workflow::from_toml(text).map_err(|err| format!("{manifest} is not valid: {err}"))?;
        match error_summary(&workflow.validate()) {
            Some(summary) => Err(format!("{manifest} is not valid: {summary}")),
            None => Ok(()),
        }
    }
}

/// The items of one category grouped by folder (`plugins/<folder>/...`):
/// folder name to (path inside the folder to item).
pub fn group_by_folder(
    items: &[Item],
    category: Category,
) -> BTreeMap<String, BTreeMap<String, &Item>> {
    let mut grouped: BTreeMap<String, BTreeMap<String, &Item>> = BTreeMap::new();
    for item in items.iter().filter(|item| item.category == category) {
        if let Some((folder, rest)) = split_folder(&item.path) {
            grouped
                .entry(folder.to_owned())
                .or_default()
                .insert(rest.to_owned(), item);
        }
    }
    grouped
}

/// The archive directory of a code-running category.
pub fn dir_of(category: Category) -> &'static str {
    if category == Category::Plugins {
        PLUGINS_DIR
    } else {
        WORKFLOWS_DIR
    }
}
