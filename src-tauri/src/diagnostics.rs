//! `sevak --diagnostics` and Settings → Help → Copy diagnostics.
//!
//! The report itself (what it contains, what is left out, the redaction) is
//! `sevak_core::diagnostics`. This module gathers the facts from this
//! computer into a [`DiagnosticsInput`]: [`gather_offline`] reads files only,
//! so the command line works with or without a running Sevak; [`apply_live`]
//! adds what only the running app knows (the shortcut's state, the tray, which
//! plugins loaded). Nothing is sent anywhere: the text goes to the terminal,
//! or to the Settings window where the user copies or saves it.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sevak_core::config::Config;
use sevak_core::diagnostics::{
    self, config_summary, BuildInfo, ConfigState, DiagnosticsInput, Health, HealthCheck, IndexSize,
    InstallEnv, InstallInfo, KeywordEntry, PathsInfo, PluginKind, PluginState, PluginStatus,
    Redactor, ReportSource, SystemInfo, LOG_TAIL_LINES,
};
use sevak_core::Plugin;
use sevak_platform::paths::{CONFIG_DIR_ENV, DATA_DIR_ENV};
use sevak_platform::{os_info, AppPaths, DisplayServer, HotkeyStrategy, PlatformProvider};
use sevak_plugins::script::{self, ScriptPluginHost};
use sevak_plugins::workflow::{self, WorkflowHost};
use sevak_plugins::{files_family, AppsPlugin, KeywordOwners, OwnerKind, PluginRegistry};
use tauri::{AppHandle, Manager, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

use crate::expansion::ExpansionStatus;
use crate::hotkey;
use crate::state::{AppState, CustomHotkeyStatus, HotkeyStatus, Mechanism};
use crate::updater::ManagedBy;

/// How long the command line waits for the app and file index counts.
const INDEX_TIMEOUT: Duration = Duration::from_secs(30);
/// The file written by the writability check.
const PROBE_FILE: &str = ".sevak-diagnostics-probe";

/// Everything gathering needs from the outside world, so tests can supply
/// their own.
pub struct Environment {
    pub paths: AppPaths,
    pub config_overridden: bool,
    pub data_overridden: bool,
    pub platform: Arc<dyn PlatformProvider>,
    pub display: DisplayServer,
    pub now_unix: u64,
    pub exe: Option<PathBuf>,
    pub install_env: InstallEnv,
    pub desktop: Option<String>,
    pub webview: Result<String, String>,
    pub managed_by: Option<String>,
}

impl Environment {
    /// This computer, for real.
    pub fn detect(
        paths: AppPaths,
        config_override: Option<&Path>,
        display: DisplayServer,
        platform: Arc<dyn PlatformProvider>,
    ) -> Self {
        let set = |name: &str| std::env::var_os(name).is_some_and(|v| !v.is_empty());
        let env = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        let mut program_files = Vec::new();
        for name in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
            program_files.extend(env(name));
        }
        Self {
            config_overridden: config_override.is_some() || set(CONFIG_DIR_ENV),
            data_overridden: set(DATA_DIR_ENV),
            paths,
            platform,
            display,
            now_unix: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_secs()),
            exe: std::env::current_exe().ok(),
            install_env: InstallEnv {
                appimage: set("APPIMAGE"),
                flatpak: set("FLATPAK_ID"),
                snap: set("SNAP"),
                program_files,
                local_app_data: env("LOCALAPPDATA"),
                home: dirs_home(),
            },
            desktop: env("XDG_CURRENT_DESKTOP"),
            webview: tauri::webview_version().map_err(|err| err.to_string()),
            managed_by: ManagedBy::detect().map(|managed| managed.name),
        }
    }
}

fn dirs_home() -> Option<String> {
    os_info::identity().home_dirs.into_iter().next()
}

// ---------------------------------------------------------------------------
// Gathering (files only)
// ---------------------------------------------------------------------------

/// Reads the config file the way Sevak does, without creating it.
pub fn read_config(file: &Path) -> ConfigState {
    match std::fs::read_to_string(file) {
        Ok(text) => match Config::from_toml_str(&text) {
            Ok(config) => ConfigState::Loaded(Box::new(config)),
            Err(_) => ConfigState::Invalid(
                config_summary::config_problem(&text)
                    .unwrap_or_else(|| "it is not valid".to_owned()),
            ),
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => ConfigState::Missing,
        Err(err) => ConfigState::Unreadable(err.to_string()),
    }
}

/// The report from files alone. `scan_indexes` also builds the app and file
/// indexes once to count them (a few seconds).
pub fn gather_offline(env: &Environment, scan_indexes: bool) -> DiagnosticsInput {
    let paths = &env.paths;
    let config_state = read_config(&paths.config_file);
    // Sevak runs on the defaults when the file is missing or unusable.
    let config = match &config_state {
        ConfigState::Loaded(config) => (**config).clone(),
        _ => Config::default(),
    };

    let scripts = ScriptPluginHost::new(
        paths.config_dir.join("plugins"),
        paths.data_dir.join("plugins"),
        paths.data_dir.join("script-plugin-approvals.json"),
    );
    let workflows = WorkflowHost::new(
        paths.config_dir.join("workflows"),
        paths.data_dir.join("workflows"),
        paths.data_dir.join("script-plugin-approvals.json"),
        Arc::new(workflow::NoSink),
    );

    let mut plugins = builtin_status(&config);
    plugins.extend(script_status(&scripts, &config));
    plugins.extend(workflow_status(&workflows, &config));

    let (indexes, indexes_note) = if scan_indexes {
        measure_indexes(&config, &env.platform, &mut plugins)
    } else {
        (
            Vec::new(),
            Some("only the running app keeps these (use Settings → Help)".to_owned()),
        )
    };

    let same_dir = paths.config_dir == paths.data_dir;
    let mut input = DiagnosticsInput {
        generated_at_unix: env.now_unix,
        source: ReportSource::CommandLine,
        build: build_info(),
        system: SystemInfo {
            os: env.platform.os_info(),
            display_server: env.display.as_str().to_owned(),
            desktop: env.desktop.clone(),
            webview: env.webview.clone(),
        },
        install: install_info(env),
        paths: PathsInfo {
            config_dir: paths.config_dir.display().to_string(),
            config_file: paths.config_file.display().to_string(),
            data_dir: paths.data_dir.display().to_string(),
            log_dir: paths.log_dir.display().to_string(),
            config_overridden: env.config_overridden,
            data_overridden: env.data_overridden,
        },
        keywords: keyword_entries(&config, &scripts, &workflows),
        health: Vec::new(),
        live: Vec::new(),
        plugins,
        indexes,
        indexes_note,
        config_dir: diagnostics::inventory(&paths.config_dir),
        data_dir: (!same_dir).then(|| diagnostics::inventory(&paths.data_dir)),
        logs: diagnostics::scan_logs(&paths.log_dir, LOG_TAIL_LINES),
        config: config_state,
    };
    input.health = offline_health(env, &input);
    input
}

fn build_info() -> BuildInfo {
    BuildInfo {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        commit: option_env!("SEVAK_GIT_COMMIT")
            .filter(|commit| !commit.is_empty())
            .map(str::to_owned),
        profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
        .to_owned(),
        target: option_env!("SEVAK_BUILD_TARGET")
            .unwrap_or("unknown")
            .to_owned(),
    }
}

fn install_info(env: &Environment) -> InstallInfo {
    let exe = env.exe.as_ref().map(|exe| exe.display().to_string());
    InstallInfo {
        kind: exe
            .as_deref()
            .map_or(diagnostics::InstallKind::Other, |exe| {
                diagnostics::classify_install(exe, &env.install_env)
            }),
        exe,
        managed_by: env.managed_by.clone(),
    }
}

/// Whether a built-in family is on: not in `[plugins] disabled`, and (for the
/// opt-in ones) switched on in its own section.
fn builtin_on(config: &Config, family: &str) -> bool {
    config.plugins.is_enabled(family)
        && match family {
            "clipboard" => config.clipboard.enabled,
            "contacts" => config.contacts.enabled,
            "1password" => config.onepassword.enabled,
            "ai" => config.ai.enabled,
            _ => true,
        }
}

/// One row per built-in plugin family (one per engine for web search), as
/// `Enabled` or `Disabled`; the running app turns `Enabled` into `Loaded`.
fn builtin_status(config: &Config) -> Vec<PluginStatus> {
    let mut rows = Vec::new();
    for descriptor in PluginRegistry::builtin().descriptors() {
        let on = builtin_on(config, descriptor.id);
        let state = |id_on: bool| {
            if on && id_on {
                PluginState::Enabled
            } else {
                PluginState::Disabled
            }
        };
        if descriptor.id == "web" {
            for engine in &config.web_search {
                let id = format!("web:{}", engine.keyword.trim());
                let engine_on = config.plugins.is_enabled(&id);
                rows.push(PluginStatus {
                    id,
                    kind: PluginKind::Builtin,
                    state: state(engine_on),
                });
            }
        } else {
            rows.push(PluginStatus {
                id: descriptor.id.to_owned(),
                kind: PluginKind::Builtin,
                state: state(true),
            });
        }
    }
    rows
}

fn script_status(host: &ScriptPluginHost, config: &Config) -> Vec<PluginStatus> {
    host.scan()
        .into_iter()
        .map(|scanned| match scanned {
            script::Scanned::Plugin(candidate) => {
                let id = candidate.manifest.id.clone();
                let on =
                    config.plugins.is_enabled(script::FAMILY) && config.plugins.is_enabled(&id);
                let state = if !on {
                    PluginState::Disabled
                } else if !candidate.approved {
                    PluginState::WaitingForApproval
                } else {
                    PluginState::Enabled
                };
                PluginStatus {
                    id,
                    kind: PluginKind::Script,
                    state,
                }
            }
            script::Scanned::Broken { folder, error } => PluginStatus {
                id: format!("{}{folder}", script::ID_PREFIX),
                kind: PluginKind::Script,
                state: PluginState::Failed(error),
            },
        })
        .collect()
}

fn workflow_status(host: &WorkflowHost, config: &Config) -> Vec<PluginStatus> {
    host.scan()
        .into_iter()
        .map(|scanned| match scanned {
            workflow::Scanned::Workflow(candidate) => {
                let id = candidate.id();
                let on = candidate.workflow.enabled
                    && config.plugins.is_enabled(workflow::FAMILY)
                    && config.plugins.is_enabled(&id);
                let state = if !on {
                    PluginState::Disabled
                } else if !candidate.approved {
                    PluginState::WaitingForApproval
                } else {
                    PluginState::Enabled
                };
                PluginStatus {
                    id,
                    kind: PluginKind::Workflow,
                    state,
                }
            }
            workflow::Scanned::Broken { folder, error } => PluginStatus {
                id: format!("{}:{folder}", workflow::FAMILY),
                kind: PluginKind::Workflow,
                state: PluginState::Failed(error),
            },
        })
        .collect()
}

fn keyword_entries(
    config: &Config,
    scripts: &ScriptPluginHost,
    workflows: &WorkflowHost,
) -> Vec<KeywordEntry> {
    KeywordOwners::collect(config, scripts, workflows)
        .uses()
        .iter()
        .map(|usage| KeywordEntry {
            keyword: usage.keyword.clone(),
            owner: match usage.kind {
                OwnerKind::Builtin => "built in",
                OwnerKind::WebSearch => "web search",
                OwnerKind::Workflow => "workflow",
                OwnerKind::ScriptPlugin => "script plugin",
            }
            .to_owned(),
        })
        .collect()
}

/// Builds the app and file indexes once and counts them. A plugin that fails
/// is marked failed in `plugins`.
fn measure_indexes(
    config: &Config,
    platform: &Arc<dyn PlatformProvider>,
    plugins: &mut [PluginStatus],
) -> (Vec<IndexSize>, Option<String>) {
    let mut targets: Vec<Arc<dyn Plugin>> = Vec::new();
    if config.plugins.is_enabled("apps") {
        targets.push(Arc::new(AppsPlugin::new(platform.clone())));
    }
    if config.plugins.is_enabled("files") {
        targets.extend(
            files_family(config, platform)
                .into_iter()
                .filter(|plugin| plugin.id() == "files"),
        );
    }
    if targets.is_empty() {
        return (
            Vec::new(),
            Some("the apps and files plugins are off".to_owned()),
        );
    }

    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("sevak-diagnostics-index".to_owned())
        .spawn(move || {
            for plugin in targets {
                let outcome = plugin
                    .refresh()
                    .map(|()| plugin.index_size().unwrap_or(0))
                    .map_err(|err| err.to_string());
                if tx.send((plugin.id().to_owned(), outcome)).is_err() {
                    return;
                }
            }
        });
    if let Err(err) = spawned {
        return (
            Vec::new(),
            Some(format!("could not start the count: {err}")),
        );
    }

    let mut sizes = Vec::new();
    let mut note = None;
    loop {
        match rx.recv_timeout(INDEX_TIMEOUT) {
            Ok((id, Ok(count))) => sizes.push(IndexSize { plugin: id, count }),
            Ok((id, Err(error))) => {
                if let Some(row) = plugins.iter_mut().find(|row| row.id == id) {
                    row.state = PluginState::Failed(error);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                note = Some(format!(
                    "counting took longer than {} seconds",
                    INDEX_TIMEOUT.as_secs()
                ));
                break;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    // What indexed fine is loaded; the rest keeps its state.
    for size in &sizes {
        if let Some(row) = plugins.iter_mut().find(|row| row.id == size.plugin) {
            if row.state == PluginState::Enabled {
                row.state = PluginState::Loaded;
            }
        }
    }
    (sizes, note)
}

// ---------------------------------------------------------------------------
// Health checks
// ---------------------------------------------------------------------------

fn offline_health(env: &Environment, input: &DiagnosticsInput) -> Vec<HealthCheck> {
    let mut checks = vec![config_check(&input.config)];
    checks.push(shortcut_parse_check(env.display, &input.config));
    checks.push(data_dir_check(&env.paths.data_dir));
    checks.push(match &input.system.webview {
        Ok(version) => HealthCheck::new("The web view runtime is installed", Health::Pass, version),
        Err(why) => HealthCheck::new(
            "The web view runtime is installed",
            Health::Fail,
            format!("not found: {why}"),
        ),
    });
    if env.display == DisplayServer::Unknown {
        checks.push(HealthCheck::new(
            "A graphical session was detected",
            Health::Warn,
            "neither X11 nor Wayland (a terminal session?)",
        ));
    }
    let needs_app = "only the running app can tell: see Settings → Help → Copy diagnostics";
    checks.push(HealthCheck::new(
        "The global shortcut is registered",
        Health::Unknown,
        needs_app,
    ));
    checks.push(HealthCheck::new(
        "The tray icon was created",
        Health::Unknown,
        needs_app,
    ));
    checks
}

fn config_check(state: &ConfigState) -> HealthCheck {
    const NAME: &str = "The config file parses";
    match state {
        ConfigState::Loaded(_) => HealthCheck::new(NAME, Health::Pass, ""),
        ConfigState::Missing => HealthCheck::new(
            NAME,
            Health::Warn,
            "there is no config file yet; Sevak writes the defaults on first run",
        ),
        ConfigState::Invalid(message) => HealthCheck::new(NAME, Health::Fail, message.clone()),
        ConfigState::Unreadable(message) => HealthCheck::new(NAME, Health::Fail, message.clone()),
    }
}

fn shortcut_parse_check(display: DisplayServer, state: &ConfigState) -> HealthCheck {
    const NAME: &str = "The configured shortcut is valid";
    let hotkey_text = match state {
        ConfigState::Loaded(config) => config.general.hotkey.clone(),
        _ => Config::default().general.hotkey,
    };
    match display.hotkey_strategy() {
        HotkeyStrategy::External => HealthCheck::new(
            NAME,
            Health::Pass,
            "on Wayland the desktop owns the key, so Sevak does not check it",
        ),
        HotkeyStrategy::InApp => match hotkey::parse_shortcut(&hotkey_text) {
            Ok(_) => HealthCheck::new(NAME, Health::Pass, hotkey_text),
            Err(reason) => HealthCheck::new(NAME, Health::Fail, reason),
        },
    }
}

/// Writes and removes a tiny file to see whether Sevak can save its data.
fn data_dir_check(dir: &Path) -> HealthCheck {
    const NAME: &str = "The data folder is writable";
    if !dir.is_dir() {
        return HealthCheck::new(
            NAME,
            Health::Warn,
            "it does not exist yet; Sevak creates it when it first runs",
        );
    }
    let probe = dir.join(PROBE_FILE);
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            HealthCheck::new(NAME, Health::Pass, "")
        }
        Err(err) => HealthCheck::new(NAME, Health::Fail, err.to_string()),
    }
}

// ---------------------------------------------------------------------------
// What the running app adds
// ---------------------------------------------------------------------------

/// What only the running app can tell.
pub struct Live {
    /// The configuration in force.
    pub config: Config,
    pub hotkey: HotkeyStatus,
    pub custom_hotkeys: Vec<CustomHotkeyStatus>,
    pub actions_hotkey: Option<CustomHotkeyStatus>,
    pub snippet_expansion: ExpansionStatus,
    pub indexing: bool,
    pub tray_created: bool,
    /// The plugins the engine has, with their index sizes.
    pub loaded: Vec<(String, Option<usize>)>,
    /// `(plugin id, error)` of the plugins whose last refresh failed.
    pub refresh_failures: Vec<(String, String)>,
}

fn mechanism_label(mechanism: Mechanism) -> &'static str {
    match mechanism {
        Mechanism::Registered => "registered with the OS",
        Mechanism::WindowsHook => "taken over with the Windows keyboard hook",
        Mechanism::SpotlightDisabled => "Spotlight's shortcut is off, with your permission",
        Mechanism::GnomeInputSourceMoved => "GNOME's input-source shortcut was moved",
        Mechanism::FallbackKey => "Option+Space stands in for Cmd+Space",
        Mechanism::Desktop => "the desktop runs `sevak --toggle`",
        Mechanism::Inactive => "not active",
    }
}

/// Adds the running app's view to a report made from files.
pub fn apply_live(input: &mut DiagnosticsInput, live: Live) {
    input.source = ReportSource::SettingsWindow;
    // The config in force, unless the file on disk has since become unusable.
    if matches!(input.config, ConfigState::Loaded(_)) {
        input.config = ConfigState::Loaded(Box::new(live.config.clone()));
    }

    // Plugins: what the engine has is loaded; a failed refresh is a failure.
    let is_loaded = |id: &str| {
        live.loaded
            .iter()
            .any(|(loaded, _)| loaded == id || loaded.starts_with(&format!("{id}:")))
    };
    let failure_of = |id: &str| {
        live.refresh_failures
            .iter()
            .find(|(failed, _)| failed == id || failed.starts_with(&format!("{id}:")))
            .map(|(_, error)| error.clone())
    };
    for row in &mut input.plugins {
        if row.state != PluginState::Enabled {
            continue;
        }
        row.state = if let Some(error) = failure_of(&row.id) {
            PluginState::Failed(error)
        } else if is_loaded(&row.id) {
            PluginState::Loaded
        } else {
            PluginState::Failed("it is on but was not loaded; see the log".to_owned())
        };
    }

    input.indexes = live
        .loaded
        .iter()
        .filter_map(|(id, size)| {
            Some(IndexSize {
                plugin: id.clone(),
                count: (*size)?,
            })
        })
        .collect();
    input.indexes_note = Some("no plugin keeps an index".to_owned());

    // Health checks that need the app.
    for check in &mut input.health {
        match check.name.as_str() {
            "The global shortcut is registered" => *check = hotkey_check(&live.hotkey),
            "The tray icon was created" => {
                *check = if live.tray_created {
                    HealthCheck::new("The tray icon was created", Health::Pass, "")
                } else {
                    HealthCheck::new(
                        "The tray icon was created",
                        Health::Warn,
                        "there is none (some Linux desktops have no tray); the shortcut and `sevak --toggle` still work",
                    )
                };
            }
            _ => {}
        }
    }

    // The running state.
    let hotkey = &live.hotkey;
    input.live.push((
        "Shortcut".to_owned(),
        format!(
            "{} ({})",
            hotkey.accelerator,
            mechanism_label(hotkey.mechanism)
        ),
    ));
    if let Some(error) = &hotkey.error {
        input
            .live
            .push(("Shortcut problem".to_owned(), error.clone()));
    }
    if let Some(actions) = &live.actions_hotkey {
        input.live.push((
            "Universal Actions shortcut".to_owned(),
            custom_line(actions),
        ));
    }
    for custom in &live.custom_hotkeys {
        input
            .live
            .push(("Extra shortcut".to_owned(), custom_line(custom)));
    }
    input.live.push((
        "Snippet expansion".to_owned(),
        match (
            &live.snippet_expansion.active,
            &live.snippet_expansion.problem,
        ) {
            (true, _) => "running".to_owned(),
            (false, Some(problem)) => format!("not running: {problem}"),
            (false, None) => "off".to_owned(),
        },
    ));
    input.live.push((
        "Indexing".to_owned(),
        if live.indexing { "in progress" } else { "done" }.to_owned(),
    ));
}

/// `Ctrl+Alt+T (Run apps:firefox.desktop): registered`, or the error.
/// What the key runs is not private (it is a result id), but a `query` is, so
/// the description never carries one.
fn custom_line(status: &CustomHotkeyStatus) -> String {
    let description = status.description.split_whitespace().next().unwrap_or("");
    match &status.error {
        Some(error) => format!("{} ({description}): {error}", status.key),
        None => format!(
            "{} ({description}): {}",
            status.key,
            mechanism_label(status.mechanism)
        ),
    }
}

fn hotkey_check(status: &HotkeyStatus) -> HealthCheck {
    const NAME: &str = "The global shortcut is registered";
    match (&status.error, status.mechanism) {
        (Some(error), _) => HealthCheck::new(NAME, Health::Fail, error.clone()),
        (None, Mechanism::Inactive) => {
            HealthCheck::new(NAME, Health::Fail, "the shortcut is not active")
        }
        (None, mechanism) => HealthCheck::new(
            NAME,
            Health::Pass,
            format!("{} ({})", status.accelerator, mechanism_label(mechanism)),
        ),
    }
}

// ---------------------------------------------------------------------------
// Rendering and the entry points
// ---------------------------------------------------------------------------

/// The report as text, redacted for this user.
pub fn render_report(input: &DiagnosticsInput) -> String {
    diagnostics::render(input, &Redactor::new(&os_info::identity()))
}

/// `sevak --diagnostics`: prints the report. Works whether or not Sevak is
/// running; reads files only.
pub fn run_cli(config_override: Option<&Path>) -> ExitCode {
    let paths = match AppPaths::resolve_with_config(config_override) {
        Ok(paths) => paths,
        Err(err) => {
            eprintln!("error: cannot determine Sevak's directories: {err}");
            return ExitCode::FAILURE;
        }
    };
    let platform: Arc<dyn PlatformProvider> = Arc::from(sevak_platform::native_provider());
    let env = Environment::detect(paths, config_override, DisplayServer::detect(), platform);
    let report = render_report(&gather_offline(&env, true));
    print_report(&report);
    ExitCode::SUCCESS
}

/// Prints without panicking when the reader goes away (`| head`).
fn print_report(report: &str) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(report.as_bytes());
    let _ = out.flush();
}

fn live_report(app: &AppHandle) -> String {
    let state = app.state::<AppState>();
    let engine = state.search.engine();
    let live = Live {
        config: state.config(),
        hotkey: state
            .hotkey
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone(),
        custom_hotkeys: state
            .custom_hotkeys
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone(),
        actions_hotkey: state
            .actions_hotkey
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone(),
        snippet_expansion: crate::expansion::status(),
        indexing: state.search.is_indexing(),
        tray_created: crate::tray::exists(app),
        loaded: engine
            .plugins()
            .iter()
            .map(|plugin| (plugin.id().to_owned(), plugin.index_size()))
            .collect(),
        refresh_failures: state.search.refresh_failures(),
    };
    let env = Environment::detect(
        state.paths.clone(),
        None,
        state.display,
        state.search.platform.clone(),
    );

    let mut input = gather_offline(&env, false);
    apply_live(&mut input, live);
    render_report(&input)
}

/// Settings → Help: the report for this running app.
#[tauri::command]
pub async fn get_diagnostics(app: AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || live_report(&app))
        .await
        .map_err(|err| format!("the report did not finish: {err}"))
}

/// Puts the text the Help page shows on the clipboard.
#[tauri::command]
pub fn copy_diagnostics(text: String) -> Result<(), String> {
    sevak_platform::clipboard::set_text(&text).map_err(|err| err.to_string())
}

/// Asks where to save the text the Help page shows, then writes it there.
/// `None` when the user cancels; otherwise where it went.
#[tauri::command]
pub async fn save_diagnostics(
    app: AppHandle,
    window: WebviewWindow,
    text: String,
) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let picked = app
            .dialog()
            .file()
            .set_title("Save the diagnostics report")
            .set_file_name("sevak-diagnostics.md")
            .add_filter("Markdown", &["md"])
            .add_filter("Text", &["txt"])
            .set_parent(&window)
            .blocking_save_file();
        let Some(picked) = picked else {
            return Ok(None);
        };
        let path = picked.into_path().map_err(|err| err.to_string())?;
        std::fs::write(&path, text).map_err(|err| format!("could not save it: {err}"))?;
        Ok(Some(sevak_platform::paths::home_relative(&path)))
    })
    .await
    .map_err(|err| format!("saving did not finish: {err}"))?
}

#[cfg(test)]
mod tests;
