//! Run a command in a terminal: `> ls -la` or `>ls -la`.
//!
//! Like Alfred's Terminal feature. The command is never run until the user
//! presses Enter on the row; the platform layer opens the terminal and shell
//! chosen by `[shell]` and runs it there (see `sevak_platform::terminal`).
//!
//! Recent commands are offered again: the usage store already remembers every
//! executed result, and the engine hands this plugin those entries through
//! [`Plugin::restore_history`]. The command text is the result key
//! (`shell:<command>`), which is the one place the "keys must not be volatile"
//! rule is relaxed: here the command *is* the identity of the result.

use std::sync::{Arc, Mutex, MutexGuard};

use sevak_core::model::score;
use sevak_core::{Action, IconSource, Plugin, PluginError, PluginResult, ResultItem, ShellConfig};
use sevak_platform::PlatformProvider;

/// Commands remembered in memory (the usage store holds the long tail).
const MAX_RECENT: usize = 50;

pub struct ShellPlugin {
    config: ShellConfig,
    platform: Arc<dyn PlatformProvider>,
    /// Most recent first, no duplicates.
    recent: Mutex<Vec<String>>,
}

/// Replaces control characters (a pasted newline) with spaces and trims, so a
/// command is always one line.
fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_owned()
}

impl ShellPlugin {
    pub fn new(config: ShellConfig, platform: Arc<dyn PlatformProvider>) -> Self {
        Self {
            config,
            platform,
            recent: Mutex::new(Vec::new()),
        }
    }

    fn recent(&self) -> MutexGuard<'_, Vec<String>> {
        self.recent.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn remember(&self, command: &str) {
        if command.is_empty() {
            return;
        }
        let mut recent = self.recent();
        recent.retain(|c| c != command);
        recent.insert(0, command.to_owned());
        recent.truncate(MAX_RECENT);
    }

    fn row(&self, command: &str, score: f64, subtitle: &str) -> ResultItem {
        ResultItem::new(
            self.id(),
            command,
            format!("Run `{command}` in terminal"),
            Action::Custom {
                payload: command.to_owned(),
            },
        )
        .with_subtitle(subtitle)
        .with_icon(IconSource::builtin("terminal"))
        .with_score(score)
    }

    fn open_terminal_row(&self) -> ResultItem {
        ResultItem::new(
            self.id(),
            "",
            "Open terminal",
            Action::Custom {
                payload: String::new(),
            },
        )
        .with_subtitle("Type a command to run it")
        .with_icon(IconSource::builtin("terminal"))
        .with_score(score::KEYWORD)
    }
}

impl Plugin for ShellPlugin {
    fn id(&self) -> &str {
        "shell"
    }

    fn name(&self) -> &str {
        "Terminal commands"
    }

    fn description(&self) -> &str {
        "Type `> command` to run it in a terminal; recent commands are offered again."
    }

    fn keyword(&self) -> Option<&str> {
        Some(">")
    }

    fn global(&self) -> bool {
        false
    }

    fn query(&self, input: &str) -> Vec<ResultItem> {
        let typed = clean(input);
        let needle = typed.to_lowercase();
        let mut items = Vec::new();

        // Every row is at or above `score::KEYWORD`, so the engine keeps this
        // order: what was typed, then recents by recency, then "open terminal".
        if !typed.is_empty() {
            items.push(self.row(
                &typed,
                score::KEYWORD + 1000.0,
                "Runs only when you press Enter",
            ));
        }
        let recent = self.recent();
        for (rank, command) in recent
            .iter()
            .filter(|c| **c != typed && c.to_lowercase().starts_with(&needle))
            .enumerate()
        {
            items.push(self.row(
                command,
                score::KEYWORD + 500.0 - rank as f64,
                "Recent command",
            ));
        }
        if typed.is_empty() {
            items.push(self.open_terminal_row());
        }
        items
    }

    /// `shell:<command>` runs that command, `shell:` opens a terminal: the key
    /// is the command, so any command can be bound to a hotkey.
    fn resolve(&self, id: &str) -> Option<ResultItem> {
        let command = clean(id.strip_prefix("shell:")?);
        Some(if command.is_empty() {
            self.open_terminal_row()
        } else {
            self.row(&command, score::KEYWORD, "Runs only when you press Enter")
        })
    }

    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        let Action::Custom { payload } = &item.action else {
            return Err(PluginError::Unsupported(item.id.clone()));
        };
        let command = clean(payload);
        self.platform
            .run_in_terminal(&command, &self.config)
            .map_err(PluginError::other)?;
        self.remember(&command);
        Ok(())
    }

    fn restore_history(&self, keys: &[String]) {
        let mut recent = self.recent();
        // Anything run in this process already is newer than the stored history.
        let known = recent.clone();
        for key in keys {
            if !key.is_empty() && !known.contains(key) {
                recent.push(key.clone());
            }
        }
        recent.truncate(MAX_RECENT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::MockPlatform;

    fn plugin() -> (ShellPlugin, Arc<MockPlatform>) {
        let platform = MockPlatform::empty();
        let plugin = ShellPlugin::new(ShellConfig::default(), platform.clone());
        (plugin, platform)
    }

    fn titles(items: &[ResultItem]) -> Vec<&str> {
        items.iter().map(|i| i.title.as_str()).collect()
    }

    #[test]
    fn metadata() {
        let (plugin, _) = plugin();
        assert_eq!(plugin.id(), "shell");
        assert_eq!(plugin.keyword(), Some(">"));
        assert!(!plugin.global());
    }

    #[test]
    fn typed_command_becomes_a_run_row() {
        let (plugin, platform) = plugin();
        let items = plugin.query("  git status  ");
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item.id, "shell:git status");
        assert_eq!(item.title, "Run `git status` in terminal");
        assert!(item.score >= score::KEYWORD);
        assert_eq!(
            item.action,
            Action::Custom {
                payload: "git status".into()
            }
        );
        // Querying never runs anything.
        assert!(platform.terminal_runs.lock().unwrap().is_empty());
    }

    #[test]
    fn empty_input_without_history_offers_to_open_a_terminal() {
        let (plugin, platform) = plugin();
        let items = plugin.query("");
        assert_eq!(titles(&items), vec!["Open terminal"]);
        plugin.execute(&items[0]).unwrap();
        let runs = platform.terminal_runs.lock().unwrap();
        assert_eq!(runs[0].0, "");
    }

    #[test]
    fn execute_runs_the_command_with_the_configured_shell_settings() {
        let platform = MockPlatform::empty();
        let config = ShellConfig {
            terminal: "kitty".into(),
            shell: "zsh".into(),
            keep_open: false,
        };
        let plugin = ShellPlugin::new(config.clone(), platform.clone());
        let item = plugin.query("make -j8").remove(0);
        plugin.execute(&item).unwrap();
        assert_eq!(
            *platform.terminal_runs.lock().unwrap(),
            vec![("make -j8".to_owned(), config)]
        );
    }

    #[test]
    fn executed_commands_are_offered_again_most_recent_first() {
        let (plugin, _) = plugin();
        for command in ["ls", "git status", "git log", "ls"] {
            plugin.execute(&plugin.query(command).remove(0)).unwrap();
        }
        assert_eq!(
            titles(&plugin.query("")),
            vec![
                "Run `ls` in terminal",
                "Run `git log` in terminal",
                "Run `git status` in terminal",
                "Open terminal",
            ]
        );
    }

    #[test]
    fn recents_match_the_typed_prefix_case_insensitively() {
        let (plugin, _) = plugin();
        plugin.restore_history(&["git status".into(), "Get-Process".into(), "ls".into()]);
        assert_eq!(
            titles(&plugin.query("g")),
            vec![
                "Run `g` in terminal",
                "Run `git status` in terminal",
                "Run `Get-Process` in terminal"
            ]
        );
        assert_eq!(
            titles(&plugin.query("GIT")),
            vec!["Run `GIT` in terminal", "Run `git status` in terminal"]
        );
        assert_eq!(titles(&plugin.query("zzz")), vec!["Run `zzz` in terminal"]);
    }

    #[test]
    fn typing_a_recent_command_exactly_does_not_duplicate_it() {
        let (plugin, _) = plugin();
        plugin.restore_history(&["git status".into()]);
        let items = plugin.query("git status");
        assert_eq!(items.len(), 1);
        assert!(items[0].score >= score::KEYWORD + 1000.0);
    }

    #[test]
    fn rows_are_ordered_by_score() {
        let (plugin, _) = plugin();
        plugin.restore_history(&["ls -a".into(), "ls -l".into()]);
        let items = plugin.query("ls");
        let by_score = {
            let mut sorted = items.clone();
            sorted.sort_by(|a, b| b.score.total_cmp(&a.score));
            sorted
        };
        assert_eq!(items, by_score);
        assert_eq!(
            titles(&items),
            vec![
                "Run `ls` in terminal",
                "Run `ls -a` in terminal",
                "Run `ls -l` in terminal"
            ]
        );
    }

    #[test]
    fn restored_history_does_not_override_commands_run_since_startup() {
        let (plugin, _) = plugin();
        plugin.execute(&plugin.query("new").remove(0)).unwrap();
        plugin.restore_history(&["old".into(), "new".into(), String::new()]);
        assert_eq!(*plugin.recent(), vec!["new", "old"]);
    }

    #[test]
    fn history_is_capped() {
        let (plugin, _) = plugin();
        let keys: Vec<String> = (0..200).map(|i| format!("cmd {i}")).collect();
        plugin.restore_history(&keys);
        assert_eq!(plugin.recent().len(), MAX_RECENT);
        assert_eq!(plugin.recent()[0], "cmd 0");
    }

    #[test]
    fn newlines_and_control_characters_become_spaces() {
        let (plugin, _) = plugin();
        let items = plugin.query("echo a\r\necho b\u{7}");
        assert_eq!(items[0].id, "shell:echo a  echo b");
        assert_eq!(clean("\n\t x \n"), "x");
    }

    #[test]
    fn resolve_rebuilds_a_command_from_its_id() {
        let (plugin, platform) = plugin();
        let item = plugin.resolve("shell:git status").unwrap();
        assert_eq!(item.id, "shell:git status");
        plugin.execute(&item).unwrap();
        assert_eq!(platform.terminal_runs.lock().unwrap()[0].0, "git status");

        let open = plugin.resolve("shell:").unwrap();
        assert_eq!(open.title, "Open terminal");
        assert!(plugin.resolve("apps:git").is_none());
    }

    #[test]
    fn foreign_actions_are_unsupported() {
        let (plugin, platform) = plugin();
        let item = ResultItem::new("shell", "x", "x", Action::CopyText { text: "x".into() });
        assert!(matches!(
            plugin.execute(&item),
            Err(PluginError::Unsupported(_))
        ));
        assert!(platform.terminal_runs.lock().unwrap().is_empty());
    }
}
