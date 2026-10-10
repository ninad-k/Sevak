//! Startup commands share the same per-user backend as the Settings checkbox.

use std::path::Path;
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
    pub error: Option<String>,
}

pub fn backend(config_file: &Path) -> Result<Native, String> {
    Native::new(LaunchCommand::current(Some(config_file))?)
}

pub fn status(config_file: &Path) -> StartupStatus {
    match backend(config_file).and_then(|backend| startup::status(&backend)) {
        Ok(state) => StartupStatus {
            registered: state.registered,
            enabled: state.enabled,
            error: None,
        },
        Err(error) => StartupStatus {
            registered: false,
            enabled: false,
            error: Some(error),
        },
    }
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
