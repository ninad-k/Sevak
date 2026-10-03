//! Tauri builder wiring.

use anyhow::Context;
use sevak_core::Config;
use sevak_platform::{AppPaths, DisplayServer};
use tauri::{AppHandle, Emitter, Manager, RunEvent};

use crate::cli::{self, Launch};
use crate::state::AppState;
use crate::{commands, hotkey, tray, window};

pub fn run(
    paths: AppPaths,
    server: DisplayServer,
    config: Config,
    launch: Launch,
) -> anyhow::Result<()> {
    let strategy = server.hotkey_strategy();
    let state = AppState::new(paths, server, config);

    let mut builder = tauri::Builder::default()
        // Must be first so a second instance exits before anything else starts.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            let launch = cli::parse_remote(&argv);
            tracing::info!(?launch, "command forwarded from another instance");
            match launch {
                Launch::Show => window::show(app),
                Launch::Toggle => window::toggle(app),
                Launch::Background => {}
                Launch::Quit => app.exit(0),
            }
        }));
    if let Some(plugin) = hotkey::plugin(strategy) {
        builder = builder.plugin(plugin);
    }

    builder
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::hide_window,
            commands::get_status,
            commands::set_content_height
        ])
        .on_window_event(window::on_window_event)
        .setup(move |app| {
            let handle = app.handle();
            tray::init(handle);
            hotkey::apply(handle);
            window::apply_configured_width(handle);
            tracing::info!(display = ?server, ?launch, "sevak is ready");

            match launch {
                Launch::Show | Launch::Toggle => window::show(handle),
                Launch::Background => {}
                Launch::Quit => handle.exit(0),
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .context("failed to build the application")?
        .run(|_app, event| {
            // Sevak stays resident: only an explicit `exit(code)` quits.
            if let RunEvent::ExitRequested {
                api, code: None, ..
            } = event
            {
                api.prevent_exit();
            }
        });
    Ok(())
}

/// Re-reads the config file and re-applies everything derived from it.
/// A config that fails to load is ignored so the current one keeps working.
pub fn reload(app: &AppHandle) {
    let state = app.state::<AppState>();
    match Config::load_or_create(&state.paths.config_file) {
        Ok((config, origin)) => {
            tracing::info!(?origin, "configuration reloaded");
            *state
                .config
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = config;
        }
        Err(err) => tracing::warn!("keeping the current configuration: {err}"),
    }

    hotkey::apply(app);
    window::apply_configured_width(app);
    // Phase 2 rebuilds the search index here.

    if let Err(err) = app.emit_to(window::MAIN_LABEL, window::EVENT_STATUS, state.status()) {
        tracing::warn!("could not emit {}: {err}", window::EVENT_STATUS);
    }
}
