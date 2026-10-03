//! 1Password: `1p <text>` finds logins by title or website.
//!
//! Opt-in (`[onepassword] enabled = true`) and built on the official `op`
//! command-line tool (<https://developer.1password.com/docs/cli>), which must be
//! installed and connected to the desktop app (Settings > Developer > "Integrate
//! with 1Password CLI"), so unlocking is the usual Touch ID / Windows Hello /
//! system-authentication prompt.
//!
//! | Key | Action |
//! |---|---|
//! | Enter | open the login's website (the item in 1Password if it has none) |
//! | `Ctrl+Enter` | open the item in the 1Password app (`onepassword://view-item/`) |
//! | `Shift+Enter` | copy the username, if `op` listed one |
//! | panel | copy the website address |
//!
//! # What is and is not read
//!
//! Only the *list* of logins: titles, vault names, website addresses and the
//! subtitle `op` prints for a login (the username). **No password, one-time
//! code, note or other field is requested, parsed, stored, copied or shown**:
//! Sevak never runs `op item get`, `op read` or `op run`, and [`op::Login`] has
//! no field a secret could occupy. Passwords stay in 1Password; open the item
//! and use its own autofill or copy button.
//!
//! # When `op` runs
//!
//! Never at startup, on a plain (global) query, or on "Reload index". Only when
//! the user types the `1p` keyword with a space, on a background thread, once
//! the in-memory list is older than `[onepassword] cache_minutes` (or missing).
//! Until it arrives the row says so; the launcher re-runs the query when it does.
//! If `op` fails (not signed in, prompt dismissed) it is *not* retried on later
//! keystrokes, so a dismissed prompt does not come back by itself: the row offers
//! "Try again". The list lives in memory only and dies with the process.
//! What is searched for and opened stays out of the usage statistics and search
//! history (`Plugin::tracks_usage`).

pub mod op;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sevak_core::config::OnePasswordConfig;
use sevak_core::model::score;
use sevak_core::plugin::ResultsNotifier;
use sevak_core::{
    Action, FuzzyQuery, IconSource, Modifier, Plugin, PluginError, PluginResult, ResultItem,
};
use sevak_platform::{DeepLink, PlatformProvider};

use self::op::{choose_account, parse_accounts, parse_logins, CliRunner, Login, OpError, OpRunner};
use crate::actions::execute_action;

/// The default keyword; `[onepassword] keyword` changes it.
pub const KEYWORD: &str = "1p";

const ID: &str = "1password";
const MAX_ROWS: usize = 30;
const ENABLE_SNIPPET: &str = "[onepassword]\nenabled = true";
const INSTALL_URL: &str = "https://developer.1password.com/docs/cli/get-started/";
const PAYLOAD_NOTHING: &str = "nothing";
const PAYLOAD_RETRY: &str = "retry";
const PAYLOAD_OPEN: &str = "open:";
/// A match on the website counts a little less than one on the title.
const URL_WEIGHT: f64 = 0.7;
/// Separates account, vault and item in an "open in 1Password" payload; it
/// cannot occur in an id, which is letters and digits.
const PAYLOAD_SEP: char = '|';

/// What the last fetch left behind.
#[derive(Default)]
struct Cache {
    logins: Vec<Login>,
    /// The account the "open in 1Password" links use.
    account_uuid: Option<String>,
    fetched_at: Option<Instant>,
    fetching: bool,
    /// The last attempt failed and nothing was fetched since. Not retried until
    /// the user asks.
    problem: Option<OpError>,
}

struct Shared {
    runner: Arc<dyn OpRunner>,
    account: String,
    ttl: Duration,
    cache: Mutex<Cache>,
    notifier: Mutex<Option<ResultsNotifier>>,
}

impl Shared {
    /// Starts a background fetch if the list is missing or stale and none is
    /// running or waiting for the user. `force` also retries after a failure.
    fn ensure_fresh(self: &Arc<Self>, force: bool) {
        {
            let Ok(mut cache) = self.cache.lock() else {
                return;
            };
            let stale = cache.fetched_at.is_none_or(|at| at.elapsed() >= self.ttl);
            let blocked = cache.problem.is_some() && !force;
            if cache.fetching || !stale || blocked {
                return;
            }
            cache.fetching = true;
            cache.problem = None;
        }
        let shared = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("sevak-1password".into())
            .spawn(move || {
                shared.fetch();
                let notifier = shared.notifier.lock().ok().and_then(|n| n.clone());
                if let Some(notify) = notifier {
                    notify(ID);
                }
            });
        if spawned.is_err() {
            if let Ok(mut cache) = self.cache.lock() {
                cache.fetching = false;
                cache.problem = Some(OpError::Failed("could not start a thread".to_owned()));
            }
        }
    }

    /// Runs `op` and replaces the cache. Blocks (up to `op`'s timeout).
    fn fetch(&self) {
        let account = Some(self.account.as_str()).filter(|a| !a.is_empty());
        let outcome = self
            .runner
            .list_logins(account)
            .and_then(|json| parse_logins(&json).map_err(OpError::Failed));
        // Never fails the list: without an account only the app link is missing.
        let account_uuid = outcome.as_ref().ok().and_then(|_| {
            let accounts = parse_accounts(&self.runner.list_accounts().ok()?).ok()?;
            choose_account(&accounts, &self.account).map(|a| a.uuid.clone())
        });

        let Ok(mut cache) = self.cache.lock() else {
            return;
        };
        cache.fetching = false;
        match outcome {
            Ok(logins) => {
                tracing::info!(logins = logins.len(), "loaded the list of 1Password logins");
                cache.logins = logins;
                cache.account_uuid = account_uuid;
                cache.fetched_at = Some(Instant::now());
                cache.problem = None;
            }
            Err(problem) => {
                // The message is op's own error line, which names no item.
                tracing::warn!(?problem, "could not read the 1Password list");
                cache.problem = Some(problem);
            }
        }
    }
}

/// The `1p` plugin.
pub struct OnePasswordPlugin {
    keyword: String,
    /// `None` while `[onepassword] enabled` is off: `op` is never started.
    shared: Option<Arc<Shared>>,
    platform: Arc<dyn PlatformProvider>,
}

impl OnePasswordPlugin {
    pub fn new(config: &OnePasswordConfig, platform: Arc<dyn PlatformProvider>) -> Self {
        Self::with_runner(config, platform, Arc::new(CliRunner::new(&config.op_path)))
    }

    /// Like [`OnePasswordPlugin::new`] with a different way of running `op`.
    pub fn with_runner(
        config: &OnePasswordConfig,
        platform: Arc<dyn PlatformProvider>,
        runner: Arc<dyn OpRunner>,
    ) -> Self {
        let shared = config.enabled.then(|| {
            Arc::new(Shared {
                runner,
                account: config.account.clone(),
                ttl: Duration::from_secs(u64::from(config.cache_minutes.max(1)) * 60),
                cache: Mutex::new(Cache::default()),
                notifier: Mutex::new(None),
            })
        });
        Self {
            keyword: config.keyword.clone(),
            shared,
            platform,
        }
    }

    fn status(&self, key: &str, title: &str, subtitle: &str, action: Action) -> ResultItem {
        ResultItem::new(ID, key, title, action)
            .with_subtitle(subtitle)
            .with_icon(IconSource::builtin("lock"))
            .with_score(score::KEYWORD)
    }

    fn rows(&self, input: &str) -> Vec<ResultItem> {
        let Some(shared) = &self.shared else {
            return vec![self.status(
                "off",
                "1Password is off",
                "Enter copies the setting to add to config.toml; then choose Reload index",
                Action::CopyText {
                    text: ENABLE_SNIPPET.to_owned(),
                },
            )];
        };
        // The one place `op` can be started: the user typed the keyword.
        shared.ensure_fresh(false);

        let Ok(cache) = shared.cache.lock() else {
            return Vec::new();
        };
        let mut rows = self.matches(&cache, input.trim());
        if !rows.is_empty() {
            return rows;
        }
        if let Some(problem) = &cache.problem {
            rows.push(self.problem_row(problem));
        } else if cache.fetching || cache.fetched_at.is_none() {
            rows.push(self.status(
                "loading",
                "Asking 1Password for your logins…",
                "Approve the 1Password prompt if it appears",
                nothing(),
            ));
        } else if cache.logins.is_empty() {
            rows.push(self.status(
                "empty",
                "No logins found",
                "op listed no items in the Login category",
                nothing(),
            ));
        } else {
            rows.push(self.status(
                "none",
                "No matching login",
                "Titles and website addresses are searched",
                nothing(),
            ));
        }
        rows
    }

    fn problem_row(&self, problem: &OpError) -> ResultItem {
        match problem {
            OpError::NotInstalled => self.status(
                "missing",
                "The 1Password command-line tool (op) was not found",
                "Enter opens the install instructions; or set [onepassword] op_path",
                Action::OpenUrl {
                    url: INSTALL_URL.to_owned(),
                },
            ),
            OpError::NotSignedIn(message) => self.status(
                "signin",
                "op is not signed in to 1Password",
                &format!("{message} · Enter to try again"),
                retry(),
            ),
            OpError::Failed(message) => self.status(
                "failed",
                "op could not list your logins",
                &format!("{message} · Enter to try again"),
                retry(),
            ),
        }
    }

    fn matches(&self, cache: &Cache, input: &str) -> Vec<ResultItem> {
        if input.is_empty() {
            return cache
                .logins
                .iter()
                .take(MAX_ROWS)
                .enumerate()
                .map(|(i, login)| self.row(login, cache).with_score(score::KEYWORD - i as f64))
                .collect();
        }
        let mut query = FuzzyQuery::new(input);
        let mut scored: Vec<(f64, &Login)> = cache
            .logins
            .iter()
            .filter_map(|login| {
                let by_title = query.score(&login.title).map(f64::from);
                let by_url = login
                    .urls
                    .iter()
                    .filter_map(|url| query.score(url_for_search(url)))
                    .max()
                    .map(|s| f64::from(s) * URL_WEIGHT);
                let best = match (by_title, by_url) {
                    (Some(a), Some(b)) => a.max(b),
                    (Some(a), None) | (None, Some(a)) => a,
                    (None, None) => return None,
                };
                Some((best, login))
            })
            .collect();
        scored.sort_by(|a, b| {
            b.0.total_cmp(&a.0)
                .then_with(|| a.1.title.to_lowercase().cmp(&b.1.title.to_lowercase()))
        });
        scored
            .into_iter()
            .take(MAX_ROWS)
            .map(|(score, login)| self.row(login, cache).with_score(score))
            .collect()
    }

    fn row(&self, login: &Login, cache: &Cache) -> ResultItem {
        let website = login.urls.iter().find(|u| is_web_url(u));
        // Only when all three ids make a valid link, so Enter never fails later.
        let app_link = cache
            .account_uuid
            .as_deref()
            .filter(|account| {
                DeepLink::onepassword_item(account, &login.vault_id, &login.id).is_some()
            })
            .map(|account| Action::Custom {
                payload: format!(
                    "{PAYLOAD_OPEN}{account}{PAYLOAD_SEP}{}{PAYLOAD_SEP}{}",
                    login.vault_id, login.id
                ),
            });

        let (primary, hint) = match (website, &app_link) {
            (Some(url), _) => (
                Action::OpenUrl { url: url.clone() },
                "Enter to open the website",
            ),
            (None, Some(open)) => (open.clone(), "Enter to open in 1Password"),
            (None, None) => (nothing(), "No website saved"),
        };
        let mut details: Vec<String> = Vec::new();
        if let Some(url) = website {
            details.push(host_of(url).to_owned());
        }
        if !login.vault_name.is_empty() {
            details.push(login.vault_name.clone());
        }
        details.push(hint.to_owned());

        let mut item = ResultItem::new(ID, &login.id, &login.title, primary)
            .with_subtitle(details.join(" · "))
            .with_icon(IconSource::builtin("lock"));
        if let (Some(open), Some(_)) = (app_link, website) {
            item = item.with_secondary("Open in 1Password", Some(Modifier::Ctrl), open);
        }
        if let Some(username) = &login.username {
            item = item.with_secondary(
                "Copy username",
                Some(Modifier::Shift),
                Action::CopyText {
                    text: username.clone(),
                },
            );
        }
        if let Some(url) = website {
            item = item.with_secondary(
                "Copy website address",
                None,
                Action::CopyText { text: url.clone() },
            );
        }
        item
    }
}

fn nothing() -> Action {
    Action::Custom {
        payload: PAYLOAD_NOTHING.to_owned(),
    }
}

fn retry() -> Action {
    Action::Custom {
        payload: PAYLOAD_RETRY.to_owned(),
    }
}

fn is_web_url(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://")
}

/// `github.com/login` from `https://github.com/login`, for matching and display.
fn url_for_search(url: &str) -> &str {
    url.split_once("://").map_or(url, |(_, rest)| rest)
}

/// The host part of a web address, for the subtitle.
fn host_of(url: &str) -> &str {
    let rest = url_for_search(url);
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = &rest[..end];
    host.rsplit_once('@').map_or(host, |(_, h)| h)
}

impl Plugin for OnePasswordPlugin {
    fn id(&self) -> &str {
        ID
    }

    fn name(&self) -> &str {
        "1Password"
    }

    fn description(&self) -> &str {
        "Type `1p` to open a login's website or its 1Password item (titles only, never passwords). Needs the op tool; off until [onepassword] enabled = true."
    }

    fn keyword(&self) -> Option<&str> {
        Some(&self.keyword)
    }

    fn global(&self) -> bool {
        false
    }

    fn query(&self, input: &str) -> Vec<ResultItem> {
        self.rows(input)
    }

    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        let Action::Custom { payload } = &item.action else {
            return execute_action(self.platform.as_ref(), &item.action);
        };
        if payload == PAYLOAD_NOTHING {
            return Ok(());
        }
        if payload == PAYLOAD_RETRY {
            if let Some(shared) = &self.shared {
                shared.ensure_fresh(true);
            }
            return Ok(());
        }
        if let Some(ids) = payload.strip_prefix(PAYLOAD_OPEN) {
            let mut parts = ids.split(PAYLOAD_SEP);
            let link = match (parts.next(), parts.next(), parts.next(), parts.next()) {
                (Some(account), Some(vault), Some(item), None) => {
                    DeepLink::onepassword_item(account, vault, item)
                }
                _ => None,
            }
            .ok_or_else(|| {
                PluginError::Message("that item cannot be opened in 1Password".into())
            })?;
            return self.platform.open_link(&link).map_err(PluginError::other);
        }
        Err(PluginError::Unsupported(item.id.clone()))
    }

    /// Who you looked up and which logins you opened stay out of `usage.json`
    /// and the search history.
    fn tracks_usage(&self) -> bool {
        false
    }

    fn attach_notifier(&self, notifier: ResultsNotifier) {
        if let Some(shared) = &self.shared {
            if let Ok(mut slot) = shared.notifier.lock() {
                *slot = Some(notifier);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;

    use super::*;
    use crate::test_util::MockPlatform;

    const ACCOUNT: &str = "A3TS2BEDIFCXBJGPI4QZ5XLMOQ";
    const VAULT: &str = "kxbbwsorulrz4gcjwqhpxn55pm";
    const GITHUB: &str = "6a6gaw4xuvyzzx7kzefbbmeq4i";
    const ROUTER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn list_json() -> String {
        format!(
            r#"[{{"id":"{GITHUB}","title":"GitHub","vault":{{"id":"{VAULT}","name":"Personal"}},
                 "additional_information":"octocat@example.org",
                 "urls":[{{"primary":true,"href":"https://github.com/login"}}]}},
                {{"id":"{ROUTER}","title":"Home router","vault":{{"id":"{VAULT}","name":"Personal"}},
                 "urls":[{{"primary":true,"href":"http://192.168.1.1/admin"}}]}},
                {{"id":"cccccccccccccccccccccccccc","title":"Offline thing","vault":{{"id":"{VAULT}","name":"Personal"}},
                 "additional_information":"me"}}]"#
        )
    }

    fn accounts_json() -> String {
        format!(
            r#"[{{"url":"my.1password.com","email":"me@example.org","account_uuid":"{ACCOUNT}"}}]"#
        )
    }

    struct Fake {
        logins: Result<String, OpError>,
        accounts: String,
        list_calls: AtomicUsize,
        accounts_calls: AtomicUsize,
        asked_for: Mutex<Vec<Option<String>>>,
    }

    impl Fake {
        fn ok() -> Arc<Self> {
            Self::with(Ok(list_json()))
        }

        fn with(logins: Result<String, OpError>) -> Arc<Self> {
            Arc::new(Self {
                logins,
                accounts: accounts_json(),
                list_calls: AtomicUsize::new(0),
                accounts_calls: AtomicUsize::new(0),
                asked_for: Mutex::new(Vec::new()),
            })
        }
    }

    impl OpRunner for Fake {
        fn list_logins(&self, account: Option<&str>) -> Result<String, OpError> {
            self.list_calls.fetch_add(1, Ordering::SeqCst);
            self.asked_for
                .lock()
                .unwrap()
                .push(account.map(str::to_owned));
            self.logins.clone()
        }

        fn list_accounts(&self) -> Result<String, OpError> {
            self.accounts_calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.accounts.clone())
        }
    }

    fn config() -> OnePasswordConfig {
        OnePasswordConfig {
            enabled: true,
            ..OnePasswordConfig::default()
        }
    }

    /// A plugin whose notifier reports each finished fetch on a channel.
    fn plugin(fake: &Arc<Fake>) -> (OnePasswordPlugin, Arc<MockPlatform>, mpsc::Receiver<String>) {
        let platform = MockPlatform::empty();
        let plugin = OnePasswordPlugin::with_runner(&config(), platform.clone(), fake.clone());
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        plugin.attach_notifier(Arc::new(move |id: &str| {
            let _ = tx.lock().unwrap().send(id.to_owned());
        }));
        (plugin, platform, rx)
    }

    fn wait(rx: &mpsc::Receiver<String>) {
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            "1password"
        );
    }

    /// Types the keyword once and waits for the answer.
    fn loaded(fake: &Arc<Fake>) -> (OnePasswordPlugin, Arc<MockPlatform>, mpsc::Receiver<String>) {
        let (plugin, platform, rx) = plugin(fake);
        // Until the answer arrives the row says so (or, if op was very quick,
        // already has the logins).
        plugin.query("");
        wait(&rx);
        (plugin, platform, rx)
    }

    fn titles(rows: &[ResultItem]) -> Vec<&str> {
        rows.iter().map(|r| r.title.as_str()).collect()
    }

    #[test]
    fn nothing_runs_unless_enabled_or_asked() {
        let fake = Fake::ok();
        let off = OnePasswordPlugin::with_runner(
            &OnePasswordConfig::default(),
            MockPlatform::empty(),
            fake.clone(),
        );
        assert_eq!(off.query("git")[0].title, "1Password is off");
        // Metadata alone, a refresh and building the plugin never start op.
        let (plugin, _, _) = plugin(&fake);
        assert_eq!(plugin.keyword(), Some("1p"));
        assert!(!plugin.global() && !plugin.tracks_usage());
        plugin.refresh().unwrap();
        assert_eq!(fake.list_calls.load(Ordering::SeqCst), 0);
        assert_eq!(fake.accounts_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn the_first_query_starts_a_background_fetch_and_the_launcher_is_told() {
        let fake = Fake::ok();
        let (plugin, _, _rx) = loaded(&fake);
        let rows = plugin.query("git");
        assert_eq!(titles(&rows), ["GitHub"]);
        assert_eq!(fake.list_calls.load(Ordering::SeqCst), 1);
        // Further keystrokes use the cache.
        plugin.query("gith");
        plugin.query("");
        assert_eq!(fake.list_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn finds_logins_by_title_and_website_without_showing_the_username() {
        let (plugin, _, _rx) = loaded(&Fake::ok());
        assert_eq!(plugin.query("router")[0].title, "Home router");
        assert_eq!(plugin.query("192.168")[0].title, "Home router");
        assert_eq!(plugin.query("github.com")[0].title, "GitHub");
        assert_eq!(
            plugin.query("nothing like it")[0].title,
            "No matching login"
        );
        let rows = plugin.query("");
        assert_eq!(rows.len(), 3);
        for row in &rows {
            assert!(!row.subtitle.contains("octocat"), "{}", row.subtitle);
        }
        assert_eq!(
            rows[0].subtitle,
            "github.com · Personal · Enter to open the website"
        );
    }

    #[test]
    fn enter_opens_the_website_and_the_secondary_actions_are_the_documented_ones() {
        let (plugin, platform, _rx) = loaded(&Fake::ok());
        let github = plugin.query("github").remove(0);
        assert_eq!(
            github.action,
            Action::OpenUrl {
                url: "https://github.com/login".into()
            }
        );
        let labels: Vec<_> = github
            .secondary
            .iter()
            .map(|s| (s.label.as_str(), s.modifier))
            .collect();
        assert_eq!(
            labels,
            [
                ("Open in 1Password", Some(Modifier::Ctrl)),
                ("Copy username", Some(Modifier::Shift)),
                ("Copy website address", None),
            ]
        );

        plugin.execute(&github).unwrap();
        plugin
            .execute(&github.secondary_as_primary(0).unwrap())
            .unwrap();
        plugin
            .execute(&github.secondary_as_primary(1).unwrap())
            .unwrap();
        plugin
            .execute(&github.secondary_as_primary(2).unwrap())
            .unwrap();
        assert_eq!(
            *platform.opened_urls.lock().unwrap(),
            ["https://github.com/login"]
        );
        assert_eq!(
            *platform.opened_links.lock().unwrap(),
            [format!(
                "onepassword://view-item/?a={ACCOUNT}&v={VAULT}&i={GITHUB}"
            )]
        );
        assert_eq!(
            *platform.clipboard.lock().unwrap(),
            ["octocat@example.org", "https://github.com/login"]
        );
    }

    #[test]
    fn an_item_without_a_website_opens_in_the_app_instead() {
        let (plugin, platform, _rx) = loaded(&Fake::ok());
        let offline = plugin.query("offline").remove(0);
        assert_eq!(offline.subtitle, "Personal · Enter to open in 1Password");
        plugin.execute(&offline).unwrap();
        assert_eq!(platform.opened_links.lock().unwrap().len(), 1);
        assert!(platform.opened_urls.lock().unwrap().is_empty());
    }

    #[test]
    fn without_a_known_account_there_is_no_app_link() {
        let fake = Arc::new(Fake {
            accounts: "[]".into(),
            ..Arc::try_unwrap(Fake::ok()).ok().unwrap()
        });
        let (plugin, _, _rx) = loaded(&fake);
        let github = plugin.query("github").remove(0);
        assert!(github
            .secondary
            .iter()
            .all(|s| s.label != "Open in 1Password"));
        let offline = plugin.query("offline").remove(0);
        assert_eq!(offline.action, nothing());
        assert_eq!(offline.subtitle, "Personal · No website saved");
    }

    #[test]
    fn a_failure_is_shown_once_and_not_retried_until_asked() {
        let fake = Fake::with(Err(OpError::NotSignedIn(
            "You are not currently signed in.".into(),
        )));
        let (plugin, _, rx) = plugin(&fake);
        plugin.query("a");
        wait(&rx);
        let rows = plugin.query("ab");
        assert_eq!(rows[0].title, "op is not signed in to 1Password");
        assert!(rows[0]
            .subtitle
            .contains("You are not currently signed in."));
        plugin.query("abc");
        assert_eq!(fake.list_calls.load(Ordering::SeqCst), 1);

        // Enter on the row tries again.
        plugin.execute(&rows[0]).unwrap();
        wait(&rx);
        assert_eq!(fake.list_calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_missing_tool_points_to_the_install_page() {
        let fake = Fake::with(Err(OpError::NotInstalled));
        let (plugin, platform, rx) = plugin(&fake);
        plugin.query("");
        wait(&rx);
        let rows = plugin.query("");
        assert_eq!(
            rows[0].title,
            "The 1Password command-line tool (op) was not found"
        );
        plugin.execute(&rows[0]).unwrap();
        assert_eq!(*platform.opened_urls.lock().unwrap(), [INSTALL_URL]);
    }

    #[test]
    fn garbage_from_op_is_a_failure_not_a_panic() {
        let fake = Fake::with(Ok("<html>not json</html>".into()));
        let (plugin, _, rx) = plugin(&fake);
        plugin.query("");
        wait(&rx);
        assert_eq!(plugin.query("")[0].title, "op could not list your logins");
    }

    #[test]
    fn a_stale_list_is_refreshed_in_the_background_but_still_answers() {
        let fake = Fake::ok();
        let platform = MockPlatform::empty();
        let mut cfg = config();
        cfg.cache_minutes = 1;
        let plugin = OnePasswordPlugin::with_runner(&cfg, platform, fake.clone());
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        plugin.attach_notifier(Arc::new(move |id: &str| {
            let _ = tx.lock().unwrap().send(id.to_owned());
        }));
        plugin.query("");
        wait(&rx);

        // Age the cache past its time to live.
        if let Some(shared) = &plugin.shared {
            shared.cache.lock().unwrap().fetched_at =
                Instant::now().checked_sub(Duration::from_secs(61));
        }
        let rows = plugin.query("git");
        assert_eq!(titles(&rows), ["GitHub"], "the old list still answers");
        wait(&rx);
        assert_eq!(fake.list_calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn the_configured_account_is_passed_to_op() {
        let fake = Fake::ok();
        let mut cfg = config();
        cfg.account = "my.1password.com".into();
        let platform = MockPlatform::empty();
        let plugin = OnePasswordPlugin::with_runner(&cfg, platform, fake.clone());
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        plugin.attach_notifier(Arc::new(move |id: &str| {
            let _ = tx.lock().unwrap().send(id.to_owned());
        }));
        plugin.query("");
        wait(&rx);
        assert_eq!(
            *fake.asked_for.lock().unwrap(),
            [Some("my.1password.com".to_owned())]
        );
    }

    #[test]
    fn only_expected_actions_can_be_executed() {
        let (plugin, platform, _rx) = loaded(&Fake::ok());
        let item = |payload: &str| {
            ResultItem::new(
                ID,
                "x",
                "X",
                Action::Custom {
                    payload: payload.into(),
                },
            )
        };
        // Malformed or tampered deep link payloads are refused.
        for payload in [
            "open:",
            "open:a|b",
            "open:short|short|short",
            &format!("open:{ACCOUNT}|{VAULT}|{GITHUB}|extra"),
            &format!("open:{ACCOUNT}&h=evil|{VAULT}|{GITHUB}"),
            "somethingelse",
        ] {
            assert!(plugin.execute(&item(payload)).is_err(), "{payload}");
        }
        assert!(platform.opened_links.lock().unwrap().is_empty());
        plugin.execute(&item("nothing")).unwrap();
    }

    #[test]
    fn web_helpers() {
        assert_eq!(
            host_of("https://user:pw@github.com:8443/login?x=1"),
            "github.com:8443"
        );
        assert_eq!(host_of("http://192.168.1.1/admin"), "192.168.1.1");
        assert_eq!(url_for_search("https://a.b/c"), "a.b/c");
        assert!(is_web_url("HTTPS://x.y"));
        assert!(!is_web_url("ftp://x.y"));
        assert!(!is_web_url("javascript:alert(1)"));
    }
}
