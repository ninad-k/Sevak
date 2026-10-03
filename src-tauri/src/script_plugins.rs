//! First-run approval of script plugins.
//!
//! A script plugin runs code with the user's privileges, so a folder that
//! appears in `<config dir>/plugins` does nothing until the user says yes once.
//! This asks with a native dialog (no webview needed, so it works from the
//! tray's "Reload index" and at startup alike) and remembers the answer per
//! plugin and command; see `sevak_plugins::script` for the rules.

use std::sync::atomic::{AtomicBool, Ordering};

use sevak_plugins::script::Candidate;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

use crate::app;
use crate::state::AppState;

const TITLE: &str = "Sevak: new script plugin";

/// One round of questions at a time.
static ASKING: AtomicBool = AtomicBool::new(false);

/// Asks about every script plugin that is new (or whose command changed) and
/// reloads Sevak if any was allowed. Returns immediately; the dialogs run on
/// their own thread.
pub fn review_new(app: &AppHandle) {
    let state = app.state::<AppState>();
    let pending = state.search.scripts.pending(&state.config());
    if pending.is_empty() || ASKING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("sevak-script-approval".to_owned())
        .spawn({
            let app = app.clone();
            move || {
                let mut allowed = false;
                for candidate in &pending {
                    allowed |= ask(&app, candidate);
                }
                ASKING.store(false, Ordering::SeqCst);
                if allowed {
                    app::reload(&app);
                }
            }
        });
    if let Err(err) = spawned {
        ASKING.store(false, Ordering::SeqCst);
        tracing::error!("could not start the script plugin approval: {err}");
    }
}

/// Shows the question for one plugin; true when it was allowed and saved.
fn ask(app: &AppHandle, candidate: &Candidate) -> bool {
    let manifest = &candidate.manifest;
    let prompt = format!(
        "Sevak found a script plugin it has not run before.\n\n\
         Name: {name}\n\
         Keyword: {keyword}\n\
         Folder: {folder}\n\
         Runs: {command}\n\n\
         A script plugin runs with your account's permissions, like any program you start. \
         Allow it only if you trust where it came from. You can switch it off any time in \
         Settings.",
        name = manifest.name,
        keyword = manifest.keyword,
        folder = candidate.dir.display(),
        command = manifest.command_line(),
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
        // Runs on this helper thread, never on the main thread.
        .blocking_show();

    let scripts = &app.state::<AppState>().search.scripts;
    if !allowed {
        tracing::info!(
            plugin = manifest.id,
            "script plugin not allowed (asked again next start)"
        );
        scripts.decline(candidate);
        return false;
    }
    match scripts.approve(candidate) {
        Ok(()) => {
            tracing::info!(plugin = manifest.id, "script plugin allowed");
            true
        }
        Err(err) => {
            tracing::error!(plugin = manifest.id, "{err}");
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
