// Release builds on Windows use the GUI subsystem so no console window appears.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod autostart;
mod cli;
mod commands;
mod hotkey;
mod icons;
mod logging;
mod script_plugins;
mod search;
mod settings;
mod state;
mod tray;
mod updater;
mod window;

use std::process::ExitCode;

use anyhow::Context;
use sevak_core::{Config, ConfigOrigin};
use sevak_platform::{process, session, AppPaths, DisplayServer};

use cli::{Invocation, Launch};

fn main() -> ExitCode {
    let invocation = match cli::parse(std::env::args().skip(1)) {
        Ok(invocation) => invocation,
        Err(message) => {
            process::attach_parent_console();
            eprintln!("sevak: {message}\n\n{}", cli::USAGE);
            return ExitCode::from(2);
        }
    };

    match invocation {
        Invocation::Help => {
            process::attach_parent_console();
            println!("{}", cli::USAGE);
            ExitCode::SUCCESS
        }
        Invocation::Version => {
            process::attach_parent_console();
            println!("sevak {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Invocation::SetupHotkey(key) => {
            process::attach_parent_console();
            setup_hotkey(key)
        }
        Invocation::Run(launch) => match run(launch) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                tracing::error!("{err:#}");
                // Logging may be unavailable and release builds have no console;
                // this still helps when started from a terminal.
                process::attach_parent_console();
                eprintln!("sevak: {err:#}");
                ExitCode::FAILURE
            }
        },
    }
}

fn run(launch: Launch) -> anyhow::Result<()> {
    let paths = AppPaths::resolve().context("cannot determine Sevak's directories")?;
    logging::init(&paths);

    let config = match Config::load_or_create(&paths.config_file) {
        Ok((config, ConfigOrigin::Created)) => {
            tracing::info!(path = %paths.config_file.display(), "created default config");
            config
        }
        Ok((config, ConfigOrigin::Loaded)) => {
            tracing::info!(path = %paths.config_file.display(), "loaded config");
            config
        }
        Err(err) => {
            // The user's file is left untouched so they can fix the typo.
            tracing::warn!("using default settings: {err}");
            Config::default()
        }
    };

    // Both calls must precede any thread creation (see `session::prefer_xwayland`);
    // logging uses blocking writes for that reason.
    let server = DisplayServer::detect();
    if let Some(backend) = session::prefer_xwayland(server, config.linux.wayland_use_xwayland) {
        tracing::info!(
            backend,
            "Wayland session: forcing the X11 backend (XWayland)"
        );
    }

    // Helps when this process turns out to be a second instance forwarding --toggle.
    process::allow_foreground_handoff();

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        display = ?server,
        ?launch,
        "starting sevak"
    );
    app::run(paths, server, config, launch)
}

/// `sevak --setup-hotkey [KEY]`: binds KEY to `sevak --toggle` where the
/// desktop owns global shortcuts (GNOME on Wayland).
fn setup_hotkey(key: Option<String>) -> ExitCode {
    let hotkey = key.unwrap_or_else(|| {
        AppPaths::resolve()
            .ok()
            .and_then(|paths| Config::load_or_create(&paths.config_file).ok())
            .map(|(config, _)| config.general.hotkey)
            .unwrap_or_else(|| Config::default().general.hotkey)
    });
    setup_hotkey_for_platform(&hotkey)
}

#[cfg(not(target_os = "linux"))]
fn setup_hotkey_for_platform(hotkey: &str) -> ExitCode {
    println!("Nothing to set up: on this platform Sevak registers {hotkey} itself while it runs.");
    ExitCode::SUCCESS
}

#[cfg(target_os = "linux")]
fn setup_hotkey_for_platform(hotkey: &str) -> ExitCode {
    use sevak_platform::gnome;

    let command = match gnome::toggle_command() {
        Ok(command) => command,
        Err(err) => {
            eprintln!("error: cannot determine the sevak executable: {err}");
            return ExitCode::FAILURE;
        }
    };

    if !session::is_gnome() {
        println!(
            "This is not a GNOME session, so the shortcut cannot be installed automatically.\n"
        );
        println!("{}", gnome::manual_instructions(hotkey, &command));
        return ExitCode::SUCCESS;
    }

    match gnome::install_shortcut(hotkey, &command) {
        Ok(report) => {
            print!("{}", report.describe());
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err}\n");
            println!("{}", gnome::manual_instructions(hotkey, &command));
            ExitCode::FAILURE
        }
    }
}
