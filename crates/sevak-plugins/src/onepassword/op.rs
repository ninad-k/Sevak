//! Talking to the official 1Password command-line tool, `op`.
//!
//! Sevak asks `op` two questions and nothing else:
//!
//! - `op item list --categories Login --format json`: the list of logins. This
//!   lists *item summaries*: title, vault, website addresses and the subtitle
//!   1Password shows (a login's username). It never contains passwords, one-time
//!   codes or any other secret field, and Sevak never runs `op item get`,
//!   `op read` or `op run`.
//! - `op account list --format json`: the signed-in accounts, to build the
//!   "open in 1Password" link. Needs no authentication.
//!
//! The runner is a trait so tests feed JSON fixtures; the real one
//! ([`CliRunner`]) is only used by the plugin and by one ignored test that runs
//! `op --version`.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;
use sevak_platform::process::{configure_helper_command, find_in_path};

/// How long `op` may take. It waits for the user at the biometric prompt, so
/// this is generous.
const RUN_TIMEOUT: Duration = Duration::from_secs(60);
/// More output than this from `op` is not a list of logins.
const MAX_OUTPUT_BYTES: u64 = 64 * 1024 * 1024;

/// A login as the list shows it. Deliberately has no secret fields: the parser
/// below does not read any, so none can leak from here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Login {
    pub id: String,
    pub title: String,
    pub vault_id: String,
    pub vault_name: String,
    /// Website addresses, the primary one first.
    pub urls: Vec<String>,
    /// The subtitle `op` prints for a login: its username (or e-mail address).
    pub username: Option<String>,
}

/// A signed-in 1Password account (`op account list`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub uuid: String,
    /// `my.1password.com`
    pub url: String,
    pub email: String,
}

/// Why `op` did not give a list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpError {
    /// The program was not found.
    NotInstalled,
    /// It ran but is not signed in, the prompt was dismissed or no account is set up.
    NotSignedIn(String),
    /// Anything else (timeout, unreadable output). The text is shown to the user.
    Failed(String),
}

/// Runs `op`. Implemented by [`CliRunner`] and by test fakes.
pub trait OpRunner: Send + Sync {
    /// The JSON of `op item list --categories Login --format json`, for
    /// `account` (passed as `--account`) or op's default.
    fn list_logins(&self, account: Option<&str>) -> Result<String, OpError>;

    /// The JSON of `op account list --format json`.
    fn list_accounts(&self) -> Result<String, OpError>;
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct RawItem {
    #[serde(default)]
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    vault: RawVault,
    #[serde(default)]
    urls: Vec<RawUrl>,
    #[serde(default)]
    additional_information: Option<String>,
}

#[derive(Deserialize, Default)]
struct RawVault {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
}

#[derive(Deserialize)]
struct RawUrl {
    #[serde(default)]
    href: String,
    #[serde(default)]
    primary: bool,
}

#[derive(Deserialize)]
struct RawAccount {
    #[serde(default)]
    account_uuid: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    email: String,
}

/// Parses `op item list --format json`. Items without an id or a title are
/// skipped; unknown fields (there are many) are ignored.
pub fn parse_logins(json: &str) -> Result<Vec<Login>, String> {
    let json = json.trim();
    // `op` prints `null` rather than `[]` for an empty list in some versions.
    if json.is_empty() || json == "null" {
        return Ok(Vec::new());
    }
    let items: Vec<RawItem> =
        serde_json::from_str(json).map_err(|err| format!("unexpected output from op: {err}"))?;
    Ok(items
        .into_iter()
        .filter(|item| !item.id.is_empty() && !item.title.trim().is_empty())
        .map(|item| {
            let mut urls: Vec<(bool, String)> = item
                .urls
                .into_iter()
                .filter(|u| !u.href.trim().is_empty())
                .map(|u| (u.primary, u.href.trim().to_owned()))
                .collect();
            urls.sort_by_key(|(primary, _)| !primary);
            Login {
                id: item.id,
                title: item.title.trim().to_owned(),
                vault_id: item.vault.id,
                vault_name: item.vault.name,
                urls: urls.into_iter().map(|(_, url)| url).collect(),
                username: item
                    .additional_information
                    .map(|u| u.trim().to_owned())
                    .filter(|u| !u.is_empty()),
            }
        })
        .collect())
}

/// Parses `op account list --format json`.
pub fn parse_accounts(json: &str) -> Result<Vec<Account>, String> {
    let json = json.trim();
    if json.is_empty() || json == "null" {
        return Ok(Vec::new());
    }
    let accounts: Vec<RawAccount> =
        serde_json::from_str(json).map_err(|err| format!("unexpected output from op: {err}"))?;
    Ok(accounts
        .into_iter()
        .filter(|a| !a.account_uuid.is_empty())
        .map(|a| Account {
            uuid: a.account_uuid,
            url: a.url,
            email: a.email,
        })
        .collect())
}

/// The account the "open in 1Password" link should use: the one named by
/// `[onepassword] account` (its address, its first label, its e-mail or its ID),
/// else the only one. `None` when it is ambiguous, so no link is better than a
/// wrong one.
pub fn choose_account<'a>(accounts: &'a [Account], wanted: &str) -> Option<&'a Account> {
    let wanted = wanted.trim();
    if wanted.is_empty() {
        return match accounts {
            [only] => Some(only),
            _ => None,
        };
    }
    accounts.iter().find(|a| {
        a.uuid.eq_ignore_ascii_case(wanted)
            || a.url.eq_ignore_ascii_case(wanted)
            || a.email.eq_ignore_ascii_case(wanted)
            || a.url
                .split('.')
                .next()
                .is_some_and(|label| label.eq_ignore_ascii_case(wanted))
    })
}

/// The first sentence of `op`'s error output, without the `[ERROR] 2024/..`
/// prefix `op` puts in front.
pub fn classify_failure(stderr: &str) -> OpError {
    let message = stderr
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("op failed");
    let message = message
        .strip_prefix("[ERROR]")
        .map(|rest| {
            // "[ERROR] 2024/05/01 12:00:00 message"
            let mut parts = rest.trim().splitn(3, ' ');
            let (date, time, text) = (parts.next(), parts.next(), parts.next());
            match (date, time, text) {
                (Some(d), Some(t), Some(text)) if d.contains('/') && t.contains(':') => {
                    text.to_owned()
                }
                _ => rest.trim().to_owned(),
            }
        })
        .unwrap_or_else(|| message.to_owned());
    let lower = message.to_lowercase();
    let needs_sign_in = [
        "not signed in",
        "not currently signed in",
        "no accounts configured",
        "authorization prompt dismissed",
        "authorization denied",
        "account is not signed in",
        "sign in",
        "unlock",
        "authenticat",
    ]
    .iter()
    .any(|marker| lower.contains(marker));
    if needs_sign_in {
        OpError::NotSignedIn(message)
    } else {
        OpError::Failed(message)
    }
}

// ---------------------------------------------------------------------------
// The real CLI
// ---------------------------------------------------------------------------

/// Runs the `op` program.
pub struct CliRunner {
    /// `[onepassword] op_path`; empty means "find it".
    configured: String,
}

impl CliRunner {
    pub fn new(op_path: &str) -> Self {
        Self {
            configured: op_path.trim().to_owned(),
        }
    }

    /// Where `op` is, if anywhere: the configured path, `PATH`, then the usual
    /// install folders (a launcher started from the desktop often has a short
    /// `PATH`, without Homebrew's folder, say).
    pub fn locate(&self) -> Option<PathBuf> {
        if !self.configured.is_empty() {
            let path = PathBuf::from(&self.configured);
            return path.is_file().then_some(path);
        }
        find_in_path("op").or_else(|| {
            usual_locations()
                .into_iter()
                .find(|candidate| candidate.is_file())
        })
    }

    fn run(&self, args: &[&str]) -> Result<String, OpError> {
        let program = self.locate().ok_or(OpError::NotInstalled)?;
        let mut command = Command::new(&program);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        configure_helper_command(&mut command);
        let mut child = command
            .spawn()
            .map_err(|err| OpError::Failed(format!("could not start op: {err}")))?;

        // Drain both pipes on threads so a chatty child cannot block on a full pipe.
        let reader = |pipe: Option<Box<dyn Read + Send>>| {
            std::thread::spawn(move || {
                let mut text = String::new();
                if let Some(pipe) = pipe {
                    let _ = pipe.take(MAX_OUTPUT_BYTES).read_to_string(&mut text);
                }
                text
            })
        };
        let stdout = reader(
            child
                .stdout
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>),
        );
        let stderr = reader(
            child
                .stderr
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>),
        );

        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if started.elapsed() > RUN_TIMEOUT => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(OpError::Failed(
                        "op did not answer in time (is the 1Password prompt waiting?)".to_owned(),
                    ));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(40)),
                Err(err) => return Err(OpError::Failed(err.to_string())),
            }
        };
        let out = stdout.join().unwrap_or_default();
        let err = stderr.join().unwrap_or_default();
        if status.success() {
            Ok(out)
        } else {
            Err(classify_failure(&err))
        }
    }

    /// `op --version`, to check the tool is there. Never touches the vault.
    pub fn version(&self) -> Result<String, OpError> {
        self.run(&["--version"]).map(|v| v.trim().to_owned())
    }
}

impl OpRunner for CliRunner {
    fn list_logins(&self, account: Option<&str>) -> Result<String, OpError> {
        let mut args = vec!["item", "list", "--categories", "Login", "--format", "json"];
        if let Some(account) = account.filter(|a| !a.is_empty()) {
            args.extend(["--account", account]);
        }
        self.run(&args)
    }

    fn list_accounts(&self) -> Result<String, OpError> {
        self.run(&["account", "list", "--format", "json"])
    }
}

fn usual_locations() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    if cfg!(target_os = "macos") {
        paths.extend(["/opt/homebrew/bin/op", "/usr/local/bin/op"].map(PathBuf::from));
    } else if cfg!(windows) {
        for (var, tail) in [
            ("LOCALAPPDATA", r"Microsoft\WinGet\Links\op.exe"),
            ("ProgramFiles", r"1Password CLI\op.exe"),
            ("LOCALAPPDATA", r"Programs\1Password CLI\op.exe"),
        ] {
            if let Some(base) = std::env::var_os(var) {
                paths.push(PathBuf::from(base).join(tail));
            }
        }
    } else {
        paths.extend(["/usr/bin/op", "/usr/local/bin/op", "/snap/bin/op"].map(PathBuf::from));
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape `op item list --format json` prints (fields trimmed to the
    /// interesting ones plus some it adds). Invented data.
    const LIST: &str = r#"[
      {"id":"6a6gaw4xuvyzzx7kzefbbmeq4i","title":"GitHub","version":3,
       "vault":{"id":"kxbbwsorulrz4gcjwqhpxn55pm","name":"Personal"},
       "category":"LOGIN","last_edited_by":"ABCDEFGHIJ","created_at":"2023-01-01T00:00:00Z",
       "updated_at":"2024-01-01T00:00:00Z","additional_information":"octocat@example.org",
       "urls":[{"label":"docs","href":"https://docs.github.com"},
               {"label":"website","primary":true,"href":"https://github.com/login"}],
       "tags":["dev"]},
      {"id":"bbbbbbbbbbbbbbbbbbbbbbbbbb","title":"  Router  ",
       "vault":{"id":"vvvvvvvvvvvvvvvvvvvvvvvvvv","name":"Home"},"urls":[],"additional_information":"admin"},
      {"id":"cccccccccccccccccccccccccc","title":"No vault or urls"},
      {"id":"","title":"No id"},
      {"id":"dddddddddddddddddddddddddd","title":"   "}
    ]"#;

    #[test]
    fn parses_titles_urls_vaults_and_usernames_only() {
        let logins = parse_logins(LIST).unwrap();
        assert_eq!(logins.len(), 3);
        let github = &logins[0];
        assert_eq!(github.id, "6a6gaw4xuvyzzx7kzefbbmeq4i");
        assert_eq!(github.title, "GitHub");
        assert_eq!(github.vault_name, "Personal");
        assert_eq!(github.vault_id, "kxbbwsorulrz4gcjwqhpxn55pm");
        // The primary address comes first whatever the order in the output.
        assert_eq!(
            github.urls,
            ["https://github.com/login", "https://docs.github.com"]
        );
        assert_eq!(github.username.as_deref(), Some("octocat@example.org"));
        assert_eq!(logins[1].title, "Router");
        assert!(logins[1].urls.is_empty());
        assert_eq!(logins[2].username, None);
        assert_eq!(logins[2].vault_id, "");
    }

    #[test]
    fn secret_looking_fields_are_never_read() {
        // Even if a future `op` put a secret into the summary, there is no field
        // in `Login` for it to land in.
        let json = r#"[{"id":"a","title":"T","password":"hunter2","fields":[{"value":"s3cret"}],"otp":"123456"}]"#;
        let logins = parse_logins(json).unwrap();
        assert_eq!(logins.len(), 1);
        let shown = format!("{logins:?}");
        assert!(
            !shown.contains("hunter2") && !shown.contains("s3cret") && !shown.contains("123456")
        );
    }

    #[test]
    fn empty_and_malformed_output() {
        assert!(parse_logins("[]").unwrap().is_empty());
        assert!(parse_logins("null").unwrap().is_empty());
        assert!(parse_logins("  ").unwrap().is_empty());
        assert!(parse_logins("{\"not\":\"a list\"}").is_err());
        assert!(parse_logins("You are not signed in").is_err());
    }

    const ACCOUNTS: &str = r#"[
      {"url":"my.1password.com","email":"me@example.org","user_uuid":"UUUUUUUUUUUUUUUUUUUUUUUUUU","account_uuid":"A3TS2BEDIFCXBJGPI4QZ5XLMOQ"},
      {"url":"acme.1password.com","email":"me@acme.example","user_uuid":"VVVVVVVVVVVVVVVVVVVVVVVVVV","account_uuid":"B4UT3CFEJGDYCKHQJ5RA6YMNPR"}
    ]"#;

    #[test]
    fn chooses_the_account_for_the_deep_link() {
        let accounts = parse_accounts(ACCOUNTS).unwrap();
        assert_eq!(accounts.len(), 2);
        // Two accounts and no preference: ambiguous, so none.
        assert_eq!(choose_account(&accounts, ""), None);
        let by = |wanted: &str| choose_account(&accounts, wanted).map(|a| a.url.as_str());
        assert_eq!(by("acme.1password.com"), Some("acme.1password.com"));
        assert_eq!(by("ACME"), Some("acme.1password.com"));
        assert_eq!(by("me@example.org"), Some("my.1password.com"));
        assert_eq!(by("b4ut3cfejgdyckhqj5ra6ymnpr"), Some("acme.1password.com"));
        assert_eq!(by("unknown"), None);
        // A single account needs no preference.
        assert_eq!(
            choose_account(&accounts[..1], "").map(|a| a.uuid.as_str()),
            Some("A3TS2BEDIFCXBJGPI4QZ5XLMOQ")
        );
        assert!(parse_accounts("[]").unwrap().is_empty());
    }

    #[test]
    fn classifies_op_errors() {
        let signed_out = classify_failure(
            "[ERROR] 2024/05/01 12:00:00 You are not currently signed in. Please run `op signin` --help for instructions",
        );
        assert!(
            matches!(&signed_out, OpError::NotSignedIn(m) if m.starts_with("You are not currently signed in"))
        );
        assert!(matches!(
            classify_failure(
                "[ERROR] 2024/05/01 12:00:00 authorization prompt dismissed, please try again"
            ),
            OpError::NotSignedIn(_)
        ));
        assert!(matches!(
            classify_failure("[ERROR] 2024/05/01 12:00:00 account is not signed in"),
            OpError::NotSignedIn(_)
        ));
        assert_eq!(
            classify_failure("\n[ERROR] 2024/05/01 12:00:00 network unreachable"),
            OpError::Failed("network unreachable".into())
        );
        assert_eq!(classify_failure(""), OpError::Failed("op failed".into()));
    }

    #[test]
    fn a_missing_program_is_reported_as_not_installed() {
        let runner = CliRunner::new("/definitely/not/here/op");
        assert_eq!(runner.locate(), None);
        assert_eq!(runner.list_accounts(), Err(OpError::NotInstalled));
        assert_eq!(runner.list_logins(Some("x")), Err(OpError::NotInstalled));
    }

    /// Runs the real `op --version` (and nothing else): `cargo test -p
    /// sevak-plugins -- --ignored op_version`.
    #[test]
    #[ignore = "needs the 1Password CLI installed"]
    fn op_version() {
        let version = CliRunner::new("").version().expect("op --version");
        println!("op {version}");
        assert!(!version.is_empty());
    }
}
