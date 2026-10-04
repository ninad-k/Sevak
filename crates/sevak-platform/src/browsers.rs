//! Where web browsers keep their profiles, for the bookmarks plugin.
//!
//! Only the *locations* are OS specific, so they are computed here (per OS, from
//! an explicit [`BrowserEnv`] so every OS's table is testable everywhere) and
//! handed out through [`crate::PlatformProvider::browser_roots`]. Reading the
//! profiles is the plugin's job.
//!
//! | Browser family | Windows | macOS | Linux |
//! |---|---|---|---|
//! | Chromium family | `%LOCALAPPDATA%\<vendor>\User Data` (Opera: `%APPDATA%`) | `~/Library/Application Support/<vendor>` | `~/.config/<vendor>`, Flatpak and Snap copies |
//! | Firefox family | `%APPDATA%\Mozilla\Firefox` | `~/Library/Application Support/Firefox` | `~/.mozilla/firefox`, Flatpak and Snap copies |
//! | Safari | - | `~/Library/Safari` | - |

use std::path::{Path, PathBuf};

/// How a browser stores its bookmarks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserFamily {
    /// A `Bookmarks` JSON file in every profile folder (Chrome, Edge, ...).
    Chromium,
    /// `places.sqlite` in every profile listed in `profiles.ini`.
    Firefox,
    /// `Bookmarks.plist` (a binary property list) in `~/Library/Safari`. macOS
    /// only; reading it needs Full Disk Access.
    Safari,
}

/// A browser's user-data folder: it holds one folder per profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserRoot {
    /// Lowercase, stable id used by `[bookmarks] browsers` (`chrome`, `edge`,
    /// `brave`, `vivaldi`, `chromium`, `opera`, `opera-gx`, `firefox`,
    /// `librewolf`, `zen`, `safari`).
    pub id: &'static str,
    /// Display name.
    pub name: &'static str,
    pub family: BrowserFamily,
    pub dir: PathBuf,
}

/// The operating systems with a browser table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetOs {
    Windows,
    MacOs,
    Linux,
}

impl TargetOs {
    pub fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Linux
        }
    }
}

/// The base directories the tables are relative to.
#[derive(Debug, Clone, Default)]
pub struct BrowserEnv {
    pub home: Option<PathBuf>,
    /// Windows: `%APPDATA%`; macOS: `~/Library/Application Support`;
    /// Linux: `$XDG_CONFIG_HOME` (`~/.config`).
    pub config: Option<PathBuf>,
    /// Windows: `%LOCALAPPDATA%`; elsewhere unused.
    pub local_data: Option<PathBuf>,
}

impl BrowserEnv {
    pub fn from_process() -> Self {
        Self {
            home: dirs::home_dir(),
            config: dirs::config_dir(),
            local_data: dirs::data_local_dir(),
        }
    }
}

/// The browser folders that exist on this machine, in a stable order.
pub fn detect_roots() -> Vec<BrowserRoot> {
    candidate_roots(TargetOs::current(), &BrowserEnv::from_process())
        .into_iter()
        .filter(|root| root.dir.is_dir())
        .collect()
}

/// Every place a supported browser may keep its data on `os`, whether or not it
/// exists.
pub fn candidate_roots(os: TargetOs, env: &BrowserEnv) -> Vec<BrowserRoot> {
    let mut roots = Vec::new();
    match os {
        TargetOs::Windows => windows_roots(env, &mut roots),
        TargetOs::MacOs => macos_roots(env, &mut roots),
        TargetOs::Linux => linux_roots(env, &mut roots),
    }
    roots
}

/// One browser installation: `(id, name, family)`.
type Browser = (&'static str, &'static str, BrowserFamily);

const CHROME: Browser = ("chrome", "Chrome", BrowserFamily::Chromium);
const EDGE: Browser = ("edge", "Edge", BrowserFamily::Chromium);
const BRAVE: Browser = ("brave", "Brave", BrowserFamily::Chromium);
const VIVALDI: Browser = ("vivaldi", "Vivaldi", BrowserFamily::Chromium);
const CHROMIUM: Browser = ("chromium", "Chromium", BrowserFamily::Chromium);
const OPERA: Browser = ("opera", "Opera", BrowserFamily::Chromium);
const OPERA_GX: Browser = ("opera-gx", "Opera GX", BrowserFamily::Chromium);
const FIREFOX: Browser = ("firefox", "Firefox", BrowserFamily::Firefox);
const LIBREWOLF: Browser = ("librewolf", "LibreWolf", BrowserFamily::Firefox);
const ZEN: Browser = ("zen", "Zen", BrowserFamily::Firefox);
const SAFARI: Browser = ("safari", "Safari", BrowserFamily::Safari);

fn push(roots: &mut Vec<BrowserRoot>, browser: Browser, base: &Option<PathBuf>, relative: &str) {
    if let Some(base) = base {
        roots.push(BrowserRoot {
            id: browser.0,
            name: browser.1,
            family: browser.2,
            dir: join_all(base, relative),
        });
    }
}

/// `relative` uses `/`; each part becomes one path component.
fn join_all(base: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .fold(base.to_path_buf(), |path, part| path.join(part))
}

fn windows_roots(env: &BrowserEnv, roots: &mut Vec<BrowserRoot>) {
    let local = &env.local_data;
    let roaming = &env.config;
    push(roots, CHROME, local, "Google/Chrome/User Data");
    push(roots, EDGE, local, "Microsoft/Edge/User Data");
    push(roots, BRAVE, local, "BraveSoftware/Brave-Browser/User Data");
    push(roots, VIVALDI, local, "Vivaldi/User Data");
    push(roots, CHROMIUM, local, "Chromium/User Data");
    push(roots, OPERA, roaming, "Opera Software/Opera Stable");
    push(roots, OPERA_GX, roaming, "Opera Software/Opera GX Stable");
    push(roots, FIREFOX, roaming, "Mozilla/Firefox");
    push(roots, LIBREWOLF, roaming, "LibreWolf");
    push(roots, ZEN, roaming, "zen");
}

fn macos_roots(env: &BrowserEnv, roots: &mut Vec<BrowserRoot>) {
    let support = &env.config;
    push(roots, CHROME, support, "Google/Chrome");
    push(roots, EDGE, support, "Microsoft Edge");
    push(roots, BRAVE, support, "BraveSoftware/Brave-Browser");
    push(roots, VIVALDI, support, "Vivaldi");
    push(roots, CHROMIUM, support, "Chromium");
    push(roots, OPERA, support, "com.operasoftware.Opera");
    push(roots, OPERA_GX, support, "com.operasoftware.OperaGX");
    push(roots, FIREFOX, support, "Firefox");
    push(roots, LIBREWOLF, support, "LibreWolf");
    push(roots, ZEN, support, "zen");
    // Not under Application Support. The folder itself can be seen without
    // Full Disk Access; the file inside it cannot be read (see the plugin).
    push(roots, SAFARI, &env.home, "Library/Safari");
}

/// Native installs first, then the Flatpak and Snap copies of the same browser.
fn linux_roots(env: &BrowserEnv, roots: &mut Vec<BrowserRoot>) {
    let config = &env.config;
    let home = &env.home;

    // (browser, folder below ~/.config, Flatpak app id, folder below ~/snap)
    let chromium_family: [(Browser, &str, &str, Option<&str>); 6] = [
        (CHROME, "google-chrome", "com.google.Chrome", None),
        (EDGE, "microsoft-edge", "com.microsoft.Edge", None),
        (
            BRAVE,
            "BraveSoftware/Brave-Browser",
            "com.brave.Browser",
            Some("brave/current/.config/BraveSoftware/Brave-Browser"),
        ),
        (VIVALDI, "vivaldi", "com.vivaldi.Vivaldi", None),
        (
            CHROMIUM,
            "chromium",
            "org.chromium.Chromium",
            Some("chromium/common/chromium"),
        ),
        (
            OPERA,
            "opera",
            "com.opera.Opera",
            Some("opera/current/.config/opera"),
        ),
    ];
    for (browser, native, flatpak, snap) in chromium_family {
        push(roots, browser, config, native);
        push(
            roots,
            browser,
            home,
            &format!(".var/app/{flatpak}/config/{native}"),
        );
        if let Some(snap) = snap {
            push(roots, browser, home, &format!("snap/{snap}"));
        }
    }

    // (browser, folder below ~, Flatpak app id, folder below ~/snap)
    let firefox_family: [(Browser, &str, &str, Option<&str>); 3] = [
        (
            FIREFOX,
            ".mozilla/firefox",
            "org.mozilla.firefox",
            Some("firefox/common/.mozilla/firefox"),
        ),
        (
            LIBREWOLF,
            ".librewolf",
            "io.gitlab.librewolf-community",
            None,
        ),
        (ZEN, ".zen", "app.zen_browser.zen", None),
    ];
    for (browser, native, flatpak, snap) in firefox_family {
        push(roots, browser, home, native);
        push(
            roots,
            browser,
            home,
            &format!(".var/app/{flatpak}/{native}"),
        );
        if let Some(snap) = snap {
            push(roots, browser, home, &format!("snap/{snap}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> BrowserEnv {
        BrowserEnv {
            home: Some(PathBuf::from("home")),
            config: Some(PathBuf::from("config")),
            local_data: Some(PathBuf::from("local")),
        }
    }

    fn dir_of(os: TargetOs, id: &str, nth: usize) -> PathBuf {
        candidate_roots(os, &env())
            .into_iter()
            .filter(|root| root.id == id)
            .nth(nth)
            .unwrap_or_else(|| panic!("no root {nth} for {id}"))
            .dir
    }

    fn path(parts: &[&str]) -> PathBuf {
        parts.iter().fold(PathBuf::new(), |a, b| a.join(b))
    }

    #[test]
    fn windows_paths() {
        assert_eq!(
            dir_of(TargetOs::Windows, "chrome", 0),
            path(&["local", "Google", "Chrome", "User Data"])
        );
        assert_eq!(
            dir_of(TargetOs::Windows, "edge", 0),
            path(&["local", "Microsoft", "Edge", "User Data"])
        );
        assert_eq!(
            dir_of(TargetOs::Windows, "opera", 0),
            path(&["config", "Opera Software", "Opera Stable"])
        );
        assert_eq!(
            dir_of(TargetOs::Windows, "firefox", 0),
            path(&["config", "Mozilla", "Firefox"])
        );
    }

    #[test]
    fn macos_paths() {
        assert_eq!(
            dir_of(TargetOs::MacOs, "chrome", 0),
            path(&["config", "Google", "Chrome"])
        );
        assert_eq!(
            dir_of(TargetOs::MacOs, "edge", 0),
            path(&["config", "Microsoft Edge"])
        );
        assert_eq!(
            dir_of(TargetOs::MacOs, "firefox", 0),
            path(&["config", "Firefox"])
        );
        assert_eq!(
            dir_of(TargetOs::MacOs, "safari", 0),
            path(&["home", "Library", "Safari"])
        );
    }

    #[test]
    fn safari_exists_on_macos_only() {
        let has_safari = |os| {
            candidate_roots(os, &env())
                .iter()
                .any(|root| root.id == "safari")
        };
        assert!(has_safari(TargetOs::MacOs));
        assert!(!has_safari(TargetOs::Windows));
        assert!(!has_safari(TargetOs::Linux));
        let safari = candidate_roots(TargetOs::MacOs, &env())
            .into_iter()
            .find(|root| root.id == "safari")
            .unwrap();
        assert_eq!(safari.family, BrowserFamily::Safari);
        assert_eq!(safari.name, "Safari");
    }

    #[test]
    fn linux_paths_include_flatpak_and_snap() {
        assert_eq!(
            dir_of(TargetOs::Linux, "chrome", 0),
            path(&["config", "google-chrome"])
        );
        assert_eq!(
            dir_of(TargetOs::Linux, "chrome", 1),
            path(&[
                "home",
                ".var",
                "app",
                "com.google.Chrome",
                "config",
                "google-chrome"
            ])
        );
        assert_eq!(
            dir_of(TargetOs::Linux, "chromium", 2),
            path(&["home", "snap", "chromium", "common", "chromium"])
        );
        assert_eq!(
            dir_of(TargetOs::Linux, "firefox", 0),
            path(&["home", ".mozilla", "firefox"])
        );
        assert_eq!(
            dir_of(TargetOs::Linux, "firefox", 2),
            path(&["home", "snap", "firefox", "common", ".mozilla", "firefox"])
        );
        assert_eq!(
            dir_of(TargetOs::Linux, "zen", 1),
            path(&["home", ".var", "app", "app.zen_browser.zen", ".zen"])
        );
    }

    #[test]
    fn every_os_covers_the_main_browsers() {
        for os in [TargetOs::Windows, TargetOs::MacOs, TargetOs::Linux] {
            let roots = candidate_roots(os, &env());
            for id in [
                "chrome", "edge", "brave", "vivaldi", "chromium", "opera", "firefox",
            ] {
                assert!(roots.iter().any(|r| r.id == id), "{os:?} lacks {id}");
            }
            for root in &roots {
                let expected = if ["firefox", "librewolf", "zen"].contains(&root.id) {
                    BrowserFamily::Firefox
                } else if root.id == "safari" {
                    BrowserFamily::Safari
                } else {
                    BrowserFamily::Chromium
                };
                assert_eq!(root.family, expected, "{}", root.id);
            }
        }
    }

    #[test]
    fn missing_base_directories_yield_nothing() {
        assert!(candidate_roots(TargetOs::Windows, &BrowserEnv::default()).is_empty());
        assert!(candidate_roots(TargetOs::Linux, &BrowserEnv::default()).is_empty());
    }

    #[test]
    fn detect_only_returns_existing_directories() {
        for root in detect_roots() {
            assert!(root.dir.is_dir(), "{}", root.dir.display());
        }
    }
}
