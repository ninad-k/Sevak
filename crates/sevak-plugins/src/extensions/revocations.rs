//! Signed lifecycle notices. No network until the caller explicitly refreshes.
//! Package catalogs remain release-pinned; this independent channel can warn
//! older builds about removed packages. It never executes or uninstalls code.
use crate::net::Transport;
use base64::{engine::general_purpose::STANDARD, Engine};
use minisign_verify::{PublicKey, Signature};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::Write;
use std::path::Path;

pub const SOURCE: &str = "https://github.com/ninad-k/Sevak/releases/download/extension-advisories/extension-revocations.json";
pub const MAX_BYTES: usize = 128 * 1024;
pub const MAX_SIGNATURE_BYTES: usize = 8 * 1024;
const MAX_LIFETIME: u64 = 30 * 86400;
const CACHE: &str = "extension-advisories.json";

/// The gallery namespace a notice applies to. Theme and package ids are
/// separate namespaces, and themes carry no version.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Subject {
    #[default]
    Package,
    Theme,
}

impl Subject {
    fn is_package(&self) -> bool {
        *self == Subject::Package
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Notice {
    /// Omitted for packages (workflows, plugins, native extensions).
    #[serde(default, skip_serializing_if = "Subject::is_package")]
    pub kind: Subject,
    pub id: String,
    pub versions: Vec<String>,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advisory_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Feed {
    pub format: u32,
    pub issued_at: u64,
    pub expires_at: u64,
    pub revocations: Vec<Notice>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    body: String,
    signature: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Status {
    /// not_checked, verified, stale, unavailable. Never a claim that a package is safe.
    pub state: &'static str,
    pub message: String,
    pub issued_at: Option<u64>,
    pub expires_at: Option<u64>,
}

#[derive(Default)]
pub struct Advisories {
    feed: Option<Feed>,
    error: Option<String>,
    cache_loaded: bool,
}

fn plain(text: &str, max: usize) -> bool {
    !text.trim().is_empty()
        && text == text.trim()
        && text.chars().count() <= max
        && !text.chars().any(char::is_control)
}

/// The gallery id grammar (packages and themes): lower case letters, digits,
/// `-` and `_`, at most 48, and not a name Windows reserves.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 48
        && id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'-' | b'_'))
        && !sevak_core::safe_names::is_reserved_device_name(id)
}

fn validate(feed: &Feed) -> Result<(), String> {
    if feed.format != 1
        || feed.issued_at == 0
        || feed.expires_at <= feed.issued_at
        || feed.expires_at - feed.issued_at > MAX_LIFETIME
        || feed.revocations.len() > 256
    {
        return Err("invalid advisory format, validity period or record count".into());
    }
    let mut ids = HashSet::new();
    for notice in &feed.revocations {
        if !valid_id(&notice.id)
            || !ids.insert((notice.kind, &notice.id))
            || !plain(&notice.reason, 500)
            || notice.versions.is_empty()
            || notice.versions.len() > 64
        {
            return Err("invalid or duplicate advisory metadata".into());
        }
        if notice.kind == Subject::Theme && notice.versions != ["*"] {
            return Err("theme advisories must use the sole version \"*\"".into());
        }
        let mut versions = HashSet::new();
        for version in &notice.versions {
            if !versions.insert(version)
                || version.is_empty()
                || version.len() > 64
                || (version == "*" && notice.versions.len() != 1)
                || (version != "*"
                    && !version
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b".+-".contains(&c)))
            {
                return Err("invalid advisory versions".into());
            }
        }
        if let Some(link) = &notice.advisory_url {
            let suffix =
                link.strip_prefix("https://github.com/ninad-k/Sevak/security/advisories/GHSA-");
            if link.len() > 200
                || !suffix.is_some_and(|s| {
                    !s.is_empty()
                        && s.bytes()
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                })
            {
                return Err("advisory links must name a Sevak GitHub security advisory".into());
            }
        }
    }
    Ok(())
}

/// Verify the exact bounded bytes before parsing any actionable metadata.
pub fn verify(body: &[u8], signature: &[u8], key: &str) -> Result<Feed, String> {
    if body.len() > MAX_BYTES || signature.len() > MAX_SIGNATURE_BYTES {
        return Err("advisory download exceeds its size limit".into());
    }
    let decode = |bytes: &[u8]| -> Result<String, String> {
        let bytes = STANDARD
            .decode(bytes.trim_ascii())
            .map_err(|_| "invalid advisory signing encoding")?;
        String::from_utf8(bytes).map_err(|_| "advisory signing data is not text".into())
    };
    let key =
        PublicKey::decode(&decode(key.as_bytes())?).map_err(|_| "invalid trusted advisory key")?;
    let signature =
        Signature::decode(&decode(signature)?).map_err(|_| "invalid advisory signature")?;
    key.verify(body, &signature, false)
        .map_err(|_| "advisory signature was not authenticated")?;
    let feed: Feed =
        serde_json::from_slice(body).map_err(|_| "invalid signed advisory document")?;
    validate(&feed)?;
    Ok(feed)
}

/// The application's updater public key (`plugins.updater.pubkey` in
/// `src-tauri/tauri.conf.json`), kept inside this crate so the library does not
/// depend on the app's layout. A test fails if the two ever differ.
const TRUSTED_KEY: &str = include_str!("revocations/trusted-key.txt");

fn trusted_key() -> String {
    // Compile-time source only; local cache/network never supplies it.
    TRUSTED_KEY.trim().to_owned()
}

/// Semantic version equality: `1.0`, `1.0.0` and `1.0.0+build` are the same
/// version. Versions that do not parse as (padded) semver match only exactly.
pub fn same_version(a: &str, b: &str) -> bool {
    fn normalized(version: &str) -> Option<semver::Version> {
        let (core, rest) = version.split_at(version.find(['-', '+']).unwrap_or(version.len()));
        let parts = core.split('.').count();
        if parts > 3 {
            return None;
        }
        semver::Version::parse(&format!("{core}{}{rest}", ".0".repeat(3 - parts))).ok()
    }
    if a == b {
        return true;
    }
    match (normalized(a), normalized(b)) {
        (Some(a), Some(b)) => a.cmp_precedence(&b) == std::cmp::Ordering::Equal,
        _ => false,
    }
}

/// Fetched but not yet verified advisory bytes: `(body, signature)`.
pub type Fetched = Result<(Vec<u8>, Vec<u8>), String>;

fn fresh(feed: &Feed, now: u64) -> Result<(), String> {
    if feed.issued_at > now.saturating_add(300) {
        return Err("advisory issue time is in the future; check your clock".into());
    }
    if now >= feed.expires_at {
        return Err(
            "signed extension advisories have expired; current revocation status is unknown".into(),
        );
    }
    Ok(())
}

/// Release tooling verifies the exact same embedded key, schema and freshness
/// checks as the application, before uploading either asset.
pub fn verify_for_build(body: &[u8], signature: &[u8], now: u64) -> Result<Feed, String> {
    let feed = verify(body, signature, &trusted_key())?;
    fresh(&feed, now)?;
    Ok(feed)
}

impl Advisories {
    #[cfg(test)]
    pub(super) fn fixture() -> Self {
        let feed = verify(
            include_bytes!("revocations/fixtures/valid.json"),
            include_bytes!("revocations/fixtures/valid.json.sig"),
            include_str!("revocations/fixtures/public-key.txt"),
        )
        .unwrap();
        Self {
            feed: Some(feed),
            cache_loaded: true,
            error: None,
        }
    }

    /// Authenticated-looking state with exactly `notices`, for other modules' tests.
    #[cfg(test)]
    pub(crate) fn with_notices(notices: Vec<Notice>) -> Self {
        let mut state = Self::fixture();
        state.feed.as_mut().unwrap().revocations = notices;
        state
    }

    /// Local, authenticated cache only; expired known revocations remain warnings.
    /// The local clock never discards authentic notices: a wrong clock only
    /// changes the verified/stale label that [`Self::status`] reports.
    pub fn load_cached(&mut self, directory: &Path) {
        self.load_cached_using(directory, &trusted_key());
    }

    fn load_cached_using(&mut self, directory: &Path, key: &str) {
        if self.cache_loaded {
            return;
        }
        self.cache_loaded = true;
        let file = directory.join(CACHE);
        if !file.exists() {
            return;
        }
        let result = (|| {
            if !std::fs::symlink_metadata(&file)
                .map_err(|_| "could not inspect saved extension advisories")?
                .file_type()
                .is_file()
            {
                return Err("saved extension advisories must be a regular file".into());
            }
            let bytes = sevak_core::bounded_read::read_capped(
                &file,
                (MAX_BYTES * 6 + MAX_SIGNATURE_BYTES * 2) as u64,
            )
            .map_err(|_| "could not read saved extension advisories".to_owned())?;
            let saved: Envelope = serde_json::from_slice(&bytes)
                .map_err(|_| "invalid saved advisory envelope".to_owned())?;
            verify(saved.body.as_bytes(), saved.signature.as_bytes(), key)
        })();
        match result {
            Ok(feed) => self.feed = Some(feed),
            Err(error) => self.error = Some(error),
        }
    }

    /// Explicit user request only. Failed or rollback responses never replace
    /// previously authenticated notices and never report a clean safety result.
    /// An authentic feed at least as new as the saved one is kept even when the
    /// local clock calls it expired or future-dated: it only adds information,
    /// and [`Self::status`] then labels it stale instead of verified.
    pub fn refresh(&mut self, transport: &dyn Transport, directory: &Path) {
        self.apply(Self::fetch(transport), directory);
    }

    /// The network half of [`Self::refresh`]. It needs no state, so a caller
    /// that shares [`Advisories`] behind a lock can fetch without holding it.
    pub fn fetch(transport: &dyn Transport) -> Fetched {
        let body = transport
            .get(SOURCE, MAX_BYTES)
            .map_err(|e| e.to_string())?;
        let signature = transport
            .get(&format!("{SOURCE}.sig"), MAX_SIGNATURE_BYTES)
            .map_err(|e| e.to_string())?;
        Ok((body, signature))
    }

    /// The local half of [`Self::refresh`]: verify, refuse rollbacks, save and
    /// swap. No network.
    pub fn apply(&mut self, fetched: Fetched, directory: &Path) {
        self.apply_using(fetched, directory, &trusted_key());
    }

    #[cfg(test)]
    fn refresh_using(&mut self, transport: &dyn Transport, directory: &Path, key: &str) {
        self.apply_using(Self::fetch(transport), directory, key);
    }

    fn apply_using(&mut self, fetched: Fetched, directory: &Path, key: &str) {
        self.load_cached_using(directory, key);
        let result = (|| {
            let (body, signature) = fetched?;
            let feed = verify(&body, &signature, key)?;
            if self
                .feed
                .as_ref()
                .is_some_and(|old| feed.issued_at < old.issued_at)
            {
                return Err(
                    "refusing older extension advisories than the verified saved copy".into(),
                );
            }
            let saved = Envelope {
                body: String::from_utf8(body).map_err(|_| "invalid advisory text".to_owned())?,
                signature: String::from_utf8(signature)
                    .map_err(|_| "invalid signature text".to_owned())?,
            };
            let serialized = serde_json::to_vec(&saved).map_err(|e| e.to_string())?;
            // Do not claim successful persistence when the disk is unwritable,
            // but retain freshly authenticated notices in this process as well.
            // A unique temporary file avoids following a preexisting .tmp link.
            let save = (|| -> std::io::Result<()> {
                sevak_platform::private_file::create_private_dir_all(directory)?;
                let mut temp = tempfile::NamedTempFile::new_in(directory)?;
                temp.write_all(&serialized)?;
                temp.as_file().sync_all()?;
                temp.persist(directory.join(CACHE)).map_err(|e| e.error)?;
                Ok(())
            })();
            let error = save.err().map(|e| format!("could not save verified advisories: {e}; new notices apply only until Sevak exits"));
            Ok((feed, error))
        })();
        match result {
            Ok((feed, error)) => {
                self.feed = Some(feed);
                self.error = error;
            }
            Err(error) => self.error = Some(error),
        }
    }

    pub fn status(&self, now: u64) -> Status {
        let (state, message) = if let Some(error) = &self.error {
            let kept = if self.feed.is_some() {
                "Previously authenticated notices still apply."
            } else {
                "No authenticated notices are available yet."
            };
            ("unavailable", format!("Revocation status could not be refreshed: {error}. {kept} This is not a safety check result."))
        } else if let Some(feed) = &self.feed {
            match fresh(feed, now) {
                Ok(()) => ("verified", "Signed extension advisories verified. Absence of a notice does not prove an extension is safe.".into()),
                Err(error) => ("stale", format!("{error}. Previously authenticated notices still apply.")),
            }
        } else {
            ("not_checked", "Current revocation status is unknown. Load or refresh the list to check signed extension advisories.".into())
        };
        Status {
            state,
            message,
            issued_at: self.feed.as_ref().map(|f| f.issued_at),
            expires_at: self.feed.as_ref().map(|f| f.expires_at),
        }
    }

    /// A theme has no version: pass `""`, and only a `"*"` theme notice matches.
    /// Versions are compared as semver where both parse (`1.0` is `1.0.0`),
    /// otherwise exactly. `"*"` matches every version, including any later
    /// fixed one, so a notice should list exact versions unless no version
    /// of the item may be installed at all.
    pub fn notice(&self, kind: Subject, id: &str, version: &str) -> Option<Notice> {
        self.feed
            .as_ref()?
            .revocations
            .iter()
            .find(|n| {
                n.kind == kind
                    && n.id == id
                    && n.versions
                        .iter()
                        .any(|v| v == "*" || same_version(v, version))
            })
            .cloned()
    }

    /// A notice for `id` whatever version it names: for a copy whose version
    /// is unknown (installed without a receipt).
    pub fn any_notice(&self, kind: Subject, id: &str) -> Option<Notice> {
        self.feed
            .as_ref()?
            .revocations
            .iter()
            .find(|n| n.kind == kind && n.id == id)
            .cloned()
    }

    /// The notices that revoke every version (`"*"`).
    pub fn all_version_notices(&self) -> Vec<Notice> {
        self.feed.as_ref().map_or_else(Vec::new, |feed| {
            feed.revocations
                .iter()
                .filter(|n| n.versions == ["*"])
                .cloned()
                .collect()
        })
    }
}

#[cfg(test)]
mod tests;
