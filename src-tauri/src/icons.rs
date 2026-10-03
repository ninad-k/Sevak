//! Result icons, served to the webview through the `sevak-icon` URI scheme.
//!
//! A custom scheme (instead of Tauri's asset protocol) means no filesystem
//! scope is exposed to the page: the webview can only ask for a short key that
//! this process handed out for an icon it was about to display.
//!
//! # URL form
//!
//! The key is the hex of a 64-bit hash of the [`IconSource`]; `search`
//! registers each File/Shell icon and puts the resulting URL in the DTO.
//!
//! | Platform       | URL                                   |
//! |----------------|---------------------------------------|
//! | Windows        | `http://sevak-icon.localhost/<key>`   |
//! | Linux / macOS  | `sevak-icon://localhost/<key>`        |
//!
//! This is the convention Tauri 2 uses for every custom protocol (WebView2
//! cannot register new schemes, so Tauri maps them to a `*.localhost` host).
//!
//! # Loading
//!
//! Extraction (COM + rendering on Windows, theme lookup + decoding on Linux)
//! is slow, so a request runs on a blocking thread. Results, including
//! failures, are cached in memory with a bounded size.

use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use sevak_core::{IconData, IconSource, ResultItem};
use sevak_platform::PlatformProvider;
use tauri::http::{header, Response, StatusCode};
use tauri::{AppHandle, Manager};

use crate::state::{lock, AppState};

/// Name of the custom URI scheme.
pub const SCHEME: &str = "sevak-icon";
/// Edge length requested from the platform: 64 px covers 32-px rows at 2x.
const ICON_SIZE: u32 = 64;
/// How long the webview may keep an icon without asking again.
const CACHE_CONTROL: &str = "max-age=86400";

/// Icons kept in the byte cache (successes and failures).
const MAX_CACHED: usize = 1000;
/// Total bytes kept in the cache, so a few huge `.ico` files cannot bloat it.
const MAX_CACHED_BYTES: usize = 32 * 1024 * 1024;
/// Registered keys; the table is reset when a batch would exceed it.
const MAX_REGISTERED: usize = 8192;

/// How the UI should draw a result's icon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IconDto {
    Url { url: String },
    Builtin { name: String },
}

pub fn icon_url(key: &str) -> String {
    if cfg!(windows) {
        format!("http://{SCHEME}.localhost/{key}")
    } else {
        format!("{SCHEME}://localhost/{key}")
    }
}

fn key_for(source: &IconSource) -> String {
    // `DefaultHasher::new()` uses fixed keys, so the key is the same for the
    // same source across runs of one build, and the webview's HTTP cache stays
    // valid across restarts.
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn valid_key(key: &str) -> bool {
    key.len() == 16 && key.bytes().all(|b| b.is_ascii_hexdigit())
}

#[derive(Default)]
struct Cache {
    /// `None` records a failed extraction.
    entries: HashMap<String, Option<Arc<IconData>>>,
    order: VecDeque<String>,
    bytes: usize,
}

impl Cache {
    fn insert(&mut self, key: String, value: Option<Arc<IconData>>) {
        if self.entries.contains_key(&key) {
            return;
        }
        self.bytes += value.as_ref().map_or(0, |data| data.bytes.len());
        self.entries.insert(key.clone(), value);
        self.order.push_back(key);
        while self.entries.len() > MAX_CACHED || self.bytes > MAX_CACHED_BYTES {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(Some(data)) = self.entries.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(data.bytes.len());
            }
        }
    }
}

pub struct IconStore {
    platform: Arc<dyn PlatformProvider>,
    sources: Mutex<HashMap<String, IconSource>>,
    cache: Mutex<Cache>,
}

impl IconStore {
    pub fn new(platform: Arc<dyn PlatformProvider>) -> Self {
        Self {
            platform,
            sources: Mutex::new(HashMap::new()),
            cache: Mutex::new(Cache::default()),
        }
    }

    /// The DTO for each item's icon; File/Shell icons are registered so the
    /// scheme handler can resolve their URLs.
    pub fn describe(&self, items: &[ResultItem]) -> Vec<Option<IconDto>> {
        let mut sources = lock(&self.sources);
        let new_keys = items
            .iter()
            .filter_map(|item| item.icon.as_ref())
            .filter(|icon| !matches!(icon, IconSource::Builtin { .. }))
            .count();
        if sources.len() + new_keys > MAX_REGISTERED {
            sources.clear();
        }
        items
            .iter()
            .map(|item| {
                item.icon.as_ref().map(|icon| match icon {
                    IconSource::Builtin { name } => IconDto::Builtin { name: name.clone() },
                    other => {
                        let key = key_for(other);
                        let url = icon_url(&key);
                        sources.entry(key).or_insert_with(|| other.clone());
                        IconDto::Url { url }
                    }
                })
            })
            .collect()
    }

    /// Loads (or recalls) the icon behind `key`. Blocking: may extract the icon.
    fn load(&self, key: &str) -> Option<Arc<IconData>> {
        if let Some(hit) = lock(&self.cache).entries.get(key) {
            return hit.clone();
        }
        let source = lock(&self.sources).get(key).cloned()?;
        let loaded = match self.platform.load_icon(&source, ICON_SIZE) {
            Ok(data) => Some(Arc::new(data)),
            Err(err) => {
                tracing::debug!(?source, "icon unavailable: {err}");
                None
            }
        };
        lock(&self.cache).insert(key.to_owned(), loaded.clone());
        loaded
    }
}

/// Answers one `sevak-icon` request. Runs on a blocking thread.
pub fn respond(app: &AppHandle, path: &str) -> Response<Cow<'static, [u8]>> {
    let key = path.trim_start_matches('/');
    let icon = valid_key(key)
        .then(|| app.try_state::<AppState>())
        .flatten()
        .and_then(|state| state.search.icons.load(key));
    match icon {
        Some(icon) => Response::builder()
            .header(header::CONTENT_TYPE, icon.mime)
            .header(header::CACHE_CONTROL, CACHE_CONTROL)
            .body(Cow::Owned(icon.bytes.clone())),
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(header::CACHE_CONTROL, "no-store")
            .body(Cow::Borrowed(&[][..])),
    }
    .unwrap_or_else(|_| Response::new(Cow::Borrowed(&[][..])))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(len: usize) -> Option<Arc<IconData>> {
        Some(Arc::new(IconData {
            mime: "image/png",
            bytes: vec![0; len],
        }))
    }

    #[test]
    fn keys_are_stable_and_distinct() {
        let a = IconSource::Shell {
            parsing_name: "a".into(),
        };
        let b = IconSource::Shell {
            parsing_name: "b".into(),
        };
        assert_eq!(key_for(&a), key_for(&a.clone()));
        assert_ne!(key_for(&a), key_for(&b));
        assert!(valid_key(&key_for(&a)));
        assert!(!valid_key("../etc/passwd"));
    }

    #[test]
    fn cache_evicts_oldest_by_count() {
        let mut cache = Cache::default();
        for i in 0..MAX_CACHED + 10 {
            cache.insert(format!("{i:016x}"), data(1));
        }
        assert_eq!(cache.entries.len(), MAX_CACHED);
        assert!(!cache.entries.contains_key(&format!("{:016x}", 0)));
        assert!(cache
            .entries
            .contains_key(&format!("{:016x}", MAX_CACHED + 9)));
    }

    #[test]
    fn cache_evicts_by_bytes_and_remembers_failures() {
        let mut cache = Cache::default();
        cache.insert("fail".into(), None);
        assert!(matches!(cache.entries.get("fail"), Some(None)));
        cache.insert("big1".into(), data(MAX_CACHED_BYTES / 2 + 1));
        cache.insert("big2".into(), data(MAX_CACHED_BYTES / 2 + 1));
        assert!(!cache.entries.contains_key("fail"));
        assert!(!cache.entries.contains_key("big1"));
        assert!(cache.entries.contains_key("big2"));
    }

    #[test]
    fn url_uses_platform_form() {
        let url = icon_url("abc");
        if cfg!(windows) {
            assert_eq!(url, "http://sevak-icon.localhost/abc");
        } else {
            assert_eq!(url, "sevak-icon://localhost/abc");
        }
    }
}
