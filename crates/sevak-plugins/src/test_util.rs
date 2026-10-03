//! A recording [`PlatformProvider`] for plugin tests.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sevak_core::{AppEntry, IconData, IconSource, LaunchTarget};
use sevak_platform::{PlatformError, PlatformProvider, Result};

#[derive(Default)]
pub struct MockPlatform {
    pub apps: Mutex<Vec<AppEntry>>,
    pub launched: Mutex<Vec<LaunchTarget>>,
    pub opened_paths: Mutex<Vec<PathBuf>>,
    pub opened_urls: Mutex<Vec<String>>,
    pub clipboard: Mutex<Vec<String>>,
    pub revealed: Mutex<Vec<PathBuf>>,
    pub elevated: Mutex<Vec<LaunchTarget>>,
    /// What `can_run_as_admin` reports; off by default, like most platforms.
    pub admin_supported: bool,
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

    /// A platform that can run applications as administrator (like Windows).
    pub fn with_admin() -> Arc<Self> {
        Arc::new(Self {
            admin_supported: true,
            ..Self::default()
        })
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

    fn reveal_path(&self, path: &Path) -> Result<()> {
        self.revealed.lock().unwrap().push(path.to_path_buf());
        Ok(())
    }

    fn can_run_as_admin(&self) -> bool {
        self.admin_supported
    }

    fn launch_as_admin(&self, target: &LaunchTarget) -> Result<()> {
        self.elevated.lock().unwrap().push(target.clone());
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
}
