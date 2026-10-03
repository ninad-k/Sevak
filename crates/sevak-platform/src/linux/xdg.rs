//! Where applications and icons live on a freedesktop system.

use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Default for an unset `XDG_DATA_DIRS`, per the XDG Base Directory spec.
const DEFAULT_DATA_DIRS: &str = "/usr/local/share:/usr/share";

/// Flatpak and snap export their entries outside `XDG_DATA_DIRS` unless the
/// distribution wires them in (some do not for graphical sessions started
/// before the first flatpak install), so they are always searched.
const FLATPAK_SYSTEM_SHARE: &str = "/var/lib/flatpak/exports/share";
const SNAP_SHARE: &str = "/var/lib/snapd/desktop";
const PIXMAPS: &str = "/usr/share/pixmaps";

/// The environment inputs for directory discovery, kept separate from
/// `std::env` so the path logic is testable.
#[derive(Debug, Clone, Default)]
pub(super) struct XdgEnv {
    pub home: Option<PathBuf>,
    pub data_home: Option<PathBuf>,
    pub data_dirs: Option<OsString>,
}

impl XdgEnv {
    pub(super) fn from_process() -> Self {
        let non_empty = |name: &str| env::var_os(name).filter(|value| !value.is_empty());
        Self {
            home: dirs::home_dir(),
            data_home: non_empty("XDG_DATA_HOME").map(PathBuf::from),
            data_dirs: non_empty("XDG_DATA_DIRS"),
        }
    }

    /// `$XDG_DATA_HOME`, or `~/.local/share`. Relative values are invalid per
    /// the spec and ignored.
    fn data_home(&self) -> Option<PathBuf> {
        self.data_home
            .clone()
            .filter(|path| path.is_absolute())
            .or_else(|| self.home.as_ref().map(|home| home.join(".local/share")))
    }

    /// `$XDG_DATA_DIRS` entries (absolute ones only), or the defaults.
    fn data_dirs(&self) -> Vec<PathBuf> {
        let value = self
            .data_dirs
            .clone()
            .unwrap_or_else(|| OsString::from(DEFAULT_DATA_DIRS));
        env::split_paths(&value)
            .filter(|path| path.is_absolute())
            .collect()
    }

    fn flatpak_user_share(&self) -> Option<PathBuf> {
        self.home
            .as_ref()
            .map(|home| home.join(".local/share/flatpak/exports/share"))
    }

    /// Directories holding `.desktop` files, highest precedence first:
    /// the user's data home, each `XDG_DATA_DIRS` entry, then flatpak (user,
    /// system) and snap exports if not already listed.
    pub(super) fn application_dirs(&self) -> Vec<PathBuf> {
        let mut roots: Vec<PathBuf> = Vec::new();
        roots.extend(self.data_home());
        roots.extend(self.data_dirs());
        roots.extend(self.flatpak_user_share());
        roots.push(PathBuf::from(FLATPAK_SYSTEM_SHARE));
        roots.push(PathBuf::from(SNAP_SHARE));
        dedupe(roots.into_iter().map(|root| root.join("applications")))
    }

    /// Icon base directories (`~/.icons` first, as the Icon Theme spec says)
    /// and the unthemed `/usr/share/pixmaps` fallback.
    pub(super) fn icon_dirs(&self) -> (Vec<PathBuf>, Vec<PathBuf>) {
        let mut bases: Vec<PathBuf> = Vec::new();
        bases.extend(self.home.as_ref().map(|home| home.join(".icons")));
        bases.extend(self.data_home().map(|share| share.join("icons")));
        bases.extend(
            self.data_dirs()
                .into_iter()
                .map(|share| share.join("icons")),
        );
        bases.extend(self.flatpak_user_share().map(|share| share.join("icons")));
        bases.push(Path::new(FLATPAK_SYSTEM_SHARE).join("icons"));
        bases.push(Path::new(SNAP_SHARE).join("icons"));
        (dedupe(bases.into_iter()), vec![PathBuf::from(PIXMAPS)])
    }
}

/// Removes repeated paths, keeping the first occurrence.
fn dedupe(paths: impl Iterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for path in paths {
        if !out.contains(&path) {
            out.push(path);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(home: &str, data_home: Option<&str>, data_dirs: Option<&str>) -> XdgEnv {
        XdgEnv {
            home: Some(PathBuf::from(home)),
            data_home: data_home.map(PathBuf::from),
            data_dirs: data_dirs.map(OsString::from),
        }
    }

    fn paths(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn default_application_dirs() {
        assert_eq!(
            env("/home/u", None, None).application_dirs(),
            paths(&[
                "/home/u/.local/share/applications",
                "/usr/local/share/applications",
                "/usr/share/applications",
                "/home/u/.local/share/flatpak/exports/share/applications",
                "/var/lib/flatpak/exports/share/applications",
                "/var/lib/snapd/desktop/applications",
            ])
        );
    }

    #[test]
    fn explicit_xdg_variables_and_dedupe() {
        let dirs = env(
            "/home/u",
            Some("/data/home"),
            Some("/usr/share:/var/lib/snapd/desktop:/usr/share:relative/dir"),
        )
        .application_dirs();
        assert_eq!(
            dirs,
            paths(&[
                "/data/home/applications",
                "/usr/share/applications",
                "/var/lib/snapd/desktop/applications",
                "/home/u/.local/share/flatpak/exports/share/applications",
                "/var/lib/flatpak/exports/share/applications",
            ])
        );
    }

    #[test]
    fn relative_data_home_is_ignored() {
        let dirs = env("/home/u", Some("relative"), None).application_dirs();
        assert_eq!(dirs[0], PathBuf::from("/home/u/.local/share/applications"));
    }

    #[test]
    fn icon_dirs_start_with_home_icons() {
        let (bases, pixmaps) = env("/home/u", None, None).icon_dirs();
        assert_eq!(
            bases,
            paths(&[
                "/home/u/.icons",
                "/home/u/.local/share/icons",
                "/usr/local/share/icons",
                "/usr/share/icons",
                "/home/u/.local/share/flatpak/exports/share/icons",
                "/var/lib/flatpak/exports/share/icons",
                "/var/lib/snapd/desktop/icons",
            ])
        );
        assert_eq!(pixmaps, paths(&["/usr/share/pixmaps"]));
    }

    #[test]
    fn no_home_still_yields_system_dirs() {
        let dirs = XdgEnv::default().application_dirs();
        assert_eq!(dirs[0], PathBuf::from("/usr/local/share/applications"));
        assert!(dirs.contains(&PathBuf::from("/var/lib/snapd/desktop/applications")));
    }
}
