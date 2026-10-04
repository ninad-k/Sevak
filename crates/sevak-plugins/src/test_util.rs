//! A recording [`PlatformProvider`] for plugin tests.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sevak_core::{AppEntry, ClipContent, IconData, IconSource, LaunchTarget, ShellConfig};
use sevak_platform::{
    ClipboardMedia, ClipboardRead, Contact, ContactsAccess, DeepLink, Drive, ForegroundApp,
    MediaCommand, MediaRequest, NowPlaying, PasteOutcome, PasteSupport, PlatformError,
    PlatformProvider, ProcessInfo, Result, RunningApp, SettingsPage, ShellQuoting, Spelling,
    SystemCommand, Task, TaskKind,
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
    /// What `shell_quoting` reports; `None` is POSIX.
    pub shell_quoting: Mutex<Option<ShellQuoting>>,
    /// Folders terminals were opened in.
    pub terminal_dirs: Mutex<Vec<PathBuf>>,
    /// `(text, restore_clipboard)` of every paste.
    pub pasted: Mutex<Vec<(String, bool)>>,
    /// When set, pasting is unavailable for this reason.
    pub copy_only: Mutex<Option<String>>,
    /// What `clipboard_text` returns (the `{clipboard}` placeholder).
    pub clipboard_now: Mutex<Option<String>>,
    pub foreground: Mutex<Option<ForegroundApp>>,
    /// What `identifies_apps` reports (a system that can tell the app in front).
    pub identifies_apps: Mutex<bool>,
    /// What `history_sealer` returns; `None` is a system without encryption.
    pub sealer: Mutex<Option<Arc<dyn sevak_core::sealed::Sealer>>>,
    pub clipboard_sequence: Mutex<Option<u64>>,
    /// What `read_clipboard` returns; `None` makes it fail like a busy clipboard.
    pub clipboard_read: Mutex<Option<ClipboardRead>>,
    /// What `read_clipboard_media` returns.
    pub clipboard_media: Mutex<ClipboardMedia>,
    /// The requests `read_clipboard_media` got.
    pub media_requests: Mutex<Vec<(bool, bool)>>,
    /// `(content, restore_clipboard)` of every image or files paste.
    pub pasted_clips: Mutex<Vec<(ClipContent, bool)>>,
    /// Every image or files copy.
    pub copied_clips: Mutex<Vec<ClipContent>>,
    /// File lists put on the clipboard.
    pub clipboard_files: Mutex<Vec<Vec<PathBuf>>>,
    /// Paths `move_to_trash` was given (the mock does not touch the disk).
    pub trashed: Mutex<Vec<PathBuf>>,
    /// Paths `move_to_trash` fails for.
    pub trash_refuses: Mutex<Vec<PathBuf>>,
    /// What `supported_tasks` reports, and every task `run_task` was given.
    pub task_kinds: Mutex<Vec<TaskKind>>,
    pub ran_tasks: Mutex<Vec<Task>>,
    /// What the live listings return.
    pub processes: Mutex<Vec<ProcessInfo>>,
    pub running_apps: Mutex<Vec<RunningApp>>,
    pub drives: Mutex<Vec<Drive>>,
    /// How many times each listing was asked for (processes, apps, drives).
    pub list_calls: Mutex<(usize, usize, usize)>,
    /// What `supported_media_commands` reports, and every button pressed.
    pub media_commands: Mutex<Vec<MediaCommand>>,
    pub media_pressed: Mutex<Vec<MediaCommand>>,
    /// What `now_playing` returns (and whether it can be asked at all).
    pub playing: Mutex<Option<NowPlaying>>,
    pub now_playing_supported: Mutex<bool>,
    pub now_playing_calls: Mutex<usize>,
    /// URLs of the `DeepLink`s opened.
    pub opened_links: Mutex<Vec<String>>,
    /// What `contacts_access` reports; `None` is "unsupported".
    pub contacts_access: Mutex<Option<ContactsAccess>>,
    /// What `request_contacts_access` switches `contacts_access` to.
    pub contacts_after_request: Mutex<Option<ContactsAccess>>,
    pub system_contacts: Mutex<Vec<Contact>>,
    pub evolution_dbs: Mutex<Vec<PathBuf>>,
    /// What `system_definition` answers, by word.
    pub definitions: Mutex<Vec<(String, String)>>,
    /// What `system_spelling` answers, by word.
    pub spellings: Mutex<Vec<(String, Spelling)>>,
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

    fn shell_quoting(&self, _config: &ShellConfig) -> ShellQuoting {
        self.shell_quoting
            .lock()
            .unwrap()
            .unwrap_or(ShellQuoting::Posix)
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

    fn paste_clip(&self, content: &ClipContent, restore_clipboard: bool) -> Result<PasteOutcome> {
        self.pasted_clips
            .lock()
            .unwrap()
            .push((content.clone(), restore_clipboard));
        Ok(PasteOutcome::Pasted)
    }

    fn set_clipboard_clip(&self, content: &ClipContent) -> Result<()> {
        self.copied_clips.lock().unwrap().push(content.clone());
        Ok(())
    }

    fn read_clipboard_media(&self, request: MediaRequest) -> ClipboardMedia {
        self.media_requests
            .lock()
            .unwrap()
            .push((request.files, request.image));
        self.clipboard_media.lock().unwrap().clone()
    }

    fn identifies_apps(&self) -> bool {
        *self.identifies_apps.lock().unwrap()
    }

    fn history_sealer(&self) -> Option<Arc<dyn sevak_core::sealed::Sealer>> {
        self.sealer.lock().unwrap().clone()
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

    fn supported_tasks(&self) -> Vec<TaskKind> {
        self.task_kinds.lock().unwrap().clone()
    }

    fn run_task(&self, task: &Task) -> Result<()> {
        self.ran_tasks.lock().unwrap().push(task.clone());
        Ok(())
    }

    fn list_processes(&self) -> Result<Vec<ProcessInfo>> {
        self.list_calls.lock().unwrap().0 += 1;
        Ok(self.processes.lock().unwrap().clone())
    }

    fn list_running_apps(&self) -> Result<Vec<RunningApp>> {
        self.list_calls.lock().unwrap().1 += 1;
        Ok(self.running_apps.lock().unwrap().clone())
    }

    fn list_removable_drives(&self) -> Result<Vec<Drive>> {
        self.list_calls.lock().unwrap().2 += 1;
        Ok(self.drives.lock().unwrap().clone())
    }

    fn supported_media_commands(&self) -> Vec<MediaCommand> {
        self.media_commands.lock().unwrap().clone()
    }

    fn media_control(&self, command: MediaCommand) -> Result<()> {
        self.media_pressed.lock().unwrap().push(command);
        Ok(())
    }

    fn now_playing_available(&self) -> bool {
        *self.now_playing_supported.lock().unwrap()
    }

    fn now_playing(&self) -> Result<Option<NowPlaying>> {
        *self.now_playing_calls.lock().unwrap() += 1;
        Ok(self.playing.lock().unwrap().clone())
    }

    fn open_link(&self, link: &DeepLink) -> Result<()> {
        self.opened_links
            .lock()
            .unwrap()
            .push(link.as_str().to_owned());
        Ok(())
    }

    fn contacts_access(&self) -> ContactsAccess {
        self.contacts_access
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(ContactsAccess::Unsupported)
    }

    fn request_contacts_access(&self) -> Result<ContactsAccess> {
        if let Some(after) = self.contacts_after_request.lock().unwrap().clone() {
            *self.contacts_access.lock().unwrap() = Some(after);
        }
        Ok(self.contacts_access())
    }

    fn system_contacts(&self) -> Result<Vec<Contact>> {
        Ok(self.system_contacts.lock().unwrap().clone())
    }

    fn evolution_address_books(&self) -> Vec<PathBuf> {
        self.evolution_dbs.lock().unwrap().clone()
    }

    fn system_definition(&self, word: &str) -> Option<String> {
        let definitions = self.definitions.lock().unwrap();
        definitions
            .iter()
            .find(|(w, _)| w == word)
            .map(|(_, d)| d.clone())
    }

    fn system_spelling(&self, word: &str) -> Option<Spelling> {
        let spellings = self.spellings.lock().unwrap();
        spellings
            .iter()
            .find(|(w, _)| w == word)
            .map(|(_, s)| s.clone())
    }
}
