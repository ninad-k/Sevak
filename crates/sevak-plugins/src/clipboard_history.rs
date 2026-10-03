//! Clipboard history: `cb <text>` lists what you copied, newest first, and
//! Enter pastes the chosen text into the app you were in.
//!
//! # Opt-in and private
//!
//! Nothing is watched or stored unless `[clipboard] enabled = true`. While it
//! is off the plugin still exists (so `cb` explains how to turn it on), but no
//! thread runs and no file is read or written.
//!
//! When on, a background thread notices clipboard changes and keeps the text in
//! `clipboard-history.json` in Sevak's data folder (readable by the user only
//! on Unix). It records **text only**, and skips:
//!
//! - content the copying app marked secret (see
//!   [`sevak_platform::ClipboardRead::sensitive`]: password managers set these
//!   markers on Windows and macOS);
//! - copies made while an app from `[clipboard] ignore_apps` had focus;
//! - text longer than `max_item_bytes`, empty and whitespace-only text;
//! - anything Sevak itself put on the clipboard (a paste or a copy it made).
//!
//! The clipboard as it is when monitoring starts is not recorded: only what is
//! copied afterwards.
//!
//! # Threads
//!
//! The history lives in a [`Shared`] that the monitor thread and the plugin
//! both hold. Plugins are rebuilt on every config reload, so a process-wide
//! table hands the new plugin the same `Shared` (and thus the same monitor
//! thread) while the old one is still alive. The thread holds only a `Weak`
//! and exits once the last plugin using it is dropped. The thread is started by
//! [`Plugin::refresh`], never by the constructor, because the settings window
//! also constructs plugins just to read their names.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sevak_core::config::{ClipboardConfig, PasteConfig};
use sevak_core::model::score;
use sevak_core::{Action, FuzzyQuery, IconSource, Plugin, PluginResult, ResultItem};
use sevak_platform::{AppPaths, ClipboardRead, PasteSupport, PlatformProvider};

use crate::actions::execute_action;

/// The keyword that routes a query to this plugin.
pub const KEYWORD: &str = "cb";
/// The history file inside Sevak's data folder.
pub const FILE_NAME: &str = "clipboard-history.json";

/// How often the clipboard is checked. Windows and macOS have a change counter
/// (a few nanoseconds to read); on X11 the text itself is compared.
const POLL_INTERVAL: Duration = Duration::from_millis(300);
/// Most rows one query returns (the engine shows at most `[search] max_results`,
/// itself capped at 20).
const MAX_ROWS: usize = 20;
/// Only this many characters of each entry are fuzzy-matched, so a query stays
/// fast even with 200 large entries.
const SEARCH_CHARS: usize = 1_000;
const TITLE_CHARS: usize = 100;

const CLEAR_TITLE: &str = "Clear clipboard history";
const PAYLOAD_CLEAR: &str = "clear";
const PAYLOAD_NOTHING: &str = "nothing";
const ENABLE_SNIPPET: &str = "[clipboard]\nenabled = true";

/// Where the history is stored by default.
pub fn default_history_path() -> Option<PathBuf> {
    AppPaths::resolve()
        .ok()
        .map(|paths| paths.data_dir.join(FILE_NAME))
}

/// One recorded copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub text: String,
    /// Seconds since the Unix epoch.
    pub copied_at: u64,
    /// The app that was focused when it was copied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Deserialize)]
struct StoredHistory {
    #[allow(dead_code)]
    version: u32,
    items: Vec<Entry>,
}

#[derive(Serialize)]
struct StoredHistoryRef<'a> {
    version: u32,
    items: Vec<&'a Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Settings {
    max_items: usize,
    max_item_bytes: usize,
    ignore_apps: Vec<String>,
}

impl From<&ClipboardConfig> for Settings {
    fn from(config: &ClipboardConfig) -> Self {
        Self {
            max_items: config.max_items,
            max_item_bytes: config.max_item_bytes,
            ignore_apps: config.ignore_apps.clone(),
        }
    }
}

/// Entries, newest first. `Arc`s so a query can take a cheap snapshot.
#[derive(Default)]
struct State {
    items: Vec<Arc<Entry>>,
    loaded: bool,
    settings: Option<Settings>,
}

impl State {
    fn push(&mut self, entry: Entry, max_items: usize) {
        self.items.retain(|existing| existing.text != entry.text);
        self.items.insert(0, Arc::new(entry));
        self.items.truncate(max_items);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The history plus the monitor thread's life cycle.
struct Shared {
    path: Option<PathBuf>,
    platform: Arc<dyn PlatformProvider>,
    state: Mutex<State>,
    /// Serializes saves, so the last one to run writes the newest state.
    write: Mutex<()>,
    monitor_started: AtomicBool,
}

type Live = Vec<(PathBuf, Weak<Shared>)>;

/// The `Shared` of every history file currently in use (see the module docs).
static LIVE: Mutex<Live> = Mutex::new(Vec::new());

impl Shared {
    fn acquire(path: Option<PathBuf>, platform: Arc<dyn PlatformProvider>) -> Arc<Self> {
        let Some(file) = path.clone() else {
            return Arc::new(Self::new(None, platform));
        };
        let mut live = lock(&LIVE);
        live.retain(|(_, shared)| shared.strong_count() > 0);
        if let Some(existing) = live
            .iter()
            .find(|(live_path, _)| *live_path == file)
            .and_then(|(_, shared)| shared.upgrade())
        {
            return existing;
        }
        let shared = Arc::new(Self::new(path, platform));
        live.push((file, Arc::downgrade(&shared)));
        shared
    }

    fn new(path: Option<PathBuf>, platform: Arc<dyn PlatformProvider>) -> Self {
        Self {
            path,
            platform,
            state: Mutex::new(State::default()),
            write: Mutex::new(()),
            monitor_started: AtomicBool::new(false),
        }
    }

    fn snapshot(&self) -> Vec<Arc<Entry>> {
        lock(&self.state).items.clone()
    }

    /// Applies new settings (after a config reload), trimming the history to a
    /// smaller `max_items`.
    fn configure(&self, settings: Settings) {
        let trimmed = {
            let mut state = lock(&self.state);
            let trimmed = state.items.len() > settings.max_items;
            state.items.truncate(settings.max_items);
            state.settings = Some(settings);
            trimmed
        };
        if trimmed {
            self.persist();
        }
    }

    /// Reads the history file once.
    fn load(&self) {
        let mut state = lock(&self.state);
        if state.loaded {
            return;
        }
        state.loaded = true;
        if let Some(path) = &self.path {
            let max_items = state.settings.as_ref().map_or(usize::MAX, |s| s.max_items);
            state.items = read_history(path)
                .into_iter()
                .take(max_items)
                .map(Arc::new)
                .collect();
            tracing::info!(items = state.items.len(), "clipboard history loaded");
        }
    }

    fn clear(&self) {
        lock(&self.state).items.clear();
        self.persist();
    }

    /// Writes the history to disk (outside the state lock; serialization of a
    /// full history takes a few milliseconds).
    fn persist(&self) {
        let Some(path) = &self.path else { return };
        let _writing = lock(&self.write);
        let items = self.snapshot();
        let stored = StoredHistoryRef {
            version: 1,
            items: items.iter().map(|entry| &**entry).collect(),
        };
        let result = serde_json::to_vec(&stored)
            .map_err(std::io::Error::other)
            .and_then(|bytes| sevak_platform::private_file::write_atomic(path, &bytes));
        if let Err(err) = result {
            tracing::warn!("could not save the clipboard history: {err}");
        }
    }

    /// One pass of the monitor: records a copy if there is a new, acceptable one.
    fn tick(&self, monitor: &mut Monitor) {
        let Some(settings) = lock(&self.state).settings.clone() else {
            return;
        };
        let Some(captured) = monitor.poll(self.platform.as_ref(), &settings) else {
            return;
        };
        lock(&self.state).push(
            Entry {
                text: captured.text,
                copied_at: now_secs(),
                source: captured.source,
            },
            settings.max_items,
        );
        self.persist();
    }

    fn ensure_monitor(self: &Arc<Self>) {
        if self.monitor_started.swap(true, Ordering::SeqCst) {
            return;
        }
        let weak = Arc::downgrade(self);
        let spawned = std::thread::Builder::new()
            .name("sevak-clipboard".to_owned())
            .spawn(move || run_monitor(&weak));
        if let Err(err) = spawned {
            self.monitor_started.store(false, Ordering::SeqCst);
            tracing::error!("could not start the clipboard monitor: {err}");
        } else {
            tracing::info!("clipboard history is recording");
        }
    }
}

fn run_monitor(shared: &Weak<Shared>) {
    let mut monitor = Monitor::default();
    loop {
        // Upgrade per pass and let go before sleeping, so dropping the last
        // plugin ends the thread within one interval.
        let Some(strong) = shared.upgrade() else {
            tracing::info!("clipboard history stopped");
            return;
        };
        strong.tick(&mut monitor);
        drop(strong);
        sleep(POLL_INTERVAL);
    }
}

/// A copy the monitor decided to keep.
#[derive(Debug, PartialEq, Eq)]
struct Captured {
    text: String,
    source: Option<String>,
}

/// Detects clipboard changes between polls.
#[derive(Default)]
struct Monitor {
    /// The first look only records where the clipboard is.
    primed: bool,
    last_sequence: Option<u64>,
    /// Used instead of the sequence number on systems without one.
    last_text_hash: Option<u64>,
}

impl Monitor {
    fn poll(&mut self, platform: &dyn PlatformProvider, settings: &Settings) -> Option<Captured> {
        let sequence = platform.clipboard_sequence();
        if let Some(sequence) = sequence {
            if self.last_sequence == Some(sequence) {
                return None;
            }
            if !self.primed {
                // Do not even read what was on the clipboard before we started.
                self.primed = true;
                self.last_sequence = Some(sequence);
                return None;
            }
        }

        let read = match platform.read_clipboard() {
            Ok(read) => read,
            Err(err) => {
                // Not recorded as seen: the next pass tries again.
                tracing::debug!("clipboard not readable: {err}");
                return None;
            }
        };

        match sequence {
            Some(sequence) => self.last_sequence = Some(sequence),
            None => {
                let hash = read.text.as_deref().map(fnv1a);
                if hash == self.last_text_hash {
                    return None;
                }
                self.last_text_hash = hash;
                if !self.primed {
                    self.primed = true;
                    return None;
                }
            }
        }
        accept(read, platform, settings)
    }
}

/// Applies the privacy rules to a clipboard change.
fn accept(
    read: ClipboardRead,
    platform: &dyn PlatformProvider,
    settings: &Settings,
) -> Option<Captured> {
    if read.sensitive {
        return None;
    }
    let text = read.text?;
    if text.trim().is_empty() || text.len() > settings.max_item_bytes {
        return None;
    }
    if sevak_platform::clipboard::take_own_write(&text) {
        return None;
    }
    let app = platform.foreground_app();
    if app
        .as_ref()
        .is_some_and(|app| app.matches_any(&settings.ignore_apps))
    {
        return None;
    }
    Some(Captured {
        text,
        source: app.map(|app| app.name),
    })
}

/// Reads the history file. A missing file is an empty history; an unreadable
/// one is moved aside (never overwritten silently) and also yields an empty one.
fn read_history(path: &Path) -> Vec<Entry> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(err) => {
            tracing::warn!("could not read {}: {err}", path.display());
            return Vec::new();
        }
    };
    match serde_json::from_slice::<StoredHistory>(&bytes) {
        Ok(stored) => stored.items,
        Err(err) => {
            let mut aside = path.as_os_str().to_owned();
            aside.push(".corrupt");
            tracing::warn!(
                "{} is not a valid clipboard history ({err}); moving it aside",
                path.display()
            );
            let _ = fs::rename(path, aside);
            Vec::new()
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// FNV-1a: a stable hash, used for result keys (which must survive restarts)
/// and for noticing that the clipboard text changed.
fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// "just now", "5 min ago", "3 h ago", "2 d ago".
fn relative_time(now: u64, then: u64) -> String {
    let seconds = now.saturating_sub(then);
    match seconds {
        0..=59 => "just now".to_owned(),
        60..=3_599 => format!("{} min ago", seconds / 60),
        3_600..=86_399 => format!("{} h ago", seconds / 3_600),
        _ => format!("{} d ago", seconds / 86_400),
    }
}

/// The first non-blank line, trimmed and shortened with an ellipsis.
fn preview(text: &str, max_chars: usize) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    let mut shown: String = line.chars().take(max_chars).collect();
    if line.chars().count() > max_chars {
        shown.push('…');
    }
    shown.replace('\t', " ")
}

/// The part of `text` that queries are matched against.
fn searchable(text: &str) -> &str {
    match text.char_indices().nth(SEARCH_CHARS) {
        Some((end, _)) => &text[..end],
        None => text,
    }
}

/// The `cb` plugin.
pub struct ClipboardPlugin {
    settings: Settings,
    restore_clipboard: bool,
    platform: Arc<dyn PlatformProvider>,
    /// `None` while disabled.
    shared: Option<Arc<Shared>>,
}

impl ClipboardPlugin {
    /// Builds the plugin. Cheap: nothing is read and no thread starts until
    /// [`Plugin::refresh`]. `history_file` is where the history is kept
    /// ([`default_history_path`]); `None` keeps it in memory only.
    pub fn new(
        config: &ClipboardConfig,
        paste: &PasteConfig,
        platform: Arc<dyn PlatformProvider>,
        history_file: Option<PathBuf>,
    ) -> Self {
        let shared = config
            .enabled
            .then(|| Shared::acquire(history_file, platform.clone()));
        Self {
            settings: Settings::from(config),
            restore_clipboard: paste.restore_clipboard,
            platform,
            shared,
        }
    }

    fn rows(&self, input: &str, now: u64) -> Vec<ResultItem> {
        let Some(shared) = &self.shared else {
            return vec![ResultItem::new(
                self.id(),
                "off",
                "Clipboard history is off",
                Action::CopyText {
                    text: ENABLE_SNIPPET.to_owned(),
                },
            )
            .with_subtitle(
                "Enter copies the setting to add to config.toml; then choose Reload index",
            )
            .with_icon(IconSource::builtin("copy"))
            .with_score(score::KEYWORD)];
        };

        let input = input.trim();
        let entries = shared.snapshot();
        let support = self.platform.paste_support();

        // (position in the history, match score, entry)
        let mut ranked: Vec<(usize, f64, &Arc<Entry>)> = Vec::new();
        if input.is_empty() {
            ranked.extend(entries.iter().enumerate().map(|(i, entry)| (i, 0.0, entry)));
        } else {
            let mut query = FuzzyQuery::new(input);
            ranked.extend(entries.iter().enumerate().filter_map(|(i, entry)| {
                let matched = query.score(searchable(&entry.text))?;
                Some((i, f64::from(matched), entry))
            }));
        }

        let total = entries.len();
        let mut rows: Vec<ResultItem> = ranked
            .into_iter()
            .map(|(position, matched, entry)| {
                // Equal fuzzy scores keep the newest first.
                let recency = (total - position) as f64 * 0.001;
                self.row(entry, &support, now)
                    .with_score(score::KEYWORD + matched + recency)
            })
            .collect();
        rows.sort_by(|a, b| b.score.total_cmp(&a.score));
        rows.truncate(MAX_ROWS);

        if rows.is_empty() && input.is_empty() {
            rows.push(
                ResultItem::new(
                    self.id(),
                    "empty",
                    "Clipboard history is empty",
                    Action::Custom {
                        payload: PAYLOAD_NOTHING.to_owned(),
                    },
                )
                .with_subtitle("Text you copy from now on shows up here")
                .with_icon(IconSource::builtin("copy"))
                .with_score(score::KEYWORD),
            );
        }

        // Always the last row, never the default one: Enter on a stray `cb cle`
        // must not wipe the history.
        if offers_clear(input) {
            rows.push(
                ResultItem::new(
                    self.id(),
                    PAYLOAD_CLEAR,
                    CLEAR_TITLE,
                    Action::Custom {
                        payload: PAYLOAD_CLEAR.to_owned(),
                    },
                )
                .with_subtitle(format!("Deletes all {total} recorded items"))
                .with_icon(IconSource::builtin("copy"))
                .with_score(score::KEYWORD - 1.0),
            );
        }
        rows
    }

    fn row(&self, entry: &Entry, support: &PasteSupport, now: u64) -> ResultItem {
        let text = entry.text.clone();
        let (action, hint) = match support {
            PasteSupport::Available => (
                Action::PasteText {
                    text,
                    restore_clipboard: self.restore_clipboard,
                },
                "Enter to paste".to_owned(),
            ),
            PasteSupport::CopyOnly(reason) => (
                Action::CopyText { text },
                format!("Copies to clipboard · {reason}"),
            ),
        };

        let mut parts = vec![relative_time(now, entry.copied_at)];
        if let Some(source) = &entry.source {
            parts.push(source.clone());
        }
        let lines = entry.text.lines().count();
        if lines > 1 {
            parts.push(format!("{lines} lines"));
        }
        parts.push(hint);

        ResultItem::new(
            self.id(),
            format!("{:016x}", fnv1a(&entry.text)),
            preview(&entry.text, TITLE_CHARS),
            action,
        )
        .with_subtitle(parts.join(" · "))
        .with_icon(IconSource::builtin("copy"))
    }
}

/// Whether to list the "clear" row: the input is the start of its title and
/// long enough not to pop up for every `c`.
fn offers_clear(input: &str) -> bool {
    let input = input.trim().to_lowercase();
    input.chars().count() >= 3 && CLEAR_TITLE.to_lowercase().starts_with(&input)
}

impl Plugin for ClipboardPlugin {
    fn id(&self) -> &str {
        "clipboard"
    }

    fn name(&self) -> &str {
        "Clipboard history"
    }

    fn description(&self) -> &str {
        "Type `cb` to paste text you copied earlier. Off until [clipboard] enabled = true."
    }

    fn keyword(&self) -> Option<&str> {
        Some(KEYWORD)
    }

    fn global(&self) -> bool {
        false
    }

    fn query(&self, input: &str) -> Vec<ResultItem> {
        self.rows(input, now_secs())
    }

    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        match &item.action {
            Action::Custom { payload } if payload == PAYLOAD_CLEAR => {
                if let Some(shared) = &self.shared {
                    shared.clear();
                    tracing::info!("clipboard history cleared");
                }
                Ok(())
            }
            Action::Custom { payload } if payload == PAYLOAD_NOTHING => Ok(()),
            action => execute_action(self.platform.as_ref(), action),
        }
    }

    /// Loads the saved history and starts recording (only when enabled).
    fn refresh(&self) -> PluginResult<()> {
        let Some(shared) = &self.shared else {
            return Ok(());
        };
        shared.configure(self.settings.clone());
        shared.load();
        shared.ensure_monitor();
        // Warm the (cached) answer so the first `cb` query does not pay for it.
        let _ = self.platform.paste_support();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sevak_platform::ForegroundApp;

    use super::*;
    use crate::test_util::MockPlatform;

    fn settings() -> Settings {
        Settings {
            max_items: 200,
            max_item_bytes: 1_000,
            ignore_apps: vec!["KeePassXC".into()],
        }
    }

    fn read(text: &str) -> ClipboardRead {
        ClipboardRead {
            text: Some(text.to_owned()),
            sensitive: false,
        }
    }

    /// A monitor that has seen the clipboard once, at sequence 1.
    fn primed_monitor(platform: &MockPlatform) -> Monitor {
        *platform.clipboard_sequence.lock().unwrap() = Some(1);
        let mut monitor = Monitor::default();
        assert_eq!(monitor.poll(platform, &settings()), None);
        monitor
    }

    fn copy(platform: &MockPlatform, sequence: u64, text: &str) {
        *platform.clipboard_sequence.lock().unwrap() = Some(sequence);
        *platform.clipboard_read.lock().unwrap() = Some(read(text));
    }

    #[test]
    fn what_was_on_the_clipboard_before_monitoring_is_not_recorded() {
        let platform = MockPlatform::empty();
        copy(&platform, 7, "already there");
        let mut monitor = Monitor::default();
        assert_eq!(monitor.poll(&*platform, &settings()), None);
        // And it is not re-read while nothing changes.
        *platform.clipboard_read.lock().unwrap() = None;
        assert_eq!(monitor.poll(&*platform, &settings()), None);
    }

    #[test]
    fn a_new_copy_is_recorded_with_its_source_app() {
        let platform = MockPlatform::empty();
        let mut monitor = primed_monitor(&platform);
        *platform.foreground.lock().unwrap() = Some(ForegroundApp::new("Code"));
        copy(&platform, 2, "fn main() {}");

        let captured = monitor.poll(&*platform, &settings()).unwrap();
        assert_eq!(captured.text, "fn main() {}");
        assert_eq!(captured.source.as_deref(), Some("Code"));
        // The same sequence number is not recorded twice.
        assert_eq!(monitor.poll(&*platform, &settings()), None);
    }

    #[test]
    fn secret_content_is_never_recorded() {
        let platform = MockPlatform::empty();
        let mut monitor = primed_monitor(&platform);
        *platform.clipboard_sequence.lock().unwrap() = Some(2);
        *platform.clipboard_read.lock().unwrap() = Some(ClipboardRead {
            text: Some("hunter2".into()),
            sensitive: true,
        });
        assert_eq!(monitor.poll(&*platform, &settings()), None);
    }

    #[test]
    fn copies_from_ignored_apps_are_skipped_case_insensitively() {
        let platform = MockPlatform::empty();
        let mut monitor = primed_monitor(&platform);
        *platform.foreground.lock().unwrap() =
            Some(ForegroundApp::new("keepassxc").with_identifier("KeePassXC.exe"));
        copy(&platform, 2, "a password");
        assert_eq!(monitor.poll(&*platform, &settings()), None);

        *platform.foreground.lock().unwrap() = Some(ForegroundApp::new("Notepad"));
        copy(&platform, 3, "a note");
        assert!(monitor.poll(&*platform, &settings()).is_some());
    }

    #[test]
    fn empty_blank_and_oversized_text_is_skipped() {
        let platform = MockPlatform::empty();
        let mut monitor = primed_monitor(&platform);
        copy(&platform, 2, "   \n\t ");
        assert_eq!(monitor.poll(&*platform, &settings()), None);
        copy(&platform, 3, &"x".repeat(1_001));
        assert_eq!(monitor.poll(&*platform, &settings()), None);
        copy(&platform, 4, &"x".repeat(1_000));
        assert!(monitor.poll(&*platform, &settings()).is_some());
        // Something that is not text.
        *platform.clipboard_sequence.lock().unwrap() = Some(5);
        *platform.clipboard_read.lock().unwrap() = Some(ClipboardRead::default());
        assert_eq!(monitor.poll(&*platform, &settings()), None);
    }

    #[test]
    fn text_sevak_put_on_the_clipboard_itself_is_skipped_once() {
        let platform = MockPlatform::empty();
        let mut monitor = primed_monitor(&platform);
        sevak_platform::clipboard::note_own_write("pasted by sevak 7f3a");
        copy(&platform, 2, "pasted by sevak 7f3a");
        assert_eq!(monitor.poll(&*platform, &settings()), None);
        // Copying the same text by hand afterwards is a real copy.
        copy(&platform, 3, "pasted by sevak 7f3a");
        assert!(monitor.poll(&*platform, &settings()).is_some());
    }

    /// The real clipboard and sequence number (restores the user's text).
    #[cfg(windows)]
    #[test]
    fn the_real_clipboard_is_watched() {
        let provider = sevak_platform::native_provider();
        let saved = provider.clipboard_text().ok().flatten();
        if provider.set_clipboard_text("sevak-test baseline").is_err() {
            return;
        }
        sevak_platform::clipboard::take_own_write("sevak-test baseline");

        let mut monitor = Monitor::default();
        assert_eq!(monitor.poll(&*provider, &settings()), None); // primed

        provider
            .set_clipboard_text("sevak-test copied elsewhere")
            .unwrap();
        // As if another app had put it there.
        sevak_platform::clipboard::take_own_write("sevak-test copied elsewhere");
        let captured = monitor.poll(&*provider, &settings());
        assert_eq!(
            captured.map(|c| c.text).as_deref(),
            Some("sevak-test copied elsewhere")
        );
        assert_eq!(monitor.poll(&*provider, &settings()), None);

        // What Sevak writes itself is not recorded.
        provider
            .set_clipboard_text("sevak-test written by sevak")
            .unwrap();
        assert_eq!(monitor.poll(&*provider, &settings()), None);

        if let Some(saved) = saved {
            let _ = provider.set_clipboard_text(&saved);
        }
    }

    #[test]
    fn a_busy_clipboard_is_retried_not_skipped() {
        let platform = MockPlatform::empty();
        let mut monitor = primed_monitor(&platform);
        *platform.clipboard_sequence.lock().unwrap() = Some(2);
        *platform.clipboard_read.lock().unwrap() = None; // read fails
        assert_eq!(monitor.poll(&*platform, &settings()), None);
        *platform.clipboard_read.lock().unwrap() = Some(read("finally"));
        assert_eq!(
            monitor.poll(&*platform, &settings()).unwrap().text,
            "finally"
        );
    }

    #[test]
    fn without_a_sequence_number_the_text_is_compared() {
        let platform = MockPlatform::empty();
        *platform.clipboard_read.lock().unwrap() = Some(read("first"));
        let mut monitor = Monitor::default();
        assert_eq!(monitor.poll(&*platform, &settings()), None); // baseline
        assert_eq!(monitor.poll(&*platform, &settings()), None); // unchanged
        *platform.clipboard_read.lock().unwrap() = Some(read("second"));
        assert_eq!(
            monitor.poll(&*platform, &settings()).unwrap().text,
            "second"
        );
        assert_eq!(monitor.poll(&*platform, &settings()), None);
    }

    #[test]
    fn pushing_moves_duplicates_up_and_enforces_the_limit() {
        let entry = |text: &str, at| Entry {
            text: text.to_owned(),
            copied_at: at,
            source: None,
        };
        let mut state = State::default();
        state.push(entry("a", 1), 3);
        state.push(entry("b", 2), 3);
        state.push(entry("c", 3), 3);
        state.push(entry("a", 4), 3);
        let texts: Vec<_> = state.items.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts, ["a", "c", "b"]);
        assert_eq!(state.items[0].copied_at, 4);
        state.push(entry("d", 5), 3);
        let texts: Vec<_> = state.items.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts, ["d", "a", "c"]);
    }

    fn plugin(
        platform: &Arc<MockPlatform>,
        enabled: bool,
        file: Option<PathBuf>,
    ) -> ClipboardPlugin {
        let config = ClipboardConfig {
            enabled,
            ..ClipboardConfig::default()
        };
        ClipboardPlugin::new(&config, &PasteConfig::default(), platform.clone(), file)
    }

    fn fill(plugin: &ClipboardPlugin, entries: &[(&str, u64, Option<&str>)]) {
        let shared = plugin.shared.as_ref().unwrap();
        let mut state = lock(&shared.state);
        for (text, at, source) in entries {
            state.push(
                Entry {
                    text: (*text).to_owned(),
                    copied_at: *at,
                    source: source.map(str::to_owned),
                },
                200,
            );
        }
    }

    #[test]
    fn metadata() {
        let plugin = plugin(&MockPlatform::empty(), false, None);
        assert_eq!(plugin.id(), "clipboard");
        assert_eq!(plugin.keyword(), Some("cb"));
        assert!(!plugin.global());
    }

    #[test]
    fn while_off_the_plugin_explains_how_to_turn_it_on() {
        let platform = MockPlatform::empty();
        let plugin = plugin(&platform, false, None);
        let rows = plugin.query("");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "Clipboard history is off");
        plugin.execute(&rows[0]).unwrap();
        assert_eq!(
            *platform.clipboard.lock().unwrap(),
            ["[clipboard]\nenabled = true"]
        );
        // No history and no thread exist while off.
        assert!(plugin.shared.is_none());
        plugin.refresh().unwrap();
    }

    #[test]
    fn an_empty_history_says_so() {
        let plugin = plugin(&MockPlatform::empty(), true, None);
        let rows = plugin.query("");
        assert_eq!(rows[0].title, "Clipboard history is empty");
        plugin.execute(&rows[0]).unwrap();
        assert!(plugin.query("zzz").is_empty());
    }

    #[test]
    fn rows_are_newest_first_with_time_source_and_paste_hint() {
        let platform = MockPlatform::empty();
        let plugin = plugin(&platform, true, None);
        fill(
            &plugin,
            &[
                ("oldest", 1_000, None),
                ("middle\nsecond line", 1_300, Some("Code")),
                ("newest", 1_590, Some("chrome")),
            ],
        );
        let rows = plugin.rows("", 1_600);
        let titles: Vec<_> = rows.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles, ["newest", "middle", "oldest"]);
        assert_eq!(rows[0].subtitle, "just now · chrome · Enter to paste");
        assert_eq!(
            rows[1].subtitle,
            "5 min ago · Code · 2 lines · Enter to paste"
        );
        assert_eq!(rows[2].subtitle, "10 min ago · Enter to paste");
        assert!(rows.iter().all(|r| r.score >= score::KEYWORD));
        assert!(rows[0].score > rows[1].score && rows[1].score > rows[2].score);
        assert_eq!(
            rows[0].action,
            Action::PasteText {
                text: "newest".into(),
                restore_clipboard: false
            }
        );
    }

    #[test]
    fn rows_are_fuzzy_filtered() {
        let plugin = plugin(&MockPlatform::empty(), true, None);
        fill(
            &plugin,
            &[
                ("meeting notes for monday", 1, None),
                ("https://example.com/pricing", 2, None),
                ("grocery list", 3, None),
            ],
        );
        let titles: Vec<_> = plugin
            .rows("pric", 10)
            .into_iter()
            .map(|r| r.title)
            .collect();
        assert_eq!(titles, ["https://example.com/pricing"]);
        assert!(plugin.rows("qqqq", 10).is_empty());
    }

    #[test]
    fn result_keys_are_stable_per_text() {
        let plugin = plugin(&MockPlatform::empty(), true, None);
        fill(&plugin, &[("same text", 1, None)]);
        let first = plugin.rows("", 5)[0].id.clone();
        fill(&plugin, &[("other", 2, None), ("same text", 3, None)]);
        let again = plugin
            .rows("same", 9)
            .into_iter()
            .find(|r| r.title == "same text")
            .unwrap();
        assert_eq!(first, again.id);
    }

    #[test]
    fn where_pasting_is_unavailable_rows_copy_and_say_why() {
        let platform = MockPlatform::empty();
        *platform.copy_only.lock().unwrap() = Some("Pasting is not possible on Wayland".into());
        let plugin = plugin(&platform, true, None);
        fill(&plugin, &[("hello", 1, None)]);
        let row = plugin.rows("", 2).remove(0);
        assert_eq!(
            row.action,
            Action::CopyText {
                text: "hello".into()
            }
        );
        assert_eq!(
            row.subtitle,
            "just now · Copies to clipboard · Pasting is not possible on Wayland"
        );
        plugin.execute(&row).unwrap();
        assert_eq!(*platform.clipboard.lock().unwrap(), ["hello"]);
    }

    #[test]
    fn enter_pastes_and_honours_restore_clipboard() {
        let platform = MockPlatform::empty();
        let config = ClipboardConfig {
            enabled: true,
            ..ClipboardConfig::default()
        };
        let paste = PasteConfig {
            restore_clipboard: true,
        };
        let plugin = ClipboardPlugin::new(&config, &paste, platform.clone(), None);
        fill(&plugin, &[("hello", 1, None)]);
        let row = plugin.rows("", 2).remove(0);
        plugin.execute(&row).unwrap();
        assert_eq!(
            *platform.pasted.lock().unwrap(),
            [("hello".to_owned(), true)]
        );
    }

    #[test]
    fn the_clear_row_is_last_and_clearing_empties_history_and_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE_NAME);
        let platform = MockPlatform::empty();
        let plugin = plugin(&platform, true, Some(file.clone()));
        fill(
            &plugin,
            &[("clear cache command", 1, None), ("other", 2, None)],
        );

        assert!(plugin.rows("", 3).iter().all(|r| r.title != CLEAR_TITLE));
        assert!(plugin.rows("c", 3).iter().all(|r| r.title != CLEAR_TITLE));
        let rows = plugin.rows("clear", 3);
        let last = rows.last().unwrap();
        assert_eq!(last.title, CLEAR_TITLE);
        assert_eq!(rows[0].title, "clear cache command");
        assert_eq!(last.subtitle, "Deletes all 2 recorded items");

        plugin.execute(last).unwrap();
        assert!(plugin.shared.as_ref().unwrap().snapshot().is_empty());
        let saved = fs::read_to_string(&file).unwrap();
        assert!(!saved.contains("clear cache command"));
    }

    #[test]
    fn history_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("nested").join(FILE_NAME);
        let platform = MockPlatform::empty();
        {
            let plugin = plugin(&platform, true, Some(file.clone()));
            plugin.refresh().unwrap();
            let shared = plugin.shared.as_ref().unwrap();
            lock(&shared.state).push(
                Entry {
                    text: "kept".into(),
                    copied_at: 42,
                    source: Some("Code".into()),
                },
                200,
            );
            shared.persist();
        }
        let plugin = plugin(&platform, true, Some(file));
        plugin.refresh().unwrap();
        let items = plugin.shared.as_ref().unwrap().snapshot();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].text, "kept");
        assert_eq!(items[0].source.as_deref(), Some("Code"));
    }

    #[test]
    fn a_corrupt_history_file_is_moved_aside() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE_NAME);
        fs::write(&file, "{ not json").unwrap();
        assert!(read_history(&file).is_empty());
        assert!(!file.exists());
        assert!(dir.path().join("clipboard-history.json.corrupt").exists());
        assert!(read_history(&dir.path().join("missing.json")).is_empty());
    }

    #[test]
    fn plugins_for_the_same_file_share_one_history() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE_NAME);
        let platform = MockPlatform::empty();
        let first = plugin(&platform, true, Some(file.clone()));
        let second = plugin(&platform, true, Some(file.clone()));
        assert!(Arc::ptr_eq(
            first.shared.as_ref().unwrap(),
            second.shared.as_ref().unwrap()
        ));
        drop((first, second));
        // Once every user is gone a fresh history starts.
        let third = plugin(&platform, true, Some(file));
        assert_eq!(Arc::strong_count(third.shared.as_ref().unwrap()), 1);
    }

    #[test]
    fn reconfiguring_trims_the_history() {
        let plugin = plugin(&MockPlatform::empty(), true, None);
        fill(&plugin, &[("a", 1, None), ("b", 2, None), ("c", 3, None)]);
        let shared = plugin.shared.as_ref().unwrap();
        shared.configure(Settings {
            max_items: 2,
            ..settings()
        });
        let texts: Vec<_> = shared.snapshot().iter().map(|e| e.text.clone()).collect();
        assert_eq!(texts, ["c", "b"]);
    }

    #[test]
    fn relative_times() {
        assert_eq!(relative_time(100, 100), "just now");
        assert_eq!(relative_time(100, 200), "just now"); // clock moved back
        assert_eq!(relative_time(160, 100), "1 min ago");
        assert_eq!(relative_time(100 + 59 * 60 + 59, 100), "59 min ago");
        assert_eq!(relative_time(100 + 3_600, 100), "1 h ago");
        assert_eq!(relative_time(100 + 86_400 * 3, 100), "3 d ago");
    }

    #[test]
    fn previews_use_the_first_line_and_shorten() {
        assert_eq!(preview("\n  hello \nworld", 100), "hello");
        assert_eq!(preview("abcdef", 3), "abc…");
        assert_eq!(preview("abc", 3), "abc");
        assert_eq!(preview("a\tb", 10), "a b");
        assert_eq!(preview("日本語のテキスト", 3), "日本語…");
    }

    #[test]
    fn searchable_text_is_bounded_on_a_character_boundary() {
        let long = "é".repeat(SEARCH_CHARS + 50);
        assert_eq!(searchable(&long).chars().count(), SEARCH_CHARS);
        assert_eq!(searchable("short"), "short");
    }

    #[test]
    fn the_clear_row_needs_a_clear_prefix() {
        assert!(offers_clear("clear"));
        assert!(offers_clear(" Clear clip"));
        assert!(offers_clear("cle"));
        assert!(!offers_clear("cl"));
        assert!(!offers_clear("clean"));
        assert!(!offers_clear(""));
    }
}
