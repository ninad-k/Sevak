//! `sevak-ext validate`: check a project folder or a package.

use std::fs;
use std::io::Write;
use std::path::Path;

use sevak_plugins::extensions::{
    lint_manifest, summarize, ExtensionPackage, Summary, CHECKSUMS_FILE, EXTENSION,
};
use sevak_plugins::net::sha256_hex;
use sevak_plugins::script::MANIFEST_FILE;

use crate::{Args, Failure};

/// The largest package read (the app's own limit for a download).
const MAX_PACKAGE_BYTES: u64 = sevak_plugins::extensions::package::MAX_PACKAGE_BYTES as u64;

/// What validating found: the facts to print, and notes to fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub summary: Summary,
    /// Platforms with a program present (in a package) or built (in a project).
    pub present: Vec<String>,
    /// Declared platforms without a program.
    pub missing: Vec<String>,
    pub notes: Vec<String>,
    /// `(path, sha256)` of the programs, for a package.
    pub program_hashes: Vec<(String, String)>,
    /// The package file's SHA-256, for a package.
    pub package_sha256: Option<String>,
}

/// Validates `path`: a folder with a `plugin.toml`, or a `.sevakext` file.
pub fn validate(path: &Path) -> Result<Report, String> {
    if path.is_dir() {
        validate_project(path)
    } else {
        validate_package(path)
    }
}

fn validate_project(dir: &Path) -> Result<Report, String> {
    let manifest_path = dir.join(MANIFEST_FILE);
    let text = fs::read_to_string(&manifest_path)
        .map_err(|err| format!("cannot read {}: {err}", manifest_path.display()))?;
    let summary = summarize(&text, "")?;
    let (mut present, mut missing) = (Vec::new(), Vec::new());
    for (platform, program) in &summary.native.binaries {
        if dir.join(program).is_file() {
            present.push(platform.clone());
        } else {
            missing.push(platform.clone());
        }
    }
    Ok(Report {
        notes: lint_manifest(&text, ""),
        summary,
        present,
        missing,
        program_hashes: Vec::new(),
        package_sha256: None,
    })
}

fn validate_package(path: &Path) -> Result<Report, String> {
    let meta =
        fs::metadata(path).map_err(|err| format!("cannot read {}: {err}", path.display()))?;
    if meta.len() > MAX_PACKAGE_BYTES {
        return Err(format!("{} is larger than Sevak downloads", path.display()));
    }
    let bytes = fs::read(path).map_err(|err| format!("cannot read {}: {err}", path.display()))?;
    let package = ExtensionPackage::read(&bytes, "")?;
    let summary = summarize(&package.manifest_text, "")?;
    let present = package.platforms();
    let missing = summary
        .native
        .binaries
        .keys()
        .filter(|platform| !present.contains(platform))
        .cloned()
        .collect();
    let program_hashes = present
        .iter()
        .filter_map(|platform| package.binary(platform))
        .map(|file| (file.path.clone(), sha256_hex(&file.data)))
        .collect();
    Ok(Report {
        notes: lint_manifest(&package.manifest_text, ""),
        summary,
        present,
        missing,
        program_hashes,
        package_sha256: Some(sha256_hex(&bytes)),
    })
}

pub(crate) fn command(args: &[String], out: &mut dyn Write) -> Result<(), Failure> {
    let args = Args::parse(args, &[], &[])?;
    let [path] = args.positional.as_slice() else {
        return Err(Failure::Usage(
            "validate needs a project folder or a .sevakext file".to_owned(),
        ));
    };
    let report = validate(Path::new(path))?;
    let is_package = report.package_sha256.is_some();
    let native = &report.summary.native;
    let w = |result: std::io::Result<()>| result.map_err(crate::io);
    w(writeln!(
        out,
        "{} {} by {} ({}) is valid.",
        report.summary.name, native.version, native.author, native.license
    ))?;
    w(writeln!(
        out,
        "  id script:{}  keyword {}",
        report.summary.id, report.summary.keyword
    ))?;
    w(writeln!(
        out,
        "  declared permissions: {}",
        if native.permissions.is_empty() {
            "none".to_owned()
        } else {
            native.permissions.join(", ")
        }
    ))?;
    let label = if is_package {
        "in the package"
    } else {
        "built"
    };
    w(writeln!(
        out,
        "  platforms {label}: {}",
        if report.present.is_empty() {
            "none".to_owned()
        } else {
            report.present.join(", ")
        }
    ))?;
    if !report.missing.is_empty() {
        w(writeln!(
            out,
            "  declared but not {}: {}",
            if is_package {
                "in the package"
            } else {
                "built"
            },
            report.missing.join(", ")
        ))?;
    }
    for (program, hash) in &report.program_hashes {
        w(writeln!(out, "  {hash}  {program}"))?;
    }
    if let Some(hash) = &report.package_sha256 {
        w(writeln!(
            out,
            "  package SHA-256 {hash} ({CHECKSUMS_FILE} and every path checked)"
        ))?;
    } else {
        w(writeln!(
            out,
            "  (a project folder: `pack` builds the .{EXTENSION} package and checks the programs too)"
        ))?;
    }
    for note in &report.notes {
        w(writeln!(out, "  note: {note}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use sevak_plugins::extensions::Builder;

    use super::*;

    const MANIFEST: &str = "protocol = 1\nid = \"script:tool\"\nkeyword = \"t\"\nname = \"Tool\"\ndescription = \"A tool.\"\n\
        [extension]\nversion = \"1.2.3\"\nauthor = \"Ada\"\nlicense = \"MIT\"\n\
        repository = \"https://github.com/example/tool\"\nmin_sevak = \"0.1.0\"\npermissions = [\"network\"]\n\
        [extension.binaries]\nwindows-x86_64 = \"bin/tool.exe\"\nlinux-x86_64 = \"bin/tool\"\n";

    #[test]
    fn a_project_is_checked_and_its_built_programs_listed() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("tool");
        fs::create_dir_all(dir.join("bin")).unwrap();
        fs::write(dir.join("plugin.toml"), MANIFEST).unwrap();
        fs::write(dir.join("bin/tool"), b"elf").unwrap();
        let report = validate(&dir).unwrap();
        assert_eq!(report.summary.id, "tool");
        assert_eq!(report.present, ["linux-x86_64"]);
        assert_eq!(report.missing, ["windows-x86_64"]);
        assert!(report.notes.is_empty(), "{:?}", report.notes);
        assert!(report.package_sha256.is_none());

        let mut out = Vec::new();
        command(&[dir.to_str().unwrap().to_owned()], &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("Tool 1.2.3 by Ada (MIT) is valid."), "{text}");
        assert!(text.contains("declared permissions: network"), "{text}");
        assert!(
            text.contains("declared but not built: windows-x86_64"),
            "{text}"
        );
    }

    #[test]
    fn a_broken_manifest_is_an_error_with_the_reason() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("plugin.toml"),
            MANIFEST.replace("version = \"1.2.3\"", "version = \"one\""),
        )
        .unwrap();
        let err = validate(tmp.path()).unwrap_err();
        assert!(err.contains("version"), "{err}");
        assert!(validate(&tmp.path().join("nothing-here")).is_err());
    }

    #[test]
    fn a_package_is_checked_end_to_end() {
        let tmp = tempfile::tempdir().unwrap();
        let mut builder = Builder::new(MANIFEST, "tool").unwrap();
        builder.add_binary("linux-x86_64", b"elf".to_vec()).unwrap();
        let bytes = builder.build(None).unwrap();
        let file = tmp.path().join("tool-1.2.3.sevakext");
        fs::write(&file, &bytes).unwrap();

        let report = validate(&file).unwrap();
        assert_eq!(report.present, ["linux-x86_64"]);
        assert_eq!(report.missing, ["windows-x86_64"]);
        assert_eq!(
            report.package_sha256.as_deref(),
            Some(sha256_hex(&bytes).as_str())
        );
        assert_eq!(report.program_hashes[0].1, sha256_hex(b"elf"));

        // A damaged package (cut short) is an error, not a pass.
        fs::write(&file, &bytes[..bytes.len() / 2]).unwrap();
        assert!(validate(&file).is_err());
    }
}
