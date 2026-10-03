//! A recording [`PlatformProvider`] for plugin tests.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sevak_core::{AppEntry, IconData, IconSource, LaunchTarget, ShellConfig};
use sevak_platform::{
    ClipboardRead, ForegroundApp, PasteOutcome, PasteSupport, PlatformError, PlatformProvider,
    Result, SettingsPage, SystemCommand,
};

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
    /// What `supported_system_commands` / `supported_settings_pages` report.
    pub system_commands: Mutex<Vec<SystemCommand>>,
    pub settings_pages: Mutex<Vec<SettingsPage>>,
    pub ran_commands: Mutex<Vec<SystemCommand>>,
    pub opened_settings: Mutex<Vec<SettingsPage>>,
    pub terminal_runs: Mutex<Vec<(String, ShellConfig)>>,
    /// Folders terminals were opened in.
    pub terminal_dirs: Mutex<Vec<PathBuf>>,
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
    /// File lists put on the clipboard.
    pub clipboard_files: Mutex<Vec<Vec<PathBuf>>>,
    /// Paths `move_to_trash` was given (the mock does not touch the disk).
    pub trashed: Mutex<Vec<PathBuf>>,
    /// Paths `move_to_trash` fails for.
    pub trash_refuses: Mutex<Vec<PathBuf>>,
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

    fn run_in_terminal(&self, command: &str, config: &ShellConfig) -> Result<()> {
        self.terminal_runs
            .lock()
            .unwrap()
            .push((command.to_owned(), config.clone()));
        Ok(())
    }

    fn open_terminal_in(&self, dir: &Path, _config: &ShellConfig) -> Result<()> {
        self.terminal_dirs.lock().unwrap().push(dir.to_path_buf());
        Ok(())
    }

    fn set_clipboard_text(&self, text: &str) -> Result<()> {
        self.clipboard.lock().unwrap().push(text.to_owned());
        Ok(())
    }

    fn set_clipboard_files(&self, paths: &[PathBuf]) -> Result<()> {
        self.clipboard_files.lock().unwrap().push(paths.to_vec());
        Ok(())
    }

    fn move_to_trash(&self, path: &Path) -> Result<()> {
        if self.trash_refuses.lock().unwrap().iter().any(|p| p == path) {
            return Err(PlatformError::Unsupported("trashing this item"));
        }
        self.trashed.lock().unwrap().push(path.to_path_buf());
        Ok(())
    }

    fn supported_system_commands(&self) -> Vec<SystemCommand> {
        self.system_commands.lock().unwrap().clone()
    }

    fn run_system_command(&self, command: SystemCommand) -> Result<()> {
        self.ran_commands.lock().unwrap().push(command);
        Ok(())
    }

    fn supported_settings_pages(&self) -> Vec<SettingsPage> {
        self.settings_pages.lock().unwrap().clone()
    }

    fn open_settings_page(&self, page: SettingsPage) -> Result<()> {
        self.opened_settings.lock().unwrap().push(page);
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
