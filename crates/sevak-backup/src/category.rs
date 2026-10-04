//! What a backup may hold: the categories and, above all, the allowlist.
//!
//! This is the one place that decides what goes into a backup and what a
//! restore will accept. It is an **allowlist**: a file or a `config.toml`
//! section that is not named here is neither written to an archive nor read
//! from one, so a new kind of data (an API key, a history database, a token
//! store) stays out until somebody adds it on purpose. Two tests guard that:
//! every top-level section of [`sevak_core::Config`] must be sorted into a
//! category or into [`LEFT_OUT_TABLES`], and the files Sevak keeps for its own
//! use (clipboard history, usage statistics, approvals, logs, ...) must not
//! pass [`classify`].

use serde::{Deserialize, Serialize};
use sevak_plugins::workflow::model::valid_folder_name;

use crate::limits::{MAX_PATH_BYTES, MAX_PATH_DEPTH};

/// A group of things the user can choose to back up or restore.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// The settings in `config.toml` (everything but snippets and web searches).
    Settings,
    /// `[[snippet]]` entries and the `[snippets]` expansion settings.
    Snippets,
    /// `[[web_search]]` engines and their keywords.
    WebSearch,
    /// Theme files and the custom stylesheet.
    Themes,
    /// Script plugins (the folders under `plugins/`), gallery installs included.
    Plugins,
    /// Workflows (the folders under `workflows/`), gallery installs included.
    Workflows,
}

impl Category {
    pub const ALL: [Self; 6] = [
        Self::Settings,
        Self::Snippets,
        Self::WebSearch,
        Self::Themes,
        Self::Plugins,
        Self::Workflows,
    ];

    /// The stable id used in the manifest, the UI and the command line.
    pub fn id(self) -> &'static str {
        match self {
            Self::Settings => "settings",
            Self::Snippets => "snippets",
            Self::WebSearch => "web_search",
            Self::Themes => "themes",
            Self::Plugins => "plugins",
            Self::Workflows => "workflows",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|category| category.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Settings => "Settings",
            Self::Snippets => "Snippets",
            Self::WebSearch => "Web search engines",
            Self::Themes => "Themes",
            Self::Plugins => "Script plugins",
            Self::Workflows => "Workflows",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Settings => {
                "Your shortcuts, appearance, search and plugin options, from config.toml. \
                 The 1Password section is left out."
            }
            Self::Snippets => "Your snippets and the snippet expansion options.",
            Self::WebSearch => "The web search engines and their keywords.",
            Self::Themes => "Theme files and your custom stylesheet.",
            Self::Plugins => {
                "Script plugins and the ones installed from the gallery, with their scripts. \
                 They run code, so Sevak asks you to allow each one again after a restore."
            }
            Self::Workflows => {
                "Workflows and the ones installed from the gallery, with their scripts. \
                 Sevak asks you to allow each one that runs code again after a restore."
            }
        }
    }

    /// Whether a restore puts code on the computer that will run with the
    /// user's privileges once it is allowed.
    pub fn runs_code(self) -> bool {
        matches!(self, Self::Plugins | Self::Workflows)
    }
}

/// How the restore treats what is already there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Add what the backup has and overwrite what has the same name or key;
    /// keep everything else.
    #[default]
    Merge,
    /// Make the chosen categories exactly like the backup: what the backup does
    /// not have is removed (or reset to its default).
    Replace,
}

impl Mode {
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "merge" => Some(Self::Merge),
            "replace" => Some(Self::Replace),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// config.toml: which sections go where
// ---------------------------------------------------------------------------

/// The `config.toml` tables of [`Category::Settings`].
pub const SETTINGS_TABLES: &[&str] = &[
    "general",
    "window",
    "linux",
    "search",
    "appearance",
    "plugins",
    "calculator",
    "files",
    "bookmarks",
    "system",
    "tasks",
    "media",
    "shell",
    "paste",
    "actions",
    "clipboard",
    "file_buffer",
    "contacts",
    "dictionary",
    "window_management",
];

/// The `[[hotkey]]` entries: part of [`Category::Settings`].
pub const HOTKEY_KEY: &str = "hotkey";
/// The `[snippets]` table of [`Category::Snippets`].
pub const SNIPPETS_TABLE: &str = "snippets";
/// The `[[snippet]]` entries of [`Category::Snippets`].
pub const SNIPPET_KEY: &str = "snippet";
/// The `[[web_search]]` engines of [`Category::WebSearch`].
pub const WEB_SEARCH_KEY: &str = "web_search";

/// Sections of `config.toml` that are left out of every backup on purpose.
/// `onepassword` names the user's 1Password account and the path of its tool;
/// nothing from 1Password travels in a backup. `ai` names the user's AI
/// provider account and endpoint (the API key is stored elsewhere and is never
/// in a backup either); set it up again after a restore.
/// (Restore ignores these too.)
pub const LEFT_OUT_TABLES: &[&str] = &["onepassword", "ai"];

/// The category a top-level `config.toml` key belongs to, or `None` when it is
/// not in the allowlist.
pub fn config_key_category(key: &str) -> Option<Category> {
    if SETTINGS_TABLES.contains(&key) || key == HOTKEY_KEY {
        Some(Category::Settings)
    } else if key == SNIPPETS_TABLE || key == SNIPPET_KEY {
        Some(Category::Snippets)
    } else if key == WEB_SEARCH_KEY {
        Some(Category::WebSearch)
    } else {
        None
    }
}

/// Key names that suggest a secret. A key like this is dropped from a backup
/// even inside an allowlisted section (a section might gain one later), and
/// from a backup being restored. A test checks that no real option of Sevak
/// matches, so this never removes a setting by accident.
const SENSITIVE_KEY_WORDS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "token",
    "api_key",
    "apikey",
    "api-key",
    "private_key",
    "credential",
    "authorization",
    "bearer",
];

pub fn is_sensitive_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    SENSITIVE_KEY_WORDS.iter().any(|word| lower.contains(word))
}

// ---------------------------------------------------------------------------
// Paths inside an archive
// ---------------------------------------------------------------------------

pub const SETTINGS_FILE: &str = "settings.toml";
pub const SNIPPETS_FILE: &str = "snippets.toml";
pub const WEB_SEARCH_FILE: &str = "web-search.toml";
pub const THEMES_DIR: &str = "themes";
pub const CSS_DIR: &str = "custom-css";
pub const PLUGINS_DIR: &str = "plugins";
pub const WORKFLOWS_DIR: &str = "workflows";

/// The manifest file of a script plugin and of a workflow.
pub const PLUGIN_MANIFEST: &str = "plugin.toml";
pub const WORKFLOW_MANIFEST: &str = "workflow.toml";

/// The kinds of data Sevak itself keeps next to the configuration that must
/// never be in a backup, by file name. Not used to decide anything (the
/// allowlist does that); the tests check that [`classify`] rejects each one.
pub const NEVER_INCLUDED_FILES: &[&str] = &[
    "clipboard-history.json",
    "clipboard/clip-1.png",
    "usage.json",
    "script-plugin-approvals.json",
    "currency-rates.json",
    "hotkey-takeover.json",
    "backup-state.json",
    "logs/sevak.2026-01-01.log",
    "history.db",
    "history.sqlite",
    "ai-key.txt",
    "api_key",
    "credentials.json",
    "secrets.toml",
    "tokens.json",
    "token",
    "1password-cache.json",
    "keychain.json",
    "cookies.sqlite",
    "id_rsa",
    ".env",
    "plugins/p/.env",
    "plugins/p/token.json",
    "plugins/p/secrets.toml",
    "plugins/p/server.pem",
    "plugins/p/history.db",
    "workflows/w/credentials.json",
    "workflows/w/.git/config",
    "themes/nested/dir.toml",
    "themes/x.json",
    "config.toml",
    "../config.toml",
    "/etc/passwd",
];

/// Names that suggest a file holds a secret or private history.
const SENSITIVE_NAME_WORDS: &[&str] = &[
    "secret",
    "password",
    "passwd",
    "credential",
    "token",
    "apikey",
    "api_key",
    "api-key",
    "private",
    "id_rsa",
    "id_ed25519",
    "cookie",
    "keychain",
    "history",
    "usage",
    "approval",
    "1password",
];

const SENSITIVE_EXTENSIONS: &[&str] = &[
    "pem", "key", "p12", "pfx", "kdbx", "kdb", "db", "sqlite", "sqlite3", "log", "jks", "keystore",
    "gpg", "asc", "ppk", "env", "age",
];

/// Script and source files are code, not data: a script called
/// `token_counter.py` is fine. Everything else is judged by its name.
const CODE_EXTENSIONS: &[&str] = &[
    "py",
    "js",
    "mjs",
    "cjs",
    "ts",
    "sh",
    "bash",
    "zsh",
    "fish",
    "ps1",
    "psm1",
    "bat",
    "cmd",
    "rb",
    "pl",
    "lua",
    "php",
    "go",
    "rs",
    "java",
    "swift",
    "applescript",
    "scpt",
    "vbs",
    "html",
    "css",
];

/// Folders inside a plugin or workflow that are never included.
pub const SKIPPED_DIRS: &[&str] = &["node_modules", "__pycache__", "venv", "target", "dist"];

fn extension(name: &str) -> Option<String> {
    name.rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
}

/// Whether a file inside a plugin or workflow folder looks like it holds a
/// secret, a database or a history, or is a hidden file (`.env`, `.git/...`).
pub fn is_sensitive_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower.starts_with('.') {
        return true;
    }
    let ext = extension(&lower);
    if let Some(ext) = &ext {
        if SENSITIVE_EXTENSIONS.contains(&ext.as_str()) {
            return true;
        }
        if CODE_EXTENSIONS.contains(&ext.as_str()) {
            return false;
        }
    }
    SENSITIVE_NAME_WORDS.iter().any(|word| lower.contains(word))
}

/// One part of a path inside an archive: plain, portable on every file system,
/// and not a name Windows reserves.
fn component_ok(part: &str) -> bool {
    if part.is_empty() || part.len() > 100 || part.starts_with('.') {
        return false;
    }
    if part.ends_with('.') || part.ends_with(' ') {
        return false;
    }
    if part.chars().any(|c| {
        c.is_control() || matches!(c, '\\' | '/' | ':' | '<' | '>' | '"' | '|' | '?' | '*')
    }) {
        return false;
    }
    let stem = part.split('.').next().unwrap_or(part).to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix)
                .is_some_and(|n| n.len() == 1 && n != "0" && n.as_bytes()[0].is_ascii_digit())
        });
    !reserved
}

/// The category a path inside an archive belongs to, or `None` when the path is
/// not on the allowlist. This is the only gate: [`crate::collect`] only writes
/// paths that pass it and [`crate::archive::read`] rejects any that do not.
///
/// | Path | Category |
/// |---|---|
/// | `settings.toml` | settings |
/// | `snippets.toml` | snippets |
/// | `web-search.toml` | web search |
/// | `themes/<name>.toml`, `custom-css/<name>.css` | themes |
/// | `plugins/<folder>/<files...>` | script plugins |
/// | `workflows/<folder>/<files...>` | workflows |
pub fn classify(path: &str) -> Option<Category> {
    if path.len() > MAX_PATH_BYTES {
        return None;
    }
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() > MAX_PATH_DEPTH || !parts.iter().all(|part| component_ok(part)) {
        return None;
    }
    let last = *parts.last()?;
    match parts.as_slice() {
        [SETTINGS_FILE] => Some(Category::Settings),
        [SNIPPETS_FILE] => Some(Category::Snippets),
        [WEB_SEARCH_FILE] => Some(Category::WebSearch),
        [THEMES_DIR, name] if name.len() > ".toml".len() && name.ends_with(".toml") => {
            Some(Category::Themes)
        }
        [CSS_DIR, name] if name.len() > ".css".len() && name.ends_with(".css") => {
            Some(Category::Themes)
        }
        [dir, folder, _, ..]
            if (*dir == PLUGINS_DIR || *dir == WORKFLOWS_DIR)
                && valid_folder_name(folder)
                && !is_sensitive_name(last)
                && !parts[2..parts.len() - 1]
                    .iter()
                    .any(|part| is_sensitive_name(part) || SKIPPED_DIRS.contains(part)) =>
        {
            Some(if *dir == PLUGINS_DIR {
                Category::Plugins
            } else {
                Category::Workflows
            })
        }
        _ => None,
    }
}

/// The folder (`plugins/<folder>` or `workflows/<folder>`) a path belongs to,
/// and the path inside it.
pub fn split_folder(path: &str) -> Option<(&str, &str)> {
    let mut parts = path.splitn(3, '/');
    let dir = parts.next()?;
    let folder = parts.next()?;
    let rest = parts.next()?;
    if dir == PLUGINS_DIR || dir == WORKFLOWS_DIR {
        Some((folder, rest))
    } else {
        None
    }
}

/// What a person sees as "never included", in the UI and the docs.
pub const NEVER_INCLUDED_TEXT: &[&str] = &[
    "AI assistant and other API keys, tokens and passwords",
    "Clipboard history and the images you copied",
    "Search and usage history",
    "Anything from a password manager or the system credential store, including 1Password",
    "The encrypted history database, if you use one",
    "Logs and diagnostic reports",
    "Which scripts, plugins and workflows you allowed to run",
    "Per-plugin data folders and cached files",
];
