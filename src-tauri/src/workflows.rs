//! Workflows in the shell: native notifications and launcher output for their
//! output nodes, the first-run Allow dialog, the commands behind Settings >
//! Workflows and Gallery, and `sevak --trigger`.
//!
//! The engine side lives in `sevak_plugins::workflow`; this module only adds
//! what needs Tauri.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::Serialize;
use sevak_plugins::keywords::{workflow_key, KeywordOwners};
use sevak_plugins::workflow::gallery::{self, Dirs, Entry, Kind};
use sevak_plugins::workflow::templates;
use sevak_plugins::workflow::{
    Candidate, Loaded, OutputSink, Problem, Saved, Scanned, Summary, Workflow,
};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_notification::NotificationExt;

use crate::direct::{OutputKind, OutputPayload, ShowPayload};
use crate::state::AppState;
use crate::{app, commands, window};

const TITLE: &str = "Sevak: new workflow";

// ---- output nodes --------------------------------------------------------

/// Shows what a workflow's output nodes produce: notifications through the
/// system, Large Type and text in the launcher window. Built before the app
/// exists (the search engine needs the workflow host first), so it gets its
/// handle later ([`TauriSink::attach`]).
#[derive(Default)]
pub struct TauriSink {
    app: OnceLock<AppHandle>,
}

impl TauriSink {
    pub fn attach(&self, app: &AppHandle) {
        let _ = self.app.set(app.clone());
    }

    fn show(&self, kind: OutputKind, heading: &str, text: &str) {
        let Some(app) = self.app.get() else { return };
        window::show_with(
            app,
            ShowPayload {
                output: Some(OutputPayload {
                    kind,
                    heading: heading.to_owned(),
                    text: text.to_owned(),
                }),
                ..ShowPayload::default()
            },
        );
    }
}

impl OutputSink for TauriSink {
    fn notify(&self, heading: &str, body: &str) {
        let Some(app) = self.app.get() else { return };
        if let Err(err) = app
            .notification()
            .builder()
            .title(heading)
            .body(body)
            .show()
        {
            tracing::warn!("could not show a notification: {err}");
        }
    }

    fn large_type(&self, text: &str) {
        self.show(OutputKind::LargeType, "", text);
    }

    fn text_view(&self, heading: &str, text: &str) {
        self.show(OutputKind::TextView, heading, text);
    }

    fn confirm(&self, question: &str) -> bool {
        let Some(app) = self.app.get() else {
            return false;
        };
        commands::confirmed(app, "Continue", question.to_owned())
    }
}

// ---- approval ------------------------------------------------------------

/// One round of questions at a time.
static ASKING: AtomicBool = AtomicBool::new(false);

/// Asks about every workflow that can run code and is new (or whose commands
/// or scripts changed), and reloads Sevak if any was allowed. Returns
/// immediately; the dialogs run on their own thread.
pub fn review_new(app: &AppHandle) {
    let state = app.state::<AppState>();
    let pending = state.search.workflows.pending(&state.config());
    if pending.is_empty() || ASKING.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("sevak-workflow-approval".to_owned())
        .spawn({
            let app = app.clone();
            move || {
                let mut allowed = false;
                for candidate in &pending {
                    allowed |= ask(&app, candidate, true);
                }
                ASKING.store(false, Ordering::SeqCst);
                if allowed {
                    app::reload(&app);
                }
            }
        });
    if let Err(err) = spawned {
        ASKING.store(false, Ordering::SeqCst);
        tracing::error!("could not start the workflow approval: {err}");
    }
}

/// Shows the question for one workflow; true when it was allowed and saved.
/// `remember_no` makes "Not now" stick until Sevak restarts.
fn ask(app: &AppHandle, candidate: &Candidate, remember_no: bool) -> bool {
    let prompt = format!(
        "Sevak found a workflow it has not run before.\n\n\
         Name: {name}\n\
         {details}\n\n\
         A workflow that runs programs does so with your account's permissions, like any \
         program you start. Allow it only if you trust where it came from. You can switch it \
         off any time in Settings > Workflows.",
        name = display_name(&candidate.workflow, &candidate.folder),
        details = candidate.describe(),
    );
    let allowed = app
        .dialog()
        .message(prompt)
        .title(TITLE)
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Allow".to_owned(),
            "Not now".to_owned(),
        ))
        // Runs on a helper thread, never on the main thread.
        .blocking_show();

    let workflows = &app.state::<AppState>().search.workflows;
    if !allowed {
        tracing::info!(
            workflow = candidate.folder,
            "workflow not allowed (asked again next start)"
        );
        if remember_no {
            workflows.decline(candidate);
        }
        return false;
    }
    match workflows.approve(candidate) {
        Ok(()) => {
            tracing::info!(workflow = candidate.folder, "workflow allowed");
            true
        }
        Err(err) => {
            tracing::error!(workflow = candidate.folder, "{err}");
            app.dialog()
                .message(format!("Could not remember your choice.\n\n{err}"))
                .title(TITLE)
                .kind(MessageDialogKind::Error)
                .buttons(MessageDialogButtons::Ok)
                .show(|_| {});
            false
        }
    }
}

fn display_name(workflow: &Workflow, folder: &str) -> String {
    let name = workflow.name.trim();
    if name.is_empty() {
        folder.to_owned()
    } else {
        name.to_owned()
    }
}

// ---- sevak --trigger -------------------------------------------------------

/// `sevak --trigger <workflow>/<id> [text]`: starts an external trigger. When
/// it cannot run (unknown, disabled, not allowed yet) the launcher opens with
/// the reason, so the command never seems to do nothing.
pub fn run_trigger(app: &AppHandle, target: String, arg: String) {
    // Now, while the user's app still has focus: a paste node returns to it.
    if let Some(state) = app.try_state::<AppState>() {
        state.search.platform.remember_foreground_app();
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let outcome = match target.split_once('/') {
            Some((folder, node)) if !folder.is_empty() && !node.is_empty() => app
                .state::<AppState>()
                .search
                .workflows
                .trigger(folder, node, &arg),
            _ => Err(format!(
                "\"{target}\" is not a trigger. Use sevak --trigger <workflow folder>/<node id>."
            )),
        };
        match outcome {
            Ok(()) => tracing::info!(trigger = target, "workflow trigger started"),
            Err(reason) => {
                tracing::warn!(trigger = target, "could not start the trigger: {reason}");
                window::show_with(
                    &app,
                    ShowPayload {
                        error: Some(reason),
                        ..ShowPayload::default()
                    },
                );
            }
        }
    });
}

// ---- Settings > Workflows ----------------------------------------------------

/// The Workflows page: every workflow folder, and where they live.
#[derive(Serialize)]
pub struct WorkflowList {
    pub folder: String,
    pub workflows: Vec<WorkflowRow>,
}

#[derive(Serialize)]
pub struct WorkflowRow {
    #[serde(flatten)]
    pub summary: Summary,
    /// Hotkey triggers that could not be registered (key, reason).
    pub hotkey_errors: Vec<HotkeyError>,
}

#[derive(Serialize)]
pub struct HotkeyError {
    pub key: String,
    pub error: String,
}

#[tauri::command]
pub async fn list_workflows(app: AppHandle) -> WorkflowList {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let config = state.config();
        let owners =
            KeywordOwners::collect(&config, &state.search.scripts, &state.search.workflows);
        let hotkeys = state
            .custom_hotkeys
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        let workflows = state
            .search
            .workflows
            .summaries(&config, &owners)
            .into_iter()
            .map(|summary| {
                let prefix = format!("Run workflow:{}:run:", summary.folder);
                let hotkey_errors = hotkeys
                    .iter()
                    .filter(|status| status.description.starts_with(&prefix))
                    .filter_map(|status| {
                        status.error.clone().map(|error| HotkeyError {
                            key: status.key.clone(),
                            error,
                        })
                    })
                    .collect();
                WorkflowRow {
                    summary,
                    hotkey_errors,
                }
            })
            .collect();
        WorkflowList {
            folder: state.search.workflows.workflows_dir().display().to_string(),
            workflows,
        }
    })
    .await
    .unwrap_or_else(|err| {
        tracing::warn!("listing workflows failed: {err}");
        WorkflowList {
            folder: String::new(),
            workflows: Vec::new(),
        }
    })
}

#[tauri::command]
pub async fn load_workflow(app: AppHandle, folder: String) -> Result<Loaded, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let mut loaded = state.search.workflows.load(&folder)?;
        let owners = current_owners(&state);
        loaded.problems.extend(
            loaded
                .workflow
                .keyword_problems(&owners, Some(&workflow_key(&folder))),
        );
        Ok(loaded)
    })
    .await
}

/// Everything that answers a keyword right now.
fn current_owners(state: &AppState) -> KeywordOwners {
    KeywordOwners::collect(
        &state.config(),
        &state.search.scripts,
        &state.search.workflows,
    )
}

/// Live validation for the builder; nothing is saved. `folder` is where the
/// workflow is saved, if it is (its own keywords do not clash with themselves).
#[tauri::command]
pub async fn check_workflow(
    app: AppHandle,
    workflow: Workflow,
    folder: Option<String>,
) -> Vec<Problem> {
    blocking(move || {
        let owners = current_owners(&app.state::<AppState>());
        Ok(check(&workflow, &owners, folder.as_deref()))
    })
    .await
    .unwrap_or_default()
}

/// The errors, then the warnings, of `workflow`, keyword clashes included.
fn check(workflow: &Workflow, owners: &KeywordOwners, folder: Option<&str>) -> Vec<Problem> {
    let mut problems = workflow.validate();
    problems.extend(workflow.keyword_problems(owners, folder.map(workflow_key).as_deref()));
    problems
}

/// Saves a workflow (a new folder when `folder` is `None`) and reloads Sevak,
/// which asks the user about it if it can run code.
#[tauri::command]
pub async fn save_workflow(
    app: AppHandle,
    folder: Option<String>,
    workflow: Workflow,
) -> Result<Saved, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let mut saved = state.search.workflows.save(folder.as_deref(), &workflow)?;
        saved.problems.extend(
            workflow.keyword_problems(&current_owners(&state), Some(&workflow_key(&saved.folder))),
        );
        tracing::info!(workflow = saved.folder, "workflow saved");
        app::reload(&app);
        Ok(saved)
    })
    .await
}

#[derive(Serialize)]
pub struct TemplateDto {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
}

#[tauri::command]
pub fn workflow_templates() -> Vec<TemplateDto> {
    templates::all()
        .into_iter()
        .map(|template| TemplateDto {
            id: template.id,
            name: template.name,
            description: template.description,
        })
        .collect()
}

/// Creates a workflow folder from a template and reloads.
#[tauri::command]
pub async fn create_workflow(app: AppHandle, template: String) -> Result<Saved, String> {
    blocking(move || {
        let saved = app
            .state::<AppState>()
            .search
            .workflows
            .create_from_template(&template)?;
        tracing::info!(workflow = saved.folder, template, "workflow created");
        app::reload(&app);
        Ok(saved)
    })
    .await
}

/// Deletes a workflow folder after asking.
#[tauri::command]
pub async fn delete_workflow(app: AppHandle, folder: String) -> Result<bool, String> {
    blocking(move || {
        let workflows = &app.state::<AppState>().search.workflows;
        let name = workflows
            .load(&folder)
            .map(|loaded| display_name(&loaded.workflow, &folder))
            .unwrap_or_else(|_| folder.clone());
        if !commands::confirmed(
            &app,
            "Delete",
            format!("Delete the workflow \"{name}\" and everything in its folder?"),
        ) {
            return Ok(false);
        }
        workflows.delete(&folder)?;
        tracing::info!(workflow = folder, "workflow deleted");
        app::reload(&app);
        Ok(true)
    })
    .await
}

#[tauri::command]
pub async fn set_workflow_enabled(
    app: AppHandle,
    folder: String,
    enabled: bool,
) -> Result<(), String> {
    blocking(move || {
        app.state::<AppState>()
            .search
            .workflows
            .set_enabled(&folder, enabled)?;
        app::reload(&app);
        Ok(())
    })
    .await
}

/// Asks about one workflow again (the Workflows page's "Review" button), even
/// if the user said "Not now" earlier. Resolves to whether it was allowed.
#[tauri::command]
pub async fn review_workflow(app: AppHandle, folder: String) -> Result<bool, String> {
    blocking(move || {
        let candidate = app
            .state::<AppState>()
            .search
            .workflows
            .scan()
            .into_iter()
            .find_map(|scanned| match scanned {
                Scanned::Workflow(c) if c.folder == folder => Some(*c),
                _ => None,
            })
            .ok_or_else(|| "That workflow cannot be loaded.".to_owned())?;
        if candidate.approved {
            return Ok(true);
        }
        let allowed = ask(&app, &candidate, false);
        if allowed {
            app::reload(&app);
        }
        Ok(allowed)
    })
    .await
}

#[tauri::command]
pub async fn open_workflows_folder(app: AppHandle) -> Result<(), String> {
    blocking(move || {
        let dir = app
            .state::<AppState>()
            .search
            .workflows
            .workflows_dir()
            .to_path_buf();
        std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
        sevak_platform::open::open_path(&dir).map_err(|err| err.to_string())
    })
    .await
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|err| format!("the operation did not finish: {err}"))?
}

// ---- Settings > Gallery --------------------------------------------------------

/// The index as last loaded. Installs name an entry by id and are looked up
/// here, so the page can never make Sevak download an address of its choosing.
static INDEX: Mutex<Option<gallery::Index>> = Mutex::new(None);

#[derive(Serialize)]
pub struct GalleryDto {
    /// Where the index was fetched from (shown so the request is no secret).
    pub source: String,
    /// Set when the index is not from this build's own release, and why.
    pub note: Option<String>,
    pub name: String,
    pub entries: Vec<GalleryEntry>,
    /// Entries left out because they were invalid or of a kind this Sevak does
    /// not know.
    pub skipped: usize,
}

#[derive(Serialize)]
pub struct GalleryEntry {
    #[serde(flatten)]
    pub entry: Entry,
    pub installed: bool,
}

fn gallery_dto(app: &AppHandle, index: &gallery::Index) -> GalleryDto {
    let state = app.state::<AppState>();
    let dirs = Dirs {
        workflows: state.search.workflows.workflows_dir(),
        plugins: state.search.scripts.plugins_dir(),
    };
    GalleryDto {
        source: index.source.clone(),
        note: index.note.clone(),
        name: index.name.clone(),
        // Native extensions are installed from Settings > Extensions.
        entries: index
            .entries
            .iter()
            .filter(|entry| entry.kind != Kind::Native)
            .map(|entry| GalleryEntry {
                installed: dirs.is_installed(entry),
                entry: entry.clone(),
            })
            .collect(),
        skipped: index.skipped.len(),
    }
}

/// Downloads the gallery index. Only the "Load gallery" button calls this.
#[tauri::command]
pub async fn gallery_load(app: AppHandle) -> Result<GalleryDto, String> {
    blocking(move || {
        tracing::info!("gallery: fetching the index (the user asked)");
        let index = gallery::fetch_index()?;
        for skipped in &index.skipped {
            tracing::warn!("gallery: skipped an entry: {skipped}");
        }
        let dto = gallery_dto(&app, &index);
        *INDEX.lock().unwrap_or_else(|p| p.into_inner()) = Some(index);
        Ok(dto)
    })
    .await
}

#[derive(Serialize)]
pub struct Installed {
    pub id: String,
    pub kind: Kind,
    pub folder: String,
}

/// Downloads, verifies and unpacks one entry of the loaded gallery. Only an
/// "Install" click calls this. The new folder still needs the user's Allow
/// before anything in it runs; Sevak asks as soon as it reloads.
#[tauri::command]
pub async fn gallery_install(app: AppHandle, id: String) -> Result<Installed, String> {
    blocking(move || {
        let entry = INDEX
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .and_then(|index| index.entries.iter().find(|entry| entry.id == id).cloned())
            .ok_or_else(|| "Load the gallery first.".to_owned())?;
        tracing::info!(entry = entry.id, "gallery: installing (the user asked)");
        let installed = {
            let state = app.state::<AppState>();
            let dirs = Dirs {
                workflows: state.search.workflows.workflows_dir(),
                plugins: state.search.scripts.plugins_dir(),
            };
            gallery::install(&entry, &dirs)?
        };
        tracing::info!(
            entry = entry.id,
            folder = installed.folder,
            files = installed.files,
            "gallery: installed"
        );
        // Reload: the new folder is scanned, and its Allow dialog follows.
        app::reload(&app);
        Ok(Installed {
            id: entry.id,
            kind: installed.kind,
            folder: installed.folder,
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_template_list_matches_the_engine() {
        let list = workflow_templates();
        assert_eq!(list.len(), 3);
        assert!(list.iter().all(|t| templates::find(t.id).is_some()));
    }

    #[test]
    fn the_builder_checks_a_workflow_without_saving_it() {
        let workflow = Workflow {
            name: "x".into(),
            ..Workflow::default()
        };
        let problems = check(&workflow, &KeywordOwners::default(), None);
        assert!(problems.iter().any(|p| p.message.contains("no trigger")));
    }
}
