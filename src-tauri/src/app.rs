//! Tauri builder wiring.

use anyhow::Context;
use sevak_core::Config;
use sevak_platform::{AppPaths, DisplayServer};
use tauri::{AppHandle, Emitter, Manager, RunEvent};

use crate::cli::{self, Launch};
use crate::state::AppState;
use crate::{commands, hotkey, icons, search, tray, window};

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
                Launch::Quit => quit(app),
            }
        }));
    if let Some(plugin) = hotkey::plugin(strategy) {
        builder = builder.plugin(plugin);
    }

    builder
        .manage(state)
        .register_asynchronous_uri_scheme_protocol(icons::SCHEME, |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            // Icon extraction is slow (COM, decoding): never on the caller's thread.
            tauri::async_runtime::spawn_blocking(move || {
                responder.respond(icons::respond(&app, request.uri().path()));
            });
        })
        .invoke_handler(tauri::generate_handler![
            commands::hide_window,
            commands::get_status,
            commands::set_content_height,
            commands::search,
            commands::execute
        ])
        .on_window_event(window::on_window_event)
        .setup(move |app| {
            let handle = app.handle();
            tray::init(handle);
            hotkey::apply(handle);
            window::apply_configured_width(handle);
            search::start(handle);
            tracing::info!(display = ?server, ?launch, "sevak is ready");

            match launch {
                Launch::Show | Launch::Toggle => window::show(handle),
                Launch::Background => {}
                Launch::Quit => quit(handle),
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
    // The index is rebuilt in the background and swapped in when ready.
    search::reload(app, &state.config());

    if let Err(err) = app.emit_to(window::MAIN_LABEL, window::EVENT_STATUS, state.status()) {
        tracing::warn!("could not emit {}: {err}", window::EVENT_STATUS);
    }
}

/// Saves the usage statistics, then exits. The one way Sevak quits.
pub fn quit(app: &AppHandle) {
    tracing::info!("quitting");
    search::save_usage(app);
    app.exit(0);
}
