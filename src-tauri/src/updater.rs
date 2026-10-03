//! Self-update from GitHub Releases (`tauri-plugin-updater`).
//!
//! Every release publishes a signed `latest.json` (see `.github/workflows/release.yml`).
//! Sevak checks it shortly after startup and then once a day, when
//! `general.check_for_updates` is on, and from the tray's "Check for updates".
//! An available update is only installed after the user agrees in a dialog;
//! its signature is verified against the public key in `tauri.conf.json`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::search;
use crate::state::AppState;

/// The first automatic check waits for startup (indexing, hotkey) to settle.
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(60);
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const TITLE: &str = "Sevak update";

/// One check at a time (the daily one and a tray click can overlap).
static CHECKING: AtomicBool = AtomicBool::new(false);
/// A version the user answered "Later" to; automatic checks don't ask again
/// for it until Sevak restarts. A manual check always asks.
static POSTPONED: Mutex<Option<String>> = Mutex::new(None);

pub fn plugin() -> tauri::plugin::TauriPlugin<Wry, tauri_plugin_updater::Config> {
    tauri_plugin_updater::Builder::new().build()
}

/// Starts the background checks. Debug builds skip them: their version is
/// whatever the repository says, so every release would look like an update.
pub fn start(app: &AppHandle) {
    if cfg!(debug_assertions) {
        tracing::debug!("automatic update checks are off in debug builds");
        return;
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("sevak-updates".into())
        .spawn(move || {
            std::thread::sleep(FIRST_CHECK_DELAY);
            loop {
                let enabled = app
                    .try_state::<AppState>()
                    .is_some_and(|state| state.config().general.check_for_updates);
                if enabled {
                    tauri::async_runtime::block_on(check(&app, Trigger::Automatic));
                }
                std::thread::sleep(CHECK_INTERVAL);
            }
        });
    if let Err(err) = spawned {
        tracing::error!("could not start the update checker: {err}");
    }
}

/// "Check for updates" from the tray: reports the outcome either way.
pub fn check_now(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { check(&app, Trigger::Manual).await });
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
