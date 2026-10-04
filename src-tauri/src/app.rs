//! Tauri builder wiring.

use std::path::Path;

use anyhow::Context;
use sevak_core::Config;
use sevak_platform::{AppPaths, DisplayServer};
use tauri::{AppHandle, Emitter, Manager, RunEvent};

use crate::cli::{self, Launch};
use crate::state::AppState;
use crate::{
    autostart, backdrop, commands, direct, expansion, file_buffer, hotkey, icons, search,
    selection, settings, takeover, themes, tray, updater, window, workflows,
};

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
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            let remote = cli::parse_remote(&argv);
            tracing::info!(launch = ?remote.launch, "command forwarded from another instance");
            note_ignored_config(app, remote.config.as_deref(), &cwd);
            match remote.launch {
                Launch::Show => window::show(app),
                Launch::Toggle => window::toggle(app),
                Launch::Query(query) => direct::open_with_query(app, query),
                Launch::Run(id) => direct::run_result(app, id),
                Launch::Actions => selection::trigger(app),
                Launch::Trigger { target, arg } => workflows::run_trigger(app, target, arg),
                Launch::Settings => settings::open(app),
                Launch::Background => {}
                Launch::Quit => quit(app),
            }
        }))
        .plugin(autostart::plugin())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(updater::plugin());
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
            commands::execute,
            commands::copy_result,
            commands::preview,
            commands::text_view,
            commands::set_large_type,
            commands::query_history,
            file_buffer::file_buffer_get,
            file_buffer::file_buffer_add,
            file_buffer::file_buffer_remove,
            file_buffer::file_buffer_clear,
            file_buffer::file_buffer_run,
            file_buffer::file_buffer_selection,
            direct::take_pending_show,
            settings::get_settings,
            settings::save_settings,
            settings::validate_hotkey,
            settings::suspend_hotkey,
            settings::resume_hotkey,
            settings::pick_directory,
            settings::pick_file,
            settings::clear_clipboard_history,
            settings::setup_wayland_hotkey,
            settings::start_hotkey_recording,
            settings::stop_hotkey_recording,
            takeover::takeover_hotkey,
            takeover::restore_takeover,
            settings::open_config_file,
            settings::open_log_dir,
            settings::close_settings,
            themes::list_themes,
            themes::save_theme,
            themes::use_builtin_theme,
            themes::import_theme,
            themes::export_theme,
            themes::open_themes_dir,
            themes::fetch_theme_gallery,
            themes::install_gallery_theme,
            workflows::list_workflows,
            workflows::load_workflow,
            workflows::check_workflow,
            workflows::save_workflow,
            workflows::create_workflow,
            workflows::delete_workflow,
            workflows::set_workflow_enabled,
            workflows::review_workflow,
            workflows::workflow_templates,
            workflows::open_workflows_folder,
            workflows::gallery_load,
            workflows::gallery_install
        ])
        .on_window_event(window::on_window_event)
        .setup(move |app| {
            // A launcher lives in the menu bar: no Dock icon or app menu.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle();
            tray::init(handle);
            hotkey::apply(handle);
            window::apply_configured_width(handle);
            backdrop::apply(handle);
            apply_theme(handle);
            autostart::sync(handle);
            search::start(handle);
            expansion::apply(handle);
            updater::start(handle);
            tracing::info!(display = ?server, ?launch, "sevak is ready");

            match launch {
                Launch::Show | Launch::Toggle => window::show(handle),
                Launch::Query(query) => direct::open_with_query(handle, query),
                Launch::Run(id) => direct::run_result(handle, id),
                Launch::Actions => selection::trigger(handle),
                Launch::Trigger { target, arg } => workflows::run_trigger(handle, target, arg),
                Launch::Settings => settings::open(handle),
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
            state.refresh_appearance();
        }
        Err(err) => tracing::warn!("keeping the current configuration: {err}"),
    }

    hotkey::apply(app);
    window::apply_configured_width(app);
    backdrop::apply(app);
    apply_theme(app);
    autostart::sync(app);
    // The index is rebuilt in the background and swapped in when ready.
    search::reload(app, &state.config());
    expansion::apply(app);

    // Both the launcher and the settings window follow the status.
    if let Err(err) = app.emit(window::EVENT_STATUS, state.status()) {
        tracing::warn!("could not emit {}: {err}", window::EVENT_STATUS);
    }
}

/// A second `sevak --config PATH` only forwards its request: the running
/// instance keeps the config it started with. Say so when the paths differ.
fn note_ignored_config(app: &AppHandle, requested: Option<&Path>, cwd: &str) {
    let Some(requested) = requested else { return };
    let (_, file) = AppPaths::config_location(requested, Path::new(cwd), None);
    let running = &app.state::<AppState>().paths.config_file;
    if file != *running {
        tracing::warn!(
            requested = %file.display(),
            running = %running.display(),
            "ignoring --config: Sevak is already running with another config; quit it (sevak --quit) to start with the requested one"
        );
    }
}

/// Applies the configured theme to the native window chrome (title bar, menus).
/// The web content themes itself from the `theme` field of the status.
pub fn apply_theme(app: &AppHandle) {
    let theme = app.state::<AppState>().config().appearance.theme;
    app.set_theme(settings::window_theme(theme));
}

/// Saves the usage statistics, then exits. The one way Sevak quits.
pub fn quit(app: &AppHandle) {
    tracing::info!("quitting");
    expansion::stop();
    hotkey::shutdown();
    search::save_usage(app);
    search::shutdown(app);
    app.exit(0);
}
