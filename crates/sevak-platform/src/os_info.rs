//! Facts about this computer for the diagnostics report: the OS name and
//! version, and who the user is (so the report can leave that out).
//!
//! Windows reads the version from the registry (`ProductName`, `DisplayVersion`
//! and the build number), macOS asks `sw_vers`, Linux reads `/etc/os-release`.
//! Nothing is sent anywhere; nothing is written.

use sevak_core::diagnostics::{osinfo, Identity, OsInfo};

/// Asks the OS what it is. Never fails: what cannot be learned is left out.
pub fn detect() -> OsInfo {
    let mut info = OsInfo::unknown();
    info.kernel = sysinfo::System::kernel_version().filter(|kernel| !kernel.is_empty());
    platform_details(&mut info);
    info
}

#[cfg(windows)]
fn platform_details(info: &mut OsInfo) {
    let Some(raw) = crate::windows::os_version::read() else {
        info.name = "Windows".to_owned();
        return;
    };
    let build_number: u32 = raw.build.parse().unwrap_or(0);
    info.name = osinfo::windows_name(&raw.product_name, build_number);
    info.version = raw.display_version;
    info.build = Some(match raw.revision {
        Some(revision) => format!("{}.{revision}", raw.build),
        None => raw.build,
    });
}

#[cfg(target_os = "macos")]
fn platform_details(info: &mut OsInfo) {
    let output = std::process::Command::new("sw_vers").output();
    let parsed = output
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| osinfo::parse_sw_vers(&String::from_utf8_lossy(&output.stdout)));
    if let Some((name, version, build)) = parsed {
        info.name = name;
        info.version = version;
        info.build = build;
    } else {
        info.name = "macOS".to_owned();
    }
}

#[cfg(target_os = "linux")]
fn platform_details(info: &mut OsInfo) {
    let text = std::fs::read_to_string("/etc/os-release")
        .or_else(|_| std::fs::read_to_string("/usr/lib/os-release"))
        .unwrap_or_default();
    let (name, version) = osinfo::parse_os_release(&text);
    info.name = name.unwrap_or_else(|| "Linux".to_owned());
    info.version = version.unwrap_or_default();
}

/// What identifies the person using this computer: the home folder, the user
/// name and the computer name, as the OS and the environment spell them.
pub fn identity() -> Identity {
    let mut identity = Identity::default();
    let add = |list: &mut Vec<String>, value: Option<String>| {
        if let Some(value) = value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty()) {
            if !list.contains(&value) {
                list.push(value);
            }
        }
    };
    let env = |name: &str| std::env::var(name).ok();

    let home = dirs::home_dir().map(|home| home.display().to_string());
    // The folder's own name is the user name on every OS.
    let folder_name = dirs::home_dir()
        .and_then(|home| home.file_name().map(|n| n.to_string_lossy().into_owned()));
    add(&mut identity.home_dirs, home);
    add(&mut identity.home_dirs, env("USERPROFILE"));
    add(&mut identity.home_dirs, env("HOME"));

    for name in ["USERNAME", "USER", "LOGNAME"] {
        add(&mut identity.user_names, env(name));
    }
    add(&mut identity.user_names, folder_name);

    add(&mut identity.host_names, sysinfo::System::host_name());
    for name in ["COMPUTERNAME", "HOSTNAME"] {
        add(&mut identity.host_names, env(name));
    }
    identity
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_os_is_described() {
        let info = detect();
        assert!(!info.name.is_empty());
        assert!(!info.arch.is_empty());
        assert!(info.describe().contains(&info.arch));
    }

    #[test]
    fn the_identity_knows_the_home_folder() {
        let identity = identity();
        if let Some(home) = dirs::home_dir() {
            assert!(identity.home_dirs.contains(&home.display().to_string()));
        }
        // No duplicates.
        let mut homes = identity.home_dirs.clone();
        homes.sort();
        homes.dedup();
        assert_eq!(homes.len(), identity.home_dirs.len());
    }
}
