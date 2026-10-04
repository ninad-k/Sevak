//! Linux implementation of [`crate::PlatformProvider`]: freedesktop `.desktop`
//! entries, icon themes, and launching through `gio`.

mod capture;
mod expand;
mod launch;
mod paste;
mod scan;
mod secret_hint;
pub(crate) mod tasks;
mod xdg;

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use sevak_core::{AppEntry, ClipContent, IconData, IconSource, LaunchTarget};

use crate::capture::{CaptureOptions, SelectionCapture};
use crate::desktop_entry::Locale;
use crate::error::{PlatformError, Result};
use crate::icon_file;
use crate::icon_theme::{self, IconResolver};
use crate::keyboard::{KeyListener, KeyListenerSupport, KeySink, TypingTarget};
use crate::paste::{ClipboardRead, ForegroundApp, PasteContent, PasteOutcome, PasteSupport};
use crate::provider::PlatformProvider;
use crate::session;

pub(crate) struct LinuxProvider;

impl LinuxProvider {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl PlatformProvider for LinuxProvider {
    fn list_applications(&self) -> Result<Vec<AppEntry>> {
        let started = Instant::now();
        let env = xdg::XdgEnv::from_process();
        let desktops = session::current_desktops();
        let locale = Locale::from_env();

        let theme = gsettings_icon_theme();
        let (icon_dirs, pixmap_dirs) = env.icon_dirs();
        let mut icons = IconResolver::new(theme.as_deref(), icon_dirs, pixmap_dirs);

        let context = scan::ScanContext {
            desktops: &desktops,
            locale: &locale,
        };
        let apps = scan::scan_applications(&env.application_dirs(), &context, &mut icons);
        tracing::info!(
            count = apps.len(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            icon_theme = theme.as_deref().unwrap_or(icon_theme::FALLBACK_THEME),
            "indexed applications"
        );
        Ok(apps)
    }

    fn launch(&self, target: &LaunchTarget) -> Result<()> {
        launch::launch(target)
    }

    fn load_icon(&self, source: &IconSource, _size: u32) -> Result<IconData> {
        match source {
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
        paste::can_identify_apps()
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

    fn read_clipboard(&self) -> Result<ClipboardRead> {
        paste::read_clipboard()
    }

    fn key_listener_support(&self) -> KeyListenerSupport {
        expand::key_listener_support()
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
}

/// The user's GNOME icon theme, from `gsettings`. `None` when gsettings is
/// missing, the schema is absent (other desktops) or it does not answer in time.
fn gsettings_icon_theme() -> Option<String> {
    let mut child = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "icon-theme"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) | Err(_) => return None,
            Ok(None) if Instant::now() >= deadline => {
                // Reap it so it does not linger; the answer is not worth waiting for.
                let _ = child.kill();
                let _ = child.wait();
                tracing::debug!("gsettings did not answer in time");
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
        }
    }

    let mut output = String::new();
    child
        .stdout
        .take()?
        .take(64 * 1024)
        .read_to_string(&mut output)
        .ok()?;
    icon_theme::parse_gsettings_string(&output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_icon_rejects_non_file_sources() {
        let provider = LinuxProvider::new();
        let builtin = provider.load_icon(&IconSource::builtin("app"), 48);
        assert!(matches!(builtin, Err(PlatformError::Unsupported(_))));
        let shell = provider.load_icon(
            &IconSource::Shell {
                parsing_name: "x".into(),
            },
            48,
        );
        assert!(matches!(shell, Err(PlatformError::Unsupported(_))));
    }

    #[test]
    fn load_icon_reads_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("icon.png");
        std::fs::write(&path, b"png-bytes").unwrap();
        let data = LinuxProvider::new()
            .load_icon(&IconSource::File { path }, 48)
            .unwrap();
        assert_eq!(data.mime, "image/png");
        assert_eq!(data.bytes, b"png-bytes");
    }

    /// Prints what the real machine's application directories yield:
    /// `cargo test -p sevak-platform -- --ignored --nocapture real_applications`
    #[test]
    #[ignore = "reads the real system's applications; run manually"]
    fn real_applications() {
        let started = Instant::now();
        let apps = LinuxProvider::new().list_applications().unwrap();
        let elapsed = started.elapsed();
        let with_icon = apps.iter().filter(|app| app.icon.is_some()).count();
        println!(
            "{} applications ({} with a resolved icon) in {elapsed:?}",
            apps.len(),
            with_icon
        );
        for app in &apps {
            let icon = match &app.icon {
                Some(IconSource::File { path }) => path.display().to_string(),
                Some(other) => format!("{other:?}"),
                None => "<no icon>".to_owned(),
            };
            println!(
                "- {} [{}]\n    desc: {:?}\n    keywords: {:?}\n    icon: {icon}\n    target: {:?}",
                app.name, app.id, app.description, app.keywords, app.target
            );
        }
        assert!(!apps.is_empty(), "no applications found");
    }
}
