//! Native extensions: script plugins whose program is a compiled binary.
//!
//! A native extension is an ordinary script plugin folder (`plugin.toml`, the
//! same protocol, the same approval, the same scrubbed environment) whose
//! manifest has an `[extension]` table in place of `command` / `script`:
//!
//! ```toml
//! protocol = 1
//! keyword  = "rh"
//! name     = "Rust hello"
//!
//! [extension]
//! version     = "0.1.0"
//! author      = "Ada Lovelace"
//! license     = "Apache-2.0"
//! min_sevak   = "0.1.0"
//! repository  = "https://github.com/example/rust-hello"
//! permissions = ["network"]
//!
//! [extension.binaries]
//! windows-x86_64 = "bin/rust-hello-windows-x86_64.exe"
//! macos-aarch64  = "bin/rust-hello-macos-aarch64"
//! linux-x86_64   = "bin/rust-hello-linux-x86_64"
//! ```
//!
//! Sevak picks the binary for the platform it runs on. Everything else about
//! trust is the script plugin's: the program is one of the files the approval
//! is bound to (its bytes are hashed), so a changed binary asks again. The
//! `[extension]` table is part of `plugin.toml`, so the declared publisher,
//! version and permissions are covered by the approval too.
//!
//! **Declared permissions are information for the user, not a sandbox.** A
//! native program runs with the account's full permissions; nothing here
//! restricts it. The Allow dialog and the documentation say so.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use sevak_core::bounded_read::read_capped;
use sevak_core::checksum::sha256_hex;

use super::approvals::MAX_HASHED_FILE;
use super::manifest::relative_inside;

/// The platforms an extension can offer a binary for, as `<os>-<arch>`.
pub const PLATFORMS: [&str; 6] = [
    "windows-x86_64",
    "windows-aarch64",
    "macos-x86_64",
    "macos-aarch64",
    "linux-x86_64",
    "linux-aarch64",
];

/// Permissions Sevak knows how to describe. An extension may declare others
/// (shown as written); these have a plain-language meaning.
pub const KNOWN_PERMISSIONS: [(&str, &str); 5] = [
    ("network", "connects to the internet or your network"),
    (
        "filesystem",
        "reads or writes files outside its own folders",
    ),
    ("processes", "starts other programs"),
    ("clipboard", "reads or changes the clipboard itself"),
    ("system", "changes system settings"),
];

const MAX_PERMISSIONS: usize = 16;
const MAX_TEXT_CHARS: usize = 120;
const MAX_URL_CHARS: usize = 300;

/// The platform this build of Sevak runs on, as `<os>-<arch>`
/// (`windows-x86_64`, `macos-aarch64`, `linux-x86_64`, ...).
pub fn current_platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// What the `[extension]` table of a native extension's `plugin.toml` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Native {
    /// The extension's own version (`0.1.0`).
    pub version: String,
    /// Who publishes it. Shown in the Allow dialog; the author's word, not a
    /// verified identity.
    pub author: String,
    /// The licence of the program, as an SPDX expression (`Apache-2.0`).
    pub license: String,
    pub homepage: Option<String>,
    /// Where the source is (shown, never opened by Sevak by itself).
    pub repository: Option<String>,
    /// The oldest Sevak that can run it.
    pub min_sevak: Option<String>,
    /// What the author says the program does beyond answering queries. Not
    /// enforced.
    pub permissions: Vec<String>,
    /// Platform to the binary's path inside the folder.
    pub binaries: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RawNative {
    version: Option<String>,
    author: Option<String>,
    license: Option<String>,
    homepage: Option<String>,
    repository: Option<String>,
    min_sevak: Option<String>,
    #[serde(default)]
    permissions: Vec<String>,
    #[serde(default)]
    binaries: BTreeMap<String, String>,
}

impl Native {
    /// The `[extension]` table of manifest `text`, validated; `None` when the
    /// manifest has no such table (an ordinary script plugin).
    pub fn parse_text(text: &str) -> Result<Option<Self>, String> {
        #[derive(Deserialize)]
        struct Wrapper {
            extension: Option<RawNative>,
        }
        let wrapper: Wrapper = toml::from_str(text).map_err(|err| err.to_string())?;
        wrapper.extension.map(Self::from_raw).transpose()
    }

    /// Validates the raw table.
    pub(super) fn from_raw(raw: RawNative) -> Result<Self, String> {
        let version = required(raw.version, "version")?;
        semver::Version::parse(&version).map_err(|err| {
            format!("`extension.version` \"{version}\" is not a version like 1.2.3: {err}")
        })?;
        let author = required(raw.author, "author")?;
        let license = required(raw.license, "license")?;
        if !license
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || " .-+()".contains(c))
        {
            return Err(format!(
                "`extension.license` must be an SPDX expression such as Apache-2.0, not \"{license}\""
            ));
        }
        let min_sevak = match raw.min_sevak.map(|v| v.trim().to_owned()) {
            Some(value) if !value.is_empty() => {
                semver::Version::parse(&value).map_err(|err| {
                    format!("`extension.min_sevak` \"{value}\" is not a version like 1.2.3: {err}")
                })?;
                Some(value)
            }
            _ => None,
        };
        let homepage = url_field(raw.homepage, "homepage")?;
        let repository = url_field(raw.repository, "repository")?;

        let mut permissions: Vec<String> = Vec::new();
        for permission in raw.permissions {
            let permission = permission.trim().to_ascii_lowercase();
            let plain = !permission.is_empty()
                && permission.len() <= 32
                && permission.starts_with(|c: char| c.is_ascii_lowercase())
                && permission
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
            if !plain {
                return Err(format!(
                    "`extension.permissions` entries are lower case words (network, filesystem, ...), not \"{permission}\""
                ));
            }
            if !permissions.contains(&permission) {
                permissions.push(permission);
            }
        }
        if permissions.len() > MAX_PERMISSIONS {
            return Err(format!(
                "`extension.permissions` lists {} entries; at most {MAX_PERMISSIONS} are allowed",
                permissions.len()
            ));
        }

        if raw.binaries.is_empty() {
            return Err(
                "`[extension.binaries]` must name at least one binary, such as \
                 linux-x86_64 = \"bin/tool\""
                    .to_owned(),
            );
        }
        let mut binaries = BTreeMap::new();
        let mut paths: Vec<String> = Vec::new();
        for (platform, path) in raw.binaries {
            if !PLATFORMS.contains(&platform.as_str()) {
                return Err(format!(
                    "`extension.binaries` has the unknown platform \"{platform}\" (known: {})",
                    PLATFORMS.join(", ")
                ));
            }
            let path = path.trim().to_owned();
            if relative_inside(std::path::Path::new(""), &path).is_none() {
                return Err(format!(
                    "the binary for {platform} must be a path inside the extension's folder, not \"{path}\""
                ));
            }
            let lower = path.to_lowercase();
            let wants_exe = platform.starts_with("windows-");
            if wants_exe != lower.ends_with(".exe") {
                return Err(format!(
                    "the binary for {platform} (\"{path}\") {} in .exe",
                    if wants_exe {
                        "must end"
                    } else {
                        "must not end"
                    }
                ));
            }
            if paths.contains(&lower) {
                return Err(format!("two platforms use the same binary path \"{path}\""));
            }
            paths.push(lower);
            binaries.insert(platform, path);
        }

        Ok(Self {
            version,
            author,
            license,
            homepage,
            repository,
            min_sevak,
            permissions,
            binaries,
        })
    }

    /// The binary for `platform`, or why there is none.
    pub fn binary_for(&self, platform: &str) -> Result<&str, String> {
        self.binaries
            .get(platform)
            .map(String::as_str)
            .ok_or_else(|| {
                let offered: Vec<&str> = self.binaries.keys().map(String::as_str).collect();
                format!(
                    "this extension has no build for {platform} (it offers {})",
                    offered.join(", ")
                )
            })
    }

    /// Checks that a Sevak of `running` version can run this extension.
    pub fn check_sevak_version(&self, running: &str) -> Result<(), String> {
        let (Some(min), Ok(running_version)) = (
            self.min_sevak
                .as_deref()
                .and_then(|v| semver::Version::parse(v).ok()),
            semver::Version::parse(running),
        ) else {
            return Ok(());
        };
        // A pre-release of the minimum version counts as that version.
        let running_core = semver::Version::new(
            running_version.major,
            running_version.minor,
            running_version.patch,
        );
        if running_core < min {
            return Err(format!(
                "this extension needs Sevak {min} or newer, and this is Sevak {running}"
            ));
        }
        Ok(())
    }

    /// What each declared permission means, in plain words, for the Allow
    /// dialog. Permissions Sevak has no description for are listed as written.
    pub fn permission_lines(&self) -> Vec<String> {
        self.permissions
            .iter()
            .map(|permission| {
                match KNOWN_PERMISSIONS
                    .iter()
                    .find(|(name, _)| name == permission)
                {
                    Some((name, meaning)) => format!("{name}: {meaning}"),
                    None => format!("{permission}: (no description; the author's own label)"),
                }
            })
            .collect()
    }
}

/// The SHA-256 of the program `relative` inside `dir`, as 64 hex digits, or
/// `None` when it is missing, unreadable or larger than an approval hashes.
/// The Allow dialog shows it so it can be compared with the publisher's.
pub fn binary_sha256(dir: &Path, relative: &str) -> Option<String> {
    let path = relative_inside(dir, relative)?;
    read_capped(&path, MAX_HASHED_FILE)
        .ok()
        .map(|bytes| sha256_hex(&bytes))
}

fn required(value: Option<String>, name: &str) -> Result<String, String> {
    let value = value.map(|v| v.trim().to_owned()).unwrap_or_default();
    if value.is_empty() {
        return Err(format!("`extension.{name}` is required"));
    }
    if value.chars().count() > MAX_TEXT_CHARS {
        return Err(format!(
            "`extension.{name}` is longer than {MAX_TEXT_CHARS} characters"
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(format!(
            "`extension.{name}` cannot contain control characters"
        ));
    }
    Ok(value)
}

fn url_field(value: Option<String>, name: &str) -> Result<Option<String>, String> {
    let Some(value) = value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    if !value.to_ascii_lowercase().starts_with("https://")
        || value.chars().count() > MAX_URL_CHARS
        || value.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err(format!(
            "`extension.{name}` must be an https:// address without spaces"
        ));
    }
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(text: &str) -> Result<Native, String> {
        #[derive(Deserialize)]
        struct Wrapper {
            extension: RawNative,
        }
        let wrapper: Wrapper = toml::from_str(text).map_err(|err| err.to_string())?;
        Native::from_raw(wrapper.extension)
    }

    const GOOD: &str = r#"
        [extension]
        version = "0.1.0"
        author = "Ada"
        license = "Apache-2.0"
        permissions = ["network", "Network", "filesystem"]
        repository = "https://github.com/example/x"
        min_sevak = "0.1.0"
        [extension.binaries]
        windows-x86_64 = "bin/x-windows-x86_64.exe"
        linux-x86_64 = "bin/x-linux-x86_64"
    "#;

    #[test]
    fn a_complete_table_parses() {
        let native = raw(GOOD).unwrap();
        assert_eq!(native.version, "0.1.0");
        assert_eq!(native.author, "Ada");
        assert_eq!(native.permissions, ["network", "filesystem"]);
        assert_eq!(
            native.binary_for("linux-x86_64").unwrap(),
            "bin/x-linux-x86_64"
        );
        assert_eq!(
            native.permission_lines()[0],
            "network: connects to the internet or your network"
        );
    }

    #[test]
    fn a_platform_without_a_build_says_what_is_offered() {
        let err = raw(GOOD).unwrap().binary_for("macos-aarch64").unwrap_err();
        assert!(err.contains("macos-aarch64"), "{err}");
        assert!(err.contains("linux-x86_64"), "{err}");
    }

    #[test]
    fn required_fields() {
        for missing in ["version", "author", "license"] {
            let text: String = GOOD
                .lines()
                .filter(|line| !line.trim_start().starts_with(missing))
                .collect::<Vec<_>>()
                .join("\n");
            let err = raw(&text).unwrap_err();
            assert!(err.contains(missing), "{missing}: {err}");
        }
        let err =
            raw(&GOOD.replace("0.1.0\"\n        author", "latest\"\n        author")).unwrap_err();
        assert!(err.contains("version"), "{err}");
    }

    #[test]
    fn binaries_are_checked() {
        let with = |binaries: &str| {
            raw(&format!(
                "[extension]\nversion=\"1.0.0\"\nauthor=\"a\"\nlicense=\"MIT\"\n[extension.binaries]\n{binaries}"
            ))
        };
        assert!(with("").unwrap_err().contains("at least one"));
        assert!(with("beos-x86_64 = \"bin/x\"")
            .unwrap_err()
            .contains("unknown platform"));
        for escape in ["../x", "/abs/x", "bin\\\\x", "C:/x", ""] {
            assert!(
                with(&format!("linux-x86_64 = \"{escape}\"")).is_err(),
                "{escape}"
            );
        }
        assert!(with("windows-x86_64 = \"bin/x\"")
            .unwrap_err()
            .contains(".exe"));
        assert!(with("linux-x86_64 = \"bin/x.exe\"")
            .unwrap_err()
            .contains(".exe"));
        assert!(with("linux-x86_64 = \"bin/x\"\nlinux-aarch64 = \"bin/X\"")
            .unwrap_err()
            .contains("same binary"));
        assert!(with("linux-x86_64 = \"bin/x\"").is_ok());
    }

    #[test]
    fn addresses_and_permissions_are_checked() {
        let tweak = |from: &str, to: &str| raw(&GOOD.replace(from, to));
        assert!(tweak("https://github.com/example/x", "http://example.com").is_err());
        assert!(tweak("https://github.com/example/x", "https://exa mple.com").is_err());
        assert!(tweak("\"filesystem\"", "\"File System\"").is_err());
        assert!(tweak("\"filesystem\"", "\"\"").is_err());
        assert!(tweak("Apache-2.0", "Apache 2 <script>").is_err());
        assert!(tweak("\"filesystem\"", "\"gpu\"")
            .unwrap()
            .permission_lines()[1]
            .contains("author's own label"));
        assert!(tweak("Ada", "A\u{7}da").is_err());
    }

    #[test]
    fn the_minimum_sevak_version_is_enforced() {
        let native = raw(GOOD).unwrap();
        assert!(native.check_sevak_version("0.1.0").is_ok());
        assert!(native.check_sevak_version("0.2.3").is_ok());
        assert!(native.check_sevak_version("0.1.0-beta.1").is_ok());
        let newer = raw(&GOOD.replace("min_sevak = \"0.1.0\"", "min_sevak = \"0.4.0\"")).unwrap();
        let err = newer.check_sevak_version("0.3.9").unwrap_err();
        assert!(err.contains("0.4.0") && err.contains("0.3.9"), "{err}");
        assert!(newer.check_sevak_version("1.0.0").is_ok());
    }

    #[test]
    fn the_current_platform_is_one_the_manifest_can_name() {
        assert!(
            PLATFORMS.contains(&current_platform().as_str()),
            "{}",
            current_platform()
        );
    }
}
