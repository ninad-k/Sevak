//! API keys: where they live, and how they are kept out of everything else.
//!
//! * A key is **never** in `config.toml`, in a settings export or in the
//!   diagnostics report: [`sevak_core::AiConfig`] has no field for one.
//! * It is stored in `ai-keys.json` in Sevak's data folder, written through
//!   [`sevak_platform::secret`]: encrypted with DPAPI on Windows, in a file
//!   only the owner can read (mode `0600`) on macOS and Linux.
//! * Or it comes from `OPENAI_API_KEY` / `ANTHROPIC_API_KEY`. A stored key
//!   wins, so a key typed into Settings is the one used.
//! * [`ApiKey`] prints as `ApiKey(****)`, and [`scrub`] removes a key (and
//!   anything shaped like one) from text that is about to be shown or logged.

use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};

use regex::Regex;
use serde::{Deserialize, Serialize};
use sevak_core::AiProvider;
use sevak_platform::paths::AppPaths;
use sevak_platform::private_file::write_atomic;
use sevak_platform::secret;

use super::error::AiError;

/// The file in the data folder.
pub const KEY_FILE: &str = "ai-keys.json";
/// Longest key accepted.
pub const MAX_KEY_CHARS: usize = 512;

/// An API key. Deliberately has no `Display`, and `Debug` hides the value.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    /// A key from what the user typed or an environment variable: surrounding
    /// whitespace is dropped; anything empty, too long or containing spaces,
    /// control characters or non-ASCII is refused (it could not be an HTTP
    /// header value, and a pasted sentence is not a key).
    pub fn new(text: &str) -> Option<Self> {
        let text = text.trim();
        let valid = !text.is_empty()
            && text.chars().count() <= MAX_KEY_CHARS
            && text.chars().all(|c| c.is_ascii_graphic());
        valid.then(|| Self(text.to_owned()))
    }

    /// The key itself, for the one place that puts it in a request header.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(****)")
    }
}

/// Shapes of well-known keys, removed from text even when `key` is not given.
static KEY_SHAPES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:sk|pk|rk)[-_][A-Za-z0-9_\-]{8,}|\bAIza[0-9A-Za-z_\-]{20,}")
        .expect("a valid pattern")
});

/// `text` with `key` and anything shaped like an API key replaced by `[key]`.
pub fn scrub(text: &str, key: Option<&ApiKey>) -> String {
    let mut out = text.to_owned();
    if let Some(key) = key {
        out = out.replace(key.expose(), "[key]");
    }
    KEY_SHAPES.replace_all(&out, "[key]").into_owned()
}

/// Where a key in use came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    /// Stored by Sevak (Settings > AI assistant).
    Stored,
    /// Read from this environment variable.
    Environment(&'static str),
}

/// What Settings shows next to the key field. Never the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyStatus {
    Missing,
    InUse(KeySource),
    /// A stored key exists but cannot be read back (and no variable is set).
    Unreadable,
}

/// Source of API keys for the assistant.
pub trait KeyProvider: Send + Sync {
    /// The key for `provider`, if one is available. `Ok(None)` for a provider
    /// that has no key (Ollama) or when none was saved.
    fn key(&self, provider: AiProvider) -> Result<Option<ApiKey>, AiError>;
}

#[derive(Serialize, Deserialize, Default)]
struct KeyFileData {
    version: u32,
    /// [`secret::Protection::id`] of the machine that wrote it.
    protection: String,
    /// Provider id -> hex of the protected key.
    keys: BTreeMap<String, String>,
}

/// One process-wide lock for the read-modify-write cycles of the file.
static FILE_LOCK: Mutex<()> = Mutex::new(());

/// The key file.
#[derive(Debug, Clone)]
pub struct KeyStore {
    path: PathBuf,
}

impl KeyStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// The store in Sevak's data folder; `None` when it cannot be found.
    pub fn default_location() -> Option<Self> {
        AppPaths::resolve()
            .ok()
            .map(|paths| Self::new(paths.data_dir.join(KEY_FILE)))
    }

    fn read(&self) -> Result<KeyFileData, AiError> {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(KeyFileData::default()),
            Err(_) => return Err(AiError::KeyUnreadable),
        };
        let data: KeyFileData =
            serde_json::from_slice(&bytes).map_err(|_| AiError::KeyUnreadable)?;
        if data.protection != secret::protection().id() && !data.keys.is_empty() {
            return Err(AiError::KeyUnreadable);
        }
        Ok(data)
    }

    /// The stored key for `provider`.
    pub fn get(&self, provider: AiProvider) -> Result<Option<ApiKey>, AiError> {
        let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let data = self.read()?;
        let Some(hex) = data.keys.get(provider.id()) else {
            return Ok(None);
        };
        let protected = from_hex(hex).ok_or(AiError::KeyUnreadable)?;
        let plain = secret::unprotect(&protected).map_err(|_| AiError::KeyUnreadable)?;
        let text = String::from_utf8(plain).map_err(|_| AiError::KeyUnreadable)?;
        ApiKey::new(&text).map(Some).ok_or(AiError::KeyUnreadable)
    }

    /// Whether a key is stored for `provider` (readable or not).
    pub fn contains(&self, provider: AiProvider) -> bool {
        let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        match std::fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice::<KeyFileData>(&bytes)
                .is_ok_and(|data| data.keys.contains_key(provider.id())),
            Err(_) => false,
        }
    }

    /// Stores `key` for `provider`, replacing an earlier one.
    pub fn set(&self, provider: AiProvider, key: &ApiKey) -> io::Result<()> {
        let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Keys that cannot be read back are dropped; the others are kept.
        let mut data = self.read().unwrap_or_default();
        let protected = secret::protect(key.expose().as_bytes())
            .map_err(|err| io::Error::other(err.to_string()))?;
        data.version = 1;
        data.protection = secret::protection().id().to_owned();
        data.keys
            .insert(provider.id().to_owned(), to_hex(&protected));
        self.write(&data)
    }

    /// Forgets the key for `provider`. The file goes when it holds no more.
    pub fn clear(&self, provider: AiProvider) -> io::Result<()> {
        let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut data = self.read().unwrap_or_default();
        data.keys.remove(provider.id());
        if data.keys.is_empty() {
            return match std::fs::remove_file(&self.path) {
                Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
                _ => Ok(()),
            };
        }
        self.write(&data)
    }

    fn write(&self, data: &KeyFileData) -> io::Result<()> {
        let json = serde_json::to_vec_pretty(data).map_err(io::Error::other)?;
        write_atomic(&self.path, &json)
    }
}

/// Reads an environment variable by name.
pub type EnvLookup = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// The keys Sevak uses: the stored one, else the environment's.
#[derive(Clone)]
pub struct StoredOrEnv {
    store: Option<KeyStore>,
    env: EnvLookup,
}

impl StoredOrEnv {
    /// The real data folder and the real environment.
    pub fn system() -> Self {
        Self::with_parts(
            KeyStore::default_location(),
            Arc::new(|name| std::env::var(name).ok()),
        )
    }

    pub fn with_parts(store: Option<KeyStore>, env: EnvLookup) -> Self {
        Self { store, env }
    }

    pub fn store(&self) -> Option<&KeyStore> {
        self.store.as_ref()
    }

    fn env_key(&self, provider: AiProvider) -> Option<(ApiKey, &'static str)> {
        let var = provider.key_env_var()?;
        let key = ApiKey::new(&(self.env)(var)?)?;
        Some((key, var))
    }

    /// The key for `provider` and where it came from.
    pub fn resolve(&self, provider: AiProvider) -> Result<Option<(ApiKey, KeySource)>, AiError> {
        if provider.key_env_var().is_none() {
            return Ok(None);
        }
        let stored = match &self.store {
            Some(store) => store.get(provider),
            None => Ok(None),
        };
        match stored {
            Ok(Some(key)) => Ok(Some((key, KeySource::Stored))),
            Ok(None) => Ok(self
                .env_key(provider)
                .map(|(key, var)| (key, KeySource::Environment(var)))),
            Err(err) => match self.env_key(provider) {
                Some((key, var)) => Ok(Some((key, KeySource::Environment(var)))),
                None => Err(err),
            },
        }
    }

    /// What Settings should say about `provider`'s key.
    pub fn status(&self, provider: AiProvider) -> KeyStatus {
        match self.resolve(provider) {
            Ok(Some((_, source))) => KeyStatus::InUse(source),
            Ok(None) => KeyStatus::Missing,
            Err(_) => KeyStatus::Unreadable,
        }
    }
}

impl KeyProvider for StoredOrEnv {
    fn key(&self, provider: AiProvider) -> Result<Option<ApiKey>, AiError> {
        Ok(self.resolve(provider)?.map(|(key, _)| key))
    }
}

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

fn from_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(text: &str) -> ApiKey {
        ApiKey::new(text).expect("a valid key")
    }

    fn env(pairs: &[(&'static str, &str)]) -> EnvLookup {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        Arc::new(move |name| {
            pairs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        })
    }

    #[test]
    fn keys_are_validated_and_hidden_from_debug_output() {
        assert!(ApiKey::new("").is_none());
        assert!(ApiKey::new("   ").is_none());
        assert!(ApiKey::new("two words").is_none());
        assert!(ApiKey::new("line\nbreak").is_none());
        assert!(ApiKey::new("sk-\u{e9}").is_none());
        assert!(ApiKey::new(&"k".repeat(MAX_KEY_CHARS + 1)).is_none());
        assert_eq!(ApiKey::new("  sk-abc123  ").unwrap().expose(), "sk-abc123");

        let text = format!("{:?}", key("sk-very-secret-value"));
        assert_eq!(text, "ApiKey(****)");
        assert!(!text.contains("secret"));
    }

    #[test]
    fn scrubbing_removes_the_key_and_lookalikes() {
        let k = key("custom-token-12345");
        let text = scrub(
            "bad custom-token-12345 and sk-proj-abcdefghij12 too",
            Some(&k),
        );
        assert_eq!(text, "bad [key] and [key] too");
        assert_eq!(scrub("sk-ant-api03-AAAAAAAAAAAA", None), "[key]");
        assert_eq!(
            scrub(
                "Authorization failed for AIzaSyA1234567890123456789012",
                None
            ),
            "Authorization failed for [key]"
        );
        assert_eq!(scrub("nothing secret here", None), "nothing secret here");
        // Ordinary words that happen to start like a key prefix are left alone.
        assert_eq!(
            scrub("a task-list, risk-free sky-blue", None),
            "a task-list, risk-free sky-blue"
        );
    }

    #[test]
    fn hex_round_trips_and_rejects_garbage() {
        let bytes = [0u8, 1, 127, 128, 255];
        assert_eq!(from_hex(&to_hex(&bytes)).unwrap(), bytes);
        assert!(from_hex("abc").is_none());
        assert!(from_hex("zz").is_none());
        assert!(from_hex("\u{e9}\u{e9}").is_none());
    }

    #[test]
    fn a_stored_key_is_read_back_and_never_written_in_plain_text() {
        let dir = tempfile::tempdir().unwrap();
        let store = KeyStore::new(dir.path().join(KEY_FILE));
        assert_eq!(store.get(AiProvider::OpenAi).unwrap(), None);
        store
            .set(AiProvider::OpenAi, &key("sk-test-plain-text-check"))
            .unwrap();
        assert_eq!(
            store.get(AiProvider::OpenAi).unwrap().unwrap().expose(),
            "sk-test-plain-text-check"
        );
        assert_eq!(store.get(AiProvider::Anthropic).unwrap(), None);
        assert!(store.contains(AiProvider::OpenAi));

        let on_disk = std::fs::read_to_string(dir.path().join(KEY_FILE)).unwrap();
        if cfg!(windows) {
            // DPAPI: the file holds only ciphertext.
            assert!(!on_disk.contains("sk-test-plain-text-check"));
            assert!(on_disk.contains("dpapi"));
        } else {
            assert!(on_disk.contains("\"file\""));
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_key_file_is_private_to_its_owner() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let store = KeyStore::new(dir.path().join(KEY_FILE));
        store.set(AiProvider::OpenAi, &key("sk-private")).unwrap();
        let mode = std::fs::metadata(dir.path().join(KEY_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn keys_for_several_providers_coexist_and_clear_independently() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(KEY_FILE);
        let store = KeyStore::new(path.clone());
        store
            .set(AiProvider::OpenAi, &key("sk-one-one-one"))
            .unwrap();
        store
            .set(AiProvider::Anthropic, &key("sk-ant-two-two"))
            .unwrap();
        store
            .set(AiProvider::OpenAi, &key("sk-replaced-key"))
            .unwrap();
        assert_eq!(
            store.get(AiProvider::OpenAi).unwrap().unwrap().expose(),
            "sk-replaced-key"
        );
        store.clear(AiProvider::OpenAi).unwrap();
        assert_eq!(store.get(AiProvider::OpenAi).unwrap(), None);
        assert!(store.get(AiProvider::Anthropic).unwrap().is_some());
        store.clear(AiProvider::Anthropic).unwrap();
        assert!(!path.exists(), "the file goes with its last key");
        store.clear(AiProvider::Anthropic).unwrap();
    }

    #[test]
    fn a_damaged_file_is_unreadable_but_can_be_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(KEY_FILE);
        std::fs::write(&path, "{ not json").unwrap();
        let store = KeyStore::new(path);
        assert_eq!(store.get(AiProvider::OpenAi), Err(AiError::KeyUnreadable));
        store
            .set(AiProvider::OpenAi, &key("sk-fresh-key-1"))
            .unwrap();
        assert!(store.get(AiProvider::OpenAi).unwrap().is_some());
    }

    #[test]
    fn a_file_from_another_protection_is_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(KEY_FILE);
        std::fs::write(
            &path,
            r#"{"version":1,"protection":"other-system","keys":{"openai":"00"}}"#,
        )
        .unwrap();
        let store = KeyStore::new(path);
        assert_eq!(store.get(AiProvider::OpenAi), Err(AiError::KeyUnreadable));
    }

    #[test]
    fn the_stored_key_wins_over_the_environment() {
        let dir = tempfile::tempdir().unwrap();
        let store = KeyStore::new(dir.path().join(KEY_FILE));
        let keys = StoredOrEnv::with_parts(
            Some(store.clone()),
            env(&[("OPENAI_API_KEY", "sk-from-environment")]),
        );
        assert_eq!(
            keys.status(AiProvider::OpenAi),
            KeyStatus::InUse(KeySource::Environment("OPENAI_API_KEY"))
        );
        store
            .set(AiProvider::OpenAi, &key("sk-from-settings"))
            .unwrap();
        assert_eq!(
            keys.status(AiProvider::OpenAi),
            KeyStatus::InUse(KeySource::Stored)
        );
        assert_eq!(
            keys.key(AiProvider::OpenAi).unwrap().unwrap().expose(),
            "sk-from-settings"
        );
    }

    #[test]
    fn the_environment_is_used_when_nothing_is_stored() {
        let keys = StoredOrEnv::with_parts(None, env(&[("ANTHROPIC_API_KEY", "sk-ant-env-key")]));
        assert_eq!(
            keys.key(AiProvider::Anthropic).unwrap().unwrap().expose(),
            "sk-ant-env-key"
        );
        assert_eq!(keys.status(AiProvider::OpenAi), KeyStatus::Missing);
        // An environment variable that is not a usable key counts as unset.
        let blank = StoredOrEnv::with_parts(None, env(&[("OPENAI_API_KEY", "   ")]));
        assert_eq!(blank.status(AiProvider::OpenAi), KeyStatus::Missing);
    }

    #[test]
    fn ollama_has_no_key() {
        let keys = StoredOrEnv::with_parts(None, env(&[("OPENAI_API_KEY", "sk-x-x-x-x-x")]));
        assert_eq!(keys.key(AiProvider::Ollama).unwrap(), None);
        assert_eq!(keys.status(AiProvider::Ollama), KeyStatus::Missing);
    }

    #[test]
    fn an_unreadable_stored_key_falls_back_to_the_environment_or_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(KEY_FILE);
        std::fs::write(&path, "garbage").unwrap();
        let without = StoredOrEnv::with_parts(Some(KeyStore::new(path.clone())), env(&[]));
        assert_eq!(without.status(AiProvider::OpenAi), KeyStatus::Unreadable);
        assert_eq!(without.key(AiProvider::OpenAi), Err(AiError::KeyUnreadable));
        let with = StoredOrEnv::with_parts(
            Some(KeyStore::new(path)),
            env(&[("OPENAI_API_KEY", "sk-from-environment")]),
        );
        assert_eq!(
            with.status(AiProvider::OpenAi),
            KeyStatus::InUse(KeySource::Environment("OPENAI_API_KEY"))
        );
    }
}
