//! Self-update from GitHub Releases (`tauri-plugin-updater`).
//!
//! Every release publishes a signed `latest.json` (see `.github/workflows/release.yml`).
//! Sevak checks it shortly after startup, then every six hours, and when the
//! launcher opens if the last check is over an hour old. All of that needs
//! `general.check_for_updates`; the tray's "Check for updates" always works.
//! An available update is only installed after the user agrees in a dialog;
//! its signature is verified against the public key in `tauri.conf.json`.
//!
//! Package managers that update Sevak themselves (Scoop, the AUR package) ship
//! a `package-manager` marker file (see [`ManagedBy`]); self-update is then off
//! so the two never fight over the installation.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::search;
use crate::state::AppState;

/// The first automatic check waits for startup (indexing, hotkey) to settle.
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(60);
/// How often the background thread checks.
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// Opening the launcher also checks, if the last check is at least this old.
const OPEN_CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);
/// The thread wakes this often to compare the wall clock. A single long sleep
/// does not count the time the computer spent suspended, so a laptop that is
/// asleep most of the day would otherwise check far too rarely.
const TICK: Duration = Duration::from_secs(5 * 60);
const TITLE: &str = "Sevak update";

/// One check at a time (the periodic one and a tray click can overlap).
static CHECKING: AtomicBool = AtomicBool::new(false);
/// A version the user answered "Later" to; automatic checks don't ask again
/// for it until Sevak restarts. A manual check always asks.
static POSTPONED: Mutex<Option<String>> = Mutex::new(None);
/// When the last automatic check started (wall clock).
static LAST_CHECK: Mutex<Option<SystemTime>> = Mutex::new(None);

pub fn plugin() -> tauri::plugin::TauriPlugin<Wry, tauri_plugin_updater::Config> {
    tauri_plugin_updater::Builder::new().build()
}

/// Whether Sevak updates itself at all: not in debug builds (their version is
/// whatever the repository says, so every release would look like an update)
/// and not when a package manager does it.
fn self_update_possible() -> bool {
    if cfg!(debug_assertions) {
        tracing::debug!("automatic update checks are off in debug builds");
        return false;
    }
    if let Some(managed) = ManagedBy::detect() {
        tracing::info!(
            manager = managed.name,
            "updates are managed by a package manager"
        );
        return false;
    }
    true
}

/// True when the last automatic check is at least `interval` old. A clock set
/// back counts as due.
fn due(interval: Duration) -> bool {
    is_due(
        *LAST_CHECK.lock().unwrap_or_else(|p| p.into_inner()),
        interval,
    )
}

fn is_due(last: Option<SystemTime>, interval: Duration) -> bool {
    last.is_none_or(|at| at.elapsed().map_or(true, |age| age >= interval))
}

/// Starts the background checks.
pub fn start(app: &AppHandle) {
    if !self_update_possible() {
        return;
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("sevak-updates".into())
        .spawn(move || {
            std::thread::sleep(FIRST_CHECK_DELAY);
            loop {
                if due(CHECK_INTERVAL) {
                    check_automatically(&app);
                }
                std::thread::sleep(TICK);
            }
        });
    if let Err(err) = spawned {
        tracing::error!("could not start the update checker: {err}");
    }
}

/// Called when the launcher opens: checks in the background if the last check
/// is more than an hour old.
pub fn check_on_open(app: &AppHandle) {
    if cfg!(debug_assertions) || !due(OPEN_CHECK_INTERVAL) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if self_update_possible() && due(OPEN_CHECK_INTERVAL) {
            check_automatically_async(&app).await;
        }
    });
}

fn check_automatically(app: &AppHandle) {
    tauri::async_runtime::block_on(check_automatically_async(app));
}

async fn check_automatically_async(app: &AppHandle) {
    let enabled = app
        .try_state::<AppState>()
        .is_some_and(|state| state.config().general.check_for_updates);
    if !enabled {
        return;
    }
    *LAST_CHECK.lock().unwrap_or_else(|p| p.into_inner()) = Some(SystemTime::now());
    check(app, Trigger::Automatic).await;
}

/// "Check for updates" from the tray: reports the outcome either way.
pub fn check_now(app: &AppHandle) {
    if let Some(managed) = ManagedBy::detect() {
        message(app, &managed.hint(), MessageDialogKind::Info);
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move { check(&app, Trigger::Manual).await });
}

/// The package manager that installed Sevak, when it handles updates itself.
///
/// Packages declare it with a `package-manager` file whose first line names
/// the manager and whose optional second line is the update command, e.g.
/// `Scoop` / `scoop update sevak`. The file sits next to the executable
/// (Scoop), or in `<prefix>/share/sevak/` for an executable in `<prefix>/bin`
/// (Linux distribution packages).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedBy {
    pub name: String,
    pub command: Option<String>,
}

impl ManagedBy {
    const FILE: &'static str = "package-manager";

    pub fn detect() -> Option<Self> {
        let exe = std::env::current_exe().ok()?;
        Self::marker_paths(&exe)
            .iter()
            .find_map(|path| std::fs::read_to_string(path).ok())
            .map(|text| Self::parse(&text))
    }

    fn marker_paths(exe: &Path) -> Vec<PathBuf> {
        let Some(dir) = exe.parent() else {
            return Vec::new();
        };
        let mut paths = vec![dir.join(Self::FILE)];
        if let Some(prefix) = dir.parent() {
            paths.push(prefix.join("share").join("sevak").join(Self::FILE));
        }
        paths
    }

    fn parse(text: &str) -> Self {
        let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
        let name = lines
            .next()
            .map_or_else(|| "your package manager".to_owned(), str::to_owned);
        let command = lines.next().map(str::to_owned);
        Self { name, command }
    }

    fn hint(&self) -> String {
        match &self.command {
            Some(command) => format!(
                "Sevak was installed with {}, which keeps it up to date.\n\nTo update now, run:\n{command}",
                self.name
            ),
            None => format!(
                "Sevak was installed with {}, which keeps it up to date.",
                self.name
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trigger {
    Automatic,
    Manual,
}

async fn check(app: &AppHandle, trigger: Trigger) {
    if CHECKING.swap(true, Ordering::SeqCst) {
        if trigger == Trigger::Manual {
            message(
                app,
                "Sevak is already checking for updates.",
                MessageDialogKind::Info,
            );
        }
        return;
    }
    let result = find_update(app).await;
    CHECKING.store(false, Ordering::SeqCst);

    let current = app.package_info().version.to_string();
    match result {
        Ok(Some(update)) => offer(app, update, trigger).await,
        Ok(None) => {
            tracing::info!(current, "no update available");
            if trigger == Trigger::Manual {
                message(
                    app,
                    &format!("You have the latest version of Sevak ({current})."),
                    MessageDialogKind::Info,
                );
            }
        }
        Err(err) => {
            tracing::warn!("update check failed: {err}");
            if trigger == Trigger::Manual {
                message(
                    app,
                    &format!("Could not check for updates.\n\n{err}"),
                    MessageDialogKind::Error,
                );
            }
        }
    }
}

async fn find_update(app: &AppHandle) -> Result<Option<Update>, String> {
    let updater = app.updater().map_err(|err| err.to_string())?;
    updater.check().await.map_err(|err| err.to_string())
}

async fn offer(app: &AppHandle, update: Update, trigger: Trigger) {
    let version = update.version.clone();
    tracing::info!(
        current = update.current_version,
        version,
        "update available"
    );
    {
        let postponed = POSTPONED.lock().unwrap_or_else(|p| p.into_inner());
        if trigger == Trigger::Automatic && postponed.as_deref() == Some(version.as_str()) {
            return;
        }
    }

    let prompt = format!(
        "Sevak {version} is available (you have {}).\n\n\
         Install it now? Sevak restarts when the update is done.\n\n\
         Release notes: https://github.com/ninad-k/Sevak/releases/tag/v{version}",
        update.current_version
    );
    let dialog = app
        .dialog()
        .message(prompt)
        .title(TITLE)
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Install and restart".to_owned(),
            "Later".to_owned(),
        ));
    // blocking_show must not run on the main thread; this is a runtime worker.
    let accepted = tauri::async_runtime::spawn_blocking(move || dialog.blocking_show())
        .await
        .unwrap_or(false);
    if !accepted {
        tracing::info!(version, "update postponed");
        *POSTPONED.lock().unwrap_or_else(|p| p.into_inner()) = Some(version);
        return;
    }

    tracing::info!(version, "downloading and installing update");
    // On Windows the installer closes Sevak, so save usage statistics first.
    search::save_usage(app);
    match update.download_and_install(|_, _| {}, || {}).await {
        Ok(()) => {
            tracing::info!(version, "update installed; restarting");
            app.restart();
        }
        Err(err) => {
            tracing::error!(version, "update failed: {err}");
            message(
                app,
                &format!(
                    "Sevak {version} could not be installed.\n\n{err}\n\n\
                     You can download it from https://github.com/ninad-k/Sevak/releases/latest"
                ),
                MessageDialogKind::Error,
            );
        }
    }
}

fn message(app: &AppHandle, text: &str, kind: MessageDialogKind) {
    app.dialog()
        .message(text)
        .title(TITLE)
        .kind(kind)
        .buttons(MessageDialogButtons::Ok)
        .show(|_| {});
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_names_the_manager_and_its_update_command() {
        let scoop = ManagedBy::parse("Scoop\r\nscoop update sevak\r\n");
        assert_eq!(scoop.name, "Scoop");
        assert_eq!(scoop.command.as_deref(), Some("scoop update sevak"));
        assert!(scoop.hint().ends_with("scoop update sevak"));

        let bare = ManagedBy::parse("\n  AUR  \n");
        assert_eq!(bare.name, "AUR");
        assert_eq!(bare.command, None);
        assert_eq!(ManagedBy::parse("").name, "your package manager");
    }

    #[test]
    fn marker_is_found_next_to_the_exe_or_in_share() {
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let paths = ManagedBy::marker_paths(&bin.join("sevak"));
        assert_eq!(
            paths,
            [
                bin.join("package-manager"),
                root.path()
                    .join("share")
                    .join("sevak")
                    .join("package-manager"),
            ]
        );
    }

    #[test]
    fn a_check_is_due_when_the_last_one_is_old_enough_or_the_clock_went_back() {
        let hour = Duration::from_secs(3600);
        let ago = |secs| Some(SystemTime::now() - Duration::from_secs(secs));
        assert!(is_due(None, hour));
        assert!(!is_due(ago(60), hour));
        assert!(is_due(ago(2 * 3600), hour));
        assert!(is_due(Some(SystemTime::now() + hour), hour));
    }

    /// The plugin parses this section at startup and Sevak cannot start if it
    /// is invalid, so catch mistakes here rather than in a release.
    #[test]
    fn updater_config_in_tauri_conf_is_valid() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let updater: tauri_plugin_updater::Config =
            serde_json::from_value(conf["plugins"]["updater"].clone()).unwrap();
        assert_eq!(
            updater.endpoints[0].as_str(),
            "https://github.com/ninad-k/Sevak/releases/latest/download/latest.json"
        );
        assert!(!updater.pubkey.is_empty());
        assert!(!updater.dangerous_insecure_transport_protocol);
    }
}
