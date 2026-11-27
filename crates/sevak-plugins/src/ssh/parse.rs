//! Reading `~/.ssh/config` and `~/.ssh/known_hosts`.
//!
//! Only what a launcher row needs is kept: each concrete `Host` alias with its
//! `HostName`, `User` and `Port`. `Host` patterns made only of wildcards
//! (`Host *`, `Host *.example.com`) name no host to connect to and are
//! skipped; `Match` blocks too. `Include` is followed (globs, `~`, paths
//! relative to `~/.ssh`), up to [`MAX_INCLUDE_DEPTH`] levels like ssh itself.
//!
//! # What is never read
//!
//! - Anything outside `~/.ssh` (after resolving symbolic links): an
//!   `Include /etc/passwd` or a link to elsewhere is skipped.
//! - Private keys and the other files ssh keeps there: `id_*`, `*.pem`,
//!   `*.key`, `*.ppk`, `authorized_keys*`, and `known_hosts*` as config.
//! - Hashed `known_hosts` entries (`|1|...`): they cannot be turned back into
//!   names, by design.

use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::files::expand_home;
use crate::local_source::{read_confined_text, wildcard_match};

/// Include levels followed (ssh allows 16).
pub const MAX_INCLUDE_DEPTH: usize = 16;
/// Config files read in total, includes counted.
const MAX_FILES: usize = 256;
/// A config file larger than this is not read.
pub const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
/// `known_hosts` larger than this is not read.
const MAX_KNOWN_HOSTS_BYTES: u64 = 8 * 1024 * 1024;
/// Hosts kept from all sources together.
pub const MAX_HOSTS: usize = 10_000;

/// Where a host came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Config,
    KnownHosts,
}

/// One host to connect to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshHost {
    /// What `ssh` is given: the `Host` alias, or the `known_hosts` name.
    pub alias: String,
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub source: Source,
}

impl SshHost {
    fn new(alias: &str, source: Source) -> Self {
        Self {
            alias: alias.to_owned(),
            hostname: None,
            user: None,
            port: None,
            source,
        }
    }

    /// `user@hostname:port`, leaving out what is not set.
    pub fn target(&self) -> String {
        let mut text = String::new();
        if let Some(user) = &self.user {
            text.push_str(user);
            text.push('@');
        }
        text.push_str(self.hostname.as_deref().unwrap_or(&self.alias));
        if let Some(port) = self.port {
            text.push_str(&format!(":{port}"));
        }
        text
    }
}

/// Whether `name` can be handed to `ssh` as one word that no shell reads
/// anything into: letters, digits, `.`, `_` and `-`, not starting with `-`.
pub fn is_safe_host(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 253
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Whether a file name in `~/.ssh` is one ssh keeps for something other than
/// configuration (a key, the known hosts), which an `Include` glob must not
/// pull in.
fn is_reserved_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return true;
    };
    let lower = name.to_ascii_lowercase();
    lower.starts_with("id_")
        || lower.starts_with("authorized_keys")
        || lower.starts_with("known_hosts")
        || [".pem", ".key", ".ppk", ".pub", ".p12", ".pfx"]
            .iter()
            .any(|ext| lower.ends_with(ext))
}

/// The hosts in `<ssh_dir>/config` and what it includes.
pub fn read_config(ssh_dir: &Path, home: Option<&Path>) -> Vec<SshHost> {
    let mut reader = Reader {
        ssh_dir,
        home,
        hosts: Vec::new(),
        current: Vec::new(),
        files_read: 0,
        open: HashSet::new(),
    };
    reader.read_file(&ssh_dir.join("config"), 0);
    reader.hosts
}

struct Reader<'a> {
    ssh_dir: &'a Path,
    home: Option<&'a Path>,
    hosts: Vec<SshHost>,
    /// Indexes of the hosts the current `Host` block applies to.
    current: Vec<usize>,
    files_read: usize,
    /// Files being read right now, so an include loop ends.
    open: HashSet<PathBuf>,
}

impl Reader<'_> {
    fn read_file(&mut self, path: &Path, depth: usize) {
        if depth > MAX_INCLUDE_DEPTH || self.files_read >= MAX_FILES || is_reserved_file(path) {
            return;
        }
        let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if self.open.contains(&key) {
            return;
        }
        let Some(text) = read_confined_text(self.ssh_dir, path, MAX_CONFIG_BYTES) else {
            return;
        };
        self.files_read += 1;
        self.open.insert(key.clone());
        for line in text.lines() {
            self.line(line, depth);
        }
        self.open.remove(&key);
    }

    fn line(&mut self, line: &str, depth: usize) {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return;
        }
        let (keyword, rest) = split_keyword(line);
        let args = split_args(rest);
        match keyword.to_ascii_lowercase().as_str() {
            "host" => {
                self.current.clear();
                for pattern in &args {
                    if pattern.starts_with('!')
                        || pattern.contains(['*', '?'])
                        || !is_safe_host(pattern)
                    {
                        continue;
                    }
                    let index = match self.hosts.iter().position(|h| h.alias == *pattern) {
                        Some(index) => index,
                        None if self.hosts.len() < MAX_HOSTS => {
                            self.hosts.push(SshHost::new(pattern, Source::Config));
                            self.hosts.len() - 1
                        }
                        None => continue,
                    };
                    if !self.current.contains(&index) {
                        self.current.push(index);
                    }
                }
            }
            // Conditional blocks name no host.
            "match" => self.current.clear(),
            "include" => {
                // What an included file's `Host` lines change ends with it.
                let saved = self.current.clone();
                for arg in &args {
                    for path in self.include_paths(arg) {
                        self.read_file(&path, depth + 1);
                    }
                }
                self.current = saved;
            }
            "hostname" => {
                if let Some(value) = args.first() {
                    self.set(|host| &mut host.hostname, value);
                }
            }
            "user" => {
                if let Some(value) = args.first() {
                    self.set(|host| &mut host.user, value);
                }
            }
            "port" => {
                if let Some(port) = args.first().and_then(|p| p.parse::<u16>().ok()) {
                    for &index in &self.current {
                        // ssh uses the first value it finds for a host.
                        self.hosts[index].port.get_or_insert(port);
                    }
                }
            }
            _ => {}
        }
    }

    /// Sets a field on every host of the current block that has none yet.
    fn set(&mut self, field: fn(&mut SshHost) -> &mut Option<String>, value: &str) {
        if value.chars().any(char::is_control) {
            return;
        }
        for &index in &self.current {
            field(&mut self.hosts[index]).get_or_insert_with(|| value.to_owned());
        }
    }

    /// The files an `Include` argument names: `~` expanded, relative to
    /// `~/.ssh`, and `*` / `?` expanded in any component.
    fn include_paths(&self, arg: &str) -> Vec<PathBuf> {
        let expanded = expand_home(arg, self.home);
        let path = if expanded.is_absolute() {
            expanded
        } else {
            self.ssh_dir.join(expanded)
        };
        if !path.to_string_lossy().contains(['*', '?']) {
            return vec![path];
        }
        glob(&path)
    }
}

/// The keyword of a config line and the rest; `Key=value` and `Key value`
/// are both allowed.
fn split_keyword(line: &str) -> (&str, &str) {
    let end = line
        .find(|c: char| c.is_whitespace() || c == '=')
        .unwrap_or(line.len());
    let (keyword, rest) = line.split_at(end);
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('=').unwrap_or(rest).trim_start();
    (keyword, rest)
}

/// Whitespace-separated arguments; `"double quotes"` group words.
fn split_args(rest: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in rest.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            // A comment after the arguments.
            '#' if !quoted && current.is_empty() => break,
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

/// Expands `*` and `?` in the components of `pattern`, sorted like ssh's
/// `glob(3)`. Hidden files only match a pattern that starts with `.`.
fn glob(pattern: &Path) -> Vec<PathBuf> {
    const MAX_MATCHES: usize = 256;
    let mut found = vec![PathBuf::new()];
    for component in pattern.components() {
        let name = match component {
            Component::Normal(name) => name.to_string_lossy(),
            other => {
                for path in &mut found {
                    path.push(other.as_os_str());
                }
                continue;
            }
        };
        if !name.contains(['*', '?']) {
            for path in &mut found {
                path.push(name.as_ref());
            }
            continue;
        }
        let mut next = Vec::new();
        for base in &found {
            let Ok(entries) = fs::read_dir(base) else {
                continue;
            };
            let mut names: Vec<String> = entries
                .filter_map(Result::ok)
                .filter_map(|entry| entry.file_name().into_string().ok())
                .filter(|entry| !entry.starts_with('.') || name.starts_with('.'))
                .filter(|entry| wildcard_match(&name, entry))
                .collect();
            names.sort();
            next.extend(names.into_iter().map(|entry| base.join(entry)));
            if next.len() >= MAX_MATCHES {
                break;
            }
        }
        next.truncate(MAX_MATCHES);
        found = next;
    }
    found
}

/// The named hosts of `<ssh_dir>/known_hosts`: `[name]:port` keeps its port;
/// hashed entries, `@cert-authority` / `@revoked` lines, wildcards and IPv6
/// addresses are skipped.
pub fn read_known_hosts(ssh_dir: &Path) -> Vec<SshHost> {
    let path = ssh_dir.join("known_hosts");
    let Some(text) = read_confined_text(ssh_dir, &path, MAX_KNOWN_HOSTS_BYTES) else {
        return Vec::new();
    };
    parse_known_hosts(&text)
}

fn parse_known_hosts(text: &str) -> Vec<SshHost> {
    let mut hosts: Vec<SshHost> = Vec::new();
    let mut seen = HashSet::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(['#', '|', '@']) {
            continue;
        }
        let Some(names) = line.split_whitespace().next() else {
            continue;
        };
        for name in names.split(',') {
            let (host, port) = match name.strip_prefix('[') {
                Some(rest) => match rest.split_once("]:") {
                    Some((host, port)) => match port.parse::<u16>() {
                        Ok(port) => (host, Some(port)),
                        Err(_) => continue,
                    },
                    None => continue,
                },
                None => (name, None),
            };
            if !is_safe_host(host) || !seen.insert((host.to_owned(), port)) {
                continue;
            }
            let mut entry = SshHost::new(host, Source::KnownHosts);
            entry.port = port.filter(|&p| p != 22);
            hosts.push(entry);
            if hosts.len() >= MAX_HOSTS {
                return hosts;
            }
        }
    }
    hosts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ssh_dir() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let ssh = dir.path().join(".ssh");
        fs::create_dir(&ssh).unwrap();
        (dir, ssh)
    }

    fn aliases(hosts: &[SshHost]) -> Vec<&str> {
        hosts.iter().map(|h| h.alias.as_str()).collect()
    }

    #[test]
    fn reads_hosts_with_their_details_and_skips_patterns() {
        let (_dir, ssh) = ssh_dir();
        fs::write(
            ssh.join("config"),
            "# my hosts\n\
             Host *\n  ServerAliveInterval 30\n  User everyone\n\
             Host web web-alias !bad *.corp db?\n\
             \tHostName web.example.com   # the box\n\
             \tUser deploy\n\
             \tPort 2222\n\
             Host=build\n  HostName=10.0.0.5\n\
             Match host foo\n  User nobody\n\
             host web\n  User later\n  Port 1\n\
             Host \"quoted\" -evil 'x\n",
        )
        .unwrap();
        let hosts = read_config(&ssh, None);
        assert_eq!(aliases(&hosts), ["web", "web-alias", "build", "quoted"]);
        let web = &hosts[0];
        assert_eq!(web.hostname.as_deref(), Some("web.example.com"));
        // The first value wins, as in ssh.
        assert_eq!(web.user.as_deref(), Some("deploy"));
        assert_eq!(web.port, Some(2222));
        assert_eq!(web.target(), "deploy@web.example.com:2222");
        assert_eq!(hosts[1].hostname.as_deref(), Some("web.example.com"));
        assert_eq!(hosts[2].hostname.as_deref(), Some("10.0.0.5"));
        assert_eq!(hosts[2].target(), "10.0.0.5");
        assert_eq!(hosts[3].target(), "quoted");
    }

    #[test]
    fn follows_includes_with_globs_and_relative_paths() {
        let (dir, ssh) = ssh_dir();
        fs::create_dir(ssh.join("config.d")).unwrap();
        fs::write(
            ssh.join("config"),
            "Include config.d/*.conf ~/.ssh/extra\nHost main\n  User me\n",
        )
        .unwrap();
        fs::write(ssh.join("config.d").join("b.conf"), "Host bravo\n").unwrap();
        fs::write(
            ssh.join("config.d").join("a.conf"),
            "Host alpha\n  HostName a.example\n",
        )
        .unwrap();
        fs::write(ssh.join("config.d").join("skip.txt"), "Host nope\n").unwrap();
        fs::write(ssh.join("config.d").join(".hidden.conf"), "Host hidden\n").unwrap();
        fs::write(ssh.join("extra"), "Host extra\n").unwrap();
        let hosts = read_config(&ssh, Some(dir.path()));
        assert_eq!(aliases(&hosts), ["alpha", "bravo", "extra", "main"]);
        assert_eq!(hosts[3].user.as_deref(), Some("me"));
    }

    #[test]
    fn an_include_inside_a_host_block_does_not_leak_its_own_blocks() {
        let (_dir, ssh) = ssh_dir();
        fs::write(
            ssh.join("config"),
            "Host one\n  Include inner\n  User outer\n",
        )
        .unwrap();
        fs::write(ssh.join("inner"), "Host two\n  HostName two.example\n").unwrap();
        let hosts = read_config(&ssh, None);
        assert_eq!(aliases(&hosts), ["one", "two"]);
        assert_eq!(hosts[0].user.as_deref(), Some("outer"));
        assert_eq!(hosts[1].user, None);
    }

    #[test]
    fn include_loops_end_and_keys_and_outside_files_are_never_read() {
        let (dir, ssh) = ssh_dir();
        fs::write(ssh.join("config"), "Include config\nInclude *\nHost a\n").unwrap();
        // Would be read by `Include *` if keys were not refused.
        fs::write(ssh.join("id_ed25519"), "Host from-a-key\n").unwrap();
        fs::write(ssh.join("id_ed25519.pub"), "Host from-a-pub\n").unwrap();
        fs::write(ssh.join("server.pem"), "Host from-a-pem\n").unwrap();
        fs::write(ssh.join("known_hosts"), "Host from-known\n").unwrap();
        fs::write(ssh.join("authorized_keys"), "Host from-auth\n").unwrap();
        fs::write(ssh.join("work"), "Host work\n").unwrap();
        fs::write(dir.path().join("outside"), "Host outside\n").unwrap();
        fs::write(
            ssh.join("more"),
            format!("Include {}\n", dir.path().join("outside").display()),
        )
        .unwrap();
        let hosts = read_config(&ssh, None);
        assert_eq!(aliases(&hosts), ["work", "a"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_config_that_links_out_of_ssh_is_not_read() {
        let (dir, ssh) = ssh_dir();
        fs::write(dir.path().join("elsewhere"), "Host linked\n").unwrap();
        std::os::unix::fs::symlink(dir.path().join("elsewhere"), ssh.join("config")).unwrap();
        assert!(read_config(&ssh, None).is_empty());
    }

    #[test]
    fn a_missing_config_has_no_hosts() {
        let (_dir, ssh) = ssh_dir();
        assert!(read_config(&ssh, None).is_empty());
        assert!(read_known_hosts(&ssh).is_empty());
    }

    #[test]
    fn known_hosts_names_ports_and_skipped_entries() {
        let hosts = parse_known_hosts(
            "# comment\n\
             github.com,140.82.121.4 ssh-ed25519 AAAA\n\
             |1|hashedsalt=|hashedname= ssh-rsa AAAA\n\
             [git.example.org]:2222 ssh-rsa AAAA\n\
             [plain.example]:22 ssh-rsa AAAA\n\
             @cert-authority *.corp ssh-rsa AAAA\n\
             *.wild ssh-rsa AAAA\n\
             [::1]:22 ssh-rsa AAAA\n\
             github.com ssh-rsa AAAA\n",
        );
        let summary: Vec<_> = hosts.iter().map(|h| (h.alias.as_str(), h.port)).collect();
        assert_eq!(
            summary,
            [
                ("github.com", None),
                ("140.82.121.4", None),
                ("git.example.org", Some(2222)),
                ("plain.example", None),
            ]
        );
        assert!(hosts.iter().all(|h| h.source == Source::KnownHosts));
    }

    #[test]
    fn only_plain_host_names_are_safe() {
        for good in ["web", "db-1.example.com", "a_b", "10.0.0.1"] {
            assert!(is_safe_host(good), "{good}");
        }
        for bad in [
            "",
            "-oProxyCommand=x",
            "a b",
            "a;b",
            "a$(x)",
            "a|b",
            "%x%",
            "'",
        ] {
            assert!(!is_safe_host(bad), "{bad}");
        }
    }

    #[test]
    fn keyword_and_argument_splitting() {
        assert_eq!(split_keyword("Host a b"), ("Host", "a b"));
        assert_eq!(split_keyword("Port=22"), ("Port", "22"));
        assert_eq!(split_keyword("Port = 22"), ("Port", "22"));
        assert_eq!(
            split_args("\"a b\" c  # trailing"),
            ["a b".to_owned(), "c".to_owned()]
        );
    }
}
