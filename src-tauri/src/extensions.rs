//! Settings > Extensions and the `ext` / `store` launcher keywords, in the shell.
//!
//! The logic is `sevak_plugins::extensions` (catalog, install, update, uninstall,
//! the launcher plugin); this module adds what needs Tauri: the commands behind
//! the page, notifications, the confirmation before a removal, reloading Sevak
//! after a change (which makes it ask about a new extension) and opening the
//! page from the launcher.
//!
//! Nothing here touches the network on its own: `extensions_refresh` runs when
//! the user presses the page's button, and an install, update or removal only
//! when they choose one. Opening the page reads local files (what is installed,
//! and the list saved by an earlier load).

use std::sync::{Arc, Mutex, OnceLock};

use serde::Serialize;
use sevak_core::config::Config;
use sevak_platform::AppPaths;
use sevak_plugins::extensions::{
    CatalogView, ExtensionStore, ExtensionsHost, Hooks, InstalledItem, ItemKind, Outcome, Report,
    StoreDirs,
};
use sevak_plugins::keywords::KeywordOwners;
use sevak_plugins::script::Scanned;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::state::AppState;
use crate::{app, commands, settings, window};

/// Emitted to every window when a change made from the launcher finished, so
/// an open page reloads its lists.
pub const EVENT_CHANGED: &str = "sevak:extensions";
/// Emitted to the settings window to switch page (see [`show_page`]).
const EVENT_PAGE: &str = "sevak:settings-page";

/// The one app handle, set at startup ([`attach`]). The launcher plugin is built
/// before the app exists, so its hooks reach the app through this.
static APP: OnceLock<AppHandle> = OnceLock::new();
/// A page the settings window should open on, for a window that is still
/// loading when [`show_page`] is called.
static PENDING_PAGE: Mutex<Option<&'static str>> = Mutex::new(None);

/// Where the store keeps things, from the app's folders.
pub fn store_dirs(paths: &AppPaths) -> StoreDirs {
    StoreDirs {
        config_dir: paths.config_dir.clone(),
        workflows: paths.config_dir.join("workflows"),
        plugins: paths.config_dir.join("plugins"),
        plugin_data: paths.data_dir.join("plugins"),
        workflow_data: paths.data_dir.join("workflows"),
        state: paths.data_dir.clone(),
        // The file script plugins and workflows share.
        approvals: paths.data_dir.join("script-plugin-approvals.json"),
    }
}

/// The store and the launcher plugins over it.
pub fn build_host(paths: &AppPaths) -> Arc<ExtensionsHost> {
    let store = Arc::new(ExtensionStore::new(store_dirs(paths)));
    // A program that runs from a folder has to be stopped before the folder is
    // replaced or removed (Windows will not move a folder in use).
    store.set_quiesce(Arc::new(|id: &str| {
        let Some(app) = APP.get() else { return };
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        if let Some(plugin) = state.search.engine().plugin(id) {
            plugin.shutdown();
        }
    }));
    Arc::new(ExtensionsHost::new(
        store,
        Hooks {
            finished: Box::new(finished),
            open_settings: Box::new(|| {
                if let Some(app) = APP.get() {
                    show_page(app);
                }
            }),
        },
    ))
}

/// Gives the module the app handle. Called once, from `search::start`.
pub fn attach(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

/// A change made from the launcher ended: tell the user, reload so a new
/// extension is found (and asked about), and tell an open settings page.
fn finished(report: Report) {
    let Some(app) = APP.get() else { return };
    tracing::info!(
        ok = report.ok,
        changed = report.changed,
        "extensions: {}",
        report.message
    );
    if let Err(err) = app
        .notification()
        .builder()
        .title("Sevak extensions")
        .body(&report.message)
        .show()
    {
        tracing::warn!("could not show a notification: {err}");
    }
    if report.changed {
        app::reload(app);
    }
    if let Err(err) = app.emit(EVENT_CHANGED, ()) {
        tracing::warn!("could not emit {EVENT_CHANGED}: {err}");
    }
}

/// Opens Settings on the Extensions page.
fn show_page(app: &AppHandle) {
    *PENDING_PAGE.lock().unwrap_or_else(|p| p.into_inner()) = Some("extensions");
    settings::open(app);
    let _ = app.emit_to(window::SETTINGS_LABEL, EVENT_PAGE, "extensions");
}

/// The page the settings window was asked to open on, once. Called when the
/// window starts, for the case where the event came before it was listening.
#[tauri::command]
pub fn extensions_take_page() -> Option<&'static str> {
    PENDING_PAGE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take()
}

// ---- what the page shows -----------------------------------------------------------

/// An installed item with how it stands now.
#[derive(Serialize)]
pub struct InstalledRow {
    #[serde(flatten)]
    pub item: InstalledItem,
    /// Switched on (not in the disabled list, a workflow that is enabled). A
    /// theme has no switch and is always "on".
    pub enabled: bool,
    /// `ready`, `waiting` (needs the user's Allow), `disabled`, `broken` or
    /// `theme`.
    pub status: &'static str,
    /// Why it is broken.
    pub problem: Option<String>,
    /// What the user types to use it.
    pub keywords: Vec<String>,
    /// Waiting for an Allow and a "Review" would ask.
    pub can_review: bool,
}

#[derive(Serialize)]
pub struct Overview {
    pub installed: Vec<InstalledRow>,
    /// The list loaded earlier (this session or saved by a previous one), if any.
    pub catalog: Option<CatalogView>,
    /// `<os>-<arch>`, the build a native extension needs.
    pub platform: String,
    pub plugins_folder: String,
}

fn rows(app: &AppHandle) -> Vec<InstalledRow> {
    let state = app.state::<AppState>();
    let config = state.config();
    let owners = KeywordOwners::collect(&config, &state.search.scripts, &state.search.workflows);
    let scanned = state.search.scripts.scan();
    let workflows = state.search.workflows.summaries(&config, &owners);
    state
        .search
        .extensions
        .store()
        .installed()
        .into_iter()
        .map(|item| match item.kind {
            ItemKind::Plugin | ItemKind::Native => {
                let found = scanned.iter().find(|scanned| match scanned {
                    Scanned::Plugin(c) => c.folder == item.folder,
                    Scanned::Broken { folder, .. } => *folder == item.folder,
                });
                match found {
                    Some(Scanned::Plugin(c)) => {
                        let id = &c.manifest.id;
                        let enabled =
                            config.plugins.is_enabled("script") && config.plugins.is_enabled(id);
                        InstalledRow {
                            status: if !enabled {
                                "disabled"
                            } else if c.approved {
                                "ready"
                            } else {
                                "waiting"
                            },
                            enabled,
                            problem: None,
                            keywords: vec![c.manifest.keyword.clone()],
                            can_review: enabled && !c.approved,
                            item,
                        }
                    }
                    Some(Scanned::Broken { error, .. }) => InstalledRow {
                        enabled: false,
                        status: "broken",
                        problem: Some(error.clone()),
                        keywords: Vec::new(),
                        can_review: false,
                        item,
                    },
                    None => InstalledRow {
                        enabled: false,
                        status: "broken",
                        problem: Some("Sevak did not find it in the plugins folder.".to_owned()),
                        keywords: Vec::new(),
                        can_review: false,
                        item,
                    },
                }
            }
            ItemKind::Workflow => match workflows.iter().find(|w| w.folder == item.folder) {
                Some(w) => InstalledRow {
                    status: if w.error.is_some() {
                        "broken"
                    } else if !w.enabled {
                        "disabled"
                    } else if w.needs_approval && !w.approved {
                        "waiting"
                    } else {
                        "ready"
                    },
                    enabled: w.enabled,
                    problem: w.error.clone(),
                    keywords: w.keywords.clone(),
                    can_review: w.enabled && w.needs_approval && !w.approved && w.error.is_none(),
                    item,
                },
                None => InstalledRow {
                    enabled: false,
                    status: "broken",
                    problem: Some("Sevak did not find it in the workflows folder.".to_owned()),
                    keywords: Vec::new(),
                    can_review: false,
                    item,
                },
            },
            ItemKind::Theme => InstalledRow {
                enabled: true,
                status: "theme",
                problem: None,
                keywords: Vec::new(),
                can_review: false,
                item,
            },
        })
        .collect()
}

fn overview(app: &AppHandle) -> Overview {
    let state = app.state::<AppState>();
    let store = state.search.extensions.store();
    // The list in memory, or the one saved by an earlier load. Local files only.
    let catalog = store.current_view().or_else(|| store.load_cached());
    Overview {
        installed: rows(app),
        catalog,
        platform: sevak_plugins::script::current_platform(),
        plugins_folder: state.paths.config_dir.join("plugins").display().to_string(),
    }
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|err| format!("the operation did not finish: {err}"))?
}

/// What is installed and the list saved earlier; no network.
#[tauri::command]
pub async fn extensions_overview(app: AppHandle) -> Result<Overview, String> {
    blocking(move || Ok(overview(&app))).await
}

/// **The only request of the page.** Loads the gallery list: one `GET` of the
/// package index and one of the theme index, at the release of this build, from
/// the Sevak repository, when the user presses the button. Nothing is installed.
#[tauri::command]
pub async fn extensions_refresh(app: AppHandle) -> Result<Overview, String> {
    blocking(move || {
        tracing::info!("extensions: loading the gallery lists (the user asked)");
        let state = app.state::<AppState>();
        state.search.extensions.store().refresh()?;
        state.search.extensions.refresh_snapshot();
        Ok(overview(&app))
    })
    .await
}

/// Installs, updates or removes one item of the loaded list (or of what the page
/// installed), then reloads Sevak so it is scanned and, if it can run code,
/// asked about.
fn change(
    app: &AppHandle,
    work: impl FnOnce(&ExtensionStore) -> Result<Outcome, String>,
) -> Result<Overview, String> {
    let result = {
        let state = app.state::<AppState>();
        work(state.search.extensions.store())
    };
    // Whatever happened, what is on disk may differ now.
    app.state::<AppState>().search.extensions.refresh_snapshot();
    app::reload(app);
    result.map(|_| overview(app))
}

#[tauri::command]
pub async fn extensions_install(app: AppHandle, id: String) -> Result<Overview, String> {
    blocking(move || {
        tracing::info!(id, "extensions: installing (the user asked)");
        change(&app, |store| store.install(&id))
    })
    .await
}

#[tauri::command]
pub async fn extensions_update(app: AppHandle, id: String) -> Result<Overview, String> {
    blocking(move || {
        tracing::info!(id, "extensions: updating (the user asked)");
        change(&app, |store| store.update(&id))
    })
    .await
}

#[tauri::command]
pub async fn extensions_uninstall(app: AppHandle, id: String) -> Result<Option<Overview>, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let installed = state
            .search
            .extensions
            .store()
            .installed()
            .into_iter()
            .find(|item| item.id == id)
            .ok_or_else(|| "Sevak did not install that, so it is not removed here.".to_owned())?;
        if installed.kind == ItemKind::Theme
            && state.config().appearance.theme_file == installed.folder
        {
            return Err(
                "That theme is in use. Choose another theme in Settings > Appearance, then remove it."
                    .to_owned(),
            );
        }
        let what = match installed.kind {
            ItemKind::Theme => "its theme file".to_owned(),
            ItemKind::Native => {
                "its folder, the program in it, the files it saved and your permission for it"
                    .to_owned()
            }
            _ => "its folder, the files it saved and your permission for it".to_owned(),
        };
        if !commands::confirmed(
            &app,
            "Remove",
            format!("Remove \"{}\"? This deletes {what}.", installed.name),
        ) {
            return Ok(None);
        }
        tracing::info!(id, "extensions: removing (the user asked)");
        change(&app, |store| store.uninstall(&id)).map(Some)
    })
    .await
}

/// Switches an installed plugin, native extension or workflow on or off.
#[tauri::command]
pub async fn extensions_set_enabled(
    app: AppHandle,
    id: String,
    enabled: bool,
) -> Result<Overview, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let item = state
            .search
            .extensions
            .store()
            .installed()
            .into_iter()
            .find(|item| item.id == id)
            .ok_or_else(|| "That is not installed.".to_owned())?;
        match item.kind {
            ItemKind::Workflow => state.search.workflows.set_enabled(&item.folder, enabled)?,
            ItemKind::Plugin | ItemKind::Native => {
                let plugin_id = state
                    .search
                    .scripts
                    .scan()
                    .into_iter()
                    .find_map(|scanned| match scanned {
                        Scanned::Plugin(c) if c.folder == item.folder => Some(c.manifest.id),
                        _ => None,
                    })
                    .ok_or_else(|| "Sevak cannot load that extension.".to_owned())?;
                set_plugin_enabled(&state, &plugin_id, enabled)?;
            }
            ItemKind::Theme => return Err("A theme has no switch.".to_owned()),
        }
        app::reload(&app);
        Ok(overview(&app))
    })
    .await
}

/// Writes the plugin's id into or out of `[plugins] disabled` of the config file.
fn set_plugin_enabled(state: &AppState, plugin_id: &str, enabled: bool) -> Result<(), String> {
    let (mut config, _) =
        Config::load_or_create(&state.paths.config_file).map_err(|err| err.to_string())?;
    config.plugins.disabled.retain(|id| id != plugin_id);
    if !enabled {
        config.plugins.disabled.push(plugin_id.to_owned());
    }
    config
        .normalized()
        .save_to(&state.paths.config_file)
        .map_err(|err| err.to_string())
}

/// Asks the Allow question again for an installed extension that is waiting
/// (the user said "Not now" earlier). Resolves to whether it was allowed.
#[tauri::command]
pub async fn extensions_review(app: AppHandle, id: String) -> Result<bool, String> {
    let (kind, folder) = {
        let state = app.state::<AppState>();
        let item = state
            .search
            .extensions
            .store()
            .installed()
            .into_iter()
            .find(|item| item.id == id)
            .ok_or_else(|| "That is not installed.".to_owned())?;
        (item.kind, item.folder)
    };
    match kind {
        ItemKind::Workflow => crate::workflows::review_workflow(app, folder).await,
        ItemKind::Plugin | ItemKind::Native => {
            blocking(move || crate::script_plugins::review_folder(&app, &folder)).await
        }
        ItemKind::Theme => Ok(true),
    }
}

/// Opens the plugins folder in the file manager.
#[tauri::command]
pub async fn extensions_open_folder(app: AppHandle) -> Result<(), String> {
    blocking(move || {
        let dir = app.state::<AppState>().paths.config_dir.join("plugins");
        std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
        sevak_platform::open::open_path(&dir).map_err(|err| err.to_string())
    })
    .await
}
