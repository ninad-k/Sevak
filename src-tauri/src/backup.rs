//! Settings > Backup & restore, the `backup settings` and `restore settings`
//! launcher commands, the scheduler and the `--backup` / `--restore` command
//! line.
//!
//! The work itself is in the `sevak-backup` crate (no Tauri types, tested with
//! temporary folders). This module is the glue: native file dialogs, the
//! checks the Settings window applies to a configuration, reloading Sevak after
//! a restore, and the background thread that makes scheduled backups.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sevak_backup::auto::{self, AutoConfig, Schedule, MANUAL_PREFIX};
use sevak_backup::{
    backup, default_file_name, latest_snapshot, preview, read_file, restore, undo_restore,
    ArchiveInfo, Category, Contents, Destination, Kind, Mode, Preview, RestoreOptions,
    RestoreReport, Roots, Stamp, State, EXTENSION,
};
use sevak_core::model::score;
use sevak_core::{Action, Config, IconSource, Plugin, PluginResult, ResultItem};
use sevak_platform::paths::home_relative;
use sevak_platform::{open, AppPaths, DisplayServer, HotkeyStrategy};
use tauri::{AppHandle, Manager, WebviewWindow};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

use crate::state::AppState;
use crate::{app, commands, settings};

/// The file chosen for a restore, between "Choose a backup" and "Restore".
static PICKED: Mutex<Option<PathBuf>> = Mutex::new(None);
/// For the launcher commands, which run outside a Tauri command.
static APP: OnceLock<AppHandle> = OnceLock::new();

fn roots_of(app: &AppHandle) -> Roots {
    Roots::from_paths(&app.state::<AppState>().paths)
}

/// The same checks the Settings window applies before it saves.
fn validator(strategy: HotkeyStrategy) -> impl Fn(&Config) -> Result<(), String> {
    move |config| settings::validate(config, strategy)
}

fn strategy_of(app: &AppHandle) -> HotkeyStrategy {
    app.state::<AppState>().display.hotkey_strategy()
}

fn parse_categories(ids: &[String]) -> Result<Vec<Category>, String> {
    let mut categories = Vec::new();
    for id in ids {
        let category = Category::from_id(id).ok_or_else(|| format!("unknown category \"{id}\""))?;
        if !categories.contains(&category) {
            categories.push(category);
        }
    }
    categories.sort();
    Ok(categories)
}

async fn blocking<T: Send + 'static>(
    app: AppHandle,
    work: impl FnOnce(&AppHandle) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || work(&app))
        .await
        .map_err(|err| format!("the operation did not finish: {err}"))?
}

// ---------------------------------------------------------------------------
// Data for the page
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct CategoryDto {
    id: &'static str,
    label: &'static str,
    description: &'static str,
    /// Ticked when the page opens.
    default_on: bool,
    /// Restored scripts ask for approval again.
    runs_code: bool,
}

fn category_dtos() -> Vec<CategoryDto> {
    Category::ALL
        .into_iter()
        .map(|category| CategoryDto {
            id: category.id(),
            label: category.label(),
            description: category.description(),
            default_on: true,
            runs_code: category.runs_code(),
        })
        .collect()
}

#[derive(Debug, Serialize)]
pub struct LastBackupDto {
    path: String,
    created: String,
    kind: &'static str,
    /// The file is still where it was saved.
    exists: bool,
}

#[derive(Debug, Serialize)]
pub struct SnapshotDto {
    created: String,
    categories: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoDto {
    /// `off`, `daily` or `weekly`.
    schedule: String,
    on_update: bool,
    keep: usize,
    /// As typed; empty means the default folder.
    folder: String,
    #[serde(default)]
    resolved_folder: String,
}

fn auto_dto(config: &AutoConfig) -> AutoDto {
    AutoDto {
        schedule: config.schedule.id().to_owned(),
        on_update: config.on_update,
        keep: config.keep,
        folder: config.folder.clone(),
        resolved_folder: config
            .resolve_folder()
            .map(|folder| home_relative(&folder))
            .unwrap_or_default(),
    }
}

#[derive(Debug, Serialize)]
pub struct OverviewDto {
    categories: Vec<CategoryDto>,
    never_included: &'static [&'static str],
    default_file_name: String,
    /// Where "Back up now" and automatic backups write.
    folder: String,
    last_backup: Option<LastBackupDto>,
    auto: AutoDto,
    /// `backup.toml` is damaged, with the reason.
    auto_problem: Option<String>,
    undo: Option<SnapshotDto>,
    version: &'static str,
}

#[tauri::command]
pub async fn backup_overview(app: AppHandle) -> Result<OverviewDto, String> {
    blocking(app, |app| {
        let roots = roots_of(app);
        let (config, auto_problem) = AutoConfig::load(&roots);
        let state = State::load(&roots);
        Ok(OverviewDto {
            categories: category_dtos(),
            never_included: sevak_backup::category::NEVER_INCLUDED_TEXT,
            default_file_name: default_file_name(&Stamp::now()),
            folder: config
                .resolve_folder()
                .map(|folder| home_relative(&folder))
                .unwrap_or_default(),
            last_backup: state.last.map(|last| LastBackupDto {
                exists: Path::new(&last.path).is_file(),
                path: home_relative(Path::new(&last.path)),
                created: last.created,
                kind: match last.kind {
                    Kind::Auto => "auto",
                    _ => "manual",
                },
            }),
            auto: auto_dto(&config),
            auto_problem,
            undo: latest_snapshot(&roots).map(|snapshot| SnapshotDto {
                created: snapshot.created,
                categories: snapshot.categories.iter().map(|c| c.id()).collect(),
            }),
            version: env!("CARGO_PKG_VERSION"),
        })
    })
    .await
}

/// What a backup of the chosen categories would hold, for the "what is
/// included" list. Writes nothing.
#[tauri::command]
pub async fn backup_contents(app: AppHandle, categories: Vec<String>) -> Result<Contents, String> {
    blocking(app, move |app| {
        let categories = parse_categories(&categories)?;
        backup::contents(&roots_of(app), &categories).map_err(|err| err.to_string())
    })
    .await
}

#[derive(Debug, Serialize)]
pub struct ReportDto {
    path: String,
    display: String,
    bytes: u64,
    created: String,
    contents: Contents,
}

fn report_dto(report: sevak_backup::BackupReport) -> ReportDto {
    ReportDto {
        display: home_relative(&report.path),
        path: report.path.display().to_string(),
        bytes: report.bytes,
        created: report.created,
        contents: report.contents,
    }
}

/// "Save a backup as...": a native save dialog, then the backup. `None` when
/// the user cancels.
#[tauri::command]
pub async fn backup_save_as(
    app: AppHandle,
    window: WebviewWindow,
    categories: Vec<String>,
) -> Result<Option<ReportDto>, String> {
    blocking(app, move |app| {
        let categories = parse_categories(&categories)?;
        let roots = roots_of(app);
        let stamp = Stamp::now();
        let (config, _) = AutoConfig::load(&roots);
        let mut dialog = app
            .dialog()
            .file()
            .set_title("Save a Sevak backup")
            .set_file_name(default_file_name(&stamp))
            .add_filter("Sevak backup", &[EXTENSION])
            .set_parent(&window);
        if let Some(folder) = config.resolve_folder().ok().filter(|f| f.is_dir()) {
            dialog = dialog.set_directory(folder);
        }
        let Some(picked) = dialog.blocking_save_file() else {
            return Ok(None);
        };
        let path = picked
            .into_path()
            .map_err(|err| format!("That is not a local file: {err}"))?;
        let report = backup::create(
            &roots,
            &categories,
            Kind::Manual,
            &stamp,
            Destination::File(&path),
        )
        .map_err(|err| err.to_string())?;
        tracing::info!(path = %report.path.display(), "settings backup saved");
        Ok(Some(report_dto(report)))
    })
    .await
}

/// "Back up now": a backup in the default (or configured) folder, no dialog.
#[tauri::command]
pub async fn backup_now(app: AppHandle, categories: Vec<String>) -> Result<ReportDto, String> {
    blocking(app, move |app| {
        let categories = parse_categories(&categories)?;
        quick_backup(&roots_of(app), &categories).map(report_dto)
    })
    .await
}

fn quick_backup(
    roots: &Roots,
    categories: &[Category],
) -> Result<sevak_backup::BackupReport, String> {
    let (config, _) = AutoConfig::load(roots);
    let folder = config.resolve_folder().map_err(|err| err.to_string())?;
    let report = backup::create(
        roots,
        categories,
        Kind::Manual,
        &Stamp::now(),
        Destination::Folder {
            dir: &folder,
            prefix: MANUAL_PREFIX,
        },
    )
    .map_err(|err| err.to_string())?;
    tracing::info!(path = %report.path.display(), "settings backup saved");
    Ok(report)
}

// ---------------------------------------------------------------------------
// Restore
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct InspectDto {
    /// The file as shown to the user.
    display: String,
    archive: ArchiveInfo,
    /// The categories the backup has, in the order of the page.
    categories: Vec<CategoryDto>,
    warnings: Vec<String>,
}

fn picked_path() -> Result<PathBuf, String> {
    PICKED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .ok_or_else(|| "Choose a backup file first.".to_owned())
}

/// "Choose a backup...": a native open dialog, then every check on the file.
/// `None` when the user cancels. Nothing is applied.
#[tauri::command]
pub async fn restore_pick(
    app: AppHandle,
    window: WebviewWindow,
) -> Result<Option<InspectDto>, String> {
    blocking(app, move |app| {
        let roots = roots_of(app);
        let (config, _) = AutoConfig::load(&roots);
        let mut dialog = app
            .dialog()
            .file()
            .set_title("Choose a Sevak backup")
            .add_filter("Sevak backup", &[EXTENSION])
            .set_parent(&window);
        if let Some(folder) = config.resolve_folder().ok().filter(|f| f.is_dir()) {
            dialog = dialog.set_directory(folder);
        }
        let Some(picked) = dialog.blocking_pick_file() else {
            return Ok(None);
        };
        let path = picked
            .into_path()
            .map_err(|err| format!("That is not a local file: {err}"))?;
        let backup = read_file(&path).map_err(|err| err.to_string())?;
        let categories = category_dtos()
            .into_iter()
            .filter(|dto| Category::from_id(dto.id).is_some_and(|category| backup.has(category)))
            .collect();
        *PICKED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(path.clone());
        Ok(Some(InspectDto {
            display: home_relative(&path),
            archive: backup.info(),
            categories,
            warnings: backup.warnings.clone(),
        }))
    })
    .await
}

/// What restoring the chosen categories in the chosen mode would change.
#[tauri::command]
pub async fn restore_preview(
    app: AppHandle,
    categories: Vec<String>,
    mode: String,
) -> Result<Preview, String> {
    blocking(app, move |app| {
        let categories = parse_categories(&categories)?;
        let mode = Mode::from_id(&mode).ok_or("unknown restore mode")?;
        let backup = read_file(&picked_path()?).map_err(|err| err.to_string())?;
        let check = validator(strategy_of(app));
        preview(&roots_of(app), &backup, &categories, mode, &check).map_err(|err| err.to_string())
    })
    .await
}

/// Applies the restore, then reloads Sevak so it takes effect at once.
/// `sha256` is the checksum the preview reported.
#[tauri::command]
pub async fn restore_apply(
    app: AppHandle,
    categories: Vec<String>,
    mode: String,
    sha256: String,
) -> Result<RestoreReport, String> {
    blocking(app, move |app| {
        let categories = parse_categories(&categories)?;
        let mode = Mode::from_id(&mode).ok_or("unknown restore mode")?;
        let backup = read_file(&picked_path()?).map_err(|err| err.to_string())?;
        let check = validator(strategy_of(app));
        let report = restore(
            &roots_of(app),
            &backup,
            &categories,
            mode,
            &RestoreOptions {
                validate: &check,
                stamp: Stamp::now(),
                expect_sha256: Some(&sha256),
            },
        )
        .map_err(|err| err.to_string())?;
        tracing::info!(changed = report.changed, ?mode, "settings restored");
        if report.changed {
            app::reload(app);
        }
        Ok(report)
    })
    .await
}

/// "Undo restore": puts back what the last restore replaced.
#[tauri::command]
pub async fn restore_undo(app: AppHandle) -> Result<RestoreReport, String> {
    blocking(app, |app| {
        let report = undo_restore(&roots_of(app), Stamp::now()).map_err(|err| err.to_string())?;
        tracing::info!("restore undone");
        app::reload(app);
        Ok(report)
    })
    .await
}

/// Saves the automatic backup options to `backup.toml`.
#[tauri::command]
pub async fn backup_set_auto(app: AppHandle, auto: AutoDto) -> Result<AutoDto, String> {
    blocking(app, move |app| {
        let schedule = match auto.schedule.as_str() {
            "off" => Schedule::Off,
            "daily" => Schedule::Daily,
            "weekly" => Schedule::Weekly,
            other => return Err(format!("unknown schedule \"{other}\"")),
        };
        let config = AutoConfig {
            schedule,
            on_update: auto.on_update,
            keep: auto.keep,
            folder: auto.folder,
        }
        .normalized();
        // A folder that could never be used is refused now, not at 3 a.m.
        config.resolve_folder().map_err(|err| err.to_string())?;
        config.save(&roots_of(app)).map_err(|err| err.to_string())?;
        Ok(auto_dto(&config))
    })
    .await
}

/// Opens the backup folder in the file manager.
#[tauri::command]
pub async fn backup_open_folder(app: AppHandle) -> Result<(), String> {
    blocking(app, |app| {
        let (config, _) = AutoConfig::load(&roots_of(app));
        let folder = config.resolve_folder().map_err(|err| err.to_string())?;
        if !folder.is_dir() {
            return Err("There is no backup folder yet: make a backup first.".to_owned());
        }
        open::open_path(&folder).map_err(|err| err.to_string())
    })
    .await
}

// ---------------------------------------------------------------------------
// The scheduler
// ---------------------------------------------------------------------------

/// Starts the background check for scheduled backups and lets the launcher
/// commands reach the app. Does nothing visible: with the default settings
/// (everything off) each check reads one small file.
pub fn start(app: &AppHandle) {
    let _ = APP.set(app.clone());
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("sevak-backup-schedule".to_owned())
        .spawn(move || {
            // Not at startup, when everything else is busy.
            std::thread::sleep(Duration::from_secs(90));
            loop {
                match auto::run_due(&roots_of(&app), &Stamp::now(), env!("CARGO_PKG_VERSION")) {
                    Ok(Some(report)) => {
                        tracing::info!(path = %report.path.display(), "automatic backup made");
                    }
                    Ok(None) => {}
                    Err(err) => tracing::warn!("automatic backup failed: {err}"),
                }
                std::thread::sleep(Duration::from_secs(30 * 60));
            }
        });
    if let Err(err) = spawned {
        tracing::warn!("could not start the backup schedule: {err}");
    }
}

// ---------------------------------------------------------------------------
// The launcher commands: `backup settings`, `restore settings`
// ---------------------------------------------------------------------------

/// Offers "Back up settings" and "Restore settings" when the user types
/// `backup settings` or `restore settings` (or the start of either, from six
/// letters). Both run through native dialogs; the Settings page has the full
/// options. Switch off with `[plugins] disabled = ["backup"]`.
pub struct BackupPlugin;

const KEY_NOW: &str = "now";
const KEY_RESTORE: &str = "restore";

impl BackupPlugin {
    fn row(key: &str, title: &str, subtitle: &str, complete: bool) -> ResultItem {
        ResultItem::new(
            "backup",
            key,
            title,
            Action::Custom {
                payload: key.to_owned(),
            },
        )
        .with_subtitle(subtitle)
        .with_icon(IconSource::builtin("settings"))
        // The whole phrase is a command; part of it must not push real matches down.
        .with_score(if complete { score::KEYWORD } else { 150.0 })
    }
}

impl Plugin for BackupPlugin {
    fn id(&self) -> &str {
        "backup"
    }

    fn name(&self) -> &str {
        "Settings backup"
    }

    fn description(&self) -> &str {
        "Type `backup settings` or `restore settings` to save or load your settings, snippets, themes, plugins and workflows."
    }

    fn keyword(&self) -> Option<&str> {
        None
    }

    fn query(&self, input: &str) -> Vec<ResultItem> {
        let typed: String = input
            .chars()
            .filter(|c| !c.is_whitespace())
            .flat_map(char::to_lowercase)
            .collect();
        if typed.chars().count() < 6 {
            return Vec::new();
        }
        let mut rows = Vec::new();
        if "backupsettings".starts_with(&typed) {
            rows.push(Self::row(
                KEY_NOW,
                "Back up settings",
                "Saves a backup file to your backup folder (no passwords, keys or history)",
                typed == "backupsettings",
            ));
        }
        if "restoresettings".starts_with(&typed) {
            rows.push(Self::row(
                KEY_RESTORE,
                "Restore settings from a backup…",
                "Choose a backup file; you see what changes before anything does",
                typed == "restoresettings",
            ));
        }
        rows
    }

    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        let Action::Custom { payload } = &item.action else {
            return Ok(());
        };
        let Some(app) = APP.get().cloned() else {
            return Ok(());
        };
        let payload = payload.clone();
        // Dialogs block: never on the thread that runs the action.
        let spawned = std::thread::Builder::new()
            .name("sevak-backup-command".to_owned())
            .spawn(move || match payload.as_str() {
                KEY_NOW => quick_backup_flow(&app),
                KEY_RESTORE => restore_flow(&app),
                _ => {}
            });
        if let Err(err) = spawned {
            tracing::warn!("could not start the backup command: {err}");
        }
        Ok(())
    }
}

fn tell(app: &AppHandle, kind: MessageDialogKind, text: String) {
    app.dialog()
        .message(text)
        .title("Sevak: settings backup")
        .kind(kind)
        .buttons(MessageDialogButtons::Ok)
        .show(|_| {});
}

fn quick_backup_flow(app: &AppHandle) {
    let roots = roots_of(app);
    match quick_backup(&roots, &Category::ALL) {
        Ok(report) => tell(
            app,
            MessageDialogKind::Info,
            format!(
                "Backup saved to\n{}\n\nIt is not encrypted, and it holds no passwords, API keys, \
                 clipboard history or search history.",
                report.path.display()
            ),
        ),
        Err(err) => tell(
            app,
            MessageDialogKind::Error,
            format!("The backup was not made.\n\n{err}"),
        ),
    }
}

fn restore_flow(app: &AppHandle) {
    let roots = roots_of(app);
    let (config, _) = AutoConfig::load(&roots);
    let mut dialog = app
        .dialog()
        .file()
        .set_title("Choose a Sevak backup to restore")
        .add_filter("Sevak backup", &[EXTENSION]);
    if let Some(folder) = config.resolve_folder().ok().filter(|f| f.is_dir()) {
        dialog = dialog.set_directory(folder);
    }
    let Some(picked) = dialog.blocking_pick_file() else {
        return;
    };
    let path = match picked.into_path() {
        Ok(path) => path,
        Err(err) => {
            tell(
                app,
                MessageDialogKind::Error,
                format!("That is not a local file: {err}"),
            );
            return;
        }
    };
    let backup = match read_file(&path) {
        Ok(backup) => backup,
        Err(err) => {
            tell(
                app,
                MessageDialogKind::Error,
                format!("This backup cannot be restored.\n\n{err}"),
            );
            return;
        }
    };
    let categories = backup.manifest.categories.clone();
    let check = validator(strategy_of(app));
    let shown = match preview(&roots, &backup, &categories, Mode::Merge, &check) {
        Ok(shown) => shown,
        Err(err) => {
            tell(
                app,
                MessageDialogKind::Error,
                format!("This backup cannot be restored.\n\n{err}"),
            );
            return;
        }
    };
    if let Some(problem) = &shown.problem {
        tell(
            app,
            MessageDialogKind::Error,
            format!("This backup cannot be restored.\n\n{problem}"),
        );
        return;
    }
    if shown.nothing_to_change {
        tell(
            app,
            MessageDialogKind::Info,
            "Everything in this backup is already how it is now. Nothing was changed.".to_owned(),
        );
        return;
    }
    let notes = if shown.warnings.is_empty() {
        String::new()
    } else {
        format!("\n\nNote:\n{}", shown.warnings.join("\n"))
    };
    let question = format!(
        "Restore this backup (made {})?\n\n{}{notes}\n\nNothing you have is removed: this adds what is \
         missing and overwrites what has the same name. A safety copy of what changes is saved \
         first; Settings, Backup & restore, Undo restore puts it back.",
        shown.archive.created,
        summary_lines(&shown).join("\n")
    );
    if !commands::confirmed(app, "Restore", question) {
        return;
    }
    let result = restore(
        &roots,
        &backup,
        &categories,
        Mode::Merge,
        &RestoreOptions {
            validate: &check,
            stamp: Stamp::now(),
            expect_sha256: Some(&backup.sha256),
        },
    );
    match result {
        Ok(report) => {
            app::reload(app);
            let mut text = "Restored. The new settings are in use.".to_owned();
            if !report.needs_approval.is_empty() {
                text.push_str(&format!(
                    "\n\nSevak will ask before it runs: {}.",
                    report.needs_approval.join(", ")
                ));
            }
            tell(app, MessageDialogKind::Info, text);
        }
        Err(err) => tell(
            app,
            MessageDialogKind::Error,
            format!("The restore was not applied.\n\n{err}"),
        ),
    }
}

/// One line per category that would change, for dialogs and the terminal.
fn summary_lines(shown: &Preview) -> Vec<String> {
    let mut lines = Vec::new();
    for category in shown.categories.iter().filter(|c| c.selected) {
        let mut parts = Vec::new();
        for (count, word) in [
            (category.added, "new"),
            (category.changed, "changed"),
            (category.removed, "removed"),
        ] {
            if count > 0 {
                parts.push(format!("{count} {word}"));
            }
        }
        let what = if parts.is_empty() {
            format!("{} unchanged", category.unchanged)
        } else {
            parts.join(", ")
        };
        lines.push(format!("  {}: {what}", category.label));
    }
    lines
}

// ---------------------------------------------------------------------------
// The command line
// ---------------------------------------------------------------------------

fn cli_roots(config: Option<&Path>) -> Result<Roots, ExitCode> {
    match AppPaths::resolve_with_config(config) {
        Ok(paths) => Ok(Roots::from_paths(&paths)),
        Err(err) => {
            eprintln!("error: cannot determine Sevak's directories: {err}");
            Err(ExitCode::FAILURE)
        }
    }
}

/// `sevak --backup PATH`: writes a backup of everything that may be backed up.
/// PATH is a file, or an existing folder (a name is made up).
pub fn run_cli_backup(path: &Path, config: Option<&Path>) -> ExitCode {
    let roots = match cli_roots(config) {
        Ok(roots) => roots,
        Err(code) => return code,
    };
    let stamp = Stamp::now();
    let destination = if path.is_dir() {
        Destination::Folder {
            dir: path,
            prefix: MANUAL_PREFIX,
        }
    } else {
        Destination::File(path)
    };
    match backup::create(&roots, &Category::ALL, Kind::Manual, &stamp, destination) {
        Ok(report) => {
            println!(
                "Backed up to {} ({} KiB).",
                report.path.display(),
                report.bytes.div_ceil(1024)
            );
            for category in &report.contents.categories {
                println!(
                    "  {}: {} file{}",
                    category.label,
                    category.files,
                    if category.files == 1 { "" } else { "s" }
                );
            }
            for warning in &report.contents.warnings {
                println!("  note: {warning}");
            }
            if !report.contents.left_out.is_empty() {
                println!(
                    "  left out on purpose: {}",
                    report.contents.left_out.join(", ")
                );
            }
            println!(
                "The file is not encrypted. It never holds passwords, API keys, clipboard or \
                 search history, or which scripts you allowed."
            );
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

/// `sevak --restore PATH [--replace]`: restores every category in the backup
/// (merge by default). Prints what changed and how to undo it.
pub fn run_cli_restore(path: &Path, replace: bool, config: Option<&Path>) -> ExitCode {
    let roots = match cli_roots(config) {
        Ok(roots) => roots,
        Err(code) => return code,
    };
    let backup = match read_file(path) {
        Ok(backup) => backup,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::FAILURE;
        }
    };
    let mode = if replace { Mode::Replace } else { Mode::Merge };
    let categories = backup.manifest.categories.clone();
    let check = validator(DisplayServer::detect().hotkey_strategy());
    let shown = match preview(&roots, &backup, &categories, mode, &check) {
        Ok(shown) => shown,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "Backup made {} by Sevak {}. Restoring in {} mode:",
        shown.archive.created,
        shown.archive.app_version,
        if replace { "replace" } else { "merge" }
    );
    for line in summary_lines(&shown) {
        println!("{line}");
    }
    for warning in &shown.warnings {
        println!("  note: {warning}");
    }
    match restore(
        &roots,
        &backup,
        &categories,
        mode,
        &RestoreOptions {
            validate: &check,
            stamp: Stamp::now(),
            expect_sha256: Some(&backup.sha256),
        },
    ) {
        Ok(report) if !report.changed => {
            println!("Nothing needed to change.");
            ExitCode::SUCCESS
        }
        Ok(report) => {
            if let Some(snapshot) = &report.snapshot {
                println!("Saved a safety copy first: {}", snapshot.display());
                println!("Undo with: sevak --undo-restore");
            }
            if !report.needs_approval.is_empty() {
                println!(
                    "Sevak asks before it runs: {}.",
                    report.needs_approval.join(", ")
                );
            }
            println!(
                "Done. If Sevak is running, choose \"Reload index\" in its tray menu (or restart \
                 it) to use the restored settings."
            );
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

/// `sevak --undo-restore`: puts back what the last restore replaced.
pub fn run_cli_undo(config: Option<&Path>) -> ExitCode {
    let roots = match cli_roots(config) {
        Ok(roots) => roots,
        Err(code) => return code,
    };
    match undo_restore(&roots, Stamp::now()) {
        Ok(_) => {
            println!(
                "Undone. If Sevak is running, choose \"Reload index\" in its tray menu (or \
                 restart it) to use the previous settings."
            );
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_ids_are_parsed_sorted_and_deduplicated() {
        let ids = ["themes", "settings", "themes"].map(str::to_owned);
        assert_eq!(
            parse_categories(&ids).unwrap(),
            vec![Category::Settings, Category::Themes]
        );
        assert!(parse_categories(&["nope".to_owned()]).is_err());
        assert!(parse_categories(&[]).unwrap().is_empty());
    }

    #[test]
    fn the_launcher_offers_the_commands_from_six_letters() {
        let plugin = BackupPlugin;
        assert!(plugin.query("").is_empty());
        assert!(plugin.query("back").is_empty());
        assert!(plugin.query("resto").is_empty());
        let titles = |input: &str| -> Vec<String> {
            plugin.query(input).into_iter().map(|r| r.title).collect()
        };
        assert_eq!(titles("backup"), ["Back up settings"]);
        assert_eq!(titles("Back up sett"), ["Back up settings"]);
        assert_eq!(
            titles("restore settings"),
            ["Restore settings from a backup…"]
        );
        assert_eq!(titles("restor"), ["Restore settings from a backup…"]);
        assert!(titles("backup settings now").is_empty());
        assert!(titles("calculator").is_empty());
        assert_eq!(plugin.query("backup")[0].id, "backup:now");
    }
}
