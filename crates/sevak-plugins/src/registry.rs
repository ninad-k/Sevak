//! The plugin registry: which plugins exist, and which of them are turned on.
//!
//! A [`PluginDescriptor`] describes one plugin *family* and carries a factory
//! that builds its instances from the [`Config`]. Most families have one
//! instance (`apps`, `calculator`, `files`, `uuid`); web search has one per
//! `[[web_search]]` engine (`web:g`, `web:yt`, ...), all under the family id
//! `web`.
//!
//! A [`PluginRegistry`] is a list of descriptors. [`PluginRegistry::builtin`]
//! is the stock set; a compiled-in third-party plugin is added with
//! [`PluginRegistry::register`]. The registry then offers two views:
//!
//! - [`PluginRegistry::instantiate`]: the live plugins the search engine uses,
//!   with `[plugins] disabled` applied;
//! - [`PluginRegistry::catalog`]: every instance, disabled ones included, for
//!   the settings UI's toggles.
//!
//! # Disabling
//!
//! `[plugins] disabled` accepts a family id (`"web"` turns off every engine) or
//! an instance id (`"web:yt"` turns off one engine). For single-instance plugins
//! the two are the same string.

use std::sync::Arc;

use serde::Serialize;
use sevak_core::{Config, Plugin};
use sevak_platform::PlatformProvider;

use crate::{AppsPlugin, CalculatorPlugin, FilesPlugin, ShellPlugin, UuidPlugin, WebSearchPlugin};

/// Builds the instances of one plugin family.
///
/// A plain function pointer (not a closure) keeps descriptors `Copy`-cheap,
/// `Send + Sync` and trivially declarable as `const`/`static` data. A factory
/// receives the whole [`Config`] so it can read its own section, and must be
/// cheap: indexes start empty and are filled later by [`Plugin::refresh`].
pub type PluginFactory = fn(&Config, &Arc<dyn PlatformProvider>) -> Vec<Arc<dyn Plugin>>;

/// Static description of a plugin family plus the means to instantiate it.
#[derive(Clone)]
pub struct PluginDescriptor {
    /// Family id, matched against `[plugins] disabled` (e.g. `web`, `uuid`).
    /// Instance ids (`web:g`) are a refinement of it and are matched too.
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    factory: PluginFactory,
}

impl PluginDescriptor {
    pub const fn new(
        id: &'static str,
        name: &'static str,
        description: &'static str,
        factory: PluginFactory,
    ) -> Self {
        Self {
            id,
            name,
            description,
            factory,
        }
    }

    fn build(&self, config: &Config, platform: &Arc<dyn PlatformProvider>) -> Vec<Arc<dyn Plugin>> {
        (self.factory)(config, platform)
    }
}

impl std::fmt::Debug for PluginDescriptor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginDescriptor")
            .field("id", &self.id)
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// One plugin instance as shown in settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PluginInfo {
    /// Instance id (`web:g`); this is what a toggle writes to
    /// `[plugins] disabled` (or the family id to switch the whole family).
    pub id: String,
    pub name: String,
    pub description: String,
    pub keyword: Option<String>,
    /// False if the instance or its family is listed in `[plugins] disabled`.
    pub enabled: bool,
}

/// An ordered collection of plugin descriptors.
#[derive(Debug, Clone, Default)]
pub struct PluginRegistry {
    descriptors: Vec<PluginDescriptor>,
}

impl PluginRegistry {
    /// An empty registry; see [`PluginRegistry::builtin`] for the stock one.
    pub fn new() -> Self {
        Self::default()
    }

    /// Apps, calculator, web search, files and the example UUID plugin, in that
    /// order. Order matters only for tie-breaking and logging.
    pub fn builtin() -> Self {
        let mut registry = Self::new();
        registry.register(PluginDescriptor::new(
            "apps",
            "Applications",
            "Launches installed applications.",
            |_, platform| vec![Arc::new(AppsPlugin::new(platform.clone()))],
        ));
        registry.register(PluginDescriptor::new(
            "calculator",
            "Calculator",
            "Evaluates math expressions as you type; Enter copies the result.",
            |_, platform| vec![Arc::new(CalculatorPlugin::new(platform.clone()))],
        ));
        registry.register(PluginDescriptor::new(
            "web",
            "Web search",
            "Keyword-triggered web searches, one per [[web_search]] engine.",
            |config, platform| {
                config
                    .web_search
                    .iter()
                    .map(|engine| {
                        Arc::new(WebSearchPlugin::new(engine, platform.clone())) as Arc<dyn Plugin>
                    })
                    .collect()
            },
        ));
        registry.register(PluginDescriptor::new(
            "files",
            "Files",
            "Finds files and folders in your configured directories.",
            |config, platform| {
                vec![Arc::new(FilesPlugin::new(
                    config.files.clone(),
                    platform.clone(),
                ))]
            },
        ));
        registry.register(PluginDescriptor::new(
            "shell",
            "Terminal commands",
            "Type `> command` to run it in a terminal; recent commands are offered again.",
            |config, platform| {
                vec![Arc::new(ShellPlugin::new(
                    config.shell.clone(),
                    platform.clone(),
                ))]
            },
        ));
        registry.register(PluginDescriptor::new(
            "uuid",
            "UUID generator",
            "Type `uuid ` to generate random UUIDs; Enter copies one.",
            |_, platform| vec![Arc::new(UuidPlugin::new(platform.clone()))],
        ));
        registry
    }

    /// Adds a plugin family. A descriptor whose id is already registered
    /// replaces the old one (keeping its position), so an embedding
    /// application can override a built-in.
    pub fn register(&mut self, descriptor: PluginDescriptor) -> &mut Self {
        match self.descriptors.iter_mut().find(|d| d.id == descriptor.id) {
            Some(existing) => *existing = descriptor,
            None => self.descriptors.push(descriptor),
        }
        self
    }

    pub fn descriptors(&self) -> &[PluginDescriptor] {
        &self.descriptors
    }

    /// Builds the enabled plugins. Disabled families are not even constructed.
    ///
    /// Logs, at `info`, the loaded plugin ids and the skipped ones.
    pub fn instantiate(
        &self,
        config: &Config,
        platform: Arc<dyn PlatformProvider>,
    ) -> Vec<Arc<dyn Plugin>> {
        let mut plugins: Vec<Arc<dyn Plugin>> = Vec::new();
        let mut skipped: Vec<String> = Vec::new();

        for descriptor in &self.descriptors {
            if !config.plugins.is_enabled(descriptor.id) {
                skipped.push(descriptor.id.to_owned());
                continue;
            }
            for plugin in descriptor.build(config, &platform) {
                if config.plugins.is_enabled(plugin.id()) {
                    plugins.push(plugin);
                } else {
                    skipped.push(plugin.id().to_owned());
                }
            }
        }

        let loaded: Vec<&str> = plugins.iter().map(|p| p.id()).collect();
        tracing::info!(loaded = ?loaded, "plugins loaded");
        if !skipped.is_empty() {
            tracing::info!(skipped = ?skipped, "plugins skipped (disabled in config)");
        }
        plugins
    }

    /// Lists every plugin instance, enabled or not, for the settings UI.
    ///
    /// This builds each family's instances (cheap: no indexing happens) just to
    /// read their metadata.
    pub fn catalog(&self, config: &Config, platform: Arc<dyn PlatformProvider>) -> Vec<PluginInfo> {
        let mut infos = Vec::new();
        for descriptor in &self.descriptors {
            let family_enabled = config.plugins.is_enabled(descriptor.id);
            for plugin in descriptor.build(config, &platform) {
                infos.push(PluginInfo {
                    id: plugin.id().to_owned(),
                    name: plugin.name().to_owned(),
                    description: plugin.description().to_owned(),
                    keyword: plugin.keyword().map(str::to_owned),
                    enabled: family_enabled && config.plugins.is_enabled(plugin.id()),
                });
            }
        }
        infos
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::MockPlatform;

    fn ids(registry: &PluginRegistry, config: &Config) -> Vec<String> {
        registry
            .instantiate(config, MockPlatform::empty())
            .iter()
            .map(|p| p.id().to_owned())
            .collect()
    }

    fn config_disabling(ids: &[&str]) -> Config {
        let mut config = Config::default();
        config.plugins.disabled = ids.iter().map(|s| (*s).to_owned()).collect();
        config
    }

    #[test]
    fn builtin_registry_has_all_families() {
        let families: Vec<_> = PluginRegistry::builtin()
            .descriptors()
            .iter()
            .map(|d| d.id)
            .collect();
        assert_eq!(
            families,
            ["apps", "calculator", "web", "files", "shell", "uuid"]
        );
    }

    #[test]
    fn instantiates_everything_by_default() {
        assert_eq!(
            ids(&PluginRegistry::builtin(), &Config::default()),
            [
                "apps",
                "calculator",
                "web:g",
                "web:yt",
                "web:gh",
                "files",
                "shell",
                "uuid"
            ]
        );
    }

    #[test]
    fn disabling_a_family_disables_every_instance() {
        let config = config_disabling(&["web"]);
        assert_eq!(
            ids(&PluginRegistry::builtin(), &config),
            ["apps", "calculator", "files", "shell", "uuid"]
        );
    }

    #[test]
    fn disabling_an_instance_keeps_its_siblings() {
        let config = config_disabling(&["web:yt", "uuid"]);
        assert_eq!(
            ids(&PluginRegistry::builtin(), &config),
            ["apps", "calculator", "web:g", "web:gh", "files", "shell"]
        );
    }

    #[test]
    fn unknown_disabled_ids_are_ignored() {
        let config = config_disabling(&["nope"]);
        assert_eq!(ids(&PluginRegistry::builtin(), &config).len(), 8);
    }

    #[test]
    fn catalog_lists_disabled_instances_too() {
        let config = config_disabling(&["web:yt", "files"]);
        let catalog = PluginRegistry::builtin().catalog(&config, MockPlatform::empty());
        let summary: Vec<_> = catalog.iter().map(|i| (i.id.as_str(), i.enabled)).collect();
        assert_eq!(
            summary,
            [
                ("apps", true),
                ("calculator", true),
                ("web:g", true),
                ("web:yt", false),
                ("web:gh", true),
                ("files", false),
                ("shell", true),
                ("uuid", true),
            ]
        );
    }

    #[test]
    fn catalog_family_toggle_disables_all_instances() {
        let catalog =
            PluginRegistry::builtin().catalog(&config_disabling(&["web"]), MockPlatform::empty());
        for info in catalog.iter().filter(|i| i.id.starts_with("web:")) {
            assert!(!info.enabled, "{}", info.id);
        }
        assert!(catalog.iter().any(|i| i.id == "apps" && i.enabled));
    }

    #[test]
    fn catalog_carries_metadata() {
        let catalog = PluginRegistry::builtin().catalog(&Config::default(), MockPlatform::empty());
        assert!(catalog.iter().all(|i| !i.name.is_empty()));
        assert!(catalog.iter().all(|i| !i.description.is_empty()));
        let uuid = catalog.iter().find(|i| i.id == "uuid").unwrap();
        assert_eq!(uuid.keyword.as_deref(), Some("uuid"));
        assert_eq!(uuid.name, "UUID generator");
        let calc = catalog.iter().find(|i| i.id == "calculator").unwrap();
        assert_eq!(calc.keyword, None);
        let google = catalog.iter().find(|i| i.id == "web:g").unwrap();
        assert_eq!(google.keyword.as_deref(), Some("g"));
    }

    #[test]
    fn plugin_info_serializes_snake_case() {
        let info = PluginInfo {
            id: "web:g".into(),
            name: "Google".into(),
            description: "d".into(),
            keyword: Some("g".into()),
            enabled: false,
        };
        assert_eq!(
            serde_json::to_string(&info).unwrap(),
            r#"{"id":"web:g","name":"Google","description":"d","keyword":"g","enabled":false}"#
        );
    }

    #[test]
    fn register_adds_and_replaces() {
        let mut registry = PluginRegistry::new();
        registry.register(PluginDescriptor::new("uuid", "A", "a", |_, p| {
            vec![Arc::new(UuidPlugin::new(p.clone()))]
        }));
        registry.register(PluginDescriptor::new("calculator", "B", "b", |_, p| {
            vec![Arc::new(CalculatorPlugin::new(p.clone()))]
        }));
        registry.register(PluginDescriptor::new("uuid", "C", "c", |_, p| {
            vec![Arc::new(UuidPlugin::new(p.clone()))]
        }));
        let names: Vec<_> = registry.descriptors().iter().map(|d| d.name).collect();
        assert_eq!(names, ["C", "B"]);
        assert_eq!(ids(&registry, &Config::default()), ["uuid", "calculator"]);
    }

    #[test]
    fn disabled_family_factory_is_not_called() {
        let mut registry = PluginRegistry::new();
        registry.register(PluginDescriptor::new("boom", "Boom", "", |_, _| {
            panic!("factory must not run for a disabled family")
        }));
        assert!(ids(&registry, &config_disabling(&["boom"])).is_empty());
    }
}
