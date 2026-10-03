//! Enumerating `.desktop` files into [`AppEntry`] values.

use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use sevak_core::{AppEntry, IconSource, LaunchTarget};

use crate::desktop_entry::{self, DesktopEntry, Locale};
use crate::icon_theme::IconResolver;
use crate::process::find_in_path;

/// Logical icon size requested from the theme (the UI draws icons at 48px).
pub(super) const ICON_SIZE: u32 = 48;

/// Applications nested deeper than this are ignored; it only guards against
/// symlink loops, real trees are one or two levels deep.
const MAX_DEPTH: usize = 8;

/// Executables that front another program; their own name is a useless keyword.
const WRAPPERS: [&str; 9] = [
    "env", "flatpak", "snap", "sh", "bash", "python", "python3", "pkexec", "sudo",
];

/// Inputs for one indexing pass.
pub(super) struct ScanContext<'a> {
    pub desktops: &'a [String],
    pub locale: &'a Locale,
}

/// Reads every `.desktop` file under `application_dirs` (highest precedence
/// first) and returns the visible applications.
///
/// The first file with a given desktop-file ID wins, so a user entry shadows a
/// system one; a winner that is hidden (`Hidden`/`NoDisplay`, wrong desktop)
/// hides the lower-precedence entries with the same ID as well.
pub(super) fn scan_applications(
    application_dirs: &[PathBuf],
    context: &ScanContext<'_>,
    icons: &mut IconResolver,
) -> Vec<AppEntry> {
    let mut seen = HashSet::new();
    let mut apps = Vec::new();
    for dir in application_dirs {
        let mut files = Vec::new();
        collect_desktop_files(dir, &mut files, 0);
        for file in files {
            let Some(id) = desktop_entry::desktop_file_id(dir, &file) else {
                continue;
            };
            if seen.contains(&id) {
                continue;
            }
            let entry = match read_entry(&file, context.locale) {
                Ok(entry) => entry,
                Err(reason) => {
                    tracing::debug!(file = %file.display(), %reason, "skipping desktop file");
                    continue;
                }
            };
            // Claimed even if hidden below, so lower-precedence copies stay hidden.
            seen.insert(id.clone());
            if !desktop_entry::is_visible(&entry, context.desktops) {
                continue;
            }
            match build_app(id, file.clone(), entry, icons) {
                Ok(app) => apps.push(app),
                Err(reason) => {
                    tracing::debug!(file = %file.display(), %reason, "skipping desktop file");
                }
            }
        }
    }
    apps
}

fn collect_desktop_files(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = read.flatten().map(|entry| entry.path()).collect();
    entries.sort();
    for path in entries {
        // `metadata` follows symlinks: flatpak exports are symlinks to files.
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            if depth < MAX_DEPTH {
                collect_desktop_files(&path, out, depth + 1);
            }
        } else if meta.is_file() && path.extension().is_some_and(|ext| ext == "desktop") {
            out.push(path);
        }
    }
}

fn read_entry(path: &Path, locale: &Locale) -> Result<DesktopEntry, String> {
    let bytes = fs::read(path).map_err(|err| err.to_string())?;
    // Desktop files must be UTF-8, but one stray byte should not lose the app.
    let content = String::from_utf8_lossy(&bytes);
    desktop_entry::parse(&content, locale).map_err(|err| err.to_string())
}

fn build_app(
    id: String,
    path: PathBuf,
    entry: DesktopEntry,
    icons: &mut IconResolver,
) -> Result<AppEntry, String> {
    let name = entry.name.clone().ok_or("no Name")?;
    if let Some(error) = &entry.exec_error {
        return Err(error.clone());
    }
    if entry.exec.is_empty() {
        return Err("no Exec".into());
    }
    if let Some(try_exec) = &entry.try_exec {
        if !try_exec_exists(try_exec) {
            return Err(format!("TryExec {try_exec} not found"));
        }
    }

    let icon = entry
        .icon
        .as_deref()
        .and_then(|icon| icons.resolve(icon, ICON_SIZE, 1))
        .map(|path| IconSource::File { path });

    Ok(AppEntry {
        description: entry.comment.clone().or_else(|| entry.generic_name.clone()),
        keywords: keywords(&entry),
        icon,
        target: LaunchTarget::DesktopEntry {
            desktop_id: id.clone(),
            path,
            working_dir: entry.path.as_deref().map(PathBuf::from),
            terminal: entry.terminal,
            exec: entry.exec,
        },
        id,
        name,
    })
}

/// `Keywords`, `GenericName` and the executable's name, without duplicates.
fn keywords(entry: &DesktopEntry) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |word: String| {
        let word = word.trim().to_owned();
        if !word.is_empty() && !out.iter().any(|w| w.eq_ignore_ascii_case(&word)) {
            out.push(word);
        }
    };
    entry.keywords.iter().cloned().for_each(&mut add);
    entry.generic_name.iter().cloned().for_each(&mut add);
    if let Some(name) = exec_name(&entry.exec) {
        add(name);
    }
    out
}

/// The program's file name, looking through `env VAR=x program` and skipping
/// launchers (flatpak, snap, shells) whose name says nothing about the app.
fn exec_name(exec: &[String]) -> Option<String> {
    let mut args = exec.iter();
    let mut program = args.next()?.as_str();
    if basename(program) == "env" {
        // Skip `VAR=value` assignments and options; `-u`/`-C` take an argument.
        let mut skip_next = false;
        program = args
            .find(|arg| {
                if std::mem::take(&mut skip_next) {
                    return false;
                }
                if matches!(arg.as_str(), "-u" | "-C" | "--unset" | "--chdir") {
                    skip_next = true;
                    return false;
                }
                !arg.contains('=') && !arg.starts_with('-')
            })?
            .as_str();
    }
    let name = basename(program);
    (!name.is_empty() && !WRAPPERS.contains(&name)).then(|| name.to_owned())
}

fn basename(program: &str) -> &str {
    program.rsplit('/').next().unwrap_or(program)
}

/// `TryExec` names an absolute path or a program on `PATH`; if it is missing
/// the application is not really installed.
fn try_exec_exists(try_exec: &str) -> bool {
    let path = Path::new(try_exec);
    if path.is_absolute() {
        fs::metadata(path)
            .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
    } else {
        find_in_path(try_exec).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::TempDir;

    fn write(dir: &Path, relative: &str, content: &str) -> PathBuf {
        let path = dir.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        path
    }

    fn entry(name: &str, extra: &str) -> String {
        format!("[Desktop Entry]\nType=Application\nName={name}\nExec=/usr/bin/{name} %U\n{extra}")
    }

    fn scan(dirs: &[&Path], desktops: &[&str]) -> Vec<AppEntry> {
        let desktops: Vec<String> = desktops.iter().map(|d| (*d).to_owned()).collect();
        let locale = Locale::default();
        let context = ScanContext {
            desktops: &desktops,
            locale: &locale,
        };
        let dirs: Vec<PathBuf> = dirs.iter().map(|d| d.to_path_buf()).collect();
        let mut icons = IconResolver::new(None, vec![], vec![]);
        scan_applications(&dirs, &context, &mut icons)
    }

    fn names(apps: &[AppEntry]) -> Vec<&str> {
        let mut names: Vec<&str> = apps.iter().map(|a| a.name.as_str()).collect();
        names.sort_unstable();
        names
    }

    #[test]
    fn builds_a_complete_entry() {
        let tmp = TempDir::new().unwrap();
        let file = write(
            tmp.path(),
            "firefox.desktop",
            "[Desktop Entry]\nType=Application\nName=Firefox\nGenericName=Web Browser\n\
             Comment=Browse the Web\nKeywords=Internet;WWW;\nExec=/usr/lib/firefox/firefox %u\n\
             Path=/tmp\nTerminal=false\n",
        );
        let apps = scan(&[tmp.path()], &["GNOME"]);
        assert_eq!(apps.len(), 1);
        let app = &apps[0];
        assert_eq!(app.id, "firefox.desktop");
        assert_eq!(app.name, "Firefox");
        assert_eq!(app.description.as_deref(), Some("Browse the Web"));
        assert_eq!(app.keywords, ["Internet", "WWW", "Web Browser", "firefox"]);
        assert_eq!(
            app.target,
            LaunchTarget::DesktopEntry {
                desktop_id: "firefox.desktop".into(),
                path: file,
                exec: vec!["/usr/lib/firefox/firefox".into()],
                terminal: false,
                working_dir: Some(PathBuf::from("/tmp")),
            }
        );
    }

    #[test]
    fn description_falls_back_to_generic_name() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "a.desktop", &entry("a", "GenericName=Editor\n"));
        let apps = scan(&[tmp.path()], &[]);
        assert_eq!(apps[0].description.as_deref(), Some("Editor"));
    }

    #[test]
    fn user_entry_overrides_system_entry() {
        let user = TempDir::new().unwrap();
        let system = TempDir::new().unwrap();
        write(user.path(), "app.desktop", &entry("User Version", ""));
        write(system.path(), "app.desktop", &entry("System Version", ""));
        write(system.path(), "other.desktop", &entry("Other", ""));
        let apps = scan(&[user.path(), system.path()], &[]);
        assert_eq!(names(&apps), ["Other", "User Version"]);
    }

    #[test]
    fn hidden_user_entry_hides_the_system_entry() {
        let user = TempDir::new().unwrap();
        let system = TempDir::new().unwrap();
        write(user.path(), "app.desktop", &entry("App", "Hidden=true\n"));
        write(system.path(), "app.desktop", &entry("App", ""));
        write(user.path(), "nd.desktop", &entry("Nd", "NoDisplay=true\n"));
        write(system.path(), "nd.desktop", &entry("Nd", ""));
        assert!(scan(&[user.path(), system.path()], &[]).is_empty());
    }

    #[test]
    fn subdirectories_are_recursed_and_ids_are_dashed() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "kde4/foo.desktop", &entry("Foo", ""));
        let apps = scan(&[tmp.path()], &[]);
        assert_eq!(apps[0].id, "kde4-foo.desktop");
    }

    #[test]
    fn desktop_filtering() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "kde.desktop",
            &entry("KdeOnly", "OnlyShowIn=KDE;\n"),
        );
        write(tmp.path(), "all.desktop", &entry("All", ""));
        write(tmp.path(), "hidden.desktop", &entry("Hid", "Hidden=true\n"));
        write(tmp.path(), "nd.desktop", &entry("Nd", "NoDisplay=true\n"));
        write(
            tmp.path(),
            "link.desktop",
            "[Desktop Entry]\nType=Link\nName=Link\nURL=https://example.com\n",
        );
        assert_eq!(names(&scan(&[tmp.path()], &["ubuntu", "GNOME"])), ["All"]);
        assert_eq!(names(&scan(&[tmp.path()], &["KDE"])), ["All", "KdeOnly"]);
    }

    #[test]
    fn skips_missing_exec_name_or_malformed_files() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "noexec.desktop",
            "[Desktop Entry]\nType=Application\nName=NoExec\n",
        );
        write(
            tmp.path(),
            "noname.desktop",
            "[Desktop Entry]\nType=Application\nExec=x\n",
        );
        write(tmp.path(), "garbage.desktop", "not a desktop file at all");
        write(
            tmp.path(),
            "badexec.desktop",
            "[Desktop Entry]\nType=Application\nName=Bad\nExec=app \"oops\n",
        );
        write(tmp.path(), "notes.txt", "ignore me");
        write(tmp.path(), "good.desktop", &entry("Good", ""));
        assert_eq!(names(&scan(&[tmp.path()], &[])), ["Good"]);
    }

    #[test]
    fn try_exec_must_exist() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "gone.desktop",
            &entry("Gone", "TryExec=/definitely/not/installed\n"),
        );
        write(
            tmp.path(),
            "gone2.desktop",
            &entry("Gone2", "TryExec=sevak-definitely-not-a-program\n"),
        );
        write(
            tmp.path(),
            "present.desktop",
            &entry("Present", "TryExec=sh\n"),
        );
        write(
            tmp.path(),
            "absolute.desktop",
            &entry("Absolute", "TryExec=/bin/sh\n"),
        );
        let apps = scan(&[tmp.path()], &[]);
        let found = names(&apps);
        assert!(found.contains(&"Present"), "{found:?}");
        assert!(!found.contains(&"Gone") && !found.contains(&"Gone2"));
        assert!(found.contains(&"Absolute"), "{found:?}");
    }

    #[test]
    fn terminal_flag_and_icons() {
        let tmp = TempDir::new().unwrap();
        let icon = write(tmp.path(), "icons/app.png", "x");
        write(
            tmp.path(),
            "apps/htop.desktop",
            &entry("htop", &format!("Terminal=true\nIcon={}\n", icon.display())),
        );
        let apps = scan(&[&tmp.path().join("apps")], &[]);
        let LaunchTarget::DesktopEntry { terminal, .. } = &apps[0].target else {
            panic!("expected a desktop entry target");
        };
        assert!(*terminal);
        assert_eq!(apps[0].icon, Some(IconSource::File { path: icon }));
    }

    #[test]
    fn keyword_construction() {
        let strings =
            |list: &[&str]| -> Vec<String> { list.iter().map(|s| (*s).to_owned()).collect() };
        assert_eq!(
            exec_name(&strings(&["/usr/bin/gnome-calculator"])).as_deref(),
            Some("gnome-calculator")
        );
        assert_eq!(
            exec_name(&strings(&["/usr/bin/flatpak", "run", "org.x.Y"])),
            None
        );
        assert_eq!(
            exec_name(&strings(&["env", "FOO=1", "-u", "BAR", "/opt/app/run"])).as_deref(),
            Some("run")
        );
        assert_eq!(exec_name(&strings(&["sh", "-c", "x"])), None);

        let entry = DesktopEntry {
            keywords: strings(&["Browser", "web"]),
            generic_name: Some("Web".into()),
            exec: strings(&["/usr/bin/BROWSER"]),
            ..DesktopEntry::default()
        };
        // Case-insensitive de-duplication.
        assert_eq!(keywords(&entry), ["Browser", "web"]);
    }
}
