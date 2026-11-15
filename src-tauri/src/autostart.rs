//! Startup commands share the same per-user backend as the Settings checkbox.
//!
//! Every in-process change (routine sync, Settings save, repair) goes through
//! `sevak_platform::startup::update`, which serializes the whole
//! snapshot/change/persist/rollback sequence. The installer's `--set-startup`
//! runs in a separate process and is not coordinated with a running Sevak;
//! repair re-reads the saved preference to detect such a change.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde::Serialize;
use sevak_platform::startup::{self, Backend, LaunchCommand, Native};
use sevak_platform::AppPaths;
use tauri::{AppHandle, Manager};

use crate::cli::StartupAction;
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct StartupStatus {
    pub registered: bool,
    pub enabled: bool,
    /// The entry starts another copy of Sevak (portable or development).
    pub other_copy: bool,
    pub error: Option<String>,
    /// The preference saved on disk now, which the installer or CLI may have
    /// changed since Settings loaded. `None` when it cannot be read.
    pub saved: Option<bool>,
}

pub fn backend(config_file: &Path) -> Result<Native, String> {
    Native::new(LaunchCommand::current(Some(config_file))?)
}

pub fn status(config_file: &Path) -> StartupStatus {
    let saved = startup::saved_preference(config_file).ok();
    match backend(config_file).and_then(|backend| startup::status(&backend)) {
        Ok(state) => StartupStatus {
            registered: state.registered,
            enabled: state.enabled,
            other_copy: state.other_copy,
            error: None,
            saved,
        },
        Err(error) => unavailable(error, saved),
    }
}

fn unavailable(error: String, saved: Option<bool>) -> StartupStatus {
    StartupStatus {
        registered: false,
        enabled: false,
        other_copy: false,
        error: Some(error),
        saved,
    }
}

/// Reading the status runs launchctl on macOS and reads the registry on
/// Windows, so commands never do it on an async worker.
pub async fn status_off_thread(config_file: PathBuf) -> StartupStatus {
    off_thread(move || status(&config_file))
        .await
        .unwrap_or_else(|error| unavailable(error, None))
}

async fn off_thread<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| format!("Could not finish checking startup: {error}"))
}

#[tauri::command]
pub async fn get_startup_status(app: AppHandle) -> Result<StartupStatus, String> {
    let config_file = app.state::<AppState>().paths.config_file.clone();
    Ok(status_off_thread(config_file).await)
}

#[tauri::command]
pub async fn repair_startup(app: AppHandle) -> Result<StartupStatus, String> {
    let config_file = app.state::<AppState>().paths.config_file.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = startup::repair(&config_file, &backend(&config_file)?)?;
        Ok(StartupStatus {
            registered: state.registered,
            enabled: state.enabled,
            other_copy: state.other_copy,
            error: None,
            saved: Some(true),
        })
    })
    .await
    .map_err(|error| format!("Could not finish repairing startup: {error}"))?
}

/// A locked-down registry must not stop the launcher. Settings, in contrast,
/// returns a failed change rather than reporting a saved setting.
pub fn sync(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let result = backend(&state.paths.config_file).and_then(|backend| {
        startup::update(
            &backend,
            state.config().general.launch_at_login,
            false,
            || Ok(()),
        )
    });
    if let Err(error) = result {
        tracing::warn!("could not synchronize launch at sign-in: {error}");
    }
}

/// Runs before Tauri initialization, so installer invocations never show a
/// window, launch indexers, or forward a request to another running instance.
pub fn run_cli(action: StartupAction, config: Option<&Path>, receipt: Option<&Path>) -> ExitCode {
    let result: Result<u8, String> = (|| {
        let paths = AppPaths::resolve_with_config(config).map_err(|error| error.to_string())?;
        let backend = backend(&paths.config_file)?;
        match action {
            StartupAction::Set(enabled) => {
                startup::set_preference(&paths.config_file, enabled, &backend)?;
                Ok(0)
            }
            StartupAction::Status => {
                let enabled = startup::status(&backend)?.enabled;
                println!("{}", if enabled { "on" } else { "off" });
                Ok(u8::from(!enabled))
            }
            StartupAction::Remove => {
                backend.remove_owned()?;
                Ok(0)
            }
            StartupAction::Refresh => {
                startup::refresh(&backend)?;
                Ok(0)
            }
        }
    })();
    let code = match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("sevak: startup: {error}");
            2
        }
    };
    if let Some(path) = receipt {
        if let Err(error) = startup::write_atomic(path, format!("{code}\n").as_bytes()) {
            eprintln!("sevak: cannot write startup result: {error}");
            return ExitCode::from(2);
        }
    }
    ExitCode::from(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_reads_run_on_a_blocking_thread() {
        let caller = std::thread::current().id();
        let worker = tauri::async_runtime::block_on(off_thread(|| std::thread::current().id()));
        assert_ne!(worker.unwrap(), caller);
    }

    #[test]
    fn malformed_config_returns_installer_error_receipt_without_touching_startup() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("config.toml");
        let receipt = temp.path().join("installer-result.txt");
        let original = "[general]\nlaunch_at_login = 'not a boolean'\n";
        std::fs::write(&config, original).unwrap();
        // Config parsing precedes Backend::snapshot/set_enabled, so this test
        // exercises the actual CLI path without reading or mutating OS entries.
        assert_eq!(
            run_cli(StartupAction::Set(true), Some(&config), Some(&receipt)),
            ExitCode::from(2)
        );
        assert_eq!(std::fs::read_to_string(config).unwrap(), original);
        assert_eq!(std::fs::read_to_string(receipt).unwrap(), "2\n");
    }
}
