//! Start Menu shortcut (`.lnk` / `.url`) enumeration and resolution.

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use sevak_core::bounded_read::{read_capped, MAX_DESCRIPTION_BYTES};
use sevak_core::{AppEntry, IconSource, LaunchTarget};
use windows::core::{Interface, HSTRING, PWSTR};
use windows::Win32::System::Com::{
    CoCreateInstance, CoTaskMemFree, IPersistFile, CLSCTX_INPROC_SERVER, STGM_READ,
};
use windows::Win32::UI::Shell::{
    FOLDERID_CommonPrograms, FOLDERID_Programs, IShellLinkW, SHGetKnownFolderPath, ShellLink,
    KF_FLAG_DEFAULT, SLGP_RAWPATH,
};

/// What a `.lnk` file says, with the target's environment variables expanded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LinkInfo {
    /// Empty for MSI "advertised" shortcuts and shell-namespace targets.
    pub(crate) target: String,
    pub(crate) arguments: String,
    pub(crate) description: String,
}

/// Reads `.lnk` files without resolving (searching for) their targets, so no UI
/// can appear and moved targets cost nothing.
pub(crate) struct LinkResolver {
    link: IShellLinkW,
    file: IPersistFile,
}

impl LinkResolver {
    /// COM must be initialized on the calling thread.
    pub(crate) fn new() -> windows::core::Result<Self> {
        // SAFETY: `ShellLink` is a registered in-process COM class and `None`
        // means no aggregation.
        let link: IShellLinkW =
            unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)? };
        let file: IPersistFile = link.cast()?;
        Ok(Self { link, file })
    }

    pub(crate) fn resolve(&self, path: &Path) -> windows::core::Result<LinkInfo> {
        let wide = HSTRING::from(path.as_os_str());
        let mut target = [0u16; 2048];
        let mut arguments = [0u16; 4096];
        let mut description = [0u16; 1024];

        // SAFETY: `wide` is a NUL-terminated string that outlives the call; the
        // output buffers are exclusively borrowed slices the callee fills and
        // NUL-terminates; a null `WIN32_FIND_DATAW` pointer is allowed.
        unsafe {
            self.file.Load(&wide, STGM_READ)?;
            // Advertised shortcuts yield an empty path (or an error); keep them.
            let _ = self
                .link
                .GetPath(&mut target, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32);
            let _ = self.link.GetArguments(&mut arguments);
            let _ = self.link.GetDescription(&mut description);
        }

        Ok(LinkInfo {
            target: expand_env(&wide_to_string(&target)),
            arguments: wide_to_string(&arguments),
            description: wide_to_string(&description),
        })
    }
}

fn wide_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    OsString::from_wide(&buf[..len])
        .to_string_lossy()
        .trim()
        .to_owned()
}

/// Replaces `%NAME%` with the variable's value; unknown variables and stray
/// percent signs are left as they are.
pub(crate) fn expand_env(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) if end > 0 => {
                let name = &after[..end];
                match std::env::var(name) {
                    Ok(value) => out.push_str(&value),
                    Err(_) => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The per-user and all-users Start Menu `Programs` folders, per-user first.
pub(crate) fn programs_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for (folder, env_var) in [
        (&FOLDERID_Programs, "APPDATA"),
        (&FOLDERID_CommonPrograms, "ProgramData"),
    ] {
        // SAFETY: the folder id is a valid static GUID; no token and default
        // flags are valid arguments.
        let known = unsafe { SHGetKnownFolderPath(folder, KF_FLAG_DEFAULT, None) };
        let path = match known {
            Ok(pwstr) => Some(PathBuf::from(take_pwstr(pwstr))),
            Err(err) => {
                tracing::debug!(%err, env_var, "SHGetKnownFolderPath failed; using environment");
                std::env::var_os(env_var)
                    .map(|base| PathBuf::from(base).join(r"Microsoft\Windows\Start Menu\Programs"))
            }
        };
        if let Some(path) = path {
            if !roots.contains(&path) {
                roots.push(path);
            }
        }
    }
    roots
}

/// Copies a shell-allocated string and frees it.
pub(super) fn take_pwstr(pwstr: PWSTR) -> String {
    // SAFETY: `pwstr` was returned by a shell API as a NUL-terminated string.
    let text = unsafe { pwstr.to_string() }.unwrap_or_default();
    // SAFETY: shell APIs allocate these strings with CoTaskMemAlloc and
    // transfer ownership to the caller.
    unsafe { CoTaskMemFree(Some(pwstr.0 as *const _)) };
    text
}

/// Scans every root for `.lnk` / `.url` files and returns launchable entries.
/// Earlier roots win when the same app appears in several (per-user first).
/// COM must be initialized on the calling thread.
pub(crate) fn scan(roots: &[PathBuf]) -> Vec<AppEntry> {
    let resolver = match LinkResolver::new() {
        Ok(resolver) => Some(resolver),
        Err(err) => {
            tracing::warn!(%err, "cannot create IShellLink; .lnk files will be skipped");
            None
        }
    };

    let mut entries = Vec::new();
    let mut seen_ids = HashSet::new();
    let mut seen_targets = HashSet::new();

    for root in roots {
        let mut files = Vec::new();
        collect_shortcut_files(root, &mut files);
        for path in files {
            let Some((entry, key)) = entry_for(root, &path, resolver.as_ref()) else {
                continue;
            };
            if !seen_ids.insert(entry.id.clone()) {
                continue;
            }
            if let Some(key) = key {
                if !seen_targets.insert(key) {
                    continue;
                }
            }
            entries.push(entry);
        }
    }
    entries
}

fn collect_shortcut_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let read = match fs::read_dir(dir) {
        Ok(read) => read,
        Err(err) => {
            tracing::debug!(dir = %dir.display(), %err, "cannot read Start Menu folder");
            return;
        }
    };
    for entry in read.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if file_type.is_dir() {
            collect_shortcut_files(&path, out);
        } else if is_shortcut_file(&path) {
            out.push(path);
        }
    }
}

fn is_shortcut_file(path: &Path) -> bool {
    matches!(extension_lower(path).as_deref(), Some("lnk" | "url"))
}

fn extension_lower(path: &Path) -> Option<String> {
    path.extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
}

/// Builds the entry for one shortcut plus its `(name, target, args)` dedupe key
/// (absent when the target is empty, i.e. nothing to compare).
fn entry_for(
    root: &Path,
    path: &Path,
    resolver: Option<&LinkResolver>,
) -> Option<(AppEntry, Option<String>)> {
    let name = path.file_stem()?.to_string_lossy().trim().to_owned();
    if name.is_empty() || is_junk_name(&name) {
        return None;
    }

    let info = if extension_lower(path).as_deref() == Some("lnk") {
        match resolver?.resolve(path) {
            Ok(info) => info,
            Err(err) => {
                tracing::debug!(path = %path.display(), %err, "cannot resolve shortcut");
                return None;
            }
        }
    } else {
        // `.url` internet shortcuts: keep app protocols (steam://, com.epicgames...),
        // drop plain web links ("Website", "Documentation").
        if read_capped(path, MAX_DESCRIPTION_BYTES)
            .is_ok_and(|bytes| is_web_url_shortcut(&String::from_utf8_lossy(&bytes)))
        {
            return None;
        }
        LinkInfo::default()
    };

    if is_junk_target(&info.target) {
        return None;
    }

    let description = (!info.description.is_empty()
        && !info.description.eq_ignore_ascii_case(&name))
    .then(|| info.description.clone());
    let keywords = target_keyword(&info.target, &name).into_iter().collect();
    let key = (!info.target.is_empty()).then(|| {
        format!(
            "{}\u{0}{}\u{0}{}",
            name.to_lowercase(),
            info.target.to_lowercase(),
            info.arguments.to_lowercase()
        )
    });

    let entry = AppEntry {
        id: shortcut_id(root, path)?,
        name,
        description,
        keywords,
        icon: Some(IconSource::Shell {
            parsing_name: path.to_string_lossy().into_owned(),
        }),
        target: LaunchTarget::Shortcut {
            path: path.to_path_buf(),
        },
    };
    Some((entry, key))
}

/// True when an `.url` file (INI text) points at an `http(s)` page.
pub(crate) fn is_web_url_shortcut(contents: &str) -> bool {
    contents
        .lines()
        .filter_map(|line| line.trim().split_once('='))
        .find(|(key, _)| key.trim().eq_ignore_ascii_case("url"))
        .is_some_and(|(_, value)| {
            let value = value.trim().to_ascii_lowercase();
            value.starts_with("http://") || value.starts_with("https://")
        })
}

/// `lnk:` + path relative to its Programs root, lowercased, `/`-separated.
pub(crate) fn shortcut_id(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let parts: Vec<String> = relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
        .collect();
    Some(format!("lnk:{}", parts.join("/")))
}

/// Shortcuts that are not applications by name: uninstallers and read-me files.
pub(crate) fn is_junk_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("uninstall")
        || lower.contains("readme")
        || lower.contains("read me")
        || lower.contains("release notes")
}

/// Shortcuts whose target is an uninstaller or a document rather than a program.
/// An empty target (advertised shortcut, shell item) is kept.
pub(crate) fn is_junk_target(target: &str) -> bool {
    if target.is_empty() {
        return false;
    }
    let Some(file_name) = Path::new(target).file_name() else {
        return false;
    };
    let file_name = file_name.to_string_lossy().to_lowercase();
    if file_name.starts_with("unins") && file_name.ends_with(".exe") {
        return true;
    }
    const DOCUMENT_EXTENSIONS: &[&str] = &[
        "txt", "chm", "hlp", "pdf", "htm", "html", "mht", "mhtml", "rtf", "md", "doc", "docx",
        "xls", "xlsx", "ppt", "pptx", "log", "ini", "xml", "url", "ps1", "json", "csv",
    ];
    match extension_lower(Path::new(&file_name)) {
        Some(ext) => DOCUMENT_EXTENSIONS.contains(&ext.as_str()),
        None => false,
    }
}

/// The target executable's stem (`code`, `msedge`) when it differs from the
/// display name; useful because people search by process name.
pub(crate) fn target_keyword(target: &str, name: &str) -> Option<String> {
    let stem = Path::new(target)
        .file_stem()?
        .to_string_lossy()
        .into_owned();
    if stem.is_empty() || stem.eq_ignore_ascii_case(name) {
        return None;
    }
    // Names shared by countless apps (Squirrel's `Update.exe`, ...) match noise.
    const GENERIC_STEMS: &[&str] = &[
        "update", "updater", "setup", "launch", "launcher", "start", "run",
    ];
    if GENERIC_STEMS.iter().any(|g| stem.eq_ignore_ascii_case(g)) {
        return None;
    }
    let ext = extension_lower(Path::new(target))?;
    matches!(ext.as_str(), "exe" | "msc" | "cpl" | "bat" | "cmd" | "com").then_some(stem)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows::com::ComGuard;

    #[test]
    fn id_is_relative_lowercase_and_slash_separated() {
        let root = Path::new(r"C:\Users\x\AppData\Roaming\Microsoft\Windows\Start Menu\Programs");
        let path = root.join(r"Accessories\Paint.lnk");
        assert_eq!(
            shortcut_id(root, &path).as_deref(),
            Some("lnk:accessories/paint.lnk")
        );
        let other = Path::new(r"D:\elsewhere\Paint.lnk");
        assert_eq!(shortcut_id(root, other), None);
    }

    #[test]
    fn web_url_shortcuts_are_detected() {
        assert!(is_web_url_shortcut(
            "[InternetShortcut]\r\nURL=https://nodejs.org/\r\n"
        ));
        assert!(is_web_url_shortcut(
            "[InternetShortcut]\nurl = HTTP://example.com"
        ));
        assert!(!is_web_url_shortcut(
            "[InternetShortcut]\nURL=steam://rungameid/42\n"
        ));
        assert!(!is_web_url_shortcut("[InternetShortcut]\n"));
    }

    #[test]
    fn junk_names_are_filtered() {
        assert!(is_junk_name("Uninstall Foo"));
        assert!(is_junk_name("Foo Uninstaller"));
        assert!(is_junk_name("Read Me"));
        assert!(is_junk_name("README"));
        assert!(is_junk_name("Release Notes"));
        assert!(!is_junk_name("Visual Studio Code"));
    }

    #[test]
    fn junk_targets_are_uninstallers_and_documents() {
        assert!(is_junk_target(r"C:\Program Files\Foo\unins000.exe"));
        assert!(is_junk_target(r"C:\Program Files\Foo\Uninstall.exe"));
        assert!(is_junk_target(r"C:\Program Files\Foo\readme.txt"));
        assert!(is_junk_target(r"C:\Program Files\Foo\help.CHM"));
        assert!(is_junk_target(r"C:\docs\manual.pdf"));
        assert!(is_junk_target(r"C:\docs\index.html"));
        assert!(!is_junk_target(r"C:\Windows\System32\notepad.exe"));
        assert!(!is_junk_target(r"C:\Windows\System32\compmgmt.msc"));
        assert!(!is_junk_target(r"C:\Windows\System32\main.cpl"));
        assert!(!is_junk_target(r"C:\Foo\run.bat"));
        // Advertised shortcuts and folder targets are kept.
        assert!(!is_junk_target(""));
        assert!(!is_junk_target(r"C:\Windows\System32"));
    }

    #[test]
    fn keyword_is_exe_stem_when_it_differs() {
        assert_eq!(
            target_keyword(r"C:\Apps\Microsoft VS Code\Code.exe", "Visual Studio Code").as_deref(),
            Some("Code")
        );
        assert_eq!(target_keyword(r"C:\Apps\notepad.exe", "Notepad"), None);
        assert_eq!(target_keyword("", "Anything"), None);
        assert_eq!(
            target_keyword(r"C:\Users\x\Discord\Update.exe", "Discord"),
            None
        );
        assert_eq!(target_keyword(r"C:\docs\a.txt", "Docs"), None);
    }

    #[test]
    fn env_vars_are_expanded() {
        std::env::set_var("SEVAK_TEST_VAR", r"C:\Sevak");
        assert_eq!(
            expand_env(r"%SEVAK_TEST_VAR%\bin\a.exe"),
            r"C:\Sevak\bin\a.exe"
        );
        assert_eq!(
            expand_env("%SEVAK_NO_SUCH_VAR%\\x"),
            "%SEVAK_NO_SUCH_VAR%\\x"
        );
        assert_eq!(expand_env("100%"), "100%");
        assert_eq!(expand_env("a%%b"), "a%%b");
    }

    #[test]
    fn resolves_a_lnk_created_through_the_shell() {
        let _com = ComGuard::new();
        let dir = tempfile::tempdir().unwrap();
        let lnk = dir.path().join("My Notepad.lnk");
        let notepad = r"C:\Windows\System32\notepad.exe";

        // SAFETY: creating and filling an in-process shell link; every string
        // is a NUL-terminated HSTRING alive for the duration of each call.
        unsafe {
            let link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
            link.SetPath(&HSTRING::from(notepad)).unwrap();
            link.SetArguments(&HSTRING::from("--flag \"a b\"")).unwrap();
            link.SetDescription(&HSTRING::from("Edits text")).unwrap();
            let file: IPersistFile = link.cast().unwrap();
            file.Save(&HSTRING::from(lnk.as_os_str()), true).unwrap();
        }

        let resolver = LinkResolver::new().unwrap();
        let info = resolver.resolve(&lnk).unwrap();
        assert!(info.target.eq_ignore_ascii_case(notepad), "{info:?}");
        assert_eq!(info.arguments, "--flag \"a b\"");
        assert_eq!(info.description, "Edits text");

        // The same file flows through the scanner into an AppEntry.
        let entries = scan(&[dir.path().to_path_buf()]);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "lnk:my notepad.lnk");
        assert_eq!(entries[0].description.as_deref(), Some("Edits text"));
        assert_eq!(entries[0].keywords, vec!["notepad".to_owned()]);
    }

    #[test]
    fn scan_dedupes_across_roots_preferring_the_first() {
        let _com = ComGuard::new();
        let user = tempfile::tempdir().unwrap();
        let common = tempfile::tempdir().unwrap();
        // `.url` shortcuts need no COM to create.
        for root in [user.path(), common.path()] {
            fs::write(root.join("Game.url"), "[InternetShortcut]\nURL=steam://x\n").unwrap();
        }
        fs::write(common.path().join("Other.url"), "[InternetShortcut]\n").unwrap();
        let entries = scan(&[user.path().to_path_buf(), common.path().to_path_buf()]);
        assert_eq!(entries.len(), 2);
        let game = entries.iter().find(|e| e.name == "Game").unwrap();
        assert_eq!(
            game.target,
            LaunchTarget::Shortcut {
                path: user.path().join("Game.url")
            }
        );
    }
}
