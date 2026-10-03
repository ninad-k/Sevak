//! Sevak's built-in plugins.
//!
//! | Plugin id        | Struct                                  | Keyword | Global |
//! |------------------|-----------------------------------------|---------|--------|
//! | `apps`           | [`AppsPlugin`]                          | none    | yes    |
//! | `calculator`     | [`CalculatorPlugin`]                    | none    | yes    |
//! | `web:<keyword>`  | [`WebSearchPlugin`] (one per engine)    | engine  | no     |
//! | `files`          | [`FilesPlugin`]                         | config  | config |
//! | `files:names`    | [`OsFilesPlugin`] (whole disk, OS index) | `ff`   | no     |
//! | `files:content`  | [`OsFilesPlugin`] (inside files)        | `in`    | no     |
//! | `bookmarks`      | [`BookmarksPlugin`]                     | config  | config |
//! | `system`         | [`SystemPlugin`]                        | none    | yes    |
//! | `tasks`          | [`TasksPlugin`] (automation tasks)      | `t`     | config |
//! | `media`          | [`MediaPlugin`] (play/pause, now playing) | `play` | config |
//! | `shell`          | [`ShellPlugin`]                         | `>`     | no     |
//! | `clipboard`      | [`ClipboardPlugin`] (opt-in history)    | `cb`    | no     |
//! | `snippets`       | [`SnippetsPlugin`]                      | `s`     | no     |
//! | `emoji:word`    | [`EmojiPlugin`] (grid of tiles)         | `emoji` | no     |
//! | `emoji:colon`    | [`EmojiPlugin`] (same, shorter keyword) | `:`     | no     |
//! | `selection`      | [`SelectionPlugin`] (Universal Actions) | none    | no     |
//! | `contacts`       | [`ContactsPlugin`] (opt-in; also `contacts:at`) | `c`, `@` | no |
//! | `1password`      | [`OnePasswordPlugin`] (opt-in)          | `1p`    | no     |
//! | `dict`           | [`DictionaryPlugin`] (also `dict:spell`) | `define`, `spell` | no |
//! | `uuid`           | [`UuidPlugin`] (a tutorial example)     | `uuid`  | no     |
//!
//! Script plugins (`script:<name>`, from `<config dir>/plugins/`) are loaded by
//! [`ScriptPluginHost`]; see [`script`].
//!
//! [`PluginRegistry`] knows these families; [`builtin_plugins`] builds the set
//! for a [`Config`], honouring `[plugins] disabled`, and
//! [`PluginRegistry::catalog`] lists them for settings. See `docs/plugins.md`.

use std::sync::Arc;

use sevak_core::{Config, Plugin};
use sevak_platform::PlatformProvider;

pub mod actions;
pub mod apps;
pub mod bookmarks;
pub mod calculator;
pub mod clipboard_history;
mod clipboard_store;
pub mod contacts;
pub mod currency;
pub mod dictionary;
pub mod emoji;
pub mod example_uuid;
pub mod file_buffer;
pub mod files;
mod live;
pub mod media;
pub mod onepassword;
pub mod os_files;
pub mod path_browse;
pub mod registry;
pub mod script;
pub mod selection;
pub mod shell;
pub mod snippet_expansion;
pub mod snippets;
pub mod system;
pub mod tasks;
pub mod units;
pub mod web_search;
pub mod workflow;

#[cfg(test)]
mod test_util;

pub use actions::execute_action;
pub use apps::AppsPlugin;
pub use bookmarks::BookmarksPlugin;
pub use calculator::CalculatorPlugin;
pub use clipboard_history::ClipboardPlugin;
pub use contacts::ContactsPlugin;
pub use dictionary::DictionaryPlugin;
pub use emoji::EmojiPlugin;
pub use example_uuid::UuidPlugin;
pub use files::FilesPlugin;
pub use media::MediaPlugin;
pub use onepassword::OnePasswordPlugin;
pub use os_files::{files_family, OsFilesPlugin};
pub use registry::{PluginDescriptor, PluginFactory, PluginInfo, PluginRegistry};
pub use script::{ScriptPlugin, ScriptPluginHost};
pub use selection::SelectionPlugin;
pub use shell::ShellPlugin;
pub use snippets::SnippetsPlugin;
pub use system::SystemPlugin;
pub use tasks::TasksPlugin;
pub use web_search::WebSearchPlugin;
pub use workflow::WorkflowHost;

/// Instantiates every enabled built-in plugin (`apps`, `calculator`, one
/// `web:<keyword>` per `[[web_search]]` engine, `files`, `bookmarks`, `system`,
/// `tasks`, `media`, `shell`, `clipboard`, `snippets`, `emoji`, `selection`,
/// `contacts`, `1password`, `dict`, `uuid`).
///
/// Shorthand for `PluginRegistry::builtin().instantiate(config, platform)`.
///
/// Indexes start empty; call [`Plugin::refresh`] on each plugin (on a
/// background thread) to populate `apps`, `files` and `bookmarks`.
pub fn builtin_plugins(
    config: &Config,
    platform: Arc<dyn PlatformProvider>,
) -> Vec<Arc<dyn Plugin>> {
    PluginRegistry::builtin().instantiate(config, platform)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::MockPlatform;

    fn ids(config: &Config) -> Vec<String> {
        builtin_plugins(config, MockPlatform::empty())
            .iter()
            .map(|p| p.id().to_owned())
            .collect()
    }

    #[test]
    fn default_config_yields_all_builtins() {
        assert_eq!(
            ids(&Config::default()),
            [
                "apps",
                "calculator",
                "web:g",
                "web:yt",
                "web:gh",
                "files",
                "files:names",
                "files:content",
                "bookmarks",
                "system",
                "tasks",
                "media",
                "shell",
                "clipboard",
                "snippets",
                "emoji:word",
                "emoji:colon",
                "selection",
                "contacts",
                "contacts:at",
                "1password",
                "dict",
                "dict:spell",
                "uuid"
            ]
        );
    }

    #[test]
    fn disabled_plugins_are_skipped() {
        let mut config = Config::default();
        config.plugins.disabled = vec!["files".into(), "web:yt".into(), "calculator".into()];
        assert_eq!(
            ids(&config),
            [
                "apps",
                "web:g",
                "web:gh",
                "bookmarks",
                "system",
                "tasks",
                "media",
                "shell",
                "clipboard",
                "snippets",
                "emoji:word",
                "emoji:colon",
                "selection",
                "contacts",
                "contacts:at",
                "1password",
                "dict",
                "dict:spell",
                "uuid"
            ]
        );
    }
}
