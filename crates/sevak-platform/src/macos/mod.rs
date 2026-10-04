//! macOS implementation of [`crate::PlatformProvider`]: `.app` bundles from the
//! standard application folders, launched and opened through `/usr/bin/open`.

mod capture;
mod contacts;
mod dictionary;
mod expand;
pub(crate) mod media;
mod paste;
pub(crate) mod trash;

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use sevak_core::{AppEntry, ClipContent, IconData, IconSource, LaunchTarget};

use crate::capture::{CaptureOptions, SelectionCapture};
use crate::contacts::{Contact, ContactsAccess};
use crate::error::{PlatformError, Result};
use crate::icon_file;
use crate::keyboard::{KeyListener, KeyListenerSupport, KeySink, TypingTarget};
use crate::paste::{ClipboardRead, ForegroundApp, PasteContent, PasteOutcome, PasteSupport};
use crate::process::spawn_detached_in;
use crate::provider::PlatformProvider;

/// `open(1)`: launches bundles and opens files/URLs with their default handler.
pub(crate) const OPEN: &str = "/usr/bin/open";

/// `sips(1)`, which ships with macOS and converts `.icns` (including the
/// JPEG 2000 variants) to PNG.
const SIPS: &str = "/usr/bin/sips";

pub(crate) use paste::clipboard_sequence;

pub(crate) struct MacProvider;

impl MacProvider {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl PlatformProvider for MacProvider {
    fn list_applications(&self) -> Result<Vec<AppEntry>> {
        let started = Instant::now();
        let apps = scan_applications(&application_dirs());
        tracing::info!(
            count = apps.len(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "indexed applications"
        );
        Ok(apps)
    }

    fn launch(&self, target: &LaunchTarget) -> Result<()> {
        match target {
            LaunchTarget::Executable {
                path,
                args,
                working_dir,
            } => {
                let program = path.to_str().ok_or_else(|| PlatformError::Os {
                    operation: "launch",
                    message: format!("executable path is not valid UTF-8: {}", path.display()),
                })?;
                spawn_detached_in(program, args, working_dir.as_deref())
            }
            LaunchTarget::Shortcut { .. } => Err(PlatformError::Unsupported("Windows shortcuts")),
            LaunchTarget::PackagedApp { .. } => Err(PlatformError::Unsupported("packaged apps")),
            LaunchTarget::DesktopEntry { .. } => {
                Err(PlatformError::Unsupported("freedesktop entries"))
            }
        }
    }

    fn load_icon(&self, source: &IconSource, size: u32) -> Result<IconData> {
        match source {
            IconSource::File { path } if is_icns(path) => icns_to_png(path, size),
            IconSource::File { path } => icon_file::load(path),
            IconSource::Shell { .. } => Err(PlatformError::Unsupported("shell icons")),
            IconSource::Builtin { .. } => Err(PlatformError::Unsupported("built-in icons")),
        }
    }

    fn remember_foreground_app(&self) {
        paste::remember_foreground_app();
    }

    fn foreground_app(&self) -> Option<ForegroundApp> {
        paste::foreground_app()
    }

    fn identifies_apps(&self) -> bool {
        true
    }

    fn paste_support(&self) -> PasteSupport {
        paste::paste_support()
    }

    fn paste_text(&self, text: &str, restore_clipboard: bool) -> Result<PasteOutcome> {
        paste::paste_text(text, restore_clipboard)
    }

    fn paste_clip(&self, content: &ClipContent, restore_clipboard: bool) -> Result<PasteOutcome> {
        paste::paste_content(PasteContent::Clip(content), restore_clipboard)
    }

    fn key_listener_support(&self) -> KeyListenerSupport {
        expand::key_listener_support()
    }

    fn request_key_listener_permission(&self) {
        expand::request_key_listener_permission();
    }

    fn start_key_listener(&self, sink: KeySink) -> Result<KeyListener> {
        expand::start(sink)
    }

    fn typing_target(&self) -> TypingTarget {
        expand::typing_target()
    }

    fn replace_typed_text(
        &self,
        delete: usize,
        text: &str,
        still_current: &dyn Fn() -> bool,
    ) -> Result<bool> {
        expand::replace_typed_text(delete, text, still_current)
    }

    fn capture_selection(&self, options: &CaptureOptions) -> SelectionCapture {
        capture::capture_selection(options)
    }

    fn clipboard_sequence(&self) -> Option<u64> {
        paste::clipboard_sequence()
    }

    fn read_clipboard(&self) -> Result<ClipboardRead> {
        paste::read_clipboard()
    }

    fn contacts_access(&self) -> ContactsAccess {
        contacts::status()
    }

    fn request_contacts_access(&self) -> Result<ContactsAccess> {
        contacts::request()
    }

    fn system_contacts(&self) -> Result<Vec<Contact>> {
        contacts::read_contacts()
    }

    fn system_definition(&self, word: &str) -> Option<String> {
        dictionary::definition(word)
    }
}

/// Where applications are installed, most specific first so a user's own copy
/// of an app wins over the system-wide one when both have the same bundle id.
fn application_dirs() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join("Applications"));
    }
    roots.extend(
        [
            "/Applications",
            "/System/Applications",
            "/System/Library/CoreServices/Applications",
        ]
        .map(PathBuf::from),
    );
    roots
}

/// Finds `.app` bundles in `roots` and one folder level below them (e.g.
/// `/Applications/Utilities`), never descending into a bundle.
fn scan_applications(roots: &[PathBuf]) -> Vec<AppEntry> {
    let mut apps = Vec::new();
    let mut seen = HashSet::new();
    for root in roots {
        for bundle in bundles_in(root, 1) {
            let Some(app) = read_bundle(&bundle) else {
                continue;
            };
            if seen.insert(app.id.clone()) {
                apps.push(app);
            }
        }
    }
    apps
}

fn bundles_in(dir: &Path, depth: u8) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut bundles = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        // `is_dir` follows symlinks, which some installers use in /Applications.
        if !path.is_dir() {
            continue;
        }
        if is_bundle(&path) {
            bundles.push(path);
        } else if depth > 0 {
            bundles.extend(bundles_in(&path, depth - 1));
        }
    }
    bundles.sort();
    bundles
}

fn is_bundle(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
}

fn is_icns(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("icns"))
}

/// Builds the entry for one bundle from its `Contents/Info.plist`. Background
/// agents and daemons (`LSBackgroundOnly`, `LSUIElement`) are skipped: they have
/// no window to bring up.
fn read_bundle(bundle: &Path) -> Option<AppEntry> {
    let info = bundle.join("Contents").join("Info.plist");
    let plist = match plist::Value::from_file(&info) {
        Ok(plist) => Some(plist),
        Err(err) => {
            tracing::debug!(path = %info.display(), %err, "unreadable Info.plist");
            None
        }
    };
    let dict = plist.as_ref().and_then(plist::Value::as_dictionary);
    let string = |key: &str| {
        dict.and_then(|d| d.get(key))
            .and_then(plist::Value::as_string)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    let flag = |key: &str| {
        dict.and_then(|d| d.get(key)).is_some_and(|value| {
            value.as_boolean().unwrap_or(false) || value.as_string() == Some("1")
        })
    };
    if flag("LSBackgroundOnly") || flag("LSUIElement") {
        return None;
    }

    let stem = bundle.file_stem()?.to_string_lossy().into_owned();
    let name = string("CFBundleDisplayName")
        .or_else(|| string("CFBundleName"))
        .map_or_else(|| stem.clone(), str::to_owned);
    let id =
        string("CFBundleIdentifier").map_or_else(|| bundle.display().to_string(), str::to_owned);

    let mut keywords = Vec::new();
    if !stem.eq_ignore_ascii_case(&name) {
        keywords.push(stem);
    }

    let icon = string("CFBundleIconFile")
        .map(|file| {
            let path = bundle.join("Contents").join("Resources").join(file);
            if path.extension().is_none() {
                path.with_extension("icns")
            } else {
                path
            }
        })
        .filter(|path| path.is_file())
        .map(|path| IconSource::File { path });

    Some(AppEntry {
        id,
        name,
        description: None,
        keywords,
        icon,
        target: LaunchTarget::Executable {
            path: PathBuf::from(OPEN),
            args: vec!["-a".to_owned(), bundle.to_string_lossy().into_owned()],
            working_dir: None,
        },
    })
}

/// Renders an `.icns` file as a PNG of at most `size` pixels with `sips`.
fn icns_to_png(path: &Path, size: u32) -> Result<IconData> {
    let out = tempfile_path();
    let status = Command::new(SIPS)
        .args(["-s", "format", "png", "-Z", &size.max(16).to_string()])
        .arg(path)
        .arg("--out")
        .arg(&out)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|err| PlatformError::CommandFailed {
            command: SIPS.to_owned(),
            message: err.to_string(),
        })?;
    let bytes = fs::read(&out);
    let _ = fs::remove_file(&out);
    if !status.success() {
        return Err(PlatformError::CommandFailed {
            command: SIPS.to_owned(),
            message: format!("{status} converting {}", path.display()),
        });
    }
    Ok(IconData {
        mime: "image/png",
        bytes: bytes?,
    })
}

/// A unique scratch file for one `sips` conversion (icons load concurrently).
fn tempfile_path() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("sevak-icon-{}-{n}.png", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_bundle(root: &Path, name: &str, plist: &str) -> PathBuf {
        let bundle = root.join(format!("{name}.app"));
        let contents = bundle.join("Contents");
        fs::create_dir_all(contents.join("Resources")).unwrap();
        fs::write(contents.join("Info.plist"), plist).unwrap();
        bundle
    }

    fn plist(entries: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>{entries}</dict></plist>"#
        )
    }

    #[test]
    fn reads_name_id_icon_and_launch_target() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = write_bundle(
            dir.path(),
            "Visual Studio Code",
            &plist(
                "<key>CFBundleName</key><string>Code</string>\
                 <key>CFBundleIdentifier</key><string>com.microsoft.VSCode</string>\
                 <key>CFBundleIconFile</key><string>Code</string>",
            ),
        );
        fs::write(bundle.join("Contents/Resources/Code.icns"), b"icns").unwrap();

        let app = read_bundle(&bundle).unwrap();
        assert_eq!(app.name, "Code");
        assert_eq!(app.id, "com.microsoft.VSCode");
        assert_eq!(app.keywords, ["Visual Studio Code"]);
        assert_eq!(
            app.icon,
            Some(IconSource::File {
                path: bundle.join("Contents/Resources/Code.icns")
            })
        );
        assert_eq!(
            app.target,
            LaunchTarget::Executable {
                path: OPEN.into(),
                args: vec!["-a".into(), bundle.to_string_lossy().into_owned()],
                working_dir: None,
            }
        );
    }

    #[test]
    fn display_name_wins_and_missing_plist_falls_back_to_the_folder_name() {
        let dir = tempfile::tempdir().unwrap();
        let named = write_bundle(
            dir.path(),
            "Safari",
            &plist(
                "<key>CFBundleName</key><string>Safari</string>\
                 <key>CFBundleDisplayName</key><string>Safari Browser</string>",
            ),
        );
        assert_eq!(read_bundle(&named).unwrap().name, "Safari Browser");

        let bare = dir.path().join("Bare.app");
        fs::create_dir_all(&bare).unwrap();
        let app = read_bundle(&bare).unwrap();
        assert_eq!(app.name, "Bare");
        assert_eq!(app.id, bare.display().to_string());
        assert!(app.icon.is_none());
    }

    #[test]
    fn background_agents_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let agent = write_bundle(
            dir.path(),
            "Helper",
            &plist("<key>LSUIElement</key><true/>"),
        );
        let daemon = write_bundle(
            dir.path(),
            "Daemon",
            &plist("<key>LSBackgroundOnly</key><string>1</string>"),
        );
        assert!(read_bundle(&agent).is_none());
        assert!(read_bundle(&daemon).is_none());
    }

    #[test]
    fn scans_one_level_deep_without_entering_bundles_and_dedupes_ids() {
        let user = tempfile::tempdir().unwrap();
        let system = tempfile::tempdir().unwrap();
        let id = |id: &str| {
            plist(&format!(
                "<key>CFBundleIdentifier</key><string>{id}</string>"
            ))
        };

        write_bundle(user.path(), "Notes", &id("com.apple.Notes"));
        write_bundle(system.path(), "Notes", &id("com.apple.Notes"));
        let utilities = system.path().join("Utilities");
        fs::create_dir_all(&utilities).unwrap();
        let terminal = write_bundle(&utilities, "Terminal", &id("com.apple.Terminal"));
        // A helper app nested inside a bundle is not an installed application.
        write_bundle(&terminal.join("Contents"), "Nested", &id("nested"));

        let apps = scan_applications(&[user.path().into(), system.path().into()]);
        let ids: Vec<_> = apps.iter().map(|app| app.id.as_str()).collect();
        assert_eq!(ids, ["com.apple.Notes", "com.apple.Terminal"]);
        assert!(matches!(
            &apps[0].target,
            LaunchTarget::Executable { args, .. } if args[1].starts_with(&*user.path().to_string_lossy())
        ));
    }

    /// Prints what the real machine's application folders yield:
    /// `cargo test -p sevak-platform -- --ignored --nocapture real_applications`
    #[test]
    #[ignore = "reads the real system's applications; run manually"]
    fn real_applications() {
        let apps = MacProvider::new().list_applications().unwrap();
        for app in &apps {
            println!("- {} [{}] icon: {:?}", app.name, app.id, app.icon);
        }
        assert!(!apps.is_empty(), "no applications found");
        let with_icon = apps.iter().find_map(|app| app.icon.clone()).unwrap();
        let png = MacProvider::new().load_icon(&with_icon, 64).unwrap();
        assert!(png.bytes.starts_with(b"\x89PNG"));
    }
}
