//! A recording [`PlatformProvider`] for plugin tests.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sevak_core::{AppEntry, IconData, IconSource, LaunchTarget};
use sevak_platform::{
    ClipboardRead, ForegroundApp, PasteOutcome, PasteSupport, PlatformError, PlatformProvider,
    Result,
};

#[derive(Default)]
pub struct MockPlatform {
    pub apps: Mutex<Vec<AppEntry>>,
    pub launched: Mutex<Vec<LaunchTarget>>,
    pub opened_paths: Mutex<Vec<PathBuf>>,
    pub opened_urls: Mutex<Vec<String>>,
    pub clipboard: Mutex<Vec<String>>,
    /// `(text, restore_clipboard)` of every paste.
    pub pasted: Mutex<Vec<(String, bool)>>,
    /// When set, pasting is unavailable for this reason.
    pub copy_only: Mutex<Option<String>>,
    /// What `clipboard_text` returns (the `{clipboard}` placeholder).
    pub clipboard_now: Mutex<Option<String>>,
    pub foreground: Mutex<Option<ForegroundApp>>,
    pub clipboard_sequence: Mutex<Option<u64>>,
    /// What `read_clipboard` returns; `None` makes it fail like a busy clipboard.
    pub clipboard_read: Mutex<Option<ClipboardRead>>,
}

impl MockPlatform {
    pub fn with_apps(apps: Vec<AppEntry>) -> Arc<Self> {
        Arc::new(Self {
            apps: Mutex::new(apps),
            ..Self::default()
        })
    }

    pub fn empty() -> Arc<Self> {
        Arc::new(Self::default())
    }
}

impl PlatformProvider for MockPlatform {
    fn list_applications(&self) -> Result<Vec<AppEntry>> {
        Ok(self.apps.lock().unwrap().clone())
    }

    fn launch(&self, target: &LaunchTarget) -> Result<()> {
        self.launched.lock().unwrap().push(target.clone());
        Ok(())
    }

    fn open_path(&self, path: &Path) -> Result<()> {
        self.opened_paths.lock().unwrap().push(path.to_path_buf());
        Ok(())
    }

    fn open_url(&self, url: &str) -> Result<()> {
        self.opened_urls.lock().unwrap().push(url.to_owned());
        Ok(())
    }

    fn load_icon(&self, _source: &IconSource, _size: u32) -> Result<IconData> {
        Err(PlatformError::Unsupported("icons in tests"))
    }

    fn set_clipboard_text(&self, text: &str) -> Result<()> {
        self.clipboard.lock().unwrap().push(text.to_owned());
        Ok(())
    }

    fn clipboard_text(&self) -> Result<Option<String>> {
        Ok(self.clipboard_now.lock().unwrap().clone())
    }

    fn foreground_app(&self) -> Option<ForegroundApp> {
        self.foreground.lock().unwrap().clone()
    }

    fn paste_support(&self) -> PasteSupport {
        match self.copy_only.lock().unwrap().clone() {
            Some(reason) => PasteSupport::CopyOnly(reason),
            None => PasteSupport::Available,
        }
    }

    fn paste_text(&self, text: &str, restore_clipboard: bool) -> Result<PasteOutcome> {
        self.pasted
            .lock()
            .unwrap()
            .push((text.to_owned(), restore_clipboard));
        Ok(PasteOutcome::Pasted)
    }

    fn clipboard_sequence(&self) -> Option<u64> {
        *self.clipboard_sequence.lock().unwrap()
    }

    fn read_clipboard(&self) -> Result<ClipboardRead> {
        self.clipboard_read
            .lock()
            .unwrap()
            .clone()
            .ok_or(PlatformError::Unsupported("a busy clipboard"))
    }
}
