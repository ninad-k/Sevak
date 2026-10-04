//! Clipboard history: `cb <text>` lists what you copied, newest first, and
//! Enter pastes the chosen text, image or files into the app you were in.
//!
//! # Opt-in and private
//!
//! Nothing is watched or stored unless `[clipboard] enabled = true`. While it
//! is off the plugin still exists (so `cb` explains how to turn it on), but no
//! thread runs and no file is read or written.
//!
//! When on, a background thread notices clipboard changes and keeps them in
//! `clipboard-history.json` in Sevak's data folder (readable by the user only
//! on Unix). It records three kinds of entry:
//!
//! - **text**;
//! - **images** (`[clipboard] images`): the picture is saved as a PNG file in
//!   the `clipboard` folder next to the history, with a small thumbnail for the
//!   result row; the history keeps only the hash that names the files (see
//!   [`crate::clipboard_store`]);
//! - **files** (`[clipboard] files`): the paths of copied files and folders.
//!   The files stay where they are.
//!
//! A copy that carries files is a files entry, otherwise text wins over an
//! image (a spreadsheet range is text and a picture of it). It skips:
//!
//! - content the copying app marked secret (see
//!   [`sevak_platform::ClipboardRead::sensitive`]: password managers set these
//!   markers on Windows and macOS);
//! - copies made while an app from `[clipboard] ignore_apps` had focus;
//! - text longer than `max_item_bytes`, images whose PNG is larger than
//!   `max_image_bytes`, empty and whitespace-only text;
//! - anything Sevak itself put on the clipboard (a paste or copy it made);
//! - what Universal Actions copies while it reads the user's selection.
//!
//! `max_items` applies to all kinds together. An identical copy (same text,
//! same pixels, same file list) moves the existing entry to the top instead of
//! adding another. Deleting an entry, trimming or clearing the history deletes
//! the image files it no longer needs, and files nothing refers to are removed
//! when the history is loaded.
//!
//! The clipboard as it is when monitoring starts is not recorded: only what is
//! copied afterwards.
//!
//! # Threads
//!
//! The history lives in a [`Shared`] that the monitor thread and the plugin
//! both hold. Plugins are rebuilt on every config reload, so a process-wide
//! table hands the new plugin the same `Shared` (and thus the same monitor
//! thread) while the old one is still alive. The thread holds only a `Weak` and
//! exits once the last plugin using it is dropped. The thread is started by
//! [`Plugin::refresh`], never by the constructor, because the settings window
//! also constructs plugins just to read their names.
//!
//! # Grid view
//!
//! An image row carries its thumbnail as an [`IconSource::File`], which is what
//! the list shows, and asks to be a Grid View tile ([`ResultItem::as_tile`]):
//! when every row of a search is an image (`cb image`) the UI draws them as a
//! grid of thumbnails, and the preview pane shows the full PNG.

use std::borrow::Cow;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sevak_core::config::{ClipboardConfig, PasteConfig};
use sevak_core::model::score;
use sevak_core::{
    Action, ClipContent, FuzzyQuery, IconSource, Modifier, Plugin, PluginError, PluginResult,
    PreviewHint, ResultItem,
};
use sevak_platform::clip_media::{self, files_hash, ClipboardImage};
use sevak_platform::{
    AppPaths, ClipboardMedia, ClipboardRead, MediaRequest, PasteSupport, PlatformProvider,
};

use crate::actions::execute_action;
use crate::clipboard_store::{self, MediaStore};

/// The keyword that routes a query to this plugin.
pub const KEYWORD: &str = "cb";
/// The history file inside Sevak's data folder.
pub const FILE_NAME: &str = "clipboard-history.json";

/// How often the clipboard is checked. Windows and macOS have a change counter
/// (a few nanoseconds to read); on X11 the text itself is compared.
const POLL_INTERVAL: Duration = Duration::from_millis(300);
/// Without a change counter an image or file list can only be noticed by
/// reading it, which costs more than comparing text; so it is looked for once
/// in this many polls (about every 1.2 s) while the clipboard holds no text.
const MEDIA_POLL_EVERY: u64 = 4;
/// Most rows one query returns (the engine shows at most `[search] max_results`,
/// itself capped at 20).
const MAX_ROWS: usize = 20;
/// Only this many characters of each entry are fuzzy-matched, so a query stays
/// fast even with 200 large entries.
const SEARCH_CHARS: usize = 1_000;
const TITLE_CHARS: usize = 100;
/// A copy of more files than this is not recorded (a half-recorded list would
/// paste the wrong thing, and the history file would balloon).
const MAX_FILES_PER_ENTRY: usize = 1_000;
/// The longest side of an image's thumbnail, in pixels: crisp in a 32 px row
/// on a 2x display.
const THUMB_EDGE: u32 = 96;
/// All the stored images together. The oldest go first when this is exceeded,
/// whatever `max_items` says: 200 images of 10 MB would be 2 GB.
const IMAGE_BUDGET_BYTES: u64 = 512 * 1024 * 1024;

const CLEAR_TITLE: &str = "Clear clipboard history";
const PAYLOAD_CLEAR: &str = "clear";
const PAYLOAD_NOTHING: &str = "nothing";
/// `save-image:<hash>`: write the image to the Desktop or Downloads folder.
const PAYLOAD_SAVE_IMAGE: &str = "save-image:";
const ENABLE_SNIPPET: &str = "[clipboard]\nenabled = true";

/// Where the history is stored by default.
pub fn default_history_path() -> Option<PathBuf> {
    AppPaths::resolve()
        .ok()
        .map(|paths| paths.data_dir.join(FILE_NAME))
}

/// An image the history keeps: the hash of its pixels names its files in the
/// [`MediaStore`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageRef {
    pub hash: u64,
    pub width: u32,
    pub height: u32,
    /// Size of the PNG file.
    pub bytes: u64,
}

/// One recorded copy: text, an image or files.
///
/// `text` is always written, so that an older Sevak that only knows text still
/// reads the file: for files it holds the paths (one per line), for an image it
/// is empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    #[serde(default)]
    pub text: String,
    /// Seconds since the Unix epoch.
    pub copied_at: u64,
    /// The app that was focused when it was copied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<ImageRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    Image,
    Files,
}

impl Entry {
    pub fn new_text(text: String, copied_at: u64, source: Option<String>) -> Self {
        Self {
            text,
            copied_at,
            source,
            image: None,
            files: Vec::new(),
        }
    }

    pub fn new_image(image: ImageRef, copied_at: u64, source: Option<String>) -> Self {
        Self {
            text: String::new(),
            copied_at,
            source,
            image: Some(image),
            files: Vec::new(),
        }
    }

    pub fn new_files(files: Vec<PathBuf>, copied_at: u64, source: Option<String>) -> Self {
        let text = files
            .iter()
            .map(|path| path.to_string_lossy())
            .collect::<Vec<_>>()
            .join("\n");
        Self {
            text,
            copied_at,
            source,
            image: None,
            files,
        }
    }

    fn kind(&self) -> Kind {
        if self.image.is_some() {
            Kind::Image
        } else if !self.files.is_empty() {
            Kind::Files
        } else {
            Kind::Text
        }
    }

    /// True if copying `other` again is the same copy as this one.
    fn same_content(&self, other: &Entry) -> bool {
        match (self.kind(), other.kind()) {
            (Kind::Text, Kind::Text) => self.text == other.text,
            (Kind::Image, Kind::Image) => self.image.map(|i| i.hash) == other.image.map(|i| i.hash),
            (Kind::Files, Kind::Files) => self.files == other.files,
            _ => false,
        }
    }

    /// What a query is matched against: the text, the file paths, or a
    /// description of the image ("image 1920x1080 png").
    fn search_text(&self) -> Cow<'_, str> {
        match &self.image {
            Some(image) => Cow::Owned(format!("image {}x{} png", image.width, image.height)),
            None => Cow::Borrowed(searchable(&self.text)),
        }
    }
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

/// Version 1 held text only. Version 2 adds `image` and `files` to an entry;
/// both read the same way.
const HISTORY_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Settings {
    max_items: usize,
    max_item_bytes: usize,
    ignore_apps: Vec<String>,
    images: bool,
    files: bool,
    max_image_bytes: usize,
}

impl From<&ClipboardConfig> for Settings {
    fn from(config: &ClipboardConfig) -> Self {
        Self {
            max_items: config.max_items,
            max_item_bytes: config.max_item_bytes,
            ignore_apps: config.ignore_apps.clone(),
            images: config.images,
            files: config.files,
            max_image_bytes: config.max_image_bytes,
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
    /// Adds `entry` on top, replacing an identical earlier copy, and trims the
    /// history to `max_items` entries and the images to [`IMAGE_BUDGET_BYTES`].
    /// Returns the images no entry refers to any more (their files can go).
    fn push(&mut self, entry: Entry, max_items: usize) -> Vec<u64> {
        let mut removed = Vec::new();
        let (same, kept): (Vec<_>, Vec<_>) = self
            .items
            .drain(..)
            .partition(|existing| existing.same_content(&entry));
        removed.extend(same);
        self.items = kept;
        self.items.insert(0, Arc::new(entry));
        removed.extend(self.trim(max_items));
        self.unreferenced(&removed)
    }

    /// Drops the oldest entries beyond `max_items`, and the oldest images
    /// beyond the storage budget. Returns the dropped entries.
    fn trim(&mut self, max_items: usize) -> Vec<Arc<Entry>> {
        let mut removed = Vec::new();
        if self.items.len() > max_items {
            removed.extend(self.items.drain(max_items..));
        }
        let mut total: u64 = self
            .items
            .iter()
            .filter_map(|entry| entry.image.map(|image| image.bytes))
            .sum();
        // From the oldest end; the newest entry always stays.
        let mut index = self.items.len();
        while total > IMAGE_BUDGET_BYTES && index > 1 {
            index -= 1;
            if let Some(image) = self.items[index].image {
                total = total.saturating_sub(image.bytes);
                removed.push(self.items.remove(index));
            }
        }
        removed
    }

    /// The image hashes of `removed` that no entry still in the history uses.
    fn unreferenced(&self, removed: &[Arc<Entry>]) -> Vec<u64> {
        let mut hashes: Vec<u64> = removed
            .iter()
            .filter_map(|entry| entry.image.map(|image| image.hash))
            .filter(|hash| !self.uses_image(*hash))
            .collect();
        hashes.sort_unstable();
        hashes.dedup();
        hashes
    }

    fn uses_image(&self, hash: u64) -> bool {
        self.items
            .iter()
            .any(|entry| entry.image.is_some_and(|image| image.hash == hash))
    }

    fn referenced_images(&self) -> HashSet<u64> {
        self.items
            .iter()
            .filter_map(|entry| entry.image.map(|image| image.hash))
            .collect()
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
    /// Where the images live; `None` for a history kept in memory only (which
    /// therefore records no images).
    store: Option<MediaStore>,
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
        let store = path
            .as_deref()
            .and_then(Path::parent)
            .map(|dir| MediaStore::new(dir.join(clipboard_store::DIR_NAME)));
        Self {
            path,
            store,
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
        let dropped = {
            let mut state = lock(&self.state);
            let removed = state.trim(settings.max_items);
            state.settings = Some(settings);
            (!removed.is_empty()).then(|| state.unreferenced(&removed))
        };
        if let Some(images) = dropped {
            self.remove_images(&images);
            self.persist();
        }
    }

    /// Reads the history file once, and deletes the image files nothing refers
    /// to (left by a crash, or by an entry the file no longer lists).
    fn load(&self) {
        let changed = {
            let mut state = lock(&self.state);
            if state.loaded {
                return;
            }
            state.loaded = true;
            let Some(path) = &self.path else { return };
            let max_items = state.settings.as_ref().map_or(usize::MAX, |s| s.max_items);
            let read = read_history_checked(path);
            let readable = read.is_some();
            let mut items = read.unwrap_or_default();
            let listed = items.len();
            // An image whose files are gone cannot be pasted.
            items.retain(|entry| {
                entry.image.is_none_or(|image| {
                    self.store
                        .as_ref()
                        .is_some_and(|store| store.contains(image.hash))
                })
            });
            items.truncate(max_items);
            state.items = items.into_iter().map(Arc::new).collect();
            tracing::info!(items = state.items.len(), "clipboard history loaded");
            // Never prune on the strength of a history file that could not be
            // read: its images may be what the user wants to recover.
            if readable {
                if let Some(store) = &self.store {
                    let deleted = store.prune(&state.referenced_images());
                    if deleted > 0 {
                        tracing::info!(deleted, "removed unused clipboard image files");
                    }
                }
            }
            state.items.len() < listed
        };
        if changed {
            self.persist();
        }
    }

    fn clear(&self) {
        lock(&self.state).items.clear();
        self.persist();
        // Plaintext copies of the old history that are not the history file: a
        // file moved aside as unreadable, and a half-written temporary.
        if let Some(path) = &self.path {
            for suffix in [".corrupt", ".tmp"] {
                let mut leftover = path.as_os_str().to_owned();
                leftover.push(suffix);
                let _ = fs::remove_file(leftover);
            }
        }
        if let Some(store) = &self.store {
            store.prune(&HashSet::new());
        }
    }

    fn remove_images(&self, hashes: &[u64]) {
        if let Some(store) = &self.store {
            for hash in hashes {
                store.remove(*hash);
            }
        }
    }

    /// Writes the history to disk (outside the state lock; serialization of a
    /// full history takes a few milliseconds).
    fn persist(&self) {
        let Some(path) = &self.path else { return };
        let _writing = lock(&self.write);
        let items = self.snapshot();
        let stored = StoredHistoryRef {
            version: HISTORY_VERSION,
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
        let now = now_secs();
        let entry = match captured.content {
            CapturedContent::Text(text) => Entry::new_text(text, now, captured.source),
            CapturedContent::Files(files) => Entry::new_files(files, now, captured.source),
            CapturedContent::Image(image) => {
                let Some(stored) = self.store_image(&image, settings.max_image_bytes) else {
                    return;
                };
                Entry::new_image(stored, now, captured.source)
            }
        };
        let dropped = lock(&self.state).push(entry, settings.max_items);
        self.remove_images(&dropped);
        self.persist();
    }

    /// Saves a copied image's files (unless this very picture is already
    /// stored). `None` if it cannot be kept: no folder, too large, not encodable.
    fn store_image(&self, image: &ClipboardImage, max_bytes: usize) -> Option<ImageRef> {
        let store = self.store.as_ref()?;
        let hash = image.content_hash();
        let known = lock(&self.state)
            .items
            .iter()
            .find_map(|entry| entry.image.filter(|stored| stored.hash == hash));
        if let Some(known) = known.filter(|_| store.contains(hash)) {
            return Some(known);
        }
        let png = match image.encode_png() {
            Ok(png) => png,
            Err(err) => {
                tracing::debug!("clipboard image not encodable: {err}");
                return None;
            }
        };
        if png.len() > max_bytes {
            tracing::debug!(
                bytes = png.len(),
                max_bytes,
                "clipboard image too large to record"
            );
            return None;
        }
        let thumb = image.thumbnail(THUMB_EDGE).encode_png().ok()?;
        if let Err(err) = store.write(hash, &png, &thumb) {
            tracing::warn!("could not save a clipboard image: {err}");
            store.remove(hash);
            return None;
        }
        Some(ImageRef {
            hash,
            width: image.width,
            height: image.height,
            bytes: png.len() as u64,
        })
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

/// What the monitor decided to keep.
#[derive(Debug, PartialEq, Eq)]
enum CapturedContent {
    Text(String),
    Files(Vec<PathBuf>),
    Image(ClipboardImage),
}

/// A copy the monitor decided to keep.
#[derive(Debug, PartialEq, Eq)]
struct Captured {
    content: CapturedContent,
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
    /// Likewise for an image or file list, when there is no text.
    last_media_hash: Option<u64>,
    polls: u64,
}

impl Monitor {
    fn poll(&mut self, platform: &dyn PlatformProvider, settings: &Settings) -> Option<Captured> {
        // Universal Actions is borrowing the clipboard to copy the user's
        // selection: that copy is not something they copied. Nothing is
        // noted as seen, so the clipboard is looked at again afterwards.
        if sevak_platform::clipboard::synthetic_copy_in_progress() {
            return None;
        }
        self.polls += 1;
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

        // Secret content is never looked into any further.
        let request = MediaRequest {
            files: settings.files && !read.sensitive,
            image: settings.images && !read.sensitive && read.text.is_none(),
        };

        let media = match sequence {
            Some(sequence) => {
                self.last_sequence = Some(sequence);
                self.read_media(platform, request)
            }
            None => self.media_if_changed(platform, &read, request)?,
        };
        accept(read, media, platform, settings)
    }

    fn read_media(&self, platform: &dyn PlatformProvider, request: MediaRequest) -> ClipboardMedia {
        if request.files || request.image {
            platform.read_clipboard_media(request)
        } else {
            ClipboardMedia::default()
        }
    }

    /// Change detection for systems without a clipboard change counter: the
    /// text is compared, and an image or file list is looked for only now and
    /// then. `None` means nothing new; `Some` carries the media that was read.
    /// The very first look only records where the clipboard is.
    fn media_if_changed(
        &mut self,
        platform: &dyn PlatformProvider,
        read: &ClipboardRead,
        request: MediaRequest,
    ) -> Option<ClipboardMedia> {
        let first_look = !self.primed;
        let changed = self.look(platform, read, request);
        self.primed = true;
        if first_look {
            None
        } else {
            changed
        }
    }

    fn look(
        &mut self,
        platform: &dyn PlatformProvider,
        read: &ClipboardRead,
        request: MediaRequest,
    ) -> Option<ClipboardMedia> {
        if let Some(hash) = read.text.as_deref().map(fnv1a) {
            if Some(hash) == self.last_text_hash {
                return None;
            }
            self.last_text_hash = Some(hash);
            self.last_media_hash = None;
            return Some(self.read_media(platform, request));
        }

        // No text: an image or a file list, or nothing.
        self.last_text_hash = None;
        if self.primed && !self.polls.is_multiple_of(MEDIA_POLL_EVERY) {
            return None;
        }
        let media = self.read_media(platform, request);
        let hash = media_hash(&media);
        if hash == self.last_media_hash {
            return None;
        }
        self.last_media_hash = hash;
        Some(media)
    }
}

/// A fingerprint of the files or image, for noticing a change without a
/// sequence number. `None` for nothing.
fn media_hash(media: &ClipboardMedia) -> Option<u64> {
    if !media.files.is_empty() {
        Some(files_hash(&media.files))
    } else {
        media.image.as_ref().map(ClipboardImage::content_hash)
    }
}

/// Applies the privacy rules to a clipboard change.
fn accept(
    read: ClipboardRead,
    media: ClipboardMedia,
    platform: &dyn PlatformProvider,
    settings: &Settings,
) -> Option<Captured> {
    if read.sensitive {
        return None;
    }
    let content = if !media.files.is_empty() {
        if media.files.len() > MAX_FILES_PER_ENTRY
            || sevak_platform::clipboard::take_own_files(&media.files)
        {
            return None;
        }
        CapturedContent::Files(media.files)
    } else if let Some(text) = read.text {
        if text.trim().is_empty()
            || text.len() > settings.max_item_bytes
            || sevak_platform::clipboard::take_own_write(&text)
        {
            return None;
        }
        CapturedContent::Text(text)
    } else {
        let image = media.image?;
        if sevak_platform::clipboard::take_own_image(&image) {
            return None;
        }
        CapturedContent::Image(image)
    };
    let app = platform.foreground_app();
    if app
        .as_ref()
        .is_some_and(|app| app.matches_any(&settings.ignore_apps))
    {
        return None;
    }
    Some(Captured {
        content,
        source: app.map(|app| app.name),
    })
}

/// Reads the history file. A missing file is an empty history; an unreadable
/// one is moved aside (never overwritten silently) and also yields an empty one.
#[cfg(test)]
fn read_history(path: &Path) -> Vec<Entry> {
    read_history_checked(path).unwrap_or_default()
}

/// Like [`read_history`], but `None` when the file exists and could not be
/// used.
fn read_history_checked(path: &Path) -> Option<Vec<Entry>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Some(Vec::new()),
        Err(err) => {
            tracing::warn!("could not read {}: {err}", path.display());
            return None;
        }
    };
    match serde_json::from_slice::<StoredHistory>(&bytes) {
        Ok(stored) => Some(stored.items),
        Err(err) => {
            let mut aside = path.as_os_str().to_owned();
            aside.push(".corrupt");
            // Where it went wrong, never serde's message: for a wrong type it
            // quotes the offending value, which here is clipboard text.
            tracing::warn!(
                "{} is not a valid clipboard history ({:?} error at line {}, column {}); \
                 moving it aside",
                path.display(),
                err.classify(),
                err.line(),
                err.column()
            );
            let _ = fs::rename(path, aside);
            None
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

/// "812 B", "340 KB", "2.4 MB".
fn human_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    match bytes {
        0..KB => format!("{bytes} B"),
        KB..MB => format!("{} KB", bytes / KB),
        _ => format!("{:.1} MB", bytes as f64 / MB as f64),
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

/// The names of copied files for a row title: all of them if they fit, else
/// the first two and a count.
fn files_title(files: &[PathBuf]) -> String {
    let name = |path: &PathBuf| {
        path.file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned()
    };
    let names: Vec<String> = files.iter().map(name).collect();
    let title = if names.len() <= 3 {
        names.join(", ")
    } else {
        format!("{} and {} more", names[..2].join(", "), names.len() - 2)
    };
    if title.chars().count() > TITLE_CHARS {
        let cut: String = title.chars().take(TITLE_CHARS).collect();
        format!("{cut}…")
    } else {
        title
    }
}

/// The paths that still exist. Files can be moved or deleted after they were
/// copied; pasting the ones that are left beats refusing, and none is an error.
fn existing_files(paths: &[PathBuf]) -> PluginResult<Vec<PathBuf>> {
    let existing: Vec<PathBuf> = paths.iter().filter(|path| path.exists()).cloned().collect();
    if existing.is_empty() {
        Err(PluginError::Message(
            "The copied files no longer exist".to_owned(),
        ))
    } else {
        Ok(existing)
    }
}

/// The `cb` plugin.
pub struct ClipboardPlugin {
    settings: Settings,
    restore_clipboard: bool,
    platform: Arc<dyn PlatformProvider>,
    /// `None` while disabled.
    shared: Option<Arc<Shared>>,
    /// Where "Save image as" writes; the Desktop or Downloads folder when unset.
    save_dir: Option<PathBuf>,
}

impl ClipboardPlugin {
    /// Builds the plugin. Cheap: nothing is read and no thread starts until
    /// [`Plugin::refresh`]. `history_file` is where the history is kept
    /// ([`default_history_path`]); `None` keeps it in memory only (and then no
    /// images, which need a folder).
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
            save_dir: None,
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
                let matched = query.score(&entry.search_text())?;
                Some((i, f64::from(matched), entry))
            }));
        }

        let total = entries.len();
        let mut rows: Vec<ResultItem> = ranked
            .into_iter()
            .map(|(position, matched, entry)| {
                // Equal fuzzy scores keep the newest first.
                let recency = (total - position) as f64 * 0.001;
                self.row(shared, entry, &support, now)
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
                .with_subtitle(self.empty_hint())
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

    /// What the empty history promises to record.
    fn empty_hint(&self) -> &'static str {
        match (self.settings.images, self.settings.files) {
            (true, true) => "Text, images and files you copy from now on show up here",
            (true, false) => "Text and images you copy from now on show up here",
            (false, true) => "Text and files you copy from now on show up here",
            (false, false) => "Text you copy from now on shows up here",
        }
    }

    /// What Enter does with an image or files: paste them, or only copy them
    /// where pasting is not possible, with the hint the subtitle ends with.
    fn clip_action(&self, content: ClipContent, support: &PasteSupport) -> (Action, String) {
        match support {
            PasteSupport::Available => (
                Action::PasteClip {
                    content,
                    restore_clipboard: self.restore_clipboard,
                },
                "Enter to paste".to_owned(),
            ),
            PasteSupport::CopyOnly(reason) => (
                Action::CopyClip { content },
                format!("Copies to clipboard · {reason}"),
            ),
        }
    }

    fn row(&self, shared: &Shared, entry: &Entry, support: &PasteSupport, now: u64) -> ResultItem {
        let mut parts = vec![relative_time(now, entry.copied_at)];
        if let Some(source) = &entry.source {
            parts.push(source.clone());
        }
        match (entry.kind(), &entry.image, &shared.store) {
            (Kind::Image, Some(image), Some(store)) => {
                self.image_row(store, *image, support, parts)
            }
            (Kind::Files, _, _) => self.files_row(entry, support, parts),
            _ => self.text_row(entry, support, parts),
        }
    }

    fn text_row(
        &self,
        entry: &Entry,
        support: &PasteSupport,
        mut parts: Vec<String>,
    ) -> ResultItem {
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

    fn image_row(
        &self,
        store: &MediaStore,
        image: ImageRef,
        support: &PasteSupport,
        mut parts: Vec<String>,
    ) -> ResultItem {
        let png = store.png_path(image.hash);
        let content = ClipContent::Image { path: png.clone() };
        let (action, hint) = self.clip_action(content.clone(), support);
        parts.push(human_size(image.bytes));
        parts.push(hint);

        let mut row = ResultItem::new(
            self.id(),
            format!("img-{:016x}", image.hash),
            format!("Image {} × {}", image.width, image.height),
            action,
        )
        .with_subtitle(parts.join(" · "))
        .with_icon(IconSource::File {
            path: store.thumb_path(image.hash),
        })
        // The thumbnail is the tile's picture when every row is an image
        // (`cb image`), and the preview pane shows the full PNG.
        .as_tile(None)
        .with_preview(PreviewHint::Path { path: png });
        if support.is_available() {
            row = row.with_secondary(
                "Copy image",
                Some(Modifier::Ctrl),
                Action::CopyClip { content },
            );
        }
        row.with_secondary(
            "Save image as…",
            Some(Modifier::Shift),
            Action::Custom {
                payload: format!("{PAYLOAD_SAVE_IMAGE}{:016x}", image.hash),
            },
        )
    }

    fn files_row(
        &self,
        entry: &Entry,
        support: &PasteSupport,
        mut parts: Vec<String>,
    ) -> ResultItem {
        let content = ClipContent::Files {
            paths: entry.files.clone(),
        };
        let (action, hint) = self.clip_action(content.clone(), support);
        let count = entry.files.len();
        parts.insert(
            0,
            format!("{count} file{}", if count == 1 { "" } else { "s" }),
        );
        parts.push(hint);

        let mut row = ResultItem::new(
            self.id(),
            format!("files-{:016x}", files_hash(&entry.files)),
            files_title(&entry.files),
            action,
        )
        .with_subtitle(parts.join(" · "))
        .with_icon(IconSource::builtin("file"))
        .with_secondary(
            "Show in folder",
            Some(Modifier::Ctrl),
            Action::RevealPath {
                path: entry.files[0].clone(),
            },
        );
        if support.is_available() {
            row = row.with_secondary(
                "Copy files",
                Some(Modifier::Shift),
                Action::CopyClip { content },
            );
        }
        row
    }

    /// Writes a stored image to the Desktop (else Downloads) under a name that
    /// is not taken, and shows it there.
    fn save_image(&self, hash: u64) -> PluginResult<()> {
        let store = self
            .shared
            .as_ref()
            .and_then(|shared| shared.store.as_ref())
            .ok_or_else(|| PluginError::Message("There is no stored image to save".to_owned()))?;
        let source = store.png_path(hash);
        if !source.is_file() {
            return Err(PluginError::Message(
                "That image is no longer stored".to_owned(),
            ));
        }
        let dir = self
            .save_dir
            .clone()
            .or_else(clip_media::save_directory)
            .ok_or_else(|| PluginError::Message("There is no folder to save into".to_owned()))?;
        let stamp = chrono::Local::now().format("%Y-%m-%d %H-%M-%S");
        let target = clip_media::unique_file_name(&dir, &format!("Clipboard image {stamp}"), "png");
        fs::copy(&source, &target).map_err(PluginError::other)?;
        tracing::info!("saved a clipboard image to {}", target.display());
        if let Err(err) = self.platform.reveal_path(&target) {
            // Saved all the same; only showing it failed.
            tracing::debug!("could not show the saved image: {err}");
        }
        Ok(())
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

    /// A row's id is a hash of the copied text and the query is often a piece
    /// of it, so none of that goes into `usage.json`, which "Clear clipboard
    /// history" does not touch.
    fn tracks_usage(&self) -> bool {
        false
    }

    fn name(&self) -> &str {
        "Clipboard history"
    }

    fn description(&self) -> &str {
        "Type `cb` to paste text, images and files you copied earlier. Off until [clipboard] enabled = true."
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
            Action::Custom { payload } if payload.starts_with(PAYLOAD_SAVE_IMAGE) => {
                let digits = &payload[PAYLOAD_SAVE_IMAGE.len()..];
                let hash = u64::from_str_radix(digits, 16)
                    .map_err(|_| PluginError::Unsupported(payload.clone()))?;
                self.save_image(hash)
            }
            // Files can be gone by now; paste the ones that are left.
            Action::PasteClip {
                content: ClipContent::Files { paths },
                restore_clipboard,
            } => execute_action(
                self.platform.as_ref(),
                &Action::PasteClip {
                    content: ClipContent::Files {
                        paths: existing_files(paths)?,
                    },
                    restore_clipboard: *restore_clipboard,
                },
            ),
            Action::CopyClip {
                content: ClipContent::Files { paths },
            } => execute_action(
                self.platform.as_ref(),
                &Action::CopyClip {
                    content: ClipContent::Files {
                        paths: existing_files(paths)?,
                    },
                },
            ),
            Action::PasteClip {
                content: ClipContent::Image { path },
                ..
            }
            | Action::CopyClip {
                content: ClipContent::Image { path },
            } if !path.is_file() => Err(PluginError::Message(
                "That image is no longer stored".to_owned(),
            )),
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
            images: true,
            files: true,
            max_image_bytes: 1_000_000,
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

    /// The text of a captured copy.
    fn text_of(captured: Option<Captured>) -> Option<String> {
        match captured?.content {
            CapturedContent::Text(text) => Some(text),
            other => panic!("not text: {other:?}"),
        }
    }

    /// A small picture; `seed` makes different pictures.
    fn picture(seed: u8) -> ClipboardImage {
        let rgba = (0..8 * 6)
            .flat_map(|i: u32| [seed, (i % 8) as u8 * 30, (i / 8) as u8 * 40, 255])
            .collect();
        ClipboardImage::new(8, 6, rgba).unwrap()
    }

    /// A picture whose PNG is large (noise does not compress).
    fn noisy_picture() -> ClipboardImage {
        let mut state = 12345u32;
        let rgba = (0..64 * 64 * 4)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 24) as u8
            })
            .collect();
        ClipboardImage::new(64, 64, rgba).unwrap()
    }

    fn set_image(platform: &MockPlatform, sequence: u64, image: ClipboardImage) {
        *platform.clipboard_sequence.lock().unwrap() = Some(sequence);
        *platform.clipboard_read.lock().unwrap() = Some(ClipboardRead::default());
        *platform.clipboard_media.lock().unwrap() = ClipboardMedia {
            files: Vec::new(),
            image: Some(image),
        };
    }

    fn set_files(platform: &MockPlatform, sequence: u64, files: &[&Path]) {
        *platform.clipboard_sequence.lock().unwrap() = Some(sequence);
        *platform.clipboard_read.lock().unwrap() = Some(ClipboardRead::default());
        *platform.clipboard_media.lock().unwrap() = ClipboardMedia {
            files: files.iter().map(|p| p.to_path_buf()).collect(),
            image: None,
        };
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
        assert_eq!(captured.source.as_deref(), Some("Code"));
        assert_eq!(
            captured.content,
            CapturedContent::Text("fn main() {}".into())
        );
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
    fn secret_images_and_files_are_not_even_read() {
        let platform = MockPlatform::empty();
        let mut monitor = primed_monitor(&platform);
        set_image(&platform, 2, picture(1));
        *platform.clipboard_read.lock().unwrap() = Some(ClipboardRead {
            text: None,
            sensitive: true,
        });
        assert_eq!(monitor.poll(&*platform, &settings()), None);
        assert!(platform.media_requests.lock().unwrap().is_empty());
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

        // The same rule covers images and files.
        *platform.foreground.lock().unwrap() = Some(ForegroundApp::new("KeePassXC"));
        set_image(&platform, 4, picture(1));
        assert_eq!(monitor.poll(&*platform, &settings()), None);
        set_files(&platform, 5, &[Path::new("/a/b.txt")]);
        assert_eq!(monitor.poll(&*platform, &settings()), None);
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
        // Something that is not text, an image or files.
        *platform.clipboard_sequence.lock().unwrap() = Some(5);
        *platform.clipboard_read.lock().unwrap() = Some(ClipboardRead::default());
        *platform.clipboard_media.lock().unwrap() = ClipboardMedia::default();
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

    #[test]
    fn images_and_files_sevak_put_on_the_clipboard_are_skipped_once() {
        let platform = MockPlatform::empty();
        let mut monitor = primed_monitor(&platform);

        let own = picture(77);
        sevak_platform::clipboard::note_own_image(&own);
        set_image(&platform, 2, own.clone());
        assert_eq!(monitor.poll(&*platform, &settings()), None);
        set_image(&platform, 3, own);
        assert!(monitor.poll(&*platform, &settings()).is_some());

        let files = [Path::new("/own/files/1.txt"), Path::new("/own/files/2.txt")];
        let owned: Vec<PathBuf> = files.iter().map(|p| p.to_path_buf()).collect();
        sevak_platform::clipboard::note_own_files(&owned);
        set_files(&platform, 4, &files);
        assert_eq!(monitor.poll(&*platform, &settings()), None);
        set_files(&platform, 5, &files);
        assert!(monitor.poll(&*platform, &settings()).is_some());
    }

    #[test]
    fn universal_actions_copies_are_not_recorded() {
        let platform = MockPlatform::empty();
        let mut monitor = primed_monitor(&platform);
        let borrowed = sevak_platform::clipboard::SyntheticCopy::begin();
        copy(&platform, 2, "the user's selection");
        assert_eq!(monitor.poll(&*platform, &settings()), None);
        drop(borrowed);
        // Nothing was noted as seen, so what is on the clipboard now is
        // looked at (here: still the selection, as if the restore had failed).
        assert!(monitor.poll(&*platform, &settings()).is_some());
    }

    #[test]
    fn a_copied_image_is_captured_when_there_is_no_text() {
        let platform = MockPlatform::empty();
        let mut monitor = primed_monitor(&platform);
        let image = picture(3);
        set_image(&platform, 2, image.clone());
        let captured = monitor.poll(&*platform, &settings()).unwrap();
        assert_eq!(captured.content, CapturedContent::Image(image));
        // Asked for files and the image.
        assert_eq!(*platform.media_requests.lock().unwrap(), [(true, true)]);
        assert_eq!(monitor.poll(&*platform, &settings()), None);
    }

    #[test]
    fn text_wins_over_an_image_and_files_win_over_text() {
        let platform = MockPlatform::empty();
        let mut monitor = primed_monitor(&platform);

        // A spreadsheet range: text, plus a picture of it.
        copy(&platform, 2, "a\tb\n1\t2");
        *platform.clipboard_media.lock().unwrap() = ClipboardMedia {
            files: Vec::new(),
            image: Some(picture(1)),
        };
        assert_eq!(
            text_of(monitor.poll(&*platform, &settings())).as_deref(),
            Some("a\tb\n1\t2")
        );
        // The picture was not even asked for.
        assert_eq!(
            platform.media_requests.lock().unwrap().last(),
            Some(&(true, false))
        );

        // A file manager's copy: the file names as text, and the files.
        copy(&platform, 3, "report.pdf");
        *platform.clipboard_media.lock().unwrap() = ClipboardMedia {
            files: vec!["/docs/report.pdf".into()],
            image: None,
        };
        let captured = monitor.poll(&*platform, &settings()).unwrap();
        assert_eq!(
            captured.content,
            CapturedContent::Files(vec!["/docs/report.pdf".into()])
        );
    }

    #[test]
    fn images_and_files_can_be_switched_off() {
        let platform = MockPlatform::empty();
        let off = Settings {
            images: false,
            files: false,
            ..settings()
        };
        *platform.clipboard_sequence.lock().unwrap() = Some(1);
        let mut monitor = Monitor::default();
        assert_eq!(monitor.poll(&*platform, &off), None);

        set_image(&platform, 2, picture(1));
        assert_eq!(monitor.poll(&*platform, &off), None);
        set_files(&platform, 3, &[Path::new("/a/b")]);
        assert_eq!(monitor.poll(&*platform, &off), None);
        // Nothing was read at all.
        assert!(platform.media_requests.lock().unwrap().is_empty());

        let images_only = Settings {
            files: false,
            ..settings()
        };
        set_image(&platform, 4, picture(2));
        assert!(monitor.poll(&*platform, &images_only).is_some());
        assert_eq!(*platform.media_requests.lock().unwrap(), [(false, true)]);
    }

    #[test]
    fn too_many_files_are_not_recorded() {
        let platform = MockPlatform::empty();
        let mut monitor = primed_monitor(&platform);
        let many: Vec<PathBuf> = (0..=MAX_FILES_PER_ENTRY)
            .map(|i| PathBuf::from(format!("/many/{i}")))
            .collect();
        *platform.clipboard_sequence.lock().unwrap() = Some(2);
        *platform.clipboard_read.lock().unwrap() = Some(ClipboardRead::default());
        *platform.clipboard_media.lock().unwrap() = ClipboardMedia {
            files: many,
            image: None,
        };
        assert_eq!(monitor.poll(&*platform, &settings()), None);
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
            text_of(captured).as_deref(),
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

    /// Images and files through the real Windows clipboard: copied by "another
    /// app" they are recorded, written by Sevak they are not. Writes the user's
    /// clipboard (and puts their text back), so it is run by hand:
    /// `cargo test -p sevak-plugins the_real_clipboard_holds_images_and_files -- --ignored`
    #[cfg(windows)]
    #[test]
    #[ignore = "writes to the real clipboard"]
    fn the_real_clipboard_holds_images_and_files() {
        let provider = sevak_platform::native_provider();
        let saved = provider.clipboard_text().ok().flatten();
        let dir = tempfile::tempdir().unwrap();

        let mut monitor = Monitor::default();
        assert_eq!(monitor.poll(&*provider, &settings()), None); // primed

        // Another app copies a picture.
        let image = picture(9);
        arboard::Clipboard::new()
            .unwrap()
            .set_image(arboard::ImageData {
                width: image.width as usize,
                height: image.height as usize,
                bytes: Cow::Borrowed(&image.rgba),
            })
            .unwrap();
        let captured = monitor.poll(&*provider, &settings()).expect("an image");
        assert_eq!(captured.content, CapturedContent::Image(image.clone()));
        assert_eq!(monitor.poll(&*provider, &settings()), None);

        // Sevak puts that picture back (from its PNG file): not recorded.
        let png = dir.path().join("p.png");
        fs::write(&png, image.encode_png().unwrap()).unwrap();
        provider
            .set_clipboard_clip(&ClipContent::Image { path: png })
            .unwrap();
        assert_eq!(monitor.poll(&*provider, &settings()), None);

        // Another app copies a file.
        let file = dir.path().join("note.txt");
        fs::write(&file, "x").unwrap();
        arboard::Clipboard::new()
            .unwrap()
            .set()
            .file_list(&[&file])
            .unwrap();
        let captured = monitor.poll(&*provider, &settings()).expect("files");
        match captured.content {
            CapturedContent::Files(files) => {
                assert_eq!(files.len(), 1);
                assert!(files[0].ends_with("note.txt"), "{files:?}");
            }
            other => panic!("{other:?}"),
        }

        // Sevak pastes the files: not recorded.
        provider
            .set_clipboard_clip(&ClipContent::Files { paths: vec![file] })
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
            text_of(monitor.poll(&*platform, &settings())).as_deref(),
            Some("finally")
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
            text_of(monitor.poll(&*platform, &settings())).as_deref(),
            Some("second")
        );
        assert_eq!(monitor.poll(&*platform, &settings()), None);
    }

    #[test]
    fn without_a_sequence_number_images_are_looked_for_only_now_and_then() {
        let platform = MockPlatform::empty();
        *platform.clipboard_read.lock().unwrap() = Some(ClipboardRead::default());
        let mut monitor = Monitor::default();
        assert_eq!(monitor.poll(&*platform, &settings()), None); // baseline: looked once
        let looks = || platform.media_requests.lock().unwrap().len();
        assert_eq!(looks(), 1);

        *platform.clipboard_media.lock().unwrap() = ClipboardMedia {
            files: Vec::new(),
            image: Some(picture(5)),
        };
        // A copy happens, but the clipboard is read for it only every Nth poll.
        let mut captured = None;
        let mut polls = 0;
        while captured.is_none() {
            polls += 1;
            assert!(polls <= MEDIA_POLL_EVERY, "never looked");
            captured = monitor.poll(&*platform, &settings());
        }
        assert!(polls > 1, "looked on every poll");
        assert_eq!(
            captured.unwrap().content,
            CapturedContent::Image(picture(5))
        );
        // The same picture is not captured again.
        for _ in 0..MEDIA_POLL_EVERY * 2 {
            assert_eq!(monitor.poll(&*platform, &settings()), None);
        }
        // Text is noticed at once, as before.
        *platform.clipboard_read.lock().unwrap() = Some(read("typed"));
        assert_eq!(
            text_of(monitor.poll(&*platform, &settings())).as_deref(),
            Some("typed")
        );
    }

    fn entry(text: &str, at: u64) -> Entry {
        Entry::new_text(text.to_owned(), at, None)
    }

    fn image_entry(hash: u64, bytes: u64, at: u64) -> Entry {
        Entry::new_image(
            ImageRef {
                hash,
                width: 8,
                height: 6,
                bytes,
            },
            at,
            None,
        )
    }

    #[test]
    fn pushing_moves_duplicates_up_and_enforces_the_limit() {
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

    #[test]
    fn the_item_limit_counts_every_kind_and_reports_images_to_delete() {
        let mut state = State::default();
        assert!(state.push(image_entry(1, 10, 1), 3).is_empty());
        state.push(entry("t", 2), 3);
        state.push(Entry::new_files(vec!["/a".into()], 3, None), 3);
        // The fourth entry pushes the oldest (the image) out.
        let freed = state.push(entry("u", 4), 3);
        assert_eq!(state.items.len(), 3);
        assert_eq!(freed, [1]);
    }

    #[test]
    fn the_same_copy_is_recognised_per_kind() {
        let mut state = State::default();
        state.push(image_entry(1, 10, 1), 10);
        state.push(
            Entry::new_files(vec!["/a".into(), "/b".into()], 2, None),
            10,
        );
        state.push(entry("/a\n/b", 3), 10); // the same words as text: different
                                            // Copying the image again moves it up and frees nothing.
        assert!(state.push(image_entry(1, 10, 4), 10).is_empty());
        // So does the same files in the same order.
        state.push(
            Entry::new_files(vec!["/a".into(), "/b".into()], 5, None),
            10,
        );
        // But another order is another copy.
        state.push(
            Entry::new_files(vec!["/b".into(), "/a".into()], 6, None),
            10,
        );
        assert_eq!(state.items.len(), 4);
        assert_eq!(
            state.items[1].files,
            [PathBuf::from("/a"), PathBuf::from("/b")]
        );
    }

    #[test]
    fn the_oldest_images_go_when_the_storage_budget_is_exceeded() {
        let half = IMAGE_BUDGET_BYTES / 2 + 1;
        let mut state = State::default();
        state.push(image_entry(1, half, 1), 100);
        state.push(entry("text stays", 2), 100);
        // Two images no longer fit: image 1 is dropped, the text is not.
        let freed = state.push(image_entry(2, half, 3), 100);
        assert_eq!(freed, [1]);
        let kinds: Vec<_> = state.items.iter().map(|e| e.kind()).collect();
        assert_eq!(kinds, [Kind::Image, Kind::Text]);
        // The newest image always stays, even if alone it is over the budget.
        let freed = state.push(image_entry(3, IMAGE_BUDGET_BYTES + 1, 4), 100);
        assert_eq!(freed, [2]);
        assert!(state.uses_image(3));
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
                Entry::new_text((*text).to_owned(), *at, source.map(str::to_owned)),
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
        assert!(rows[0].subtitle.contains("images and files"));
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
    fn clearing_also_removes_plaintext_leftovers_and_usage_is_not_tracked() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE_NAME);
        let corrupt = dir.path().join(format!("{FILE_NAME}.corrupt"));
        let temp = dir.path().join(format!("{FILE_NAME}.tmp"));
        fs::write(&corrupt, "old secret").unwrap();
        fs::write(&temp, "half written secret").unwrap();
        let plugin = plugin(&MockPlatform::empty(), true, Some(file));
        assert!(!plugin.tracks_usage());

        plugin.shared.as_ref().unwrap().clear();
        assert!(!corrupt.exists());
        assert!(!temp.exists());
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
            lock(&shared.state).push(Entry::new_text("kept".into(), 42, Some("Code".into())), 200);
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
    fn a_text_only_history_file_from_before_images_still_loads() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE_NAME);
        fs::write(
            &file,
            r#"{"version":1,"items":[
                {"text":"newer","copied_at":20,"source":"Code"},
                {"text":"older","copied_at":10}
            ]}"#,
        )
        .unwrap();
        let platform = MockPlatform::empty();
        let plugin = plugin(&platform, true, Some(file.clone()));
        plugin.refresh().unwrap();
        let shared = plugin.shared.as_ref().unwrap();
        let items = shared.snapshot();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].text, "newer");
        assert_eq!(items[0].source.as_deref(), Some("Code"));
        assert_eq!(items[1].kind(), Kind::Text);
        assert!(items[1].image.is_none() && items[1].files.is_empty());
        // Nothing was lost or moved aside, and it still pastes.
        assert!(!dir.path().join("clipboard-history.json.corrupt").exists());
        let rows = plugin.rows("", 30);
        assert_eq!(rows[0].title, "newer");

        // Saved again, it is still readable as before: the same keys, and the
        // version says what wrote it.
        shared.persist();
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        assert_eq!(saved["version"], HISTORY_VERSION);
        assert_eq!(
            saved["items"][0],
            serde_json::json!({"text":"newer","copied_at":20,"source":"Code"})
        );
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

    // ---- images and files in a real history folder ----

    /// Configures and loads a plugin's history, without the monitor thread.
    fn open(plugin: &ClipboardPlugin) {
        let shared = plugin.shared.as_ref().unwrap();
        shared.configure(plugin.settings.clone());
        shared.load();
    }

    /// A plugin with a history file in a temporary folder, configured and
    /// loaded, plus a monitor that has seen the (empty) clipboard.
    struct Rig {
        dir: tempfile::TempDir,
        platform: Arc<MockPlatform>,
        plugin: ClipboardPlugin,
        monitor: Monitor,
        sequence: u64,
    }

    impl Rig {
        fn new() -> Self {
            Self::with(ClipboardConfig::default())
        }

        fn with(config: ClipboardConfig) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let platform = MockPlatform::empty();
            let config = ClipboardConfig {
                enabled: true,
                ..config
            };
            let plugin = ClipboardPlugin::new(
                &config,
                &PasteConfig::default(),
                platform.clone(),
                Some(dir.path().join(FILE_NAME)),
            );
            // Not `refresh`: that would start a monitor thread to compete with
            // the one the test drives by hand.
            open(&plugin);
            let monitor = primed_monitor(&platform);
            Self {
                dir,
                platform,
                plugin,
                monitor,
                sequence: 1,
            }
        }

        fn shared(&self) -> &Arc<Shared> {
            self.plugin.shared.as_ref().unwrap()
        }

        fn store(&self) -> &MediaStore {
            self.shared().store.as_ref().unwrap()
        }

        fn media_dir(&self) -> PathBuf {
            self.dir.path().join(clipboard_store::DIR_NAME)
        }

        fn media_files(&self) -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(self.media_dir())
                .map(|entries| {
                    entries
                        .flatten()
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();
            names.sort();
            names
        }

        /// Copies `image` "from another app" and runs one pass of the monitor.
        fn copy_image(&mut self, image: &ClipboardImage) {
            self.sequence += 1;
            set_image(&self.platform, self.sequence, image.clone());
            Arc::clone(self.shared()).tick(&mut self.monitor);
        }

        fn copy_files(&mut self, files: &[&Path]) {
            self.sequence += 1;
            set_files(&self.platform, self.sequence, files);
            Arc::clone(self.shared()).tick(&mut self.monitor);
        }

        fn copy_text(&mut self, text: &str) {
            self.sequence += 1;
            copy(&self.platform, self.sequence, text);
            Arc::clone(self.shared()).tick(&mut self.monitor);
        }
    }

    #[test]
    fn a_copied_image_is_saved_as_png_with_a_thumbnail_and_listed() {
        let mut rig = Rig::new();
        let image = picture(4);
        rig.copy_image(&image);

        let items = rig.shared().snapshot();
        assert_eq!(items.len(), 1);
        let stored = items[0].image.expect("an image entry");
        assert_eq!((stored.width, stored.height), (8, 6));
        assert_eq!(stored.hash, image.content_hash());
        assert_eq!(
            rig.media_files(),
            [
                format!("{:016x}.png", stored.hash),
                format!("{:016x}.thumb.png", stored.hash)
            ]
        );
        let png = fs::read(rig.store().png_path(stored.hash)).unwrap();
        assert_eq!(stored.bytes, png.len() as u64);
        assert_eq!(ClipboardImage::decode_png(&png).unwrap(), image);

        // It was written to the history file, and is a row with its thumbnail.
        let saved = fs::read_to_string(rig.dir.path().join(FILE_NAME)).unwrap();
        assert!(
            saved.contains(&format!("\"hash\":{}", stored.hash)),
            "{saved}"
        );
        let row = rig.plugin.rows("", 100).remove(0);
        assert_eq!(row.title, "Image 8 × 6");
        assert_eq!(
            row.icon,
            Some(IconSource::File {
                path: rig.store().thumb_path(stored.hash)
            })
        );
        assert_eq!(
            row.action,
            Action::PasteClip {
                content: ClipContent::Image {
                    path: rig.store().png_path(stored.hash)
                },
                restore_clipboard: false
            }
        );
        assert_eq!(
            row.subtitle,
            format!("just now · {} · Enter to paste", human_size(stored.bytes))
        );
        // A Grid View tile (its icon is the picture), previewing the full PNG.
        assert!(row.is_tile());
        assert_eq!(
            row.preview,
            Some(PreviewHint::Path {
                path: rig.store().png_path(stored.hash)
            })
        );
    }

    #[test]
    fn copying_the_same_picture_again_adds_nothing_and_writes_nothing() {
        let mut rig = Rig::new();
        let image = picture(4);
        rig.copy_image(&image);
        rig.copy_text("between");
        let first_files = rig.media_files();
        let before = fs::metadata(rig.store().png_path(image.content_hash()))
            .unwrap()
            .modified()
            .unwrap();

        rig.copy_image(&image);
        let items = rig.shared().snapshot();
        assert_eq!(items.len(), 2);
        // Moved to the top, files untouched.
        assert!(items[0].image.is_some());
        assert_eq!(rig.media_files(), first_files);
        let after = fs::metadata(rig.store().png_path(image.content_hash()))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(before, after);

        // A different picture is a new entry with its own files.
        rig.copy_image(&picture(5));
        assert_eq!(rig.shared().snapshot().len(), 3);
        assert_eq!(rig.media_files().len(), 4);
    }

    #[test]
    fn an_image_over_the_size_limit_is_not_recorded_and_leaves_no_files() {
        let mut rig = Rig::with(ClipboardConfig {
            max_image_bytes: 500,
            ..ClipboardConfig::default()
        });
        rig.copy_image(&noisy_picture());
        assert!(rig.shared().snapshot().is_empty());
        assert!(rig.media_files().is_empty());
        // A small one still is.
        rig.copy_image(&picture(1));
        assert_eq!(rig.shared().snapshot().len(), 1);
    }

    #[test]
    fn an_in_memory_history_records_no_images() {
        let platform = MockPlatform::empty();
        let plugin = plugin(&platform, true, None);
        plugin.refresh().unwrap();
        let shared = plugin.shared.as_ref().unwrap();
        assert!(shared.store.is_none());
        assert!(shared.store_image(&picture(1), 1_000_000).is_none());
    }

    #[test]
    fn copied_files_are_listed_by_name_and_count() {
        let mut rig = Rig::new();
        rig.copy_files(&[Path::new("/docs/report.pdf"), Path::new("/docs/notes.txt")]);
        let items = rig.shared().snapshot();
        assert_eq!(items[0].kind(), Kind::Files);
        assert_eq!(items[0].files.len(), 2);

        let row = rig.plugin.rows("", 100).remove(0);
        assert_eq!(row.title, "report.pdf, notes.txt");
        assert_eq!(row.subtitle, "2 files · just now · Enter to paste");
        assert_eq!(
            row.action,
            Action::PasteClip {
                content: ClipContent::Files {
                    paths: vec!["/docs/report.pdf".into(), "/docs/notes.txt".into()]
                },
                restore_clipboard: false
            }
        );
        // Searchable by file name.
        assert_eq!(rig.plugin.rows("notes", 100).len(), 1);
        assert!(rig.plugin.rows("invoice", 100).is_empty());
        // Persisted.
        let saved = fs::read_to_string(rig.dir.path().join(FILE_NAME)).unwrap();
        assert!(saved.contains("report.pdf"));
    }

    #[test]
    fn file_titles_shorten_long_lists() {
        let paths = |names: &[&str]| -> Vec<PathBuf> {
            names
                .iter()
                .map(|n| PathBuf::from(format!("/d/{n}")))
                .collect()
        };
        assert_eq!(files_title(&paths(&["a.txt"])), "a.txt");
        assert_eq!(files_title(&paths(&["a", "b", "c"])), "a, b, c");
        assert_eq!(
            files_title(&paths(&["a", "b", "c", "d", "e"])),
            "a, b and 3 more"
        );
        let long = "x".repeat(200);
        assert!(files_title(&paths(&[&long])).chars().count() <= TITLE_CHARS + 1);
    }

    #[test]
    fn image_and_file_rows_offer_the_other_actions() {
        let mut rig = Rig::new();
        rig.copy_image(&picture(2));
        rig.copy_files(&[Path::new("/docs/a.txt"), Path::new("/docs/b.txt")]);
        let rows = rig.plugin.rows("", 100);
        let (files, image) = (&rows[0], &rows[1]);

        let labels = |row: &ResultItem| -> Vec<(String, Option<Modifier>)> {
            row.secondary
                .iter()
                .map(|s| (s.label.clone(), s.modifier))
                .collect()
        };
        assert_eq!(
            labels(image),
            [
                ("Copy image".to_owned(), Some(Modifier::Ctrl)),
                ("Save image as…".to_owned(), Some(Modifier::Shift))
            ]
        );
        assert_eq!(
            labels(files),
            [
                ("Show in folder".to_owned(), Some(Modifier::Ctrl)),
                ("Copy files".to_owned(), Some(Modifier::Shift))
            ]
        );
        assert_eq!(
            files.secondary[0].action,
            Action::RevealPath {
                path: "/docs/a.txt".into()
            }
        );
        // Ctrl+C on a files row copies the paths as text.
        assert_eq!(
            files.copy_text().as_deref(),
            Some("/docs/a.txt\n/docs/b.txt")
        );
        assert_eq!(image.copy_text(), None);
    }

    #[test]
    fn where_pasting_is_unavailable_image_and_file_rows_only_copy() {
        let mut rig = Rig::new();
        *rig.platform.copy_only.lock().unwrap() = Some("No pasting on Wayland".into());
        rig.copy_image(&picture(2));
        rig.copy_files(&[Path::new("/docs/a.txt")]);
        let rows = rig.plugin.rows("", 100);
        for row in &rows {
            assert!(matches!(row.action, Action::CopyClip { .. }), "{row:?}");
            assert!(row
                .subtitle
                .ends_with("Copies to clipboard · No pasting on Wayland"));
        }
        // No redundant "copy" action next to a primary action that copies.
        assert_eq!(rows[0].secondary.len(), 1); // files: show in folder
        assert_eq!(rows[1].secondary.len(), 1); // image: save as
        rig.plugin.execute(&rows[1]).unwrap();
        assert_eq!(rig.platform.copied_clips.lock().unwrap().len(), 1);
    }

    #[test]
    fn enter_on_an_image_pastes_the_same_image() {
        let mut rig = Rig::new();
        rig.copy_image(&picture(6));
        let row = rig.plugin.rows("", 100).remove(0);
        rig.plugin.execute(&row).unwrap();
        let pasted = rig.platform.pasted_clips.lock().unwrap().clone();
        assert_eq!(pasted.len(), 1);
        assert!(matches!(&pasted[0].0, ClipContent::Image { path } if path.is_file()));
        // The text paste was not used.
        assert!(rig.platform.pasted.lock().unwrap().is_empty());

        // "Copy image" only copies.
        rig.plugin
            .execute(&row.secondary_as_primary(0).unwrap())
            .unwrap();
        assert_eq!(rig.platform.copied_clips.lock().unwrap().len(), 1);
        assert_eq!(rig.platform.pasted_clips.lock().unwrap().len(), 1);
    }

    #[test]
    fn enter_on_files_pastes_the_ones_that_still_exist() {
        let mut rig = Rig::new();
        let kept = rig.dir.path().join("kept.txt");
        let gone = rig.dir.path().join("gone.txt");
        fs::write(&kept, "x").unwrap();
        rig.copy_files(&[&kept, &gone]);
        let row = rig.plugin.rows("", 100).remove(0);
        rig.plugin.execute(&row).unwrap();
        assert_eq!(
            *rig.platform.pasted_clips.lock().unwrap(),
            [(ClipContent::Files { paths: vec![kept] }, false)]
        );

        // Nothing left: an error the shell can show, and no paste.
        let mut rig = Rig::new();
        rig.copy_files(&[&gone]);
        let row = rig.plugin.rows("", 100).remove(0);
        let err = rig.plugin.execute(&row).unwrap_err();
        assert!(err.to_string().contains("no longer exist"), "{err}");
        assert!(rig.platform.pasted_clips.lock().unwrap().is_empty());
    }

    #[test]
    fn a_vanished_image_file_is_an_error_not_a_paste() {
        let mut rig = Rig::new();
        rig.copy_image(&picture(6));
        let row = rig.plugin.rows("", 100).remove(0);
        let hash = rig.shared().snapshot()[0].image.unwrap().hash;
        fs::remove_file(rig.store().png_path(hash)).unwrap();
        let err = rig.plugin.execute(&row).unwrap_err();
        assert!(err.to_string().contains("no longer stored"), "{err}");
        assert!(rig.platform.pasted_clips.lock().unwrap().is_empty());
    }

    #[test]
    fn save_image_writes_a_uniquely_named_png_and_shows_it() {
        let mut rig = Rig::new();
        let image = picture(8);
        rig.copy_image(&image);
        let desktop = tempfile::tempdir().unwrap();
        rig.plugin.save_dir = Some(desktop.path().to_path_buf());

        let row = rig.plugin.rows("", 100).remove(0);
        let save = row.secondary_as_primary(1).unwrap();
        assert!(
            matches!(&save.action, Action::Custom { payload } if payload.starts_with("save-image:"))
        );
        rig.plugin.execute(&save).unwrap();
        rig.plugin.execute(&save).unwrap();

        let mut saved: Vec<PathBuf> = fs::read_dir(desktop.path())
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .collect();
        saved.sort();
        assert_eq!(saved.len(), 2, "{saved:?}");
        for path in &saved {
            let name = path.file_name().unwrap().to_string_lossy();
            assert!(name.starts_with("Clipboard image "), "{name}");
            assert!(name.ends_with(".png"), "{name}");
            assert_eq!(
                ClipboardImage::decode_png(&fs::read(path).unwrap()).unwrap(),
                image
            );
        }
        assert_ne!(saved[0], saved[1]);
        // Shown in the file manager.
        assert_eq!(rig.platform.revealed.lock().unwrap().len(), 2);

        // A made-up payload is refused.
        let bogus = ResultItem::new(
            "clipboard",
            "x",
            "x",
            Action::Custom {
                payload: "save-image:zzzz".into(),
            },
        );
        assert!(rig.plugin.execute(&bogus).is_err());
    }

    #[test]
    fn clearing_and_trimming_delete_the_image_files() {
        let mut rig = Rig::new();
        rig.copy_image(&picture(1));
        rig.copy_image(&picture(2));
        rig.copy_text("text");
        assert_eq!(rig.media_files().len(), 4);

        // Trimmed to two entries: the oldest image goes, its files with it.
        rig.shared().configure(Settings {
            max_items: 2,
            ..settings()
        });
        assert_eq!(rig.media_files().len(), 2);
        assert_eq!(rig.shared().snapshot().len(), 2);

        // A new entry pushes the other image out too.
        rig.copy_text("more text");
        assert!(rig.media_files().is_empty(), "{:?}", rig.media_files());

        // And clearing removes whatever is left.
        rig.copy_image(&picture(3));
        assert_eq!(rig.media_files().len(), 2);
        rig.shared().clear();
        assert!(rig.media_files().is_empty());
        assert!(rig.shared().snapshot().is_empty());
        let saved = fs::read_to_string(rig.dir.path().join(FILE_NAME)).unwrap();
        assert!(!saved.contains("hash"), "{saved}");
    }

    #[test]
    fn images_survive_a_restart_and_orphans_are_trimmed_on_start() {
        let mut rig = Rig::new();
        let image = picture(1);
        rig.copy_image(&image);
        let hash = image.content_hash();

        // Files nothing refers to: a leftover pair, a temp file of an
        // interrupted write, and the user's own file in the same folder.
        let store = rig.store().clone();
        store.write(0xdead, b"x", b"x").unwrap();
        fs::write(store.dir().join("0000000000000bad.png.tmp"), b"half").unwrap();
        fs::write(store.dir().join("keep-me.png"), b"mine").unwrap();
        let file = rig.dir.path().join(FILE_NAME);
        let platform = rig.platform.clone();
        let dir = rig.dir;
        drop((rig.plugin, rig.monitor));

        let again = ClipboardPlugin::new(
            &ClipboardConfig {
                enabled: true,
                ..ClipboardConfig::default()
            },
            &PasteConfig::default(),
            platform,
            Some(file),
        );
        open(&again);
        let shared = again.shared.as_ref().unwrap();
        let items = shared.snapshot();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].image.unwrap().hash, hash);
        let mut names: Vec<String> = fs::read_dir(store.dir())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                format!("{hash:016x}.png"),
                format!("{hash:016x}.thumb.png"),
                "keep-me.png".to_owned()
            ]
        );
        drop(dir);
    }

    #[test]
    fn entries_whose_image_files_are_gone_are_dropped_on_start() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE_NAME);
        fs::write(
            &file,
            r#"{"version":2,"items":[
                {"text":"","copied_at":3,"image":{"hash":99,"width":2,"height":2,"bytes":10}},
                {"text":"/a/b.txt","copied_at":2,"files":["/a/b.txt"]},
                {"text":"hello","copied_at":1}
            ]}"#,
        )
        .unwrap();
        let plugin = plugin(&MockPlatform::empty(), true, Some(file.clone()));
        plugin.refresh().unwrap();
        let items = plugin.shared.as_ref().unwrap().snapshot();
        let kinds: Vec<_> = items.iter().map(|e| e.kind()).collect();
        assert_eq!(kinds, [Kind::Files, Kind::Text]);
        // And the file was rewritten without it.
        assert!(!fs::read_to_string(&file).unwrap().contains("\"hash\""));
    }

    #[test]
    fn an_unreadable_history_does_not_cost_the_images() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE_NAME);
        fs::write(&file, "{ broken").unwrap();
        let store = MediaStore::new(dir.path().join(clipboard_store::DIR_NAME));
        store.write(5, b"png", b"thumb").unwrap();
        let plugin = plugin(&MockPlatform::empty(), true, Some(file));
        plugin.refresh().unwrap();
        // The broken file was moved aside, and what it may have listed is kept
        // for the user to recover.
        assert!(store.contains(5));
        assert!(plugin.shared.as_ref().unwrap().snapshot().is_empty());
    }

    #[test]
    fn image_entries_are_found_by_searching_for_image() {
        let mut rig = Rig::new();
        rig.copy_text("grocery list");
        rig.copy_image(&picture(1));
        let titles: Vec<_> = rig
            .plugin
            .rows("image", 100)
            .into_iter()
            .map(|r| r.title)
            .collect();
        assert_eq!(titles, ["Image 8 × 6"]);
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
    fn sizes_are_human() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1023), "1023 B");
        assert_eq!(human_size(1024), "1 KB");
        assert_eq!(human_size(340 * 1024 + 5), "340 KB");
        assert_eq!(human_size(2_500_000), "2.4 MB");
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
