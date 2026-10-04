//! Working out what a restore would change.
//!
//! [`build`] compares a checked [`Backup`] with the current configuration for a
//! set of categories and a [`Mode`]. Its result is both the preview the user
//! reads and the list of changes [`crate::restore`] makes, so what is shown is
//! what happens.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use sevak_core::config::{Snippet, WebSearchEngine};
use sevak_core::Config;
use sevak_plugins::script::Manifest;
use sevak_plugins::workflow::model::Workflow;

use crate::apply::{Op, StagedFile};
use crate::archive::Backup;
use crate::category::{
    Category, Mode, CSS_DIR, HOTKEY_KEY, PLUGIN_MANIFEST, SETTINGS_TABLES, SNIPPETS_TABLE,
    SNIPPET_KEY, THEMES_DIR, WEB_SEARCH_KEY, WORKFLOW_MANIFEST,
};
use crate::collect::{collect, parse_config, read_config_text, Collected};
use crate::error::{Error, Result};
use crate::item::{group_by_folder, Item};
use crate::util::Roots;

/// What a restore does to one thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    Added,
    Changed,
    Unchanged,
    /// Only in Replace mode: the backup does not have it, so it goes (or goes
    /// back to its default).
    Removed,
}

#[derive(Debug, Clone, Serialize)]
pub struct ItemPreview {
    pub name: String,
    pub change: Change,
    /// The files involved (for a folder: what differs).
    pub files: Vec<String>,
    /// Sevak will ask before it runs this.
    pub needs_approval: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CategoryPreview {
    pub category: Category,
    pub label: &'static str,
    pub description: &'static str,
    pub in_backup: bool,
    pub selected: bool,
    pub added: usize,
    pub changed: usize,
    pub unchanged: usize,
    pub removed: usize,
    pub items: Vec<ItemPreview>,
    /// Why this category cannot be restored right now.
    pub problem: Option<String>,
}

impl CategoryPreview {
    fn new(category: Category, in_backup: bool, selected: bool) -> Self {
        Self {
            category,
            label: category.label(),
            description: category.description(),
            in_backup,
            selected,
            added: 0,
            changed: 0,
            unchanged: 0,
            removed: 0,
            items: Vec::new(),
            problem: None,
        }
    }

    fn push(&mut self, item: ItemPreview) {
        match item.change {
            Change::Added => self.added += 1,
            Change::Changed => self.changed += 1,
            Change::Unchanged => self.unchanged += 1,
            Change::Removed => self.removed += 1,
        }
        self.items.push(item);
    }

    /// Whether applying this category would change anything.
    pub fn changes_something(&self) -> bool {
        self.added + self.changed + self.removed > 0
    }
}

/// The current `config.toml`, parsed.
pub(crate) struct ConfigState {
    pub text: Option<String>,
    pub table: toml::Table,
    pub config: Config,
}

fn load_config_state(roots: &Roots) -> Result<ConfigState> {
    let text = read_config_text(roots)?;
    let (table, config) = match &text {
        Some(text) => {
            let table = parse_config(text)?;
            let config = Config::from_toml_str(text).map_err(|err| {
                Error::Current(format!(
                    "your config.toml has a mistake ({}). Fix it first, or leave Settings, \
                     Snippets and Web search engines out.",
                    err.message()
                ))
            })?;
            (table, config)
        }
        None => (toml::Table::new(), Config::default()),
    };
    Ok(ConfigState {
        text,
        table,
        config,
    })
}

/// Everything [`build`] works out.
#[derive(Default)]
pub(crate) struct Plan {
    pub previews: Vec<CategoryPreview>,
    /// The file changes, without the configuration file.
    pub ops: Vec<Op>,
    /// Whether any selected category changes `config.toml`.
    pub touches_config: bool,
    pub config_state: Option<ConfigState>,
    /// The configuration the restore would leave, if it touches it.
    pub after: Option<Config>,
    /// Snippets after the restore, if they change.
    pub snippets_after: Option<Vec<Snippet>>,
    /// Approval records to drop before the new files go in.
    pub revoke: Vec<String>,
    /// Names of the things that will ask for approval.
    pub needs_approval: Vec<String>,
    /// Why the result would not be accepted, if so.
    pub validation_error: Option<String>,
    pub warnings: Vec<String>,
}

const CONFIG_CATEGORIES: [Category; 3] =
    [Category::Settings, Category::Snippets, Category::WebSearch];

fn owned_keys(category: Category) -> Vec<&'static str> {
    match category {
        Category::Settings => SETTINGS_TABLES
            .iter()
            .copied()
            .chain([HOTKEY_KEY])
            .collect(),
        Category::Snippets => vec![SNIPPETS_TABLE, SNIPPET_KEY],
        Category::WebSearch => vec![WEB_SEARCH_KEY],
        _ => Vec::new(),
    }
}

fn key_of(value: &toml::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(toml::Value::as_str)
        .map(|text| {
            text.chars()
                .filter(|c| !c.is_whitespace())
                .collect::<String>()
                .to_lowercase()
        })
        .unwrap_or_default()
}

/// `current` with every item of `incoming` added, or replacing the one with the
/// same key.
fn merge_keyed(current: &[toml::Value], incoming: &[toml::Value], key: &str) -> Vec<toml::Value> {
    let mut out = current.to_vec();
    let mut claimed: BTreeSet<usize> = BTreeSet::new();
    for item in incoming {
        let wanted = key_of(item, key);
        let found = out
            .iter()
            .enumerate()
            .position(|(i, existing)| !claimed.contains(&i) && key_of(existing, key) == wanted);
        match found {
            Some(index) => {
                out[index] = item.clone();
                claimed.insert(index);
            }
            None => {
                out.push(item.clone());
                claimed.insert(out.len() - 1);
            }
        }
    }
    out
}

fn overlay(current: &mut toml::Table, incoming: &toml::Table) {
    for (key, value) in incoming {
        match (current.get_mut(key), value) {
            (Some(toml::Value::Table(here)), toml::Value::Table(there)) => overlay(here, there),
            _ => {
                current.insert(key.clone(), value.clone());
            }
        }
    }
}

fn merge_key_for(key: &str) -> Option<&'static str> {
    match key {
        HOTKEY_KEY => Some("key"),
        SNIPPET_KEY => Some("name"),
        WEB_SEARCH_KEY => Some("keyword"),
        _ => None,
    }
}

/// The `config.toml` table that results from applying `categories` of the
/// backup to `current`.
fn merged_table(
    current: &toml::Table,
    backup: &Backup,
    categories: &[Category],
    mode: Mode,
) -> toml::Table {
    let mut table = current.clone();
    for category in CONFIG_CATEGORIES {
        if !categories.contains(&category) || !backup.has(category) {
            continue;
        }
        let fragment = backup.fragments.get(&category).cloned().unwrap_or_default();
        match mode {
            Mode::Replace => {
                for key in owned_keys(category) {
                    table.remove(key);
                }
                for (key, value) in fragment {
                    table.insert(key, value);
                }
                if category == Category::WebSearch && !table.contains_key(WEB_SEARCH_KEY) {
                    // "None" must be written out, or the defaults would come back.
                    table.insert(WEB_SEARCH_KEY.to_owned(), toml::Value::Array(Vec::new()));
                }
            }
            Mode::Merge => {
                if category == Category::WebSearch && !table.contains_key(WEB_SEARCH_KEY) {
                    // No list in the file means the defaults are in effect: merge into those.
                    if let Ok(toml::Value::Array(defaults)) =
                        toml::Value::try_from(WebSearchEngine::defaults())
                    {
                        table.insert(WEB_SEARCH_KEY.to_owned(), toml::Value::Array(defaults));
                    }
                }
                for (key, value) in &fragment {
                    match (merge_key_for(key), table.get(key), value) {
                        (Some(by), existing, toml::Value::Array(incoming)) => {
                            let current_list = match existing {
                                Some(toml::Value::Array(list)) => list.clone(),
                                _ => Vec::new(),
                            };
                            table.insert(
                                key.clone(),
                                toml::Value::Array(merge_keyed(&current_list, incoming, by)),
                            );
                        }
                        (None, Some(toml::Value::Table(_)), toml::Value::Table(there)) => {
                            if let Some(toml::Value::Table(here)) = table.get_mut(key) {
                                overlay(here, there);
                            }
                        }
                        _ => {
                            table.insert(key.clone(), value.clone());
                        }
                    }
                }
            }
        }
    }
    table
}

fn typed(table: &toml::Table) -> Result<Config> {
    let text = toml::to_string(table).map_err(|err| {
        Error::Rejected(format!(
            "the settings that would result are not valid: {err}"
        ))
    })?;
    Config::from_toml_str(&text).map_err(|err| {
        Error::Rejected(format!(
            "the settings that would result are not valid: {}",
            err.message()
        ))
    })
}

fn table_label(key: &str) -> String {
    match key {
        "general" => "General (shortcuts, startup, updates)",
        "window" => "Window width",
        "linux" => "Linux options",
        "search" => "Search",
        "appearance" => "Appearance",
        "plugins" => "Plugins that are switched off",
        "calculator" => "Calculator",
        "files" => "Files",
        "bookmarks" => "Bookmarks",
        "system" => "System commands",
        "tasks" => "Automation tasks",
        "media" => "Media controls",
        "shell" => "Terminal commands",
        "paste" => "Pasting",
        "actions" => "Universal Actions",
        "clipboard" => "Clipboard history options",
        "file_buffer" => "File buffer",
        "contacts" => "Contacts",
        "dictionary" => "Dictionary",
        "hotkey" => "Extra hotkeys",
        other => other,
    }
    .to_owned()
}

fn json(config: &Config) -> serde_json::Value {
    serde_json::to_value(config).unwrap_or(serde_json::Value::Null)
}

fn preview_settings(
    state: &ConfigState,
    backup: &Backup,
    mode: Mode,
    after: &Config,
    preview: &mut CategoryPreview,
) {
    let fragment = backup
        .fragments
        .get(&Category::Settings)
        .cloned()
        .unwrap_or_default();
    let before = json(&state.config);
    let after = json(after);
    for key in owned_keys(Category::Settings) {
        let in_backup = fragment.contains_key(key);
        let explicit = state.table.contains_key(key);
        if !in_backup && !(mode == Mode::Replace && explicit) {
            continue;
        }
        let equal = before.get(key) == after.get(key);
        let change = if equal {
            Change::Unchanged
        } else if !in_backup {
            Change::Removed
        } else if explicit {
            Change::Changed
        } else {
            Change::Added
        };
        preview.push(ItemPreview {
            name: table_label(key),
            change,
            files: vec![if key == HOTKEY_KEY {
                "[[hotkey]]".to_owned()
            } else {
                format!("[{key}]")
            }],
            needs_approval: false,
        });
    }
}

fn snippets_of(fragment: &toml::Table) -> Vec<Snippet> {
    fragment
        .get(SNIPPET_KEY)
        .cloned()
        .and_then(|value| value.try_into().ok())
        .unwrap_or_default()
}

fn engines_of(fragment: &toml::Table) -> Vec<WebSearchEngine> {
    fragment
        .get(WEB_SEARCH_KEY)
        .cloned()
        .and_then(|value| value.try_into().ok())
        .unwrap_or_default()
}

fn preview_snippets(
    state: &ConfigState,
    backup: &Backup,
    mode: Mode,
    after: &Config,
    preview: &mut CategoryPreview,
) {
    let fragment = backup
        .fragments
        .get(&Category::Snippets)
        .cloned()
        .unwrap_or_default();
    if fragment.contains_key(SNIPPETS_TABLE)
        || (mode == Mode::Replace && state.table.contains_key(SNIPPETS_TABLE))
    {
        let change = if json(&state.config).get(SNIPPETS_TABLE) == json(after).get(SNIPPETS_TABLE) {
            Change::Unchanged
        } else if !fragment.contains_key(SNIPPETS_TABLE) {
            Change::Removed
        } else if state.table.contains_key(SNIPPETS_TABLE) {
            Change::Changed
        } else {
            Change::Added
        };
        preview.push(ItemPreview {
            name: "Snippet expansion options".to_owned(),
            change,
            files: vec!["[snippets]".to_owned()],
            needs_approval: false,
        });
    }
    let incoming = snippets_of(&fragment);
    let current = &state.config.snippet;
    let mut matched: BTreeSet<usize> = BTreeSet::new();
    for snippet in &incoming {
        let found = current
            .iter()
            .enumerate()
            .position(|(i, c)| !matched.contains(&i) && c.name == snippet.name);
        let change = match found {
            Some(i) => {
                matched.insert(i);
                if &current[i] == snippet {
                    Change::Unchanged
                } else {
                    Change::Changed
                }
            }
            None => Change::Added,
        };
        preview.push(ItemPreview {
            name: snippet.name.clone(),
            change,
            files: Vec::new(),
            needs_approval: false,
        });
    }
    if mode == Mode::Replace {
        for (i, snippet) in current.iter().enumerate() {
            if !matched.contains(&i) {
                preview.push(ItemPreview {
                    name: snippet.name.clone(),
                    change: Change::Removed,
                    files: Vec::new(),
                    needs_approval: false,
                });
            }
        }
    }
}

fn preview_web_search(
    state: &ConfigState,
    backup: &Backup,
    mode: Mode,
    preview: &mut CategoryPreview,
) {
    let fragment = backup
        .fragments
        .get(&Category::WebSearch)
        .cloned()
        .unwrap_or_default();
    let incoming = engines_of(&fragment);
    let current = &state.config.web_search;
    let same = |a: &str, b: &str| a.trim().eq_ignore_ascii_case(b.trim());
    let mut matched: BTreeSet<usize> = BTreeSet::new();
    for engine in &incoming {
        let found = current
            .iter()
            .enumerate()
            .position(|(i, c)| !matched.contains(&i) && same(&c.keyword, &engine.keyword));
        let change = match found {
            Some(i) => {
                matched.insert(i);
                if &current[i] == engine {
                    Change::Unchanged
                } else {
                    Change::Changed
                }
            }
            None => Change::Added,
        };
        preview.push(ItemPreview {
            name: format!("{} ({})", engine.name, engine.keyword),
            change,
            files: Vec::new(),
            needs_approval: false,
        });
    }
    if mode == Mode::Replace {
        for (i, engine) in current.iter().enumerate() {
            if !matched.contains(&i) {
                preview.push(ItemPreview {
                    name: format!("{} ({})", engine.name, engine.keyword),
                    change: Change::Removed,
                    files: Vec::new(),
                    needs_approval: false,
                });
            }
        }
    }
}

fn theme_target(roots: &Roots, path: &str) -> std::path::PathBuf {
    match path.strip_prefix(&format!("{CSS_DIR}/")) {
        Some(name) => roots.config_dir.join(name),
        None => roots.config_dir.join(path),
    }
}

fn theme_name(path: &str) -> String {
    match path.strip_prefix(&format!("{CSS_DIR}/")) {
        Some(name) => format!("{name} (stylesheet)"),
        None => path
            .strip_prefix(&format!("{THEMES_DIR}/"))
            .unwrap_or(path)
            .to_owned(),
    }
}

fn plan_themes(
    roots: &Roots,
    backup: &Backup,
    current: &Collected,
    mode: Mode,
    preview: &mut CategoryPreview,
    ops: &mut Vec<Op>,
) {
    let existing: BTreeMap<&str, &Item> = current
        .of(Category::Themes)
        .map(|item| (item.path.as_str(), item))
        .collect();
    let mut incoming: BTreeSet<&str> = BTreeSet::new();
    for item in backup.of(Category::Themes) {
        incoming.insert(item.path.as_str());
        let change = match existing.get(item.path.as_str()) {
            None => Change::Added,
            Some(here) if here.data == item.data => Change::Unchanged,
            Some(_) => Change::Changed,
        };
        if change != Change::Unchanged {
            ops.push(Op::WriteFile {
                target: theme_target(roots, &item.path),
                data: item.data.clone(),
            });
        }
        preview.push(ItemPreview {
            name: theme_name(&item.path),
            change,
            files: vec![item.path.clone()],
            needs_approval: false,
        });
    }
    if mode == Mode::Replace {
        for path in existing.keys() {
            if !incoming.contains(path) {
                ops.push(Op::Remove {
                    target: theme_target(roots, path),
                });
                preview.push(ItemPreview {
                    name: theme_name(path),
                    change: Change::Removed,
                    files: vec![(*path).to_owned()],
                    needs_approval: false,
                });
            }
        }
    }
}

fn plan_folders(
    roots: &Roots,
    backup: &Backup,
    current: &Collected,
    category: Category,
    mode: Mode,
    preview: &mut CategoryPreview,
    plan: &mut Plan,
) {
    let incoming = group_by_folder(&backup.items, category);
    let existing = group_by_folder(&current.items, category);
    let root = if category == Category::Plugins {
        roots.plugins_dir()
    } else {
        roots.workflows_dir()
    };
    let what = if category == Category::Plugins {
        "Plugin"
    } else {
        "Workflow"
    };

    for (folder, files) in &incoming {
        let (change, details) = match existing.get(folder) {
            None => (Change::Added, files.keys().cloned().collect::<Vec<_>>()),
            Some(here) => {
                let mut details = Vec::new();
                for (rel, item) in files {
                    match here.get(rel) {
                        None => details.push(format!("+ {rel}")),
                        Some(old) if old.data != item.data => details.push(format!("~ {rel}")),
                        Some(_) => {}
                    }
                }
                for rel in here.keys() {
                    if !files.contains_key(rel) {
                        details.push(format!("- {rel}"));
                    }
                }
                if details.is_empty() {
                    (Change::Unchanged, Vec::new())
                } else {
                    (Change::Changed, details)
                }
            }
        };

        let runs_code = match category {
            Category::Plugins => true,
            _ => files
                .get(WORKFLOW_MANIFEST)
                .and_then(|item| std::str::from_utf8(&item.data).ok())
                .and_then(|text| Workflow::from_toml(text).ok())
                .is_none_or(|workflow| workflow.needs_approval()),
        };
        let needs_approval = change != Change::Unchanged && runs_code;
        if change != Change::Unchanged {
            plan.ops.push(Op::ReplaceDir {
                target: root.join(folder),
                files: files
                    .iter()
                    .map(|(rel, item)| StagedFile {
                        rel: rel.clone(),
                        data: item.data.clone(),
                        executable: item.executable,
                    })
                    .collect(),
            });
            // Approval is bound to the command (plugins) or the scripts' contents
            // (workflows); either way a restored folder must be allowed again, even
            // when an old approval for the same name and command is still on record.
            if category == Category::Plugins {
                for source in [Some(files), existing.get(folder)].into_iter().flatten() {
                    if let Some(id) = source
                        .get(PLUGIN_MANIFEST)
                        .and_then(|item| std::str::from_utf8(&item.data).ok())
                        .and_then(|text| Manifest::parse(text, folder).ok())
                        .map(|manifest| manifest.id)
                    {
                        plan.revoke.push(id);
                    }
                }
            } else {
                plan.revoke.push(format!(
                    "{}:{folder}",
                    sevak_plugins::workflow::model::FAMILY
                ));
            }
            if needs_approval {
                plan.needs_approval.push(format!("{what} \"{folder}\""));
            }
        }
        preview.push(ItemPreview {
            name: folder.clone(),
            change,
            files: details,
            needs_approval,
        });
    }

    if mode == Mode::Replace {
        for (folder, files) in &existing {
            if !incoming.contains_key(folder) {
                plan.ops.push(Op::Remove {
                    target: root.join(folder),
                });
                preview.push(ItemPreview {
                    name: folder.clone(),
                    change: Change::Removed,
                    files: files.keys().cloned().collect(),
                    needs_approval: false,
                });
            }
        }
    }
}

/// Compares `backup` with the current configuration for `categories`.
pub(crate) fn build(
    roots: &Roots,
    backup: &Backup,
    categories: &[Category],
    mode: Mode,
    validate: &dyn Fn(&Config) -> std::result::Result<(), String>,
) -> Result<Plan> {
    let mut plan = Plan {
        warnings: backup.warnings.clone(),
        ..Plan::default()
    };

    let wants_config = CONFIG_CATEGORIES.iter().any(|c| backup.has(*c));
    let config_state = if wants_config {
        Some(load_config_state(roots))
    } else {
        None
    };
    let current = collect(
        roots,
        &[Category::Themes, Category::Plugins, Category::Workflows],
    )?;

    // The result of the config categories that are selected, for the validator
    // and for the new config.toml.
    let selected_config: Vec<Category> = CONFIG_CATEGORIES
        .iter()
        .copied()
        .filter(|c| categories.contains(c) && backup.has(*c))
        .collect();
    let mut after_selected: Option<Config> = None;
    let mut config_problem: Option<String> = None;
    if let Some(state) = &config_state {
        match state {
            Ok(state) => {
                if !selected_config.is_empty() {
                    match typed(&merged_table(&state.table, backup, &selected_config, mode)) {
                        Ok(after) => {
                            if let Err(why) = validate(&after) {
                                plan.validation_error = Some(why);
                            }
                            after_selected = Some(after);
                        }
                        Err(err) => plan.validation_error = Some(err.to_string()),
                    }
                }
            }
            Err(err) => config_problem = Some(err.to_string()),
        }
    }

    for category in Category::ALL {
        let in_backup = backup.has(category);
        let selected = categories.contains(&category) && in_backup;
        let mut preview = CategoryPreview::new(category, in_backup, selected);
        if in_backup {
            match category {
                Category::Settings | Category::Snippets | Category::WebSearch => {
                    match (&config_state, &config_problem) {
                        (Some(Ok(state)), _) => {
                            // Each category is previewed on its own, whatever is selected.
                            let own = merged_table(&state.table, backup, &[category], mode);
                            match typed(&own) {
                                Ok(after) => match category {
                                    Category::Settings => {
                                        preview_settings(state, backup, mode, &after, &mut preview);
                                    }
                                    Category::Snippets => {
                                        preview_snippets(state, backup, mode, &after, &mut preview);
                                    }
                                    _ => preview_web_search(state, backup, mode, &mut preview),
                                },
                                Err(err) => preview.problem = Some(err.to_string()),
                            }
                        }
                        (_, Some(problem)) => preview.problem = Some(problem.clone()),
                        _ => {}
                    }
                }
                Category::Themes => {
                    let mut ops = Vec::new();
                    plan_themes(roots, backup, &current, mode, &mut preview, &mut ops);
                    if selected {
                        plan.ops.extend(ops);
                    }
                }
                Category::Plugins | Category::Workflows => {
                    let mut scratch = Plan::default();
                    plan_folders(
                        roots,
                        backup,
                        &current,
                        category,
                        mode,
                        &mut preview,
                        &mut scratch,
                    );
                    if selected {
                        plan.absorb(scratch);
                    }
                }
            }
        }
        plan.previews.push(preview);
    }

    // The configuration file.
    if let (Some(Ok(state)), Some(after)) = (config_state, after_selected) {
        if selected_config.contains(&Category::Snippets) && after.snippet != state.config.snippet {
            plan.snippets_after = Some(after.snippet.clone());
        }
        plan.touches_config = true;
        plan.after = Some(after);
        plan.config_state = Some(state);
    }
    if let Some(problem) = config_problem {
        if selected_config.is_empty() {
            // Nothing selected needs the file.
            plan.warnings.push(problem);
        }
    }
    // A theme the settings name must exist afterwards.
    if let Some(after) = &plan.after {
        let theme = after.appearance.theme_file.trim();
        if !theme.is_empty() {
            let will_exist = backup.of(Category::Themes).any(|i| i.path == theme)
                || current.of(Category::Themes).any(|i| i.path == theme);
            if !will_exist {
                plan.warnings.push(format!(
                    "The settings use the theme file {theme}, which is not in your themes folder \
                     and not in the backup. Include Themes, or pick another theme."
                ));
            }
        }
    }
    Ok(plan)
}

impl Plan {
    fn absorb(&mut self, other: Self) {
        self.ops.extend(other.ops);
        self.revoke.extend(other.revoke);
        self.needs_approval.extend(other.needs_approval);
    }
}
