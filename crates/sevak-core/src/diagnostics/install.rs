//! Where Sevak is installed, as far as the executable's path and environment
//! tell. Pure: the caller reads the path and the environment.

/// How Sevak seems to have been installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallKind {
    /// Under `%LOCALAPPDATA%` (the installer's default, no admin rights).
    WindowsPerUser,
    /// Under Program Files (the MSI, or an all-users install).
    WindowsPerMachine,
    Scoop,
    /// `/Applications`.
    MacApplications,
    /// `~/Applications`.
    MacUserApplications,
    Homebrew,
    AppImage,
    Flatpak,
    Snap,
    /// `/usr`, `/opt`: a `.deb`, `.rpm` or distribution package.
    LinuxSystem,
    /// Run from a `target/debug` or `target/release` folder.
    DevelopmentBuild,
    /// Anywhere else (a portable copy, a custom folder).
    Other,
}

impl InstallKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::WindowsPerUser => "installed for this user only",
            Self::WindowsPerMachine => "installed for all users (Program Files)",
            Self::Scoop => "Scoop",
            Self::MacApplications => "installed in /Applications",
            Self::MacUserApplications => "installed in ~/Applications",
            Self::Homebrew => "Homebrew",
            Self::AppImage => "AppImage",
            Self::Flatpak => "Flatpak",
            Self::Snap => "Snap",
            Self::LinuxSystem => "system package (.deb, .rpm or distribution package)",
            Self::DevelopmentBuild => "development build",
            Self::Other => "portable or custom location",
        }
    }
}

/// What the environment says about the install.
#[derive(Debug, Clone, Default)]
pub struct InstallEnv {
    /// `APPIMAGE` is set.
    pub appimage: bool,
    /// `FLATPAK_ID` is set.
    pub flatpak: bool,
    /// `SNAP` is set.
    pub snap: bool,
    /// `%ProgramFiles%` and `%ProgramFiles(x86)%`.
    pub program_files: Vec<String>,
    /// `%LOCALAPPDATA%`.
    pub local_app_data: Option<String>,
    /// The home folder.
    pub home: Option<String>,
}

/// A path normalised for comparison: lower case, `/` only.
fn normal(path: &str) -> String {
    path.replace('\\', "/").to_lowercase()
}

fn is_under(path: &str, root: &str) -> bool {
    let root = normal(root);
    let root = root.trim_end_matches('/');
    !root.is_empty() && path.starts_with(root) && path[root.len()..].starts_with('/')
}

/// Classifies the executable at `exe`.
pub fn classify_install(exe: &str, env: &InstallEnv) -> InstallKind {
    let path = normal(exe);
    if path.contains("/target/debug/") || path.contains("/target/release/") {
        return InstallKind::DevelopmentBuild;
    }
    if env.appimage || path.ends_with(".appimage") || path.starts_with("/tmp/.mount_") {
        return InstallKind::AppImage;
    }
    if env.flatpak || path.starts_with("/app/") {
        return InstallKind::Flatpak;
    }
    if env.snap || path.starts_with("/snap/") {
        return InstallKind::Snap;
    }
    if path.contains("/scoop/apps/") {
        return InstallKind::Scoop;
    }
    if path.contains("/caskroom/") || path.contains("/cellar/") {
        return InstallKind::Homebrew;
    }
    if env.program_files.iter().any(|root| is_under(&path, root)) {
        return InstallKind::WindowsPerMachine;
    }
    if env
        .local_app_data
        .as_deref()
        .is_some_and(|root| is_under(&path, root))
    {
        return InstallKind::WindowsPerUser;
    }
    if path.starts_with("/applications/") {
        return InstallKind::MacApplications;
    }
    if env
        .home
        .as_deref()
        .is_some_and(|home| is_under(&path, &format!("{}/Applications", normal(home))))
    {
        return InstallKind::MacUserApplications;
    }
    if path.starts_with("/usr/") || path.starts_with("/opt/") {
        return InstallKind::LinuxSystem;
    }
    InstallKind::Other
}

#[cfg(test)]
mod tests {
    use super::*;

    fn windows() -> InstallEnv {
        InstallEnv {
            program_files: vec![r"C:\Program Files".into(), r"C:\Program Files (x86)".into()],
            local_app_data: Some(r"C:\Users\me\AppData\Local".into()),
            home: Some(r"C:\Users\me".into()),
            ..InstallEnv::default()
        }
    }

    #[test]
    fn windows_installs() {
        let env = windows();
        assert_eq!(
            classify_install(r"C:\Program Files\Sevak\sevak.exe", &env),
            InstallKind::WindowsPerMachine
        );
        assert_eq!(
            classify_install(r"C:\Users\me\AppData\Local\Sevak\sevak.exe", &env),
            InstallKind::WindowsPerUser
        );
        assert_eq!(
            classify_install(r"C:\users\me\appdata\local\programs\sevak\sevak.exe", &env),
            InstallKind::WindowsPerUser
        );
        assert_eq!(
            classify_install(r"C:\Users\me\scoop\apps\sevak\current\sevak.exe", &env),
            InstallKind::Scoop
        );
        assert_eq!(
            classify_install(r"D:\tools\sevak\sevak.exe", &env),
            InstallKind::Other
        );
        // A folder that only starts with the same letters is not inside it.
        assert_eq!(
            classify_install(r"C:\Program Files Extra\sevak.exe", &env),
            InstallKind::Other
        );
    }

    #[test]
    fn development_builds_win_over_everything() {
        assert_eq!(
            classify_install(r"D:\PProjects\Sevak\target\debug\sevak.exe", &windows()),
            InstallKind::DevelopmentBuild
        );
        assert_eq!(
            classify_install(
                "/home/me/Sevak/target/release/sevak",
                &InstallEnv::default()
            ),
            InstallKind::DevelopmentBuild
        );
    }

    #[test]
    fn mac_installs() {
        let env = InstallEnv {
            home: Some("/Users/me".into()),
            ..InstallEnv::default()
        };
        assert_eq!(
            classify_install("/Applications/Sevak.app/Contents/MacOS/sevak", &env),
            InstallKind::MacApplications
        );
        assert_eq!(
            classify_install(
                "/Users/me/Applications/Sevak.app/Contents/MacOS/sevak",
                &env
            ),
            InstallKind::MacUserApplications
        );
        assert_eq!(
            classify_install(
                "/opt/homebrew/Caskroom/sevak/0.1.0/Sevak.app/Contents/MacOS/sevak",
                &env
            ),
            InstallKind::Homebrew
        );
    }

    #[test]
    fn linux_installs() {
        let none = InstallEnv::default();
        assert_eq!(
            classify_install("/usr/bin/sevak", &none),
            InstallKind::LinuxSystem
        );
        assert_eq!(
            classify_install("/opt/sevak/sevak", &none),
            InstallKind::LinuxSystem
        );
        assert_eq!(
            classify_install("/tmp/.mount_SevakX1/usr/bin/sevak", &none),
            InstallKind::AppImage
        );
        let appimage = InstallEnv {
            appimage: true,
            ..InstallEnv::default()
        };
        assert_eq!(
            classify_install("/home/me/Sevak.AppImage", &appimage),
            InstallKind::AppImage
        );
        let flatpak = InstallEnv {
            flatpak: true,
            ..InstallEnv::default()
        };
        assert_eq!(
            classify_install("/app/bin/sevak", &flatpak),
            InstallKind::Flatpak
        );
        assert_eq!(
            classify_install("/snap/sevak/1/bin/sevak", &none),
            InstallKind::Snap
        );
        assert_eq!(
            classify_install("/home/me/bin/sevak", &none),
            InstallKind::Other
        );
    }
}
