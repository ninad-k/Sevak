//! Sevak's built-in plugins.
//!
//! | Plugin id        | Struct                                  | Keyword | Global |
//! |------------------|-----------------------------------------|---------|--------|
//! | `apps`           | [`AppsPlugin`]                          | none    | yes    |
//! | `calculator`     | [`CalculatorPlugin`]                    | none    | yes    |
//! | `web:<keyword>`  | [`WebSearchPlugin`] (one per engine)    | engine  | no     |
//! | `files`          | [`FilesPlugin`]                         | config  | config |
//! | `uuid`           | [`UuidPlugin`] (a tutorial example)     | `uuid`  | no     |
//!
//! [`PluginRegistry`] knows these families; [`builtin_plugins`] builds the set
//! for a [`Config`], honouring `[plugins] disabled`, and
//! [`PluginRegistry::catalog`] lists them for settings. See `docs/plugins.md`.

use std::sync::Arc;

use sevak_core::{Config, Plugin};
use sevak_platform::PlatformProvider;

pub mod actions;
pub mod apps;
pub mod calculator;
pub mod example_uuid;
pub mod files;
pub mod path_browse;
pub mod registry;
pub mod web_search;

#[cfg(test)]
mod test_util;

pub use actions::execute_action;
pub use apps::AppsPlugin;
pub use calculator::CalculatorPlugin;
pub use example_uuid::UuidPlugin;
pub use files::FilesPlugin;
pub use registry::{PluginDescriptor, PluginFactory, PluginInfo, PluginRegistry};
pub use web_search::WebSearchPlugin;

/// Instantiates every enabled built-in plugin (`apps`, `calculator`, one
/// `web:<keyword>` per `[[web_search]]` engine, `files`, `uuid`).
///
/// Shorthand for `PluginRegistry::builtin().instantiate(config, platform)`.
///
/// Indexes start empty; call [`Plugin::refresh`] on each plugin (on a
/// background thread) to populate `apps` and `files`.
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
                "uuid"
            ]
        );
    }

    #[test]
    fn disabled_plugins_are_skipped() {
        let mut config = Config::default();
        config.plugins.disabled = vec!["files".into(), "web:yt".into(), "calculator".into()];
        assert_eq!(ids(&config), ["apps", "web:g", "web:gh", "uuid"]);
    }
}
