//! SSH hosts: `ssh <name>` lists the hosts in `~/.ssh/config`.
//!
//! | Key | Action |
//! |---|---|
//! | Enter | open a terminal running `ssh <host>` (the `[shell]` terminal) |
//! | `Alt+Enter` | copy `ssh <host>` |
//! | `Ctrl+Enter` | copy the host name |
//!
//! # Where the hosts come from
//!
//! `~/.ssh/config` and the files it `Include`s, and, when `[ssh] known_hosts`
//! is on, `~/.ssh/known_hosts`. See [`parse`] for what is (never) read.
//!
//! # Privacy
//!
//! The files are read on a background thread by [`Plugin::refresh`] and the
//! hosts kept in memory only. The log has counts, never names. Nothing is sent
//! anywhere; `ssh` only runs when you press Enter.

pub mod parse;

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use sevak_core::config::SshConfig;
use sevak_core::model::score;
use sevak_core::{
    Action, FuzzyQuery, IconSource, Modifier, Plugin, PluginError, PluginResult, ResultItem,
    ShellConfig,
};
use sevak_platform::PlatformProvider;

use crate::actions::execute_action;
use crate::files::home_dir;
use parse::{is_safe_host, read_config, read_known_hosts, Source, SshHost, MAX_HOSTS};

const ID: &str = "ssh";
/// Rows returned for one query.
const MAX_ROWS: usize = 30;
/// A host whose alias starts with the input beats a looser match.
const PREFIX_BONUS: f64 = 60.0;
/// Weight of a match on the host name (not the alias).
const HOSTNAME_WEIGHT: f64 = 0.7;
const PAYLOAD_NOTHING: &str = "nothing";
const PAYLOAD_CONNECT: &str = "connect:";

#[derive(Default)]
struct Snapshot {
    hosts: Vec<SshHost>,
    loaded: bool,
}

/// The `ssh` plugin.
pub struct SshPlugin {
    config: SshConfig,
    shell: ShellConfig,
    platform: Arc<dyn PlatformProvider>,
    /// `~/.ssh`; set by tests.
    ssh_dir: Option<PathBuf>,
    home: Option<PathBuf>,
    snapshot: RwLock<Arc<Snapshot>>,
}

impl SshPlugin {
    pub fn new(
        config: &SshConfig,
        shell: &ShellConfig,
        platform: Arc<dyn PlatformProvider>,
    ) -> Self {
        let home = home_dir();
        Self::with_dirs(
            config,
            shell,
            platform,
            home.as_ref().map(|h| h.join(".ssh")),
            home,
        )
    }

    /// Reads `ssh_dir` instead of `~/.ssh`.
    pub fn with_dirs(
        config: &SshConfig,
        shell: &ShellConfig,
        platform: Arc<dyn PlatformProvider>,
        ssh_dir: Option<PathBuf>,
        home: Option<PathBuf>,
    ) -> Self {
        Self {
            config: config.clone(),
            shell: shell.clone(),
            platform,
            ssh_dir,
            home,
            snapshot: RwLock::new(Arc::new(Snapshot::default())),
        }
    }

    fn snapshot(&self) -> Arc<Snapshot> {
        self.snapshot
            .read()
            .map(|s| Arc::clone(&s))
            .unwrap_or_default()
    }

    fn load(&self) -> Vec<SshHost> {
        let Some(dir) = &self.ssh_dir else {
            return Vec::new();
        };
        let mut hosts = read_config(dir, self.home.as_deref());
        if self.config.known_hosts {
            for host in read_known_hosts(dir) {
                let known = hosts.iter().any(|h| {
                    h.alias.eq_ignore_ascii_case(&host.alias)
                        || h.hostname
                            .as_deref()
                            .is_some_and(|n| n.eq_ignore_ascii_case(&host.alias))
                });
                if !known && hosts.len() < MAX_HOSTS {
                    hosts.push(host);
                }
            }
        }
        hosts
    }

    fn status(&self, key: &str, title: &str, subtitle: &str) -> ResultItem {
        ResultItem::new(
            ID,
            key,
            title,
            Action::Custom {
                payload: PAYLOAD_NOTHING.to_owned(),
            },
        )
        .with_subtitle(subtitle)
        .with_icon(IconSource::builtin("terminal"))
        .with_score(score::KEYWORD)
    }

    fn row(&self, host: &SshHost) -> ResultItem {
        let key = connect_key(&host.alias, host.port, host.source);
        let command = ssh_command(&key).unwrap_or_default();
        let origin = match host.source {
            Source::Config => "",
            Source::KnownHosts => " · from known_hosts",
        };
        ResultItem::new(
            ID,
            &key,
            &host.alias,
            Action::Custom {
                payload: format!("{PAYLOAD_CONNECT}{key}"),
            },
        )
        .with_subtitle(format!(
            "{}{origin} · Enter connects in a terminal",
            host.target()
        ))
        .with_icon(IconSource::builtin("terminal"))
        .with_secondary(
            format!("Copy `{command}`"),
            Some(Modifier::Alt),
            Action::CopyText {
                text: command.clone(),
            },
        )
        .with_secondary(
            "Copy host name",
            Some(Modifier::Ctrl),
            Action::CopyText {
                text: host.hostname.clone().unwrap_or_else(|| host.alias.clone()),
            },
        )
        .with_large_text(command)
    }

    fn matches(&self, hosts: &[SshHost], input: &str) -> Vec<ResultItem> {
        if input.is_empty() {
            return hosts
                .iter()
                .take(MAX_ROWS)
                .enumerate()
                .map(|(i, host)| self.row(host).with_score(score::KEYWORD - i as f64))
                .collect();
        }
        let lower = input.to_lowercase();
        let mut query = FuzzyQuery::new(input);
        let mut scored: Vec<(f64, usize, &SshHost)> = hosts
            .iter()
            .enumerate()
            .filter_map(|(i, host)| {
                let alias = query.score(&host.alias).map(|s| {
                    let bonus = if host.alias.to_lowercase().starts_with(&lower) {
                        PREFIX_BONUS
                    } else {
                        0.0
                    };
                    f64::from(s) + bonus
                });
                let name = host
                    .hostname
                    .as_deref()
                    .and_then(|n| query.score(n))
                    .map(|s| f64::from(s) * HOSTNAME_WEIGHT);
                let best = match (alias, name) {
                    (Some(a), Some(n)) => Some(a.max(n)),
                    (a, n) => a.or(n),
                }?;
                Some((best, i, host))
            })
            .collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        scored
            .into_iter()
            .take(MAX_ROWS)
            .map(|(s, _, host)| self.row(host).with_score(score::KEYWORD + s))
            .collect()
    }
}

/// The result key of a host: the alias, with `:port` for a `known_hosts`
/// entry on another port (a config alias carries its own port).
fn connect_key(alias: &str, port: Option<u16>, source: Source) -> String {
    match (source, port) {
        (Source::KnownHosts, Some(port)) => format!("{alias}:{port}"),
        _ => alias.to_owned(),
    }
}

/// `ssh <host>` or `ssh -p <port> <host>` for a key, if the host is a plain
/// name that no shell reads anything into.
fn ssh_command(key: &str) -> Option<String> {
    let (host, port) = match key.rsplit_once(':') {
        Some((host, port)) => (host, Some(port.parse::<u16>().ok()?)),
        None => (key, None),
    };
    if !is_safe_host(host) {
        return None;
    }
    Some(match port {
        Some(port) => format!("ssh -p {port} {host}"),
        None => format!("ssh {host}"),
    })
}

impl Plugin for SshPlugin {
    fn id(&self) -> &str {
        ID
    }

    fn name(&self) -> &str {
        "SSH hosts"
    }

    fn description(&self) -> &str {
        "Type `ssh` to list the hosts in ~/.ssh/config; Enter connects in a terminal. Private keys are never read."
    }

    fn keyword(&self) -> Option<&str> {
        Some(&self.config.keyword)
    }

    fn global(&self) -> bool {
        false
    }

    fn query(&self, input: &str) -> Vec<ResultItem> {
        let snapshot = self.snapshot();
        let input = input.trim();
        if !snapshot.loaded {
            return vec![self.status("loading", "Reading ~/.ssh/config…", "Try again in a moment")];
        }
        if snapshot.hosts.is_empty() {
            return vec![self.status(
                "empty",
                "No SSH hosts found",
                "Add Host entries to ~/.ssh/config, then choose Reload index",
            )];
        }
        self.matches(&snapshot.hosts, input)
    }

    fn resolve(&self, id: &str) -> Option<ResultItem> {
        let key = id.strip_prefix("ssh:")?;
        let snapshot = self.snapshot();
        let found = snapshot
            .hosts
            .iter()
            .find(|host| connect_key(&host.alias, host.port, host.source) == key);
        match found {
            Some(host) => Some(self.row(host).with_score(score::KEYWORD)),
            None => {
                ssh_command(key)?;
                let (alias, port) = match key.rsplit_once(':') {
                    Some((alias, port)) => (alias, port.parse().ok()),
                    None => (key, None),
                };
                let mut host = SshHost {
                    alias: alias.to_owned(),
                    hostname: None,
                    user: None,
                    port,
                    source: Source::KnownHosts,
                };
                if port.is_none() {
                    host.source = Source::Config;
                }
                Some(self.row(&host).with_score(score::KEYWORD))
            }
        }
    }

    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        let Action::Custom { payload } = &item.action else {
            return execute_action(self.platform.as_ref(), &item.action);
        };
        if payload == PAYLOAD_NOTHING {
            return Ok(());
        }
        let Some(key) = payload.strip_prefix(PAYLOAD_CONNECT) else {
            return Err(PluginError::Unsupported(item.id.clone()));
        };
        let command = ssh_command(key).ok_or_else(|| {
            PluginError::Message("that host name cannot be passed to ssh safely".into())
        })?;
        self.platform
            .run_in_terminal(&command, &self.shell)
            .map_err(PluginError::other)
    }

    fn refresh(&self) -> PluginResult<()> {
        let hosts = self.load();
        tracing::info!(hosts = hosts.len(), "loaded ssh hosts");
        if let Ok(mut slot) = self.snapshot.write() {
            *slot = Arc::new(Snapshot {
                hosts,
                loaded: true,
            });
        }
        Ok(())
    }

    fn index_size(&self) -> Option<usize> {
        Some(self.snapshot().hosts.len())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::test_util::MockPlatform;

    fn plugin_with(
        config: &str,
        known: Option<&str>,
        known_hosts: bool,
    ) -> (SshPlugin, Arc<MockPlatform>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let ssh = dir.path().join(".ssh");
        fs::create_dir(&ssh).unwrap();
        fs::write(ssh.join("config"), config).unwrap();
        if let Some(known) = known {
            fs::write(ssh.join("known_hosts"), known).unwrap();
        }
        let platform = MockPlatform::empty();
        let plugin = SshPlugin::with_dirs(
            &SshConfig {
                known_hosts,
                ..SshConfig::default()
            },
            &ShellConfig::default(),
            platform.clone(),
            Some(ssh),
            Some(dir.path().to_path_buf()),
        );
        plugin.refresh().unwrap();
        (plugin, platform, dir)
    }

    const CONFIG: &str = "Host web\n  HostName web.example.com\n  User deploy\n\
                          Host db\n  HostName 10.1.2.3\n  Port 2200\nHost *\n  User x\n";

    fn titles(rows: &[ResultItem]) -> Vec<&str> {
        rows.iter().map(|r| r.title.as_str()).collect()
    }

    #[test]
    fn metadata() {
        let (plugin, _, _dir) = plugin_with(CONFIG, None, false);
        assert_eq!(plugin.id(), "ssh");
        assert_eq!(plugin.keyword(), Some("ssh"));
        assert!(!plugin.global());
        assert_eq!(plugin.index_size(), Some(2));
    }

    #[test]
    fn lists_hosts_in_config_order_and_finds_them_by_alias_or_name() {
        let (plugin, _, _dir) = plugin_with(CONFIG, None, false);
        assert_eq!(titles(&plugin.query("")), ["web", "db"]);
        assert_eq!(titles(&plugin.query("db")), ["db"]);
        assert_eq!(plugin.query("10.1")[0].title, "db");
        assert_eq!(plugin.query("example")[0].title, "web");
        assert!(plugin.query("zzzz").is_empty());
        let web = &plugin.query("web")[0];
        assert_eq!(web.id, "ssh:web");
        assert!(web.subtitle.starts_with("deploy@web.example.com · "));
        assert!(web.score >= score::KEYWORD);
    }

    #[test]
    fn enter_runs_ssh_in_the_terminal_and_alt_copies_the_command() {
        let (plugin, platform, _dir) = plugin_with(CONFIG, None, false);
        let db = plugin.query("db").remove(0);
        // Querying never runs anything.
        assert!(platform.terminal_runs.lock().unwrap().is_empty());
        plugin.execute(&db).unwrap();
        assert_eq!(platform.terminal_runs.lock().unwrap()[0].0, "ssh db");

        let alt = db
            .secondary
            .iter()
            .position(|s| s.modifier == Some(Modifier::Alt))
            .unwrap();
        plugin
            .execute(&db.secondary_as_primary(alt).unwrap())
            .unwrap();
        let ctrl = db
            .secondary
            .iter()
            .position(|s| s.modifier == Some(Modifier::Ctrl))
            .unwrap();
        plugin
            .execute(&db.secondary_as_primary(ctrl).unwrap())
            .unwrap();
        assert_eq!(*platform.clipboard.lock().unwrap(), ["ssh db", "10.1.2.3"]);
    }

    #[test]
    fn known_hosts_are_added_when_asked_without_duplicates() {
        let known = "web.example.com ssh-ed25519 AAAA\n[git.example]:2222 ssh-rsa AAAA\n\
                     |1|abc=|def= ssh-rsa AAAA\nother.example ssh-rsa AAAA\n";
        let (off, _, _d1) = plugin_with(CONFIG, Some(known), false);
        assert_eq!(titles(&off.query("")), ["web", "db"]);

        let (plugin, platform, _d2) = plugin_with(CONFIG, Some(known), true);
        assert_eq!(
            titles(&plugin.query("")),
            ["web", "db", "git.example", "other.example"]
        );
        let git = plugin.query("git").remove(0);
        assert_eq!(git.id, "ssh:git.example:2222");
        plugin.execute(&git).unwrap();
        assert_eq!(
            platform.terminal_runs.lock().unwrap()[0].0,
            "ssh -p 2222 git.example"
        );
    }

    #[test]
    fn status_rows_before_loading_and_without_hosts() {
        let platform = MockPlatform::empty();
        let plugin = SshPlugin::with_dirs(
            &SshConfig::default(),
            &ShellConfig::default(),
            platform.clone(),
            None,
            None,
        );
        assert_eq!(plugin.query("")[0].title, "Reading ~/.ssh/config…");
        plugin.refresh().unwrap();
        let rows = plugin.query("x");
        assert_eq!(rows[0].title, "No SSH hosts found");
        plugin.execute(&rows[0]).unwrap();
        assert!(platform.terminal_runs.lock().unwrap().is_empty());
    }

    #[test]
    fn an_unsafe_payload_is_refused() {
        let (plugin, platform, _dir) = plugin_with(CONFIG, None, false);
        for payload in [
            "connect:-oProxyCommand=calc",
            "connect:a;b",
            "connect:a:99999",
            "other",
        ] {
            let item = ResultItem::new(
                "ssh",
                "x",
                "x",
                Action::Custom {
                    payload: payload.into(),
                },
            );
            assert!(plugin.execute(&item).is_err(), "{payload}");
        }
        assert!(platform.terminal_runs.lock().unwrap().is_empty());
    }

    #[test]
    fn resolve_rebuilds_a_host_row_for_a_hotkey() {
        let (plugin, platform, _dir) = plugin_with(CONFIG, None, false);
        let web = plugin.resolve("ssh:web").unwrap();
        assert!(web.subtitle.contains("web.example.com"));
        let other = plugin.resolve("ssh:new-host").unwrap();
        plugin.execute(&other).unwrap();
        assert_eq!(platform.terminal_runs.lock().unwrap()[0].0, "ssh new-host");
        assert!(plugin.resolve("ssh:bad host").is_none());
        assert!(plugin.resolve("apps:x").is_none());
    }
}
