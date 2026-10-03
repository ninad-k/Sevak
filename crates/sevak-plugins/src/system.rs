//! System commands: lock, sleep, restart, shut down, log out, empty the trash
//! and shortcuts to the operating system's settings pages.
//!
//! A global plugin: typing `lock` or `bluetooth` finds the command the way
//! typing `fire` finds Firefox. What is offered depends on the machine (the
//! platform provider reports which commands can work), and `[system] disabled`
//! hides entries the user does not want near the Enter key.
//!
//! Activation is two-phase. [`Plugin::confirmation`] lets the shell ask the
//! user first for the destructive commands (unless `[system] confirm = false`),
//! then [`Plugin::execute`] calls the platform provider. Every entry's action is
//! an [`Action::Custom`] naming the command, so the provider only ever receives
//! a variant of its closed vocabulary.

use std::sync::{Arc, RwLock};

use sevak_core::config::SystemConfig;
use sevak_core::{Action, FuzzyQuery, IconSource, Plugin, PluginError, PluginResult, ResultItem};
use sevak_platform::{PlatformProvider, SettingsPage, SystemCommand};

use crate::apps::name_bonus;

/// Queries shorter than this match nothing: one letter would put `Sleep`,
/// `Shut down` and `Sound settings` above the applications the user wants.
const MIN_QUERY_CHARS: usize = 2;
/// Aliases and the `settings <page>` spelling are weaker evidence than the title.
const ALIAS_WEIGHT: f64 = 0.9;
/// Breaks ties in favour of shorter (more specific) titles, as for apps.
const LENGTH_PENALTY: f64 = 0.01;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Command(SystemCommand),
    Settings(SettingsPage),
}

/// Prefix of the [`Action::Custom`] payload for a command.
const COMMAND_PREFIX: &str = "command:";
/// Prefix of the [`Action::Custom`] payload for a settings page.
const SETTINGS_PREFIX: &str = "settings:";

impl Target {
    /// The `[system] disabled` spelling and the result id suffix.
    fn key(self) -> String {
        match self {
            Self::Command(command) => command.key().to_owned(),
            Self::Settings(page) => format!("{SETTINGS_PREFIX}{}", page.key()),
        }
    }

    fn payload(self) -> String {
        match self {
            Self::Command(command) => format!("{COMMAND_PREFIX}{}", command.key()),
            Self::Settings(page) => format!("{SETTINGS_PREFIX}{}", page.key()),
        }
    }

    fn from_payload(payload: &str) -> Option<Self> {
        if let Some(key) = payload.strip_prefix(COMMAND_PREFIX) {
            return SystemCommand::from_key(key).map(Self::Command);
        }
        payload
            .strip_prefix(SETTINGS_PREFIX)
            .and_then(SettingsPage::from_key)
            .map(Self::Settings)
    }
}

struct Entry {
    target: Target,
    title: String,
    title_lower: String,
    subtitle: String,
    /// Other names the entry answers to (lowercase).
    aliases: Vec<String>,
    icon: &'static str,
}

/// The system commands and settings pages this machine offers.
pub struct SystemPlugin {
    platform: Arc<dyn PlatformProvider>,
    config: SystemConfig,
    /// Rebuilt by [`Plugin::refresh`]; queries read a snapshot.
    entries: RwLock<Arc<Vec<Entry>>>,
}

impl SystemPlugin {
    pub fn new(config: SystemConfig, platform: Arc<dyn PlatformProvider>) -> Self {
        Self {
            platform,
            config,
            entries: RwLock::new(Arc::new(Vec::new())),
        }
    }

    fn snapshot(&self) -> Arc<Vec<Entry>> {
        self.entries
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn build_entries(&self) -> Vec<Entry> {
        let mut entries = Vec::new();

        let commands = self.platform.supported_system_commands();
        for command in SystemCommand::ALL {
            if commands.contains(&command) && !self.config.is_disabled(command.key()) {
                entries.push(command_entry(command));
            }
        }

        if !self.config.is_disabled("settings") {
            let pages = self.platform.supported_settings_pages();
            for page in SettingsPage::ALL {
                let target = Target::Settings(page);
                if pages.contains(&page) && !self.config.is_disabled(&target.key()) {
                    entries.push(settings_entry(page));
                }
            }
        }
        entries
    }
}

fn entry(
    target: Target,
    title: &str,
    subtitle: &str,
    aliases: &[&str],
    icon: &'static str,
) -> Entry {
    Entry {
        target,
        title: title.to_owned(),
        title_lower: title.to_lowercase(),
        subtitle: subtitle.to_owned(),
        aliases: aliases.iter().map(|alias| alias.to_lowercase()).collect(),
        icon,
    }
}

fn command_entry(command: SystemCommand) -> Entry {
    let target = Target::Command(command);
    match command {
        SystemCommand::Lock => entry(
            target,
            "Lock screen",
            "Lock your session",
            &["lock", "lock computer", "lock workstation"],
            "lock",
        ),
        SystemCommand::Sleep => entry(
            target,
            "Sleep",
            "Put the computer to sleep",
            &["suspend", "standby"],
            "sleep",
        ),
        SystemCommand::Hibernate => entry(
            target,
            "Hibernate",
            "Save the session to disk and power off",
            &[],
            "sleep",
        ),
        SystemCommand::Restart => entry(
            target,
            "Restart",
            "Restart the computer",
            &["reboot"],
            "restart",
        ),
        SystemCommand::ShutDown => entry(
            target,
            "Shut down",
            "Turn the computer off",
            &["shutdown", "power off", "poweroff", "turn off"],
            "power",
        ),
        SystemCommand::LogOut => entry(
            target,
            "Log out",
            "End your session",
            &["logout", "log off", "logoff", "sign out", "signout"],
            "logout",
        ),
        SystemCommand::EmptyTrash if cfg!(windows) => entry(
            target,
            "Empty Recycle Bin",
            "Permanently delete everything in the Recycle Bin",
            &["empty trash", "recycle bin", "clear recycle bin"],
            "trash",
        ),
        SystemCommand::EmptyTrash => entry(
            target,
            "Empty Trash",
            "Permanently delete everything in the Trash",
            &["empty recycle bin", "clear trash", "trash"],
            "trash",
        ),
    }
}

fn settings_entry(page: SettingsPage) -> Entry {
    let name = match page {
        SettingsPage::Display => "Display",
        SettingsPage::Sound => "Sound",
        SettingsPage::Bluetooth => "Bluetooth",
        SettingsPage::Network => "Network",
        SettingsPage::Wifi => "Wi-Fi",
        SettingsPage::Apps => "Apps",
        SettingsPage::Notifications => "Notifications",
        SettingsPage::Power => "Power and battery",
        SettingsPage::Keyboard => "Keyboard",
        SettingsPage::Mouse => "Mouse",
        SettingsPage::Privacy => "Privacy",
        SettingsPage::DateTime => "Date and time",
    };
    let lower = name.to_lowercase();
    let mut aliases = vec![
        format!("settings {lower}"),
        format!("open settings {lower}"),
    ];
    if page == SettingsPage::Wifi {
        aliases.push("wifi".to_owned());
    }
    Entry {
        target: Target::Settings(page),
        title: format!("{name} settings"),
        title_lower: format!("{lower} settings"),
        subtitle: format!("Open {name} settings"),
        aliases,
        icon: "settings",
    }
}

/// The question asked before a destructive command runs.
fn confirmation_text(command: SystemCommand) -> &'static str {
    match command {
        SystemCommand::Restart => {
            "Restart the computer now? Unsaved work in open apps may be lost."
        }
        SystemCommand::ShutDown => {
            "Shut down the computer now? Unsaved work in open apps may be lost."
        }
        SystemCommand::LogOut => "Log out now? Unsaved work in open apps may be lost.",
        SystemCommand::EmptyTrash if cfg!(windows) => {
            "Permanently delete everything in the Recycle Bin? This cannot be undone."
        }
        SystemCommand::EmptyTrash => {
            "Permanently delete everything in the Trash? This cannot be undone."
        }
        SystemCommand::Lock | SystemCommand::Sleep | SystemCommand::Hibernate => "",
    }
}

impl Plugin for SystemPlugin {
    fn id(&self) -> &str {
        "system"
    }

    fn name(&self) -> &str {
        "System commands"
    }

    fn description(&self) -> &str {
        "Lock, sleep, restart, shut down, log out, empty the trash and open settings pages."
    }

    fn keyword(&self) -> Option<&str> {
        None
    }

    fn query(&self, input: &str) -> Vec<ResultItem> {
        let input = input.trim();
        if input.chars().count() < MIN_QUERY_CHARS {
            return Vec::new();
        }
        let mut query = FuzzyQuery::new(input);
        if query.is_empty() {
            return Vec::new();
        }
        let query_lower = input.to_lowercase();

        let entries = self.snapshot();
        let mut scored: Vec<(f64, &Entry)> = Vec::new();
        for entry in entries.iter() {
            let mut best = query.score(&entry.title).map(|s| {
                f64::from(s) + name_bonus(&entry.title_lower, &query_lower)
                    - entry.title_lower.len() as f64 * LENGTH_PENALTY
            });
            for alias in &entry.aliases {
                if let Some(s) = query.score(alias) {
                    let s = f64::from(s) * ALIAS_WEIGHT + name_bonus(alias, &query_lower);
                    best = Some(best.map_or(s, |b| b.max(s)));
                }
            }
            if let Some(best) = best {
                scored.push((best, entry));
            }
        }

        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        scored
            .into_iter()
            .map(|(score, entry)| {
                ResultItem::new(
                    "system",
                    entry.target.key(),
                    &entry.title,
                    Action::Custom {
                        payload: entry.target.payload(),
                    },
                )
                .with_subtitle(&entry.subtitle)
                .with_icon(IconSource::builtin(entry.icon))
                .with_score(score)
            })
            .collect()
    }

    fn confirmation(&self, item: &ResultItem) -> Option<String> {
        if !self.config.confirm {
            return None;
        }
        let Action::Custom { payload } = &item.action else {
            return None;
        };
        match Target::from_payload(payload)? {
            Target::Command(command) if command.is_destructive() => {
                Some(confirmation_text(command).to_owned())
            }
            Target::Command(_) | Target::Settings(_) => None,
        }
    }

    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        let Action::Custom { payload } = &item.action else {
            return Err(PluginError::Unsupported(item.id.clone()));
        };
        match Target::from_payload(payload) {
            Some(Target::Command(command)) => self
                .platform
                .run_system_command(command)
                .map_err(PluginError::other),
            Some(Target::Settings(page)) => self
                .platform
                .open_settings_page(page)
                .map_err(PluginError::other),
            None => Err(PluginError::Unsupported(item.id.clone())),
        }
    }

    fn refresh(&self) -> PluginResult<()> {
        let entries = Arc::new(self.build_entries());
        *self
            .entries
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = entries;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::MockPlatform;

    fn platform(commands: &[SystemCommand], pages: &[SettingsPage]) -> Arc<MockPlatform> {
        let platform = MockPlatform::empty();
        *platform.system_commands.lock().unwrap() = commands.to_vec();
        *platform.settings_pages.lock().unwrap() = pages.to_vec();
        platform
    }

    fn plugin(config: SystemConfig, platform: &Arc<MockPlatform>) -> SystemPlugin {
        let plugin = SystemPlugin::new(config, platform.clone());
        plugin.refresh().unwrap();
        plugin
    }

    fn everything() -> Arc<MockPlatform> {
        platform(&SystemCommand::ALL, &SettingsPage::ALL)
    }

    fn titles(plugin: &SystemPlugin, input: &str) -> Vec<String> {
        plugin
            .query(input)
            .into_iter()
            .map(|item| item.title)
            .collect()
    }

    fn first(plugin: &SystemPlugin, input: &str) -> ResultItem {
        plugin.query(input).into_iter().next().unwrap_or_else(|| {
            panic!("no result for {input:?}");
        })
    }

    #[test]
    fn metadata() {
        let plugin = SystemPlugin::new(SystemConfig::default(), everything());
        assert_eq!(plugin.id(), "system");
        assert_eq!(plugin.keyword(), None);
        assert!(plugin.global());
    }

    #[test]
    fn nothing_is_offered_before_the_first_refresh() {
        let plugin = SystemPlugin::new(SystemConfig::default(), everything());
        assert!(plugin.query("lock").is_empty());
    }

    #[test]
    fn finds_commands_by_title_and_alias() {
        let plugin = plugin(SystemConfig::default(), &everything());
        assert_eq!(first(&plugin, "lock").title, "Lock screen");
        assert_eq!(first(&plugin, "reboot").title, "Restart");
        assert_eq!(first(&plugin, "shutdown").title, "Shut down");
        assert_eq!(first(&plugin, "power off").title, "Shut down");
        assert_eq!(first(&plugin, "logout").title, "Log out");
        assert_eq!(first(&plugin, "sign out").title, "Log out");
        assert_eq!(first(&plugin, "suspend").title, "Sleep");
        assert_eq!(first(&plugin, "hibern").title, "Hibernate");
    }

    #[test]
    fn empty_trash_is_named_for_the_platform() {
        let plugin = plugin(SystemConfig::default(), &everything());
        let expected = if cfg!(windows) {
            "Empty Recycle Bin"
        } else {
            "Empty Trash"
        };
        assert_eq!(first(&plugin, "empty trash").title, expected);
        assert_eq!(first(&plugin, "recycle bin").title, expected);
    }

    #[test]
    fn finds_settings_pages_in_either_word_order() {
        let plugin = plugin(SystemConfig::default(), &everything());
        assert_eq!(first(&plugin, "bluetooth").title, "Bluetooth settings");
        assert_eq!(
            first(&plugin, "settings bluetooth").title,
            "Bluetooth settings"
        );
        assert_eq!(first(&plugin, "wifi").title, "Wi-Fi settings");
        assert_eq!(first(&plugin, "display").title, "Display settings");
    }

    #[test]
    fn short_and_unrelated_queries_match_nothing() {
        let plugin = plugin(SystemConfig::default(), &everything());
        assert!(plugin.query("").is_empty());
        assert!(plugin.query("s").is_empty());
        assert!(plugin.query("  l ").is_empty());
        assert!(plugin.query("firefox").is_empty());
    }

    #[test]
    fn only_supported_entries_are_offered() {
        let platform = platform(
            &[SystemCommand::Lock, SystemCommand::Sleep],
            &[SettingsPage::Display],
        );
        let plugin = plugin(SystemConfig::default(), &platform);
        assert!(titles(&plugin, "restart").is_empty());
        assert!(titles(&plugin, "hibernate").is_empty());
        assert!(titles(&plugin, "bluetooth").is_empty());
        assert_eq!(first(&plugin, "sleep").title, "Sleep");
        assert_eq!(first(&plugin, "display").title, "Display settings");
    }

    #[test]
    fn disabled_commands_and_pages_are_hidden() {
        let config = SystemConfig {
            confirm: true,
            disabled: vec![
                "restart".into(),
                "Shutdown".into(),
                "settings:bluetooth".into(),
            ],
        };
        let plugin = plugin(config, &everything());
        assert!(titles(&plugin, "reboot").is_empty());
        assert!(titles(&plugin, "shut down")
            .iter()
            .all(|t| t != "Shut down"));
        assert!(titles(&plugin, "bluetooth")
            .iter()
            .all(|t| t != "Bluetooth settings"));
        assert_eq!(first(&plugin, "lock").title, "Lock screen");
        assert_eq!(first(&plugin, "sound").title, "Sound settings");
    }

    #[test]
    fn disabling_settings_hides_every_page_but_not_commands() {
        let config = SystemConfig {
            confirm: true,
            disabled: vec!["settings".into()],
        };
        let plugin = plugin(config, &everything());
        assert!(titles(&plugin, "display").is_empty());
        assert_eq!(first(&plugin, "lock").title, "Lock screen");
    }

    #[test]
    fn results_are_stable_custom_actions_with_icons() {
        let plugin = plugin(SystemConfig::default(), &everything());
        let item = first(&plugin, "restart");
        assert_eq!(item.id, "system:restart");
        assert_eq!(item.plugin_id, "system");
        assert_eq!(
            item.action,
            Action::Custom {
                payload: "command:restart".into()
            }
        );
        assert_eq!(item.icon, Some(IconSource::builtin("restart")));
        assert!(item.score > 0.0);

        let settings = first(&plugin, "bluetooth");
        assert_eq!(settings.id, "system:settings:bluetooth");
        assert_eq!(settings.icon, Some(IconSource::builtin("settings")));
    }

    #[test]
    fn exact_name_ranks_above_a_loose_match() {
        let plugin = plugin(SystemConfig::default(), &everything());
        let results = plugin.query("sleep");
        assert_eq!(results[0].title, "Sleep");
        assert!(results.windows(2).all(|w| w[0].score >= w[1].score));
    }

    #[test]
    fn destructive_commands_ask_first_and_the_others_do_not() {
        let plugin = plugin(SystemConfig::default(), &everything());
        for (query, asks) in [
            ("restart", true),
            ("shutdown", true),
            ("logout", true),
            ("empty trash", true),
            ("lock", false),
            ("sleep", false),
            ("hibernate", false),
            ("display", false),
        ] {
            let item = first(&plugin, query);
            assert_eq!(plugin.confirmation(&item).is_some(), asks, "{query}");
        }
    }

    #[test]
    fn confirmation_can_be_switched_off() {
        let config = SystemConfig {
            confirm: false,
            disabled: Vec::new(),
        };
        let plugin = plugin(config, &everything());
        assert_eq!(plugin.confirmation(&first(&plugin, "shutdown")), None);
    }

    #[test]
    fn confirmation_text_names_the_consequence() {
        let plugin = plugin(SystemConfig::default(), &everything());
        let text = plugin.confirmation(&first(&plugin, "shutdown")).unwrap();
        assert!(text.starts_with("Shut down"), "{text}");
        let text = plugin.confirmation(&first(&plugin, "empty trash")).unwrap();
        assert!(text.contains("cannot be undone"), "{text}");
    }

    #[test]
    fn execute_runs_the_command_through_the_platform() {
        let platform = everything();
        let plugin = plugin(SystemConfig::default(), &platform);
        plugin.execute(&first(&plugin, "restart")).unwrap();
        plugin.execute(&first(&plugin, "lock")).unwrap();
        assert_eq!(
            *platform.ran_commands.lock().unwrap(),
            [SystemCommand::Restart, SystemCommand::Lock]
        );
        assert!(platform.opened_settings.lock().unwrap().is_empty());
    }

    #[test]
    fn execute_opens_settings_pages_without_touching_open_url() {
        let platform = everything();
        let plugin = plugin(SystemConfig::default(), &platform);
        plugin.execute(&first(&plugin, "bluetooth")).unwrap();
        assert_eq!(
            *platform.opened_settings.lock().unwrap(),
            [SettingsPage::Bluetooth]
        );
        assert!(platform.opened_urls.lock().unwrap().is_empty());
        assert!(platform.ran_commands.lock().unwrap().is_empty());
    }

    #[test]
    fn execute_rejects_foreign_and_malformed_actions() {
        let platform = everything();
        let plugin = plugin(SystemConfig::default(), &platform);
        let bad_payload = ResultItem::new(
            "system",
            "x",
            "X",
            Action::Custom {
                payload: "command:format_disk".into(),
            },
        );
        assert!(matches!(
            plugin.execute(&bad_payload),
            Err(PluginError::Unsupported(_))
        ));
        let copy = ResultItem::new("system", "y", "Y", Action::CopyText { text: "t".into() });
        assert!(matches!(
            plugin.execute(&copy),
            Err(PluginError::Unsupported(_))
        ));
        assert!(platform.ran_commands.lock().unwrap().is_empty());
        assert_eq!(plugin.confirmation(&bad_payload), None);
    }

    #[test]
    fn payloads_round_trip() {
        for command in SystemCommand::ALL {
            let target = Target::Command(command);
            assert_eq!(Target::from_payload(&target.payload()), Some(target));
        }
        for page in SettingsPage::ALL {
            let target = Target::Settings(page);
            assert_eq!(Target::from_payload(&target.payload()), Some(target));
        }
        assert_eq!(Target::from_payload("command:"), None);
        assert_eq!(Target::from_payload("settings:nope"), None);
    }
}
