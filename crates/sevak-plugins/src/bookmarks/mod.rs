//! Bookmark search across the user's web browsers.
//!
//! Everything is read locally and read-only, from every profile of every
//! detected browser (or those named in `[bookmarks] browsers`):
//!
//! - the Chromium family (Chrome, Edge, Brave, Vivaldi, Chromium, Opera): the
//!   `Bookmarks` JSON file of each profile, see [`chromium`];
//! - the Firefox family (Firefox, LibreWolf, Zen): `places.sqlite` of each
//!   profile in `profiles.ini`, read from a private temporary copy because the
//!   browser keeps the original locked, see [`firefox`].
//!
//! Where the browsers keep their data is the platform layer's business
//! ([`PlatformProvider::browser_roots`]); this module only parses.
//!
//! # Indexing
//!
//! [`Plugin::refresh`] runs at startup, on "Reload index" and every few minutes,
//! always on a background thread. Each source file is re-read only when its
//! modification time or size changed (for Firefox: the database or its
//! write-ahead log), so an idle refresh costs a few `stat` calls. Queries read an
//! immutable snapshot and never wait for a refresh. A source that cannot be read
//! (for example a file the browser is rewriting) keeps its previous contents.
//!
//! Bookmarks whose URL is not `http(s)` (`javascript:`, `chrome:`, `file:`,
//! `place:`, ...) are skipped. Identical URLs are merged across browsers and
//! profiles into one result that names the browsers it was found in.

mod chromium;
mod firefox;

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Instant, SystemTime};

use sevak_core::config::BookmarksConfig;
use sevak_core::{Action, FuzzyQuery, IconSource, Plugin, PluginResult, ResultItem};
use sevak_platform::{BrowserFamily, BrowserRoot, PlatformProvider};

use crate::actions::execute_action;

/// Hard limit on indexed bookmarks, to bound memory and per-query work.
pub const MAX_BOOKMARKS: usize = 50_000;
const MAX_CANDIDATES: usize = 50;
const MIN_QUERY_CHARS: usize = 2;
/// Sources larger than this are skipped (a corrupt or non-bookmark file).
const MAX_SOURCE_BYTES: u64 = 512 * 1024 * 1024;
const FOLDER_SEPARATOR: &str = " / ";

const TITLE_PREFIX_BONUS: f64 = 40.0;
const HOST_PREFIX_BONUS: f64 = 30.0;
const TITLE_CONTAINS_BONUS: f64 = 25.0;
const HOST_CONTAINS_BONUS: f64 = 20.0;
/// A bookmark matched only through its URL ranks below a title match.
const URL_ONLY_WEIGHT: f64 = 0.6;

/// One bookmark as a browser stores it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RawBookmark {
    title: String,
    url: String,
    /// Folder names joined with [`FOLDER_SEPARATOR`], root folder first.
    folder: String,
}

/// A bookmark file of one profile.
#[derive(Debug, Clone)]
struct Source {
    browser: &'static str,
    family: BrowserFamily,
    /// `Bookmarks` (Chromium) or `places.sqlite` (Firefox).
    path: PathBuf,
}

type FileStamp = Option<(SystemTime, u64)>;

impl Source {
    /// Changes to any of these files mean the bookmarks may have changed.
    fn watched_files(&self) -> Vec<PathBuf> {
        match self.family {
            BrowserFamily::Chromium => vec![self.path.clone()],
            BrowserFamily::Firefox => vec![self.path.clone(), firefox::wal_file(&self.path)],
        }
    }

    fn stamp(&self) -> Vec<FileStamp> {
        self.watched_files()
            .iter()
            .map(|file| {
                let meta = fs::metadata(file).ok()?;
                Some((meta.modified().ok()?, meta.len()))
            })
            .collect()
    }

    fn read(&self) -> Result<Vec<RawBookmark>, String> {
        match self.family {
            BrowserFamily::Chromium => chromium::read(&self.path),
            BrowserFamily::Firefox => firefox::read(&self.path),
        }
    }
}

struct CachedSource {
    stamp: Vec<FileStamp>,
    bookmarks: Vec<RawBookmark>,
}

/// A searchable bookmark, merged over browsers and profiles.
#[derive(Debug, Clone)]
struct Entry {
    key: String,
    title: String,
    title_lower: String,
    url: String,
    folder: String,
    /// Domain for display and matching, without `www.`.
    host: String,
    /// `title` + URL without its scheme: what URL-only matches run against.
    haystack: String,
    browsers: Vec<&'static str>,
}

impl Entry {
    fn subtitle(&self) -> String {
        let browsers = self.browsers.join(", ");
        [self.folder.as_str(), self.host.as_str(), browsers.as_str()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

/// Searches the bookmarks of the user's browsers by title and URL.
pub struct BookmarksPlugin {
    config: BookmarksConfig,
    keyword: Option<String>,
    platform: Arc<dyn PlatformProvider>,
    /// Replaces the platform's browser detection (tests).
    roots: Option<Vec<BrowserRoot>>,
    cache: Mutex<HashMap<PathBuf, CachedSource>>,
    index: RwLock<Arc<Vec<Entry>>>,
}

impl BookmarksPlugin {
    pub fn new(config: BookmarksConfig, platform: Arc<dyn PlatformProvider>) -> Self {
        Self::build(config, platform, None)
    }

    /// Reads the given browser folders instead of detecting them.
    pub fn with_roots(
        config: BookmarksConfig,
        platform: Arc<dyn PlatformProvider>,
        roots: Vec<BrowserRoot>,
    ) -> Self {
        Self::build(config, platform, Some(roots))
    }

    fn build(
        config: BookmarksConfig,
        platform: Arc<dyn PlatformProvider>,
        roots: Option<Vec<BrowserRoot>>,
    ) -> Self {
        let keyword = Some(config.keyword.trim().to_owned()).filter(|k| !k.is_empty());
        Self {
            config,
            keyword,
            platform,
            roots,
            cache: Mutex::new(HashMap::new()),
            index: RwLock::new(Arc::new(Vec::new())),
        }
    }

    fn snapshot(&self) -> Arc<Vec<Entry>> {
        self.index
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn set_index(&self, entries: Vec<Entry>) {
        *self
            .index
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Arc::new(entries);
    }

    fn selected(&self, root: &BrowserRoot) -> bool {
        let wanted: Vec<&str> = self
            .config
            .browsers
            .iter()
            .map(|b| b.trim())
            .filter(|b| !b.is_empty())
            .collect();
        wanted.is_empty()
            || wanted
                .iter()
                .any(|b| b.eq_ignore_ascii_case(root.id) || b.eq_ignore_ascii_case(root.name))
    }

    /// Every bookmark file of every selected browser, in a stable order.
    fn sources(&self) -> Vec<Source> {
        let roots = match &self.roots {
            Some(roots) => roots.clone(),
            None => self.platform.browser_roots(),
        };
        let mut sources: Vec<Source> = Vec::new();
        for root in roots.iter().filter(|root| self.selected(root)) {
            let files = match root.family {
                BrowserFamily::Chromium => chromium::bookmark_files(&root.dir),
                BrowserFamily::Firefox => firefox::places_files(&root.dir),
            };
            for path in files {
                if !sources.iter().any(|s| s.path == path) {
                    sources.push(Source {
                        browser: root.name,
                        family: root.family,
                        path,
                    });
                }
            }
        }
        sources
    }

    fn search(&self, input: &str) -> Vec<ResultItem> {
        let input = input.trim();
        if input.chars().count() < MIN_QUERY_CHARS {
            return Vec::new();
        }
        let mut query = FuzzyQuery::new(input);
        if query.is_empty() {
            return Vec::new();
        }
        let input_lower = input.to_lowercase();
        let index = self.snapshot();

        let mut scored: Vec<(f64, usize)> = Vec::new();
        for (i, entry) in index.iter().enumerate() {
            let base = match query.score(&entry.title) {
                Some(s) => f64::from(s),
                None => match query.score(&entry.haystack) {
                    Some(s) => f64::from(s) * URL_ONLY_WEIGHT,
                    None => continue,
                },
            };
            scored.push((base + name_bonus(entry, &input_lower), i));
        }

        let by_score_desc = |a: &(f64, usize), b: &(f64, usize)| b.0.total_cmp(&a.0);
        if scored.len() > MAX_CANDIDATES {
            scored.select_nth_unstable_by(MAX_CANDIDATES - 1, by_score_desc);
            scored.truncate(MAX_CANDIDATES);
        }
        scored.sort_by(by_score_desc);

        scored
            .into_iter()
            .map(|(score, i)| {
                let entry = &index[i];
                ResultItem::new(
                    "bookmarks",
                    &entry.key,
                    &entry.title,
                    Action::OpenUrl {
                        url: entry.url.clone(),
                    },
                )
                .with_subtitle(entry.subtitle())
                .with_icon(IconSource::builtin("web"))
                .with_score(score)
            })
            .collect()
    }
}

/// Bonus for a title or domain that starts with, or contains, the (lowercased)
/// input as written. Fuzzy scores fall with the length of the title, so a long
/// title that spells the word out would otherwise lose to a short scattered match.
fn name_bonus(entry: &Entry, input_lower: &str) -> f64 {
    if entry.title_lower.starts_with(input_lower) {
        TITLE_PREFIX_BONUS
    } else if entry.host.starts_with(input_lower) {
        HOST_PREFIX_BONUS
    } else if entry.title_lower.contains(input_lower) {
        TITLE_CONTAINS_BONUS
    } else if entry.host.contains(input_lower) {
        HOST_CONTAINS_BONUS
    } else {
        0.0
    }
}

/// Whether `url` may be opened: web pages only, and no control characters.
fn is_openable(url: &str) -> bool {
    let scheme_is = |scheme: &str| {
        url.get(..scheme.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme))
    };
    (scheme_is("http://") || scheme_is("https://")) && !url.chars().any(char::is_control)
}

/// The lowercase domain of `url` without `www.`, port or credentials.
fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let host = if authority.starts_with('[') {
        // IPv6 literal: keep the brackets, drop a port after them.
        authority.split_inclusive(']').next().unwrap_or(authority)
    } else {
        authority.split(':').next().unwrap_or(authority)
    };
    let host = host.to_lowercase();
    host.strip_prefix("www.").unwrap_or(&host).to_owned()
}

/// FNV-1a, 64 bit. Unlike `std`'s hashers it is specified and so identical on
/// every platform and Rust version, which result ids (usage statistics) need.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The stable result key of a bookmark: a hash of its URL, so the same page
/// keeps its usage history across browsers, profiles and restarts.
fn url_key(url: &str) -> String {
    format!("{:016x}", fnv1a(url.as_bytes()))
}

/// Merges the bookmarks of all sources: drops non-web URLs, keeps one entry per
/// URL (the first seen, so detection order decides the title and folder) and
/// records every browser that has it.
fn merge(sources: &[(&'static str, &[RawBookmark])]) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    let mut by_url: HashMap<String, usize> = HashMap::new();

    for (browser, bookmarks) in sources {
        for bookmark in *bookmarks {
            let url = bookmark.url.trim();
            if !is_openable(url) {
                continue;
            }
            if let Some(&i) = by_url.get(url) {
                if !entries[i].browsers.contains(browser) {
                    entries[i].browsers.push(browser);
                }
                continue;
            }
            if entries.len() >= MAX_BOOKMARKS {
                tracing::warn!(
                    limit = MAX_BOOKMARKS,
                    "bookmark index reached its size limit; the rest are not searchable"
                );
                return entries;
            }
            let host = host_of(url);
            let without_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
            let title = match bookmark.title.trim() {
                "" => without_scheme.to_owned(),
                title => title.to_owned(),
            };
            by_url.insert(url.to_owned(), entries.len());
            entries.push(Entry {
                key: url_key(url),
                title_lower: title.to_lowercase(),
                haystack: format!("{title} {without_scheme}"),
                title,
                url: url.to_owned(),
                folder: bookmark.folder.clone(),
                host,
                browsers: vec![*browser],
            });
        }
    }
    entries
}

impl Plugin for BookmarksPlugin {
    fn id(&self) -> &str {
        "bookmarks"
    }

    fn name(&self) -> &str {
        "Bookmarks"
    }

    fn description(&self) -> &str {
        "Finds bookmarks in your browsers (read from disk; nothing is sent anywhere)."
    }

    fn keyword(&self) -> Option<&str> {
        self.keyword.as_deref()
    }

    fn global(&self) -> bool {
        self.config.global || self.keyword.is_none()
    }

    fn query(&self, input: &str) -> Vec<ResultItem> {
        self.search(input)
    }

    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        execute_action(self.platform.as_ref(), &item.action)
    }

    fn refresh(&self) -> PluginResult<()> {
        let started = Instant::now();
        let sources = self.sources();

        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut previous = std::mem::take(&mut *cache);
        let mut reread = 0;
        let mut failed = 0;
        for source in &sources {
            let stamp = source.stamp();
            let old = previous.remove(&source.path);
            let entry = match old {
                Some(cached) if cached.stamp == stamp => cached,
                old => match source.read() {
                    Ok(bookmarks) => {
                        reread += 1;
                        CachedSource { stamp, bookmarks }
                    }
                    Err(err) => {
                        failed += 1;
                        tracing::warn!(
                            browser = source.browser,
                            file = %source.path.display(),
                            %err,
                            "could not read bookmarks"
                        );
                        // Keep what we had; the old stamp makes the next
                        // refresh try again.
                        old.unwrap_or(CachedSource {
                            stamp: Vec::new(),
                            bookmarks: Vec::new(),
                        })
                    }
                },
            };
            cache.insert(source.path.clone(), entry);
        }

        if reread == 0 && failed == 0 && previous.is_empty() {
            // Same files, same stamps: the index is already current.
            tracing::debug!(sources = sources.len(), "bookmarks unchanged");
            return Ok(());
        }

        let inputs: Vec<(&'static str, &[RawBookmark])> = sources
            .iter()
            .filter_map(|s| cache.get(&s.path).map(|c| (s.browser, &c.bookmarks[..])))
            .collect();
        let entries = merge(&inputs);
        tracing::info!(
            count = entries.len(),
            sources = sources.len(),
            reread,
            failed,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "bookmarks indexed"
        );
        drop(inputs);
        drop(cache);
        self.set_index(entries);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration;

    use super::*;
    use crate::test_util::MockPlatform;

    fn raw(title: &str, url: &str, folder: &str) -> RawBookmark {
        RawBookmark {
            title: title.into(),
            url: url.into(),
            folder: folder.into(),
        }
    }

    fn plugin_with(sources: &[(&'static str, Vec<RawBookmark>)]) -> BookmarksPlugin {
        let plugin = BookmarksPlugin::new(BookmarksConfig::default(), MockPlatform::empty());
        let inputs: Vec<(&'static str, &[RawBookmark])> =
            sources.iter().map(|(b, v)| (*b, &v[..])).collect();
        plugin.set_index(merge(&inputs));
        plugin
    }

    fn titles(plugin: &BookmarksPlugin, query: &str) -> Vec<String> {
        plugin.query(query).into_iter().map(|r| r.title).collect()
    }

    #[test]
    fn only_web_urls_are_openable() {
        for url in [
            "http://a.test/",
            "https://a.test/x?y=1#z",
            "HTTPS://A.TEST/",
        ] {
            assert!(is_openable(url), "{url}");
        }
        for url in [
            "javascript:alert(1)",
            "chrome://settings",
            "edge://flags",
            "about:blank",
            "file:///etc/passwd",
            "place:sort=8",
            "ftp://a.test/",
            "mailto:a@b.test",
            "data:text/html,hi",
            "https://a.test/\nx",
            "",
            "http:/",
        ] {
            assert!(!is_openable(url), "{url}");
        }
    }

    #[test]
    fn extracts_the_domain() {
        assert_eq!(host_of("https://www.Rust-Lang.org/learn"), "rust-lang.org");
        assert_eq!(host_of("http://user:pw@host.test:8080/x"), "host.test");
        assert_eq!(host_of("https://docs.rs?q=1"), "docs.rs");
        assert_eq!(host_of("https://[::1]:3000/x"), "[::1]");
        assert_eq!(host_of("https://localhost"), "localhost");
    }

    #[test]
    fn keys_are_stable_hashes_of_the_url() {
        // Published FNV-1a 64 test vectors: the key must never change.
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a(b"foobar"), 0x8594_4171_f739_67e8);
        assert_eq!(url_key("a"), "af63dc4c8601ec8c");
        assert_ne!(url_key("https://a.test/"), url_key("https://b.test/"));
    }

    #[test]
    fn identical_urls_merge_and_list_their_browsers() {
        let plugin = plugin_with(&[
            (
                "Chrome",
                vec![
                    raw("Rust", "https://www.rust-lang.org/", "Bookmarks bar"),
                    raw("Again", "https://www.rust-lang.org/", "Other bookmarks"),
                ],
            ),
            (
                "Firefox",
                vec![
                    raw("Rust Language", "https://www.rust-lang.org/", "Menu"),
                    raw("Docs", "https://docs.rs/", "Menu"),
                ],
            ),
            ("Edge", vec![raw("Rust", "https://www.rust-lang.org/", "")]),
        ]);
        let results = plugin.query("rust");
        assert_eq!(results.len(), 1, "{results:?}");
        let item = &results[0];
        assert_eq!(item.title, "Rust", "first browser wins");
        assert_eq!(
            item.subtitle,
            "Bookmarks bar · rust-lang.org · Chrome, Firefox, Edge"
        );
        assert_eq!(
            item.id,
            format!("bookmarks:{}", url_key("https://www.rust-lang.org/"))
        );
        assert_eq!(plugin.snapshot().len(), 2);
    }

    #[test]
    fn skips_non_web_bookmarks_and_titles_default_to_the_url() {
        let plugin = plugin_with(&[(
            "Chrome",
            vec![
                raw("Evil", "javascript:alert(1)", ""),
                raw("Settings", "chrome://settings", ""),
                raw("", "https://untitled.test/page", "Bar"),
            ],
        )]);
        assert!(plugin.query("evil").is_empty());
        assert!(plugin.query("settings").is_empty());
        let results = plugin.query("untitled");
        assert_eq!(results[0].title, "untitled.test/page");
        assert_eq!(
            results[0].action,
            Action::OpenUrl {
                url: "https://untitled.test/page".into()
            }
        );
    }

    #[test]
    fn matches_title_domain_and_url_and_ranks_titles_first() {
        let plugin = plugin_with(&[(
            "Chrome",
            vec![
                raw("Pull requests", "https://github.com/pulls", ""),
                raw("Hacker News", "https://news.ycombinator.com/", ""),
                raw("GitHub", "https://github.com/", ""),
                raw("My notes", "https://example.com/github-notes", ""),
            ],
        )]);
        let found = titles(&plugin, "github");
        assert_eq!(found[0], "GitHub", "{found:?}");
        assert!(found.contains(&"Pull requests".to_owned()), "domain match");
        assert!(found.contains(&"My notes".to_owned()), "URL path match");
        assert!(!found.contains(&"Hacker News".to_owned()));
        // The title match outranks the URL-only matches.
        let notes = found.iter().position(|t| t == "My notes").unwrap();
        assert!(notes > 0);

        let found = titles(&plugin, "ycombinator");
        assert_eq!(found, ["Hacker News"]);
        // Terms may be spread over title and domain.
        assert_eq!(titles(&plugin, "pull github"), ["Pull requests"]);
    }

    #[test]
    fn a_domain_that_starts_with_the_input_beats_a_path_match() {
        let plugin = plugin_with(&[(
            "Chrome",
            vec![
                raw("Blog", "https://blog.test/crates-are-great", ""),
                raw("Registry", "https://crates.io/", ""),
            ],
        )]);
        assert_eq!(titles(&plugin, "crates"), ["Registry", "Blog"]);
    }

    #[test]
    fn short_queries_and_an_unindexed_plugin_return_nothing() {
        let plugin = plugin_with(&[("Chrome", vec![raw("Rust", "https://rust.test/", "")])]);
        assert!(plugin.query("").is_empty());
        assert!(plugin.query(" r ").is_empty());
        assert!(!plugin.query("ru").is_empty());
        let fresh = BookmarksPlugin::new(BookmarksConfig::default(), MockPlatform::empty());
        assert!(fresh.query("rust").is_empty());
    }

    #[test]
    fn at_most_fifty_candidates_best_first() {
        let many: Vec<RawBookmark> = (0..120)
            .map(|i| raw(&format!("Doc {i:03}"), &format!("https://d.test/{i}"), ""))
            .collect();
        let plugin = plugin_with(&[("Chrome", many)]);
        let results = plugin.query("doc");
        assert_eq!(results.len(), MAX_CANDIDATES);
        assert!(results.windows(2).all(|w| w[0].score >= w[1].score));
    }

    #[test]
    fn result_shape() {
        let plugin = plugin_with(&[(
            "Brave",
            vec![raw("Crates", "https://crates.io/", "Bookmarks bar / Dev")],
        )]);
        let item = &plugin.query("crates")[0];
        assert_eq!(item.plugin_id, "bookmarks");
        assert_eq!(item.subtitle, "Bookmarks bar / Dev · crates.io · Brave");
        assert_eq!(item.icon, Some(IconSource::builtin("web")));
    }

    #[test]
    fn keyword_and_global_follow_config() {
        let platform = MockPlatform::empty();
        let plugin = BookmarksPlugin::new(BookmarksConfig::default(), platform.clone());
        assert_eq!(plugin.id(), "bookmarks");
        assert_eq!(plugin.name(), "Bookmarks");
        assert_eq!(plugin.keyword(), Some("b"));
        assert!(plugin.global());

        let config = BookmarksConfig {
            global: false,
            ..BookmarksConfig::default()
        };
        assert!(!BookmarksPlugin::new(config, platform.clone()).global());

        let config = BookmarksConfig {
            keyword: " ".into(),
            global: false,
            ..BookmarksConfig::default()
        };
        let plugin = BookmarksPlugin::new(config, platform);
        assert_eq!(plugin.keyword(), None);
        assert!(
            plugin.global(),
            "without a keyword the plugin must be global"
        );
    }

    #[test]
    fn execute_opens_the_url() {
        let platform = MockPlatform::empty();
        let plugin = BookmarksPlugin::new(BookmarksConfig::default(), platform.clone());
        let rust = [raw("Rust", "https://www.rust-lang.org/", "")];
        plugin.set_index(merge(&[("Chrome", &rust[..])]));
        let results = plugin.query("rust");
        plugin.execute(&results[0]).unwrap();
        assert_eq!(
            *platform.opened_urls.lock().unwrap(),
            ["https://www.rust-lang.org/"]
        );
    }

    // --- refresh against real files -------------------------------------

    fn chromium_json(urls: &[(&str, &str)]) -> String {
        let children: Vec<String> = urls
            .iter()
            .map(|(title, url)| format!(r#"{{"type":"url","name":"{title}","url":"{url}"}}"#))
            .collect();
        format!(
            r#"{{"roots":{{"bookmark_bar":{{"type":"folder","name":"Bookmarks bar","children":[{}]}}}}}}"#,
            children.join(",")
        )
    }

    fn write_profile(root: &Path, profile: &str, json: &str) -> PathBuf {
        let dir = root.join(profile);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Bookmarks");
        fs::write(&file, json).unwrap();
        file
    }

    fn set_mtime(file: &Path, mtime: SystemTime) {
        fs::OpenOptions::new()
            .write(true)
            .open(file)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
    }

    fn chrome_root(dir: &Path) -> BrowserRoot {
        BrowserRoot {
            id: "chrome",
            name: "Chrome",
            family: BrowserFamily::Chromium,
            dir: dir.to_path_buf(),
        }
    }

    fn firefox_root(dir: &Path) -> BrowserRoot {
        BrowserRoot {
            id: "firefox",
            name: "Firefox",
            family: BrowserFamily::Firefox,
            dir: dir.to_path_buf(),
        }
    }

    fn firefox_profile(root: &Path, urls: &[(&str, &str)]) {
        let dir = root.join("Profiles").join("abc.default-release");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            root.join("profiles.ini"),
            "[Profile0]\nName=default\nIsRelative=1\nPath=Profiles/abc.default-release\n",
        )
        .unwrap();
        let conn = firefox::tests::create_places(&dir.join(firefox::PLACES));
        for (i, (title, url)) in urls.iter().enumerate() {
            firefox::tests::add_bookmark(&conn, 100 + i as i64, 3, Some(title), url);
        }
    }

    #[test]
    fn indexes_all_profiles_of_all_browsers() {
        let chrome = tempfile::tempdir().unwrap();
        write_profile(
            chrome.path(),
            "Default",
            &chromium_json(&[("Rust", "https://www.rust-lang.org/")]),
        );
        write_profile(
            chrome.path(),
            "Profile 1",
            &chromium_json(&[("Work wiki", "https://wiki.corp.test/")]),
        );
        let firefox = tempfile::tempdir().unwrap();
        firefox_profile(
            firefox.path(),
            &[
                ("Rust Language", "https://www.rust-lang.org/"),
                ("Docs.rs", "https://docs.rs/"),
            ],
        );

        let plugin = BookmarksPlugin::with_roots(
            BookmarksConfig::default(),
            MockPlatform::empty(),
            vec![chrome_root(chrome.path()), firefox_root(firefox.path())],
        );
        plugin.refresh().unwrap();

        assert_eq!(plugin.snapshot().len(), 3);
        assert_eq!(titles(&plugin, "wiki"), ["Work wiki"]);
        let rust = plugin.query("rust-lang");
        assert_eq!(rust.len(), 1);
        assert_eq!(
            rust[0].subtitle,
            "Bookmarks bar · rust-lang.org · Chrome, Firefox"
        );
        assert_eq!(titles(&plugin, "docs"), ["Docs.rs"]);
    }

    #[test]
    fn browsers_option_limits_the_sources() {
        let chrome = tempfile::tempdir().unwrap();
        write_profile(
            chrome.path(),
            "Default",
            &chromium_json(&[("Chrome only", "https://chrome.test/")]),
        );
        let firefox = tempfile::tempdir().unwrap();
        firefox_profile(firefox.path(), &[("Firefox only", "https://firefox.test/")]);
        let roots = vec![chrome_root(chrome.path()), firefox_root(firefox.path())];

        let only = |browsers: &[&str]| {
            let config = BookmarksConfig {
                browsers: browsers.iter().map(|b| (*b).to_owned()).collect(),
                ..BookmarksConfig::default()
            };
            let plugin = BookmarksPlugin::with_roots(config, MockPlatform::empty(), roots.clone());
            plugin.refresh().unwrap();
            let mut found: Vec<String> =
                plugin.snapshot().iter().map(|e| e.title.clone()).collect();
            found.sort();
            found
        };
        assert_eq!(only(&[]), ["Chrome only", "Firefox only"]);
        assert_eq!(only(&["firefox"]), ["Firefox only"]);
        assert_eq!(only(&[" Chrome "]), ["Chrome only"]);
        assert_eq!(only(&["firefox", "CHROME"]).len(), 2);
        assert!(only(&["brave"]).is_empty());
    }

    #[test]
    fn refresh_rereads_only_changed_files() {
        let chrome = tempfile::tempdir().unwrap();
        let file = write_profile(
            chrome.path(),
            "Default",
            &chromium_json(&[("Alpha", "https://alpha.test/")]),
        );
        let original = fs::metadata(&file).unwrap().modified().unwrap();
        let plugin = BookmarksPlugin::with_roots(
            BookmarksConfig::default(),
            MockPlatform::empty(),
            vec![chrome_root(chrome.path())],
        );
        plugin.refresh().unwrap();
        assert_eq!(titles(&plugin, "alpha"), ["Alpha"]);

        // Nothing changed: the index is not even rebuilt.
        let before = plugin.snapshot();
        plugin.refresh().unwrap();
        assert!(Arc::ptr_eq(&before, &plugin.snapshot()));

        // Same size, same mtime: the file is not read again.
        fs::write(&file, chromium_json(&[("Omega", "https://alpha.test/")])).unwrap();
        set_mtime(&file, original);
        plugin.refresh().unwrap();
        assert_eq!(titles(&plugin, "alpha"), ["Alpha"], "served from the cache");

        // A newer mtime triggers a re-read.
        set_mtime(&file, original + Duration::from_secs(30));
        plugin.refresh().unwrap();
        assert_eq!(titles(&plugin, "omega"), ["Omega"]);
        // The URL still contains "alpha", but the old title is gone.
        assert_eq!(titles(&plugin, "alpha"), ["Omega"]);
    }

    #[test]
    fn refresh_notices_new_removed_and_broken_sources() {
        let chrome = tempfile::tempdir().unwrap();
        let first = write_profile(
            chrome.path(),
            "Default",
            &chromium_json(&[("Alpha", "https://alpha.test/")]),
        );
        let plugin = BookmarksPlugin::with_roots(
            BookmarksConfig::default(),
            MockPlatform::empty(),
            vec![chrome_root(chrome.path())],
        );
        plugin.refresh().unwrap();

        // A second profile appears.
        write_profile(
            chrome.path(),
            "Profile 3",
            &chromium_json(&[("Beta", "https://beta.test/")]),
        );
        plugin.refresh().unwrap();
        assert_eq!(titles(&plugin, "beta"), ["Beta"]);

        // A file caught mid-write keeps its previous contents...
        fs::write(&first, "{ this is not json").unwrap();
        set_mtime(&first, SystemTime::now() + Duration::from_secs(60));
        plugin.refresh().unwrap();
        assert_eq!(titles(&plugin, "alpha"), ["Alpha"]);

        // ...and a deleted profile disappears.
        fs::remove_file(&first).unwrap();
        plugin.refresh().unwrap();
        assert!(titles(&plugin, "alpha").is_empty());
        assert_eq!(titles(&plugin, "beta"), ["Beta"]);
    }

    #[test]
    fn no_browsers_means_no_results_and_no_errors() {
        let plugin = BookmarksPlugin::with_roots(
            BookmarksConfig::default(),
            MockPlatform::empty(),
            vec![chrome_root(Path::new("definitely-not-here"))],
        );
        plugin.refresh().unwrap();
        assert!(plugin.query("anything").is_empty());

        let plugin = BookmarksPlugin::new(BookmarksConfig::default(), MockPlatform::empty());
        plugin.refresh().unwrap();
    }

    #[test]
    fn firefox_changes_in_the_wal_trigger_a_reread() {
        let firefox = tempfile::tempdir().unwrap();
        firefox_profile(firefox.path(), &[("Old", "https://old.test/")]);
        let source = Source {
            browser: "Firefox",
            family: BrowserFamily::Firefox,
            path: firefox
                .path()
                .join("Profiles")
                .join("abc.default-release")
                .join(firefox::PLACES),
        };
        let before = source.stamp();
        assert_eq!(before.len(), 2);
        assert!(before[0].is_some());
        fs::write(firefox::wal_file(&source.path), b"wal").unwrap();
        assert_ne!(source.stamp(), before);
    }
}
