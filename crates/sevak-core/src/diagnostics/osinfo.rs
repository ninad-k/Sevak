//! Operating-system facts for the diagnostics report: the type, and the pure
//! parsers of what each OS reports. Reading them (registry, `sw_vers`,
//! `/etc/os-release`) is `sevak_platform::os_info`'s job.

/// What the OS calls itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsInfo {
    /// `Windows 11 Pro`, `macOS`, `Ubuntu 24.04.1 LTS`.
    pub name: String,
    /// `24H2`, `15.1`, `24.04`; empty when unknown.
    pub version: String,
    /// The build: `26100.2314`, `24B83`.
    pub build: Option<String>,
    pub kernel: Option<String>,
    /// `x86_64`, `aarch64`.
    pub arch: String,
}

impl OsInfo {
    /// Only what the compiler knows: used when the OS cannot be asked.
    pub fn unknown() -> Self {
        Self {
            name: std::env::consts::OS.to_owned(),
            version: String::new(),
            build: None,
            kernel: None,
            arch: std::env::consts::ARCH.to_owned(),
        }
    }

    /// `Windows 11 Pro 24H2 (build 26100.2314), x86_64`.
    pub fn describe(&self) -> String {
        let mut text = self.name.clone();
        if !self.version.is_empty() {
            text.push(' ');
            text.push_str(&self.version);
        }
        if let Some(build) = &self.build {
            text.push_str(&format!(" (build {build})"));
        }
        text.push_str(&format!(", {}", self.arch));
        text
    }
}

/// `PRETTY_NAME` (or `NAME`) and `VERSION_ID` of an `/etc/os-release` file.
pub fn parse_os_release(text: &str) -> (Option<String>, Option<String>) {
    let value = |key: &str| -> Option<String> {
        text.lines().find_map(|line| {
            let (name, value) = line.trim().split_once('=')?;
            if name.trim() != key {
                return None;
            }
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                .unwrap_or(value);
            let value = value.replace("\\\"", "\"").replace("\\\\", "\\");
            (!value.is_empty()).then_some(value)
        })
    };
    let name = value("PRETTY_NAME").or_else(|| value("NAME"));
    (name, value("VERSION_ID"))
}

/// `(ProductName, ProductVersion, BuildVersion)` from `sw_vers` output.
pub fn parse_sw_vers(text: &str) -> Option<(String, String, Option<String>)> {
    let field = |key: &str| -> Option<String> {
        text.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            (name.trim() == key).then(|| value.trim().to_owned())
        })
    };
    let name = field("ProductName")?;
    let version = field("ProductVersion")?;
    Some((
        name,
        version,
        field("BuildVersion").filter(|b| !b.is_empty()),
    ))
}

/// The marketing name of a Windows version. The registry's `ProductName` still
/// says "Windows 10" on Windows 11 (the build number is what tells them apart).
pub fn windows_name(product_name: &str, build: u32) -> String {
    let product_name = product_name.trim();
    if build >= 22000 && product_name.starts_with("Windows 10") {
        product_name.replacen("Windows 10", "Windows 11", 1)
    } else if product_name.is_empty() {
        "Windows".to_owned()
    } else {
        product_name.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_release_is_read_with_or_without_quotes() {
        let text = "NAME=\"Ubuntu\"\nVERSION=\"24.04.1 LTS (Noble Numbat)\"\nID=ubuntu\n\
                    PRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\nVERSION_ID=\"24.04\"\n";
        assert_eq!(
            parse_os_release(text),
            (Some("Ubuntu 24.04.1 LTS".into()), Some("24.04".into()))
        );
        let text = "NAME=Arch Linux\nPRETTY_NAME='Arch Linux'\nID=arch\n";
        assert_eq!(parse_os_release(text), (Some("Arch Linux".into()), None));
        assert_eq!(parse_os_release("NAME=Fedora\n").0, Some("Fedora".into()));
        assert_eq!(parse_os_release(""), (None, None));
        assert_eq!(parse_os_release("PRETTY_NAME=\n").0, None);
    }

    #[test]
    fn sw_vers_is_read() {
        let text = "ProductName:\t\tmacOS\nProductVersion:\t\t15.1\nBuildVersion:\t\t24B83\n";
        assert_eq!(
            parse_sw_vers(text),
            Some(("macOS".into(), "15.1".into(), Some("24B83".into())))
        );
        assert_eq!(parse_sw_vers("garbage"), None);
        assert_eq!(
            parse_sw_vers("ProductName: macOS\nProductVersion: 14.0\n"),
            Some(("macOS".into(), "14.0".into(), None))
        );
    }

    #[test]
    fn windows_eleven_is_told_apart_by_its_build() {
        assert_eq!(windows_name("Windows 10 Pro", 26100), "Windows 11 Pro");
        assert_eq!(windows_name("Windows 10 Home", 19045), "Windows 10 Home");
        assert_eq!(windows_name("Windows 11 Home", 22631), "Windows 11 Home");
        assert_eq!(windows_name("", 22631), "Windows");
    }

    #[test]
    fn the_description_reads_naturally() {
        let info = OsInfo {
            name: "Windows 11 Pro".into(),
            version: "24H2".into(),
            build: Some("26100.2314".into()),
            kernel: None,
            arch: "x86_64".into(),
        };
        assert_eq!(
            info.describe(),
            "Windows 11 Pro 24H2 (build 26100.2314), x86_64"
        );
        let info = OsInfo {
            name: "linux".into(),
            version: String::new(),
            build: None,
            kernel: None,
            arch: "aarch64".into(),
        };
        assert_eq!(info.describe(), "linux, aarch64");
    }
}
