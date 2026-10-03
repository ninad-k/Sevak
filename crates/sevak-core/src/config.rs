//! User configuration stored as TOML.
//!
//! Every field has a default and every section is `#[serde(default)]`, so a
//! config written by an older Sevak (or a hand-trimmed one) keeps loading as new
//! options are added. Unknown keys are ignored for the same reason.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The file written on first run. It mirrors [`Config::default`] (enforced by a
/// unit test) but carries comments, which `toml::to_string` cannot produce.
pub const DEFAULT_CONFIG_TOML: &str = r#"# Sevak configuration
#
# Created with default values on first run. Edit it, then choose "Reload index"
# from the tray menu (or restart Sevak) to apply changes.

[general]
# Shortcut that shows and hides Sevak. Examples: "Alt+Space", "Ctrl+Space",
# "Super+Space", "Ctrl+Shift+K".
# On Linux Wayland sessions applications cannot grab global keys. Run
# `sevak --setup-hotkey` to bind this key to `sevak --toggle` in GNOME instead.
hotkey = "Alt+Space"

# Hide the window when it loses focus.
hide_on_blur = true

# Start Sevak in the background when you log in.
launch_at_login = false

# Check GitHub for a new version at startup and once a day. Updates are only
# installed after you agree. This is the only request Sevak makes on its own.
check_for_updates = true

[window]
# Width of the search window in logical pixels (400-1600).
width = 720

[linux]
# On Wayland sessions, draw Sevak's window through XWayland. Native Wayland
# windows cannot position themselves and may be refused focus, so this keeps the
# launcher centered and typeable. Set to false to use the native Wayland backend.
wayland_use_xwayland = true

[search]
# Number of results shown (1-20).
max_results = 8
# Keyword of the web search engine offered when nothing else matches ("" to
# disable). A list offers several, in order: ["g", "yt", "gh"].
fallback_web_search = "g"
# Up/Down on an empty search box recalls the last 50 searches you ran. They are
# kept in usage.json in the data folder; false stops recording and forgets them.
query_history = true

[appearance]
# "system", "light" or "dark".
theme = "system"

[plugins]
# Ids of built-in plugins to turn off: "apps", "calculator", "files", "web:<keyword>".
disabled = []

[files]
# Folders whose files and subfolders are searchable. "~" is your home folder.
directories = ["~/Desktop", "~/Documents", "~/Downloads"]
# How many folder levels below each directory are indexed.
max_depth = 4
# Index dot-files and dot-folders.
include_hidden = false
# Type "<keyword> <name>" to search only files.
keyword = "f"
# Also show (lower-ranked) file results for plain queries.
global = true

# Web search engines: type "<keyword> <terms>". "{query}" is replaced by the
# URL-encoded terms. Defining any [[web_search]] entry replaces this list.
[[web_search]]
keyword = "g"
name = "Google"
url = "https://www.google.com/search?q={query}"

[[web_search]]
keyword = "yt"
name = "YouTube"
url = "https://www.youtube.com/results?search_query={query}"

[[web_search]]
keyword = "gh"
name = "GitHub"
url = "https://github.com/search?q={query}"
"#;

pub const MIN_WINDOW_WIDTH: u32 = 400;
pub const MAX_WINDOW_WIDTH: u32 = 1600;
pub const MAX_RESULTS_LIMIT: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: GeneralConfig,
    pub window: WindowConfig,
    pub linux: LinuxConfig,
    pub search: SearchConfig,
    pub appearance: AppearanceConfig,
    pub plugins: PluginsConfig,
    pub files: FilesConfig,
    pub web_search: Vec<WebSearchEngine>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            window: WindowConfig::default(),
            linux: LinuxConfig::default(),
            search: SearchConfig::default(),
            appearance: AppearanceConfig::default(),
            plugins: PluginsConfig::default(),
            files: FilesConfig::default(),
            web_search: WebSearchEngine::defaults(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    /// Accelerator string, e.g. `"Alt+Space"`. Parsed by the shell, because the
    /// accepted key names depend on the hotkey backend.
    pub hotkey: String,
    pub hide_on_blur: bool,
    pub launch_at_login: bool,
    /// Look for a new release at startup and daily (asks before installing).
    pub check_for_updates: bool,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            hotkey: "Alt+Space".to_owned(),
            hide_on_blur: true,
            launch_at_login: false,
            check_for_updates: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowConfig {
    /// Logical width of the launcher window.
    pub width: u32,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self { width: 720 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LinuxConfig {
    pub wayland_use_xwayland: bool,
}

impl Default for LinuxConfig {
    fn default() -> Self {
        Self {
            wayland_use_xwayland: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchConfig {
    pub max_results: usize,
    /// Keyword(s) of the `[[web_search]]` engines offered when nothing matched;
    /// empty disables the fallback.
    pub fallback_web_search: FallbackSearch,
    /// Remember executed queries so Up/Down can recall them.
    pub query_history: bool,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            max_results: 8,
            fallback_web_search: FallbackSearch::single("g"),
            query_history: true,
        }
    }
}

/// `fallback_web_search`: one keyword written as a string (`"g"`, the original
/// form) or several written as a list (`["g", "yt"]`). Both forms read and
/// write back as they were written, so saving never rewrites the user's choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallbackSearch {
    keywords: Vec<String>,
    /// Written as a list rather than a string.
    list: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum FallbackRepr {
    One(String),
    Many(Vec<String>),
}

impl FallbackSearch {
    /// One keyword in string form; `""` disables the fallback.
    pub fn single(keyword: impl Into<String>) -> Self {
        Self::from_repr(FallbackRepr::One(keyword.into()))
    }

    /// Several keywords in list form, tried in this order.
    pub fn list(keywords: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::from_repr(FallbackRepr::Many(
            keywords.into_iter().map(Into::into).collect(),
        ))
    }

    /// The keywords in order: trimmed, without blanks or case-insensitive repeats.
    pub fn keywords(&self) -> &[String] {
        &self.keywords
    }

    pub fn is_empty(&self) -> bool {
        self.keywords.is_empty()
    }

    fn from_repr(repr: FallbackRepr) -> Self {
        let (raw, list) = match repr {
            FallbackRepr::One(keyword) => (vec![keyword], false),
            FallbackRepr::Many(keywords) => (keywords, true),
        };
        let mut keywords: Vec<String> = Vec::new();
        for keyword in raw {
            let keyword = keyword.trim();
            let repeated = keywords.iter().any(|k| k.eq_ignore_ascii_case(keyword));
            if !keyword.is_empty() && !repeated {
                keywords.push(keyword.to_owned());
            }
        }
        Self { keywords, list }
    }
}

impl Serialize for FallbackSearch {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.list {
            self.keywords.serialize(serializer)
        } else {
            self.keywords
                .first()
                .map_or("", String::as_str)
                .serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for FallbackSearch {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        FallbackRepr::deserialize(deserializer).map(Self::from_repr)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearanceConfig {
    pub theme: Theme,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginsConfig {
    /// Plugin ids that are not loaded.
    pub disabled: Vec<String>,
}

impl PluginsConfig {
    pub fn is_enabled(&self, plugin_id: &str) -> bool {
        !self.disabled.iter().any(|id| id == plugin_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FilesConfig {
    /// Roots to index; a leading `~` means the home directory (expanded by the
    /// files plugin, since core has no notion of the user's home).
    pub directories: Vec<String>,
    pub max_depth: usize,
    pub include_hidden: bool,
    pub keyword: String,
    pub global: bool,
}

impl Default for FilesConfig {
    fn default() -> Self {
        Self {
            directories: vec![
                "~/Desktop".to_owned(),
                "~/Documents".to_owned(),
                "~/Downloads".to_owned(),
            ],
            max_depth: 4,
            include_hidden: false,
            keyword: "f".to_owned(),
            global: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebSearchEngine {
    pub keyword: String,
    pub name: String,
    /// URL template; `{query}` is replaced by the URL-encoded search terms.
    pub url: String,
}

impl WebSearchEngine {
    pub fn defaults() -> Vec<Self> {
        [
            ("g", "Google", "https://www.google.com/search?q={query}"),
            (
                "yt",
                "YouTube",
                "https://www.youtube.com/results?search_query={query}",
            ),
            ("gh", "GitHub", "https://github.com/search?q={query}"),
        ]
        .into_iter()
        .map(|(keyword, name, url)| Self {
            keyword: keyword.to_owned(),
            name: name.to_owned(),
            url: url.to_owned(),
        })
        .collect()
    }
}

/// How the configuration returned by [`Config::load_or_create`] was obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigOrigin {
    /// No file existed; defaults were written to disk.
    Created,
    /// An existing file was parsed.
    Loaded,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("cannot access config file {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid config file {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: Box<toml::de::Error>,
    },
    #[error("cannot update config file {path}: it is not valid TOML: {source}")]
    Edit {
        path: PathBuf,
        #[source]
        source: Box<toml_edit::TomlError>,
    },
    #[error("cannot serialize the configuration: {0}")]
    Serialize(#[from] toml_edit::ser::Error),
}

impl Config {
    /// Parses TOML text and normalizes out-of-range values.
    pub fn from_toml_str(text: &str) -> Result<Self, toml::de::Error> {
        let config: Config = toml::from_str(text)?;
        Ok(config.normalized())
    }

    /// Loads the config at `path`, writing [`DEFAULT_CONFIG_TOML`] there first
    /// if the file does not exist yet.
    ///
    /// A file that exists but fails to parse is reported as an error and left
    /// untouched, so a typo never costs the user their settings.
    pub fn load_or_create(path: &Path) -> Result<(Self, ConfigOrigin), ConfigError> {
        let io_err = |source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        };

        match fs::read_to_string(path) {
            Ok(text) => {
                let config = Self::from_toml_str(&text).map_err(|source| ConfigError::Parse {
                    path: path.to_path_buf(),
                    source: Box::new(source),
                })?;
                Ok((config, ConfigOrigin::Loaded))
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(io_err)?;
                }
                fs::write(path, DEFAULT_CONFIG_TOML).map_err(io_err)?;
                Ok((Self::default(), ConfigOrigin::Created))
            }
            Err(err) => Err(io_err(err)),
        }
    }

    /// Writes this configuration to `path`, editing the user's existing document
    /// in place so their comments, key order and untouched values survive.
    ///
    /// - A missing file is created from [`DEFAULT_CONFIG_TOML`] first.
    /// - Scalar values and arrays are replaced only when they differ, keeping
    ///   the key's comments and any trailing inline comment.
    /// - Keys Sevak does not know are left alone.
    /// - `[[web_search]]` is rewritten as a whole (it is a list, so there is no
    ///   meaningful per-entry merge); the comment above its first entry stays.
    /// - The file is replaced atomically (temporary file + rename).
    pub fn save_to(&self, path: &Path) -> Result<(), ConfigError> {
        let io_err = |source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        };

        let existing = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == io::ErrorKind::NotFound => DEFAULT_CONFIG_TOML.to_owned(),
            Err(err) => return Err(io_err(err)),
        };
        let mut document = existing
            .parse::<toml_edit::DocumentMut>()
            .map_err(|source| ConfigError::Edit {
                path: path.to_path_buf(),
                source: Box::new(source),
            })?;

        let updated = toml_edit::ser::to_document(self)?;
        merge_document(&mut document, &updated);
        let mut text = document.to_string();
        // The parser normalizes line endings to LF; give a CRLF file its own back.
        if existing.contains("\r\n") {
            text = text.replace("\r\n", "\n").replace('\n', "\r\n");
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(io_err)?;
        }
        let mut temp_name = path.file_name().unwrap_or_default().to_owned();
        temp_name.push(".tmp");
        let temp = path.with_file_name(temp_name);
        let written = fs::write(&temp, text).and_then(|()| fs::rename(&temp, path));
        if let Err(err) = written {
            let _ = fs::remove_file(&temp);
            return Err(io_err(err));
        }
        Ok(())
    }

    /// Clamps values into their supported ranges.
    #[must_use]
    pub fn normalized(mut self) -> Self {
        self.window.width = self.window.width.clamp(MIN_WINDOW_WIDTH, MAX_WINDOW_WIDTH);
        self.search.max_results = self.search.max_results.clamp(1, MAX_RESULTS_LIMIT);
        // Engines without a keyword or a `{query}` placeholder cannot work.
        self.web_search
            .retain(|engine| !engine.keyword.trim().is_empty() && engine.url.contains("{query}"));
        let hotkey = self.general.hotkey.trim();
        self.general.hotkey = if hotkey.is_empty() {
            GeneralConfig::default().hotkey
        } else {
            hotkey.to_owned()
        };
        self
    }
}

/// Key of the one array of tables in the schema.
const WEB_SEARCH_KEY: &str = "web_search";

/// Applies `updated` (a freshly serialized config) onto `document`.
fn merge_document(document: &mut toml_edit::DocumentMut, updated: &toml_edit::DocumentMut) {
    use toml_edit::Item;

    for (key, new_item) in updated.as_table() {
        if key == WEB_SEARCH_KEY {
            merge_web_search(document.as_table_mut(), new_item);
            continue;
        }
        // The serializer emits sections as inline tables; edit them as tables.
        let new_table = match new_item {
            Item::Table(table) => Some(table.clone()),
            other => other.clone().into_table().ok(),
        };
        let Some(new_table) = new_table else {
            merge_item(document.as_table_mut(), key, new_item);
            continue;
        };
        match document.get_mut(key) {
            Some(Item::Table(table)) => merge_table(table, &new_table),
            // Missing, or not a table: write the section fresh.
            _ => {
                let mut table = new_table;
                table.set_implicit(false);
                document.insert(key, Item::Table(table));
            }
        }
    }
}

fn merge_table(table: &mut toml_edit::Table, new_table: &toml_edit::Table) {
    for (key, new_item) in new_table {
        merge_item(table, key, new_item);
    }
}

/// Sets `table[key]` to `new_item` unless it already holds an equal value.
fn merge_item(table: &mut toml_edit::Table, key: &str, new_item: &toml_edit::Item) {
    let Some(new_value) = new_item.as_value() else {
        return;
    };
    match table.get_mut(key) {
        Some(existing) => {
            if existing
                .as_value()
                .is_some_and(|old| values_equal(old, new_value))
            {
                return;
            }
            let mut replacement = new_value.clone();
            // Keep the spacing and trailing `# comment` of the old value.
            if let Some(old) = existing.as_value() {
                *replacement.decor_mut() = old.decor().clone();
            }
            *existing = toml_edit::Item::Value(replacement);
        }
        None => {
            table.insert(key, toml_edit::Item::Value(new_value.clone()));
        }
    }
}

fn merge_web_search(root: &mut toml_edit::Table, new_item: &toml_edit::Item) {
    use toml_edit::Item;

    let mut new_engines = match new_item {
        Item::ArrayOfTables(tables) => tables.clone(),
        other => other
            .clone()
            .into_array_of_tables()
            .unwrap_or_else(|_| toml_edit::ArrayOfTables::new()),
    };

    let unchanged = match root.get(WEB_SEARCH_KEY) {
        Some(Item::ArrayOfTables(old)) => {
            old.len() == new_engines.len()
                && old
                    .iter()
                    .zip(new_engines.iter())
                    .all(|(a, b)| tables_equal(a, b))
        }
        Some(Item::Value(toml_edit::Value::Array(old))) => old.is_empty() && new_engines.is_empty(),
        _ => false,
    };
    if unchanged {
        return;
    }

    // An empty list cannot be written as `[[web_search]]` tables, and omitting
    // the key would bring the default engines back on the next load.
    if new_engines.is_empty() {
        root.insert(
            WEB_SEARCH_KEY,
            Item::Value(toml_edit::Value::Array(toml_edit::Array::new())),
        );
        return;
    }

    // The comment block above the first `[[web_search]]` belongs to the list.
    let leading_decor = match root.get(WEB_SEARCH_KEY) {
        Some(Item::ArrayOfTables(old)) => old.iter().next().map(|t| t.decor().clone()),
        _ => None,
    };
    if let (Some(decor), Some(first)) = (leading_decor, new_engines.iter_mut().next()) {
        *first.decor_mut() = decor;
    }
    root.insert(WEB_SEARCH_KEY, Item::ArrayOfTables(new_engines));
}

fn tables_equal(a: &toml_edit::Table, b: &toml_edit::Table) -> bool {
    a.len() == b.len()
        && a.iter().all(|(key, item)| {
            match (
                item.as_value(),
                b.get(key).and_then(toml_edit::Item::as_value),
            ) {
                (Some(x), Some(y)) => values_equal(x, y),
                _ => false,
            }
        })
}

/// Semantic equality: formatting and comments are ignored.
fn values_equal(a: &toml_edit::Value, b: &toml_edit::Value) -> bool {
    use toml_edit::Value;
    match (a, b) {
        (Value::String(x), Value::String(y)) => x.value() == y.value(),
        (Value::Integer(x), Value::Integer(y)) => x.value() == y.value(),
        (Value::Float(x), Value::Float(y)) => x.value() == y.value(),
        (Value::Boolean(x), Value::Boolean(y)) => x.value() == y.value(),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| values_equal(p, q))
        }
        (Value::InlineTable(x), Value::InlineTable(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(key, p)| y.get(key).is_some_and(|q| values_equal(p, q)))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_template_matches_default_struct() {
        let parsed = Config::from_toml_str(DEFAULT_CONFIG_TOML).expect("template parses");
        assert_eq!(parsed, Config::default());
    }

    #[test]
    fn empty_file_yields_defaults() {
        assert_eq!(Config::from_toml_str("").unwrap(), Config::default());
    }

    #[test]
    fn partial_file_fills_missing_fields() {
        let config = Config::from_toml_str("[general]\nhotkey = \"Ctrl+Space\"\n").unwrap();
        assert_eq!(config.general.hotkey, "Ctrl+Space");
        assert!(config.general.hide_on_blur);
        assert_eq!(config.window, WindowConfig::default());
        assert_eq!(config.linux, LinuxConfig::default());
        assert_eq!(config.web_search, WebSearchEngine::defaults());
    }

    #[test]
    fn web_search_entries_replace_defaults_and_invalid_ones_are_dropped() {
        let config = Config::from_toml_str(
            r#"
[[web_search]]
keyword = "ddg"
name = "DuckDuckGo"
url = "https://duckduckgo.com/?q={query}"

[[web_search]]
keyword = "x"
name = "Broken"
url = "https://example.com"
"#,
        )
        .unwrap();
        assert_eq!(config.web_search.len(), 1);
        assert_eq!(config.web_search[0].keyword, "ddg");
    }

    #[test]
    fn theme_and_plugin_toggles_parse() {
        let config = Config::from_toml_str(
            "[appearance]\ntheme = \"dark\"\n[plugins]\ndisabled = [\"files\"]\n",
        )
        .unwrap();
        assert_eq!(config.appearance.theme, Theme::Dark);
        assert!(!config.plugins.is_enabled("files"));
        assert!(config.plugins.is_enabled("apps"));
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let config = Config::from_toml_str("[future]\nthing = 1\n[general]\nnew_key = true\n");
        assert_eq!(config.unwrap(), Config::default());
    }

    #[test]
    fn out_of_range_values_are_normalized() {
        let config =
            Config::from_toml_str("[general]\nhotkey = \"  \"\n[window]\nwidth = 10\n").unwrap();
        assert_eq!(config.general.hotkey, "Alt+Space");
        assert_eq!(config.window.width, MIN_WINDOW_WIDTH);

        let config = Config::from_toml_str("[window]\nwidth = 99999\n").unwrap();
        assert_eq!(config.window.width, MAX_WINDOW_WIDTH);
    }

    #[test]
    fn wrong_types_are_rejected() {
        assert!(Config::from_toml_str("[general]\nhide_on_blur = \"yes\"\n").is_err());
    }

    #[test]
    fn load_or_create_writes_defaults_then_reads_them_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("config.toml");

        let (config, origin) = Config::load_or_create(&path).unwrap();
        assert_eq!(origin, ConfigOrigin::Created);
        assert_eq!(config, Config::default());
        assert_eq!(fs::read_to_string(&path).unwrap(), DEFAULT_CONFIG_TOML);

        let (config, origin) = Config::load_or_create(&path).unwrap();
        assert_eq!(origin, ConfigOrigin::Loaded);
        assert_eq!(config, Config::default());
    }

    #[test]
    fn invalid_file_is_reported_and_left_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "[general\nhotkey = ").unwrap();

        let err = Config::load_or_create(&path).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
        assert_eq!(fs::read_to_string(&path).unwrap(), "[general\nhotkey = ");
    }

    fn saved(existing: Option<&str>, config: &Config) -> String {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        if let Some(text) = existing {
            fs::write(&path, text).unwrap();
        }
        config.save_to(&path).unwrap();
        assert!(
            !dir.path().join("config.toml.tmp").exists(),
            "temporary file left behind"
        );
        fs::read_to_string(&path).unwrap()
    }

    #[test]
    fn saving_an_unchanged_config_keeps_the_file_byte_identical() {
        assert_eq!(
            saved(Some(DEFAULT_CONFIG_TOML), &Config::default()),
            DEFAULT_CONFIG_TOML
        );
    }

    #[test]
    fn saving_without_a_file_creates_it_from_the_template() {
        assert_eq!(saved(None, &Config::default()), DEFAULT_CONFIG_TOML);
    }

    #[test]
    fn saving_creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a").join("b").join("config.toml");
        Config::default().save_to(&path).unwrap();
        assert!(path.is_file());
    }

    #[test]
    fn changed_values_persist_and_comments_survive() {
        let mut config = Config::default();
        config.general.hotkey = "Ctrl+Shift+K".to_owned();
        config.general.launch_at_login = true;
        config.search.max_results = 12;
        config.appearance.theme = Theme::Dark;
        config.window.width = 900;
        config.files.directories = vec!["~/Projects".to_owned()];
        config.plugins.disabled = vec!["files".to_owned()];

        let text = saved(Some(DEFAULT_CONFIG_TOML), &config);
        for comment in [
            "# Sevak configuration",
            "# Shortcut that shows and hides Sevak.",
            "# Width of the search window in logical pixels (400-1600).",
            "# \"system\", \"light\" or \"dark\".",
            "# Index dot-files and dot-folders.",
            "# Web search engines: type",
        ] {
            assert!(text.contains(comment), "lost comment {comment:?}:\n{text}");
        }
        assert_eq!(Config::from_toml_str(&text).unwrap(), config);

        // Order is the document's own: the hotkey stays the first key of [general].
        let general = text.find("[general]").unwrap();
        let hotkey = text.find("hotkey = ").unwrap();
        let hide = text.find("hide_on_blur").unwrap();
        assert!(general < hotkey && hotkey < hide);
    }

    fn fallbacks(text: &str) -> Vec<String> {
        Config::from_toml_str(text)
            .unwrap()
            .search
            .fallback_web_search
            .keywords()
            .to_vec()
    }

    #[test]
    fn fallback_accepts_a_string_or_a_list() {
        assert_eq!(fallbacks(""), ["g"]);
        assert_eq!(
            fallbacks("[search]\nfallback_web_search = \" yt \"\n"),
            ["yt"]
        );
        assert!(fallbacks("[search]\nfallback_web_search = \"\"\n").is_empty());
        assert_eq!(
            fallbacks("[search]\nfallback_web_search = [\"g\", \"yt\", \"G\", \" \", \"gh\"]\n"),
            ["g", "yt", "gh"]
        );
        assert!(fallbacks("[search]\nfallback_web_search = []\n").is_empty());
        assert!(Config::from_toml_str("[search]\nfallback_web_search = 3\n").is_err());
    }

    #[test]
    fn fallback_round_trips_through_json_in_either_form() {
        let one = FallbackSearch::single("g");
        assert_eq!(serde_json::to_string(&one).unwrap(), "\"g\"");
        let many = FallbackSearch::list(["g", "yt"]);
        assert_eq!(serde_json::to_string(&many).unwrap(), "[\"g\",\"yt\"]");
        for value in [
            one,
            many,
            FallbackSearch::single(""),
            FallbackSearch::list(["g"]),
        ] {
            let json = serde_json::to_string(&value).unwrap();
            assert_eq!(
                serde_json::from_str::<FallbackSearch>(&json).unwrap(),
                value
            );
        }
    }

    #[test]
    fn saving_keeps_the_fallback_form_and_its_comments() {
        let mut config = Config::default();
        // Unchanged: a list of one stays a list, the file stays byte-identical.
        let existing = DEFAULT_CONFIG_TOML.replace(
            "fallback_web_search = \"g\"",
            "fallback_web_search = [\"g\"]",
        );
        config.search.fallback_web_search = FallbackSearch::list(["g"]);
        assert_eq!(saved(Some(&existing), &config), existing);

        config.search.fallback_web_search = FallbackSearch::list(["g", "yt", "gh"]);
        config.search.query_history = false;
        let text = saved(Some(&existing), &config);
        assert!(
            text.contains("fallback_web_search = [\"g\", \"yt\", \"gh\"]"),
            "{text}"
        );
        assert!(text.contains("# disable). A list offers several"), "{text}");
        assert!(text.contains("query_history = false"), "{text}");
        assert_eq!(Config::from_toml_str(&text).unwrap(), config);

        // Back to the string form.
        config.search.fallback_web_search = FallbackSearch::single("yt");
        let text = saved(Some(&text), &config);
        assert!(text.contains("fallback_web_search = \"yt\""), "{text}");
    }

    #[test]
    fn query_history_defaults_on_and_is_added_to_older_files() {
        assert!(Config::default().search.query_history);
        let old = "[search]\nmax_results = 8\n";
        assert!(Config::from_toml_str(old).unwrap().search.query_history);
        let text = saved(Some(old), &Config::default());
        assert!(text.contains("query_history = true"), "{text}");
    }

    #[test]
    fn user_comments_and_unknown_keys_survive() {
        let existing = "\
# my own notes
[general]
hotkey = \"Alt+Space\" # my favourite
future_key = 42

[search]
max_results = 8 # keep it short

[custom]
thing = true
";
        let mut config = Config::default();
        config.general.hotkey = "Ctrl+Space".to_owned();
        config.search.max_results = 5;
        let text = saved(Some(existing), &config);

        assert!(text.starts_with("# my own notes\n[general]\n"));
        assert!(text.contains("hotkey = \"Ctrl+Space\" # my favourite"));
        assert!(text.contains("max_results = 5 # keep it short"));
        assert!(text.contains("future_key = 42"));
        assert!(text.contains("[custom]\nthing = true"));
        assert_eq!(Config::from_toml_str(&text).unwrap().search.max_results, 5);
    }

    #[test]
    fn missing_sections_and_keys_are_added() {
        let text = saved(
            Some("[general]\nhotkey = \"Alt+Space\"\n"),
            &Config::default(),
        );
        assert!(text.contains("hide_on_blur = true"));
        assert!(text.contains("[window]\nwidth = 720"));
        assert_eq!(Config::from_toml_str(&text).unwrap(), Config::default());
    }

    #[test]
    fn web_search_list_is_replaced_and_keeps_its_comment() {
        let config = Config {
            web_search: vec![
                WebSearchEngine {
                    keyword: "ddg".to_owned(),
                    name: "DuckDuckGo".to_owned(),
                    url: "https://duckduckgo.com/?q={query}".to_owned(),
                },
                WebSearchEngine {
                    keyword: "g".to_owned(),
                    name: "Google".to_owned(),
                    url: "https://www.google.com/search?q={query}".to_owned(),
                },
            ],
            ..Config::default()
        };
        let text = saved(Some(DEFAULT_CONFIG_TOML), &config);

        assert!(text.contains("# Web search engines: type"));
        assert!(text.contains("keyword = \"ddg\""));
        assert!(!text.contains("YouTube"));
        assert_eq!(
            text.matches(
                "
[[web_search]]
"
            )
            .count(),
            2
        );
        assert_eq!(
            Config::from_toml_str(&text).unwrap().web_search,
            config.web_search
        );
    }

    #[test]
    fn an_empty_web_search_list_is_remembered() {
        let mut config = Config::default();
        config.web_search.clear();
        let text = saved(Some(DEFAULT_CONFIG_TOML), &config);
        assert!(!text.contains("[[web_search]]"));
        assert!(Config::from_toml_str(&text).unwrap().web_search.is_empty());

        // And back again.
        let text = saved(Some(&text), &Config::default());
        assert_eq!(
            Config::from_toml_str(&text).unwrap().web_search,
            WebSearchEngine::defaults()
        );
    }

    #[test]
    fn saving_over_an_invalid_file_fails_and_leaves_it_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "[general\nhotkey = ").unwrap();
        let err = Config::default().save_to(&path).unwrap_err();
        assert!(matches!(err, ConfigError::Edit { .. }));
        assert_eq!(fs::read_to_string(&path).unwrap(), "[general\nhotkey = ");
    }

    #[test]
    fn crlf_files_stay_parseable_after_saving() {
        let existing = DEFAULT_CONFIG_TOML.replace('\n', "\r\n");
        let mut config = Config::default();
        config.search.max_results = 3;
        let text = saved(Some(&existing), &config);
        assert!(text.contains("# Sevak configuration\r\n"), "{text:?}");
        assert_eq!(Config::from_toml_str(&text).unwrap(), config);
    }
}
