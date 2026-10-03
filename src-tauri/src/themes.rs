//! The theme editor's and gallery's commands (Settings, Appearance).
//!
//! Themes are files in `<config dir>/themes`; the validation and the checks on
//! downloads live in `sevak_core::theme_store`. This module is the glue: file
//! dialogs, the one opt-in network request (see `fetch_theme_gallery`) and
//! telling the windows when the applied theme changed.

use std::sync::Mutex;

use serde::Serialize;
use sevak_core::theme_file;
use sevak_core::theme_store::{self, GalleryEntry, StoredTheme, MAX_INDEX_BYTES, MAX_THEME_BYTES};
use sevak_platform::open;
use sevak_plugins::net;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

use crate::state::AppState;
use crate::window;

/// The gallery index as last fetched. Installing looks the entry up here, so
/// the URL and checksum come from the index and not from the webview.
static GALLERY: Mutex<Vec<GalleryEntry>> = Mutex::new(Vec::new());

/// Everything the editor lists.
#[derive(Debug, Serialize)]
pub struct ThemesDto {
    /// The themes that ship with Sevak, with the file each is written to when chosen.
    builtin: Vec<StoredTheme>,
    /// The valid `*.toml` files in the themes folder.
    installed: Vec<StoredTheme>,
    themes_dir: String,
}

#[tauri::command]
pub async fn list_themes(app: AppHandle) -> ThemesDto {
    let config_dir = app.state::<AppState>().paths.config_dir.clone();
    tauri::async_runtime::spawn_blocking(move || ThemesDto {
        builtin: theme_store::builtin(),
        installed: theme_store::list(&config_dir),
        themes_dir: config_dir
            .join(theme_store::THEMES_DIR)
            .display()
            .to_string(),
    })
    .await
    .unwrap_or_else(|err| {
        tracing::warn!("listing themes failed: {err}");
        ThemesDto {
            builtin: theme_store::builtin(),
            installed: Vec::new(),
            themes_dir: String::new(),
        }
    })
}

/// Validates the editor's theme and writes it as `themes/<name>.toml`.
#[tauri::command]
pub async fn save_theme(app: AppHandle, theme: serde_json::Value) -> Result<StoredTheme, String> {
    blocking(app, move |app| {
        let config_dir = app.state::<AppState>().paths.config_dir.clone();
        let spec = theme_file::from_value(&theme).spec;
        let stored = theme_store::save(&config_dir, spec)?;
        refresh_if_active(app, &stored.file);
        Ok(stored)
    })
    .await
}

/// Writes the built-in theme `name` to the themes folder (unless it is there).
#[tauri::command]
pub async fn use_builtin_theme(app: AppHandle, name: String) -> Result<StoredTheme, String> {
    blocking(app, move |app| {
        let config_dir = app.state::<AppState>().paths.config_dir.clone();
        theme_store::install_builtin(&config_dir, &name)
    })
    .await
}

/// "Import…": a native file picker, then the same checks as a download. `None`
/// when the user cancels.
#[tauri::command]
pub async fn import_theme(
    app: AppHandle,
    window: WebviewWindow,
) -> Result<Option<StoredTheme>, String> {
    blocking(app, move |app| {
        let Some(picked) = app
            .dialog()
            .file()
            .set_title("Import a theme")
            .add_filter("Sevak theme", &["toml"])
            .set_parent(&window)
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        let path = picked
            .into_path()
            .map_err(|err| format!("That is not a local file: {err}"))?;
        let size = std::fs::metadata(&path)
            .map_err(|err| format!("Cannot read the file: {err}"))?
            .len();
        if size > MAX_THEME_BYTES {
            return Err(format!(
                "The file is larger than {} KiB, so it is not a theme.",
                MAX_THEME_BYTES / 1024
            ));
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|err| format!("Cannot read the file as text: {err}"))?;
        let hint = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("Imported theme");
        let config_dir = app.state::<AppState>().paths.config_dir.clone();
        let stored = theme_store::install_text(&config_dir, &text, hint)?;
        refresh_if_active(app, &stored.file);
        Ok(Some(stored))
    })
    .await
}

/// "Export…": a native save dialog, then the theme file's canonical text.
/// `false` when the user cancels.
#[tauri::command]
pub async fn export_theme(
    app: AppHandle,
    window: WebviewWindow,
    file: String,
) -> Result<bool, String> {
    blocking(app, move |app| {
        let config_dir = app.state::<AppState>().paths.config_dir.clone();
        let theme = theme_store::load(&config_dir, &file)?;
        let name = if theme.spec.name.is_empty() {
            "theme".to_owned()
        } else {
            theme_store::file_stem(&theme.spec.name)
        };
        let Some(target) = app
            .dialog()
            .file()
            .set_title("Export the theme")
            .set_file_name(format!("{name}.toml"))
            .add_filter("Sevak theme", &["toml"])
            .set_parent(&window)
            .blocking_save_file()
        else {
            return Ok(false);
        };
        let target = target
            .into_path()
            .map_err(|err| format!("That is not a local file: {err}"))?;
        std::fs::write(&target, theme.spec.to_toml())
            .map_err(|err| format!("Cannot write {}: {err}", target.display()))?;
        Ok(true)
    })
    .await
}

#[tauri::command]
pub async fn open_themes_dir(app: AppHandle) -> Result<(), String> {
    let dir = app
        .state::<AppState>()
        .paths
        .config_dir
        .join(theme_store::THEMES_DIR);
    let _ = std::fs::create_dir_all(&dir);
    open::open_path(&dir).map_err(|err| err.to_string())
}

/// A gallery entry as the editor lists it.
#[derive(Debug, Serialize)]
pub struct GalleryItem {
    #[serde(flatten)]
    entry: GalleryEntry,
    /// A theme file of this name is already in the themes folder.
    installed: bool,
}

/// **The only network request of the theme editor**, made when the user clicks
/// "Browse online themes": one `GET` of the gallery index
/// ([`theme_store::GALLERY_INDEX_URL`]) with no cookies, no query string and no
/// identifying headers. Nothing is installed by it.
#[tauri::command]
pub async fn fetch_theme_gallery(app: AppHandle) -> Result<Vec<GalleryItem>, String> {
    blocking(app, |app| {
        let bytes = download(theme_store::GALLERY_INDEX_URL, MAX_INDEX_BYTES)?;
        let text = String::from_utf8(bytes)
            .map_err(|_| "The gallery index is not UTF-8 text.".to_owned())?;
        let entries = theme_store::parse_index(&text)?;
        *GALLERY
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = entries.clone();

        let config_dir = app.state::<AppState>().paths.config_dir.clone();
        Ok(entries
            .into_iter()
            .map(|entry| GalleryItem {
                installed: config_dir
                    .join(theme_store::file_for(&entry.name))
                    .is_file(),
                entry,
            })
            .collect())
    })
    .await
}

/// Downloads gallery theme `id` (from the index fetched above), checks its
/// SHA-256 against the index and only then writes it to the themes folder.
#[tauri::command]
pub async fn install_gallery_theme(app: AppHandle, id: String) -> Result<StoredTheme, String> {
    blocking(app, move |app| {
        let entry = GALLERY
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .find(|entry| entry.id == id)
            .cloned()
            .ok_or("Open the gallery again; that theme is no longer in the list.")?;
        let bytes = download(&entry.url, MAX_THEME_BYTES)?;
        let text = theme_store::verify_download(&bytes, &entry.sha256)?;
        let config_dir = app.state::<AppState>().paths.config_dir.clone();
        let stored = theme_store::install_text(&config_dir, &text, &entry.name)?;
        tracing::info!(theme = %entry.name, file = %stored.file, "installed a gallery theme");
        refresh_if_active(app, &stored.file);
        Ok(stored)
    })
    .await
}

/// Runs `work` on a blocking thread (disk, dialogs and the network all block).
async fn blocking<T: Send + 'static>(
    app: AppHandle,
    work: impl FnOnce(&AppHandle) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || work(&app))
        .await
        .map_err(|err| format!("the operation did not finish: {err}"))?
}

/// A downloaded file of at most `limit` bytes, through the download the
/// workflow gallery uses too (`sevak_plugins::net::fetch_https`: https only,
/// redirects only to https, a timeout, nothing sent but the request itself).
/// The address must also pass the theme index's own URL rules.
fn download(url: &str, limit: u64) -> Result<Vec<u8>, String> {
    if !theme_store::is_https_url(url) {
        return Err("Only https:// downloads are allowed.".to_owned());
    }
    let limit = usize::try_from(limit).unwrap_or(usize::MAX);
    net::fetch_https(url, limit).map_err(|err| format!("Cannot download it: {err}."))
}

/// `themes\Nord.toml`, `./themes/Nord.toml` and `themes/Nord.toml` are one file.
fn same_file(a: &str, b: &str) -> bool {
    let normal = |path: &str| {
        path.trim()
            .replace('\\', "/")
            .trim_start_matches("./")
            .to_lowercase()
    };
    normal(a) == normal(b)
}

/// When `file` is the theme in use, re-reads it and tells the windows: the
/// launcher and Settings follow the new look at once.
fn refresh_if_active(app: &AppHandle, file: &str) {
    let state = app.state::<AppState>();
    if !same_file(&state.config().appearance.theme_file, file) {
        return;
    }
    state.refresh_appearance();
    window::apply_configured_width(app);
    if let Err(err) = app.emit(window::EVENT_STATUS, state.status()) {
        tracing::warn!("could not emit {}: {err}", window::EVENT_STATUS);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spellings_of_one_theme_path_are_the_same_file() {
        assert!(same_file("themes/Nord.toml", "themes\\nord.toml"));
        assert!(same_file("./themes/Nord.toml", " themes/Nord.toml "));
        assert!(!same_file("", "themes/Nord.toml"));
        assert!(!same_file("themes/Nord.toml", "themes/Dracula.toml"));
    }

    #[test]
    fn only_https_downloads_are_attempted() {
        for url in [
            "http://example.com/a.toml",
            "file:///etc/passwd",
            "ftp://x/y",
            "",
        ] {
            assert!(download(url, 10).unwrap_err().contains("https"), "{url}");
        }
    }
}
