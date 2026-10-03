//! Windows implementation of [`crate::PlatformProvider`].

use std::collections::HashSet;
use std::ffi::OsString;
use std::time::Instant;

use sevak_core::{AppEntry, IconData, IconSource, LaunchTarget};

use crate::error::{PlatformError, Result};
use crate::paste::{ClipboardRead, ForegroundApp, PasteOutcome, PasteSupport};
use crate::provider::PlatformProvider;

use super::com::ComGuard;
use super::{allow_foreground_handoff, icons, packaged, paste, shell_execute_in, shortcuts};

pub(crate) struct WindowsProvider;

impl WindowsProvider {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl PlatformProvider for WindowsProvider {
    fn list_applications(&self) -> Result<Vec<AppEntry>> {
        let started = Instant::now();
        let _com = ComGuard::new();

        let roots = shortcuts::programs_roots();
        let mut entries = shortcuts::scan(&roots);
        let shortcut_count = entries.len();
        let shortcut_elapsed = started.elapsed();

        let known_names: HashSet<String> = entries.iter().map(|e| e.name.to_lowercase()).collect();
        let mut packaged_count = 0;
        match packaged::enumerate() {
            Ok(apps) => {
                for app in apps {
                    if !known_names.contains(&app.name.to_lowercase()) {
                        packaged_count += 1;
                        entries.push(app);
                    }
                }
            }
            Err(err) => tracing::warn!(%err, "could not enumerate packaged apps"),
        }

        entries.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.id.cmp(&b.id))
        });
        tracing::info!(
            total = entries.len(),
            shortcuts = shortcut_count,
            packaged = packaged_count,
            shortcut_scan_ms = shortcut_elapsed.as_millis() as u64,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "enumerated applications"
        );
        Ok(entries)
    }

    fn launch(&self, target: &LaunchTarget) -> Result<()> {
        launch_with_verb("open", target)
    }

    fn can_run_as_admin(&self) -> bool {
        true
    }

    fn launch_as_admin(&self, target: &LaunchTarget) -> Result<()> {
        // Packaged (Store) apps have no `runas` verb: they always run as the user.
        if matches!(target, LaunchTarget::PackagedApp { .. }) {
            return Err(PlatformError::Unsupported(
                "running Store apps as administrator",
            ));
        }
        launch_with_verb("runas", target).map_err(|err| match err {
            // Declining the UAC prompt surfaces as "access denied".
            PlatformError::Os { message, .. } if message == "access denied" => PlatformError::Os {
                operation: "run as administrator",
                message: "administrator permission was not granted".to_owned(),
            },
            other => other,
        })
    }

    fn load_icon(&self, source: &IconSource, size: u32) -> Result<IconData> {
        match source {
            IconSource::File { path } => crate::icon_file::load(path),
            IconSource::Shell { parsing_name } => icons::load_shell_icon(parsing_name, size),
            IconSource::Builtin { .. } => Err(PlatformError::Unsupported(
                "builtin icons are drawn by the UI",
            )),
        }
    }

    fn remember_foreground_app(&self) {
        paste::remember_foreground_app();
    }

    fn foreground_app(&self) -> Option<ForegroundApp> {
        paste::foreground_app()
    }

    fn paste_support(&self) -> PasteSupport {
        paste::paste_support()
    }

    fn paste_text(&self, text: &str, restore_clipboard: bool) -> Result<PasteOutcome> {
        paste::paste_text(text, restore_clipboard)
    }

    fn clipboard_sequence(&self) -> Option<u64> {
        paste::clipboard_sequence()
    }

    fn read_clipboard(&self) -> Result<ClipboardRead> {
        paste::read_clipboard()
    }
}

/// Starts `target` through the shell with `verb`: `open`, or `runas` to ask
/// for elevation (the user then sees the UAC prompt).
fn launch_with_verb(verb: &str, target: &LaunchTarget) -> Result<()> {
    let _com = ComGuard::new();
    allow_foreground_handoff();
    match target {
        LaunchTarget::Shortcut { path } => {
            shell_execute_in(verb, path.as_os_str(), None, None)?;
        }
        LaunchTarget::PackagedApp { app_user_model_id } => {
            let name = format!(r"shell:AppsFolder\{app_user_model_id}");
            shell_execute_in(verb, name.as_ref(), None, None)?;
        }
        LaunchTarget::Executable {
            path,
            args,
            working_dir,
        } => {
            let parameters = (!args.is_empty()).then(|| OsString::from(join_args(args)));
            shell_execute_in(
                verb,
                path.as_os_str(),
                parameters.as_deref(),
                working_dir.as_deref().map(|dir| dir.as_os_str()),
            )?;
        }
        LaunchTarget::DesktopEntry { .. } => {
            return Err(PlatformError::Unsupported(
                "launching .desktop entries on Windows",
            ));
        }
    }
    Ok(())
}

/// Joins arguments into a single command-line string that
/// `CommandLineToArgvW` (and the MSVC runtime) split back into `args`.
pub(crate) fn join_args(args: &[String]) -> String {
    args.iter()
        .map(|arg| quote_arg(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn push_backslashes(out: &mut String, count: usize) {
    out.extend(std::iter::repeat_n('\\', count));
}

pub(crate) fn quote_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '\n', '\x0b', '"']) {
        return arg.to_owned();
    }
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    let mut backslashes = 0usize;
    for ch in arg.chars() {
        match ch {
            '\\' => backslashes += 1,
            '"' => {
                // Backslashes before a quote must be doubled, and the quote escaped.
                push_backslashes(&mut out, backslashes * 2 + 1);
                out.push('"');
                backslashes = 0;
            }
            _ => {
                push_backslashes(&mut out, backslashes);
                out.push(ch);
                backslashes = 0;
            }
        }
    }
    // Backslashes before the closing quote must be doubled too.
    push_backslashes(&mut out, backslashes * 2);
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_arguments_are_left_alone() {
        assert_eq!(quote_arg("--flag"), "--flag");
        assert_eq!(quote_arg(r"C:\dir\file.txt"), r"C:\dir\file.txt");
    }

    #[test]
    fn arguments_with_spaces_or_quotes_are_quoted() {
        assert_eq!(quote_arg(""), r#""""#);
        assert_eq!(quote_arg("a b"), r#""a b""#);
        assert_eq!(quote_arg(r#"say "hi""#), r#""say \"hi\"""#);
        // Backslashes only matter before a quote or at the end.
        assert_eq!(quote_arg(r"C:\My Dir\x"), r#""C:\My Dir\x""#);
        assert_eq!(quote_arg(r"C:\My Dir\"), r#""C:\My Dir\\""#);
        assert_eq!(quote_arg(r#"a\"b"#), r#""a\\\"b""#);
    }

    #[test]
    fn arguments_are_joined_with_spaces() {
        let args = ["-f".to_owned(), "a b".to_owned(), String::new()];
        assert_eq!(join_args(&args), r#"-f "a b" """#);
    }

    #[test]
    fn desktop_entries_and_builtin_icons_are_unsupported() {
        let provider = WindowsProvider::new();
        let target = LaunchTarget::DesktopEntry {
            desktop_id: "x.desktop".into(),
            path: "x.desktop".into(),
            exec: vec![],
            terminal: false,
            working_dir: None,
        };
        assert!(matches!(
            provider.launch(&target),
            Err(PlatformError::Unsupported(_))
        ));
        assert!(matches!(
            provider.load_icon(&IconSource::builtin("app"), 32),
            Err(PlatformError::Unsupported(_))
        ));
    }

    #[test]
    fn administrator_launch_is_offered_but_never_for_store_apps() {
        let provider = WindowsProvider::new();
        assert!(provider.can_run_as_admin());
        // Refused before any shell call, so no UAC prompt appears in tests.
        let store_app = LaunchTarget::PackagedApp {
            app_user_model_id: "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App".into(),
        };
        assert!(matches!(
            provider.launch_as_admin(&store_app),
            Err(PlatformError::Unsupported(_))
        ));
    }

    #[test]
    fn lists_real_applications() {
        let provider = WindowsProvider::new();
        let started = Instant::now();
        let apps = provider.list_applications().unwrap();
        let elapsed = started.elapsed();

        let shortcut_count = apps
            .iter()
            .filter(|a| matches!(a.target, LaunchTarget::Shortcut { .. }))
            .count();
        let packaged_count = apps
            .iter()
            .filter(|a| matches!(a.target, LaunchTarget::PackagedApp { .. }))
            .count();
        println!(
            "{} apps ({shortcut_count} shortcuts, {packaged_count} packaged) in {elapsed:?}",
            apps.len()
        );
        let step = (apps.len() / 10).max(1);
        let sample = if std::env::var_os("SEVAK_DUMP_APPS").is_some() {
            1
        } else {
            step
        };
        for app in apps
            .iter()
            .step_by(sample)
            .take(if sample == 1 { usize::MAX } else { 10 })
        {
            println!("  {:<40} {:<45} kw={:?}", app.name, app.id, app.keywords);
        }

        assert!(!apps.is_empty());
        assert!(packaged_count > 0, "expected at least one packaged app");
        assert!(shortcut_count > 0, "expected at least one shortcut");
        for app in &apps {
            let lower = app.name.to_lowercase();
            assert!(!lower.contains("uninstall"), "junk entry: {}", app.name);
            assert!(!lower.contains("read me"), "junk entry: {}", app.name);
        }
        let ids: HashSet<_> = apps.iter().map(|a| &a.id).collect();
        assert_eq!(ids.len(), apps.len(), "ids must be unique");
        assert!(apps
            .windows(2)
            .all(|w| w[0].name.to_lowercase() <= w[1].name.to_lowercase()));
    }

    #[test]
    fn icons_load_for_a_path_and_a_packaged_app() {
        let provider = WindowsProvider::new();
        let signature: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

        let icon = provider
            .load_icon(
                &IconSource::Shell {
                    parsing_name: r"C:\Windows\System32\notepad.exe".into(),
                },
                32,
            )
            .unwrap();
        assert_eq!(icon.mime, "image/png");
        assert!(icon.bytes.starts_with(signature));

        let apps = provider.list_applications().unwrap();
        let app = apps
            .iter()
            .find(|a| matches!(a.target, LaunchTarget::PackagedApp { .. }))
            .expect("a packaged app");
        let icon = provider
            .load_icon(app.icon.as_ref().unwrap(), 48)
            .unwrap_or_else(|e| panic!("icon for {}: {e}", app.name));
        assert!(icon.bytes.starts_with(signature));

        // Optional: dump PNGs for eyeballing (`SEVAK_ICON_DUMP_DIR=... cargo test`).
        if let Some(dir) = std::env::var_os("SEVAK_ICON_DUMP_DIR") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::write(dir.join("packaged.png"), &icon.bytes).unwrap();
            for name in ["Calculator", "Notepad", "Visual Studio Code", "Settings"] {
                if let Some(app) = apps.iter().find(|a| a.name == name) {
                    let icon = provider.load_icon(app.icon.as_ref().unwrap(), 64).unwrap();
                    std::fs::write(
                        dir.join(format!("{}.png", name.replace(' ', "_"))),
                        icon.bytes,
                    )
                    .unwrap();
                }
            }
        }
    }
}
