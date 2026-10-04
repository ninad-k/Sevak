//! The `.sevakext` package: what a native extension is shipped as.
//!
//! A `.sevakext` file is a zip with a flat layout:
//!
//! ```text
//! plugin.toml          the manifest, byte for byte what gets installed
//! checksums.sha256     "<sha256 hex>  <path>" for every other file
//! bin/<program>        one binary per platform the package carries
//! ...                  optional extra files (an icon, data the program reads)
//! ```
//!
//! The package is built deterministically (sorted entries, fixed timestamps and
//! permissions), so packing the same inputs twice gives the same bytes and the
//! same checksum. The gallery lists the SHA-256 of the whole file; the
//! `checksums.sha256` inside lets anyone check each file, such as the binary,
//! against the publisher's list.
//!
//! Reading is strict because packages are untrusted input: size limits on the
//! file, every entry and the unpacked total; no absolute paths, `..`, drive
//! letters, links, control characters or Windows device names; no duplicate
//! names (compared case-insensitively); every file covered by the checksums
//! file and every checksum matching; a manifest that parses for every platform
//! it declares; and only binaries the manifest declares.
//!
//! Nothing here installs anything; see [`super::store`].

use std::collections::{BTreeMap, HashSet};
use std::io::{Cursor, Read, Write};

use sevak_core::checksum::sha256_hex;
use zip::write::SimpleFileOptions;

use crate::script::{Manifest, Native, MANIFEST_FILE, PLATFORMS};
use crate::workflow::gallery::clean_path;

/// The file extension of a package.
pub const EXTENSION: &str = "sevakext";
/// The checksums file inside a package.
pub const CHECKSUMS_FILE: &str = "checksums.sha256";

/// The largest package downloaded or read.
#[cfg(not(test))]
pub const MAX_PACKAGE_BYTES: usize = 32 * 1024 * 1024;
const MAX_FILES: usize = 100;
#[cfg(not(test))]
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
#[cfg(not(test))]
const MAX_UNPACKED_BYTES: u64 = 128 * 1024 * 1024;
// The unit tests build packages at the limits, so they use small ones.
#[cfg(test)]
pub const MAX_PACKAGE_BYTES: usize = 1024 * 1024;
#[cfg(test)]
const MAX_FILE_BYTES: u64 = 256 * 1024;
#[cfg(test)]
const MAX_UNPACKED_BYTES: u64 = 600 * 1024;

/// One file of a package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackedFile {
    /// Forward-slash path relative to the extension's folder.
    pub path: String,
    pub data: Vec<u8>,
    pub executable: bool,
}

/// A package that was read and checked.
#[derive(Debug, Clone)]
pub struct ExtensionPackage {
    /// The manifest, exactly as shipped.
    pub manifest_text: String,
    /// What it declares (validated for every platform it names).
    pub native: Native,
    /// Every file except the checksums file, the manifest included.
    pub files: Vec<PackedFile>,
    /// The checksums file as shipped.
    pub checksums: String,
}

impl ExtensionPackage {
    /// Reads and checks a package. `folder` is the folder the extension will
    /// install to, which must be the id in its manifest (`script:<folder>`);
    /// an empty `folder` checks the package without that (for tools).
    pub fn read(bytes: &[u8], folder: &str) -> Result<Self, String> {
        if bytes.len() > MAX_PACKAGE_BYTES {
            return Err("the package is larger than expected".to_owned());
        }
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|err| format!("the package is not a valid zip file: {err}"))?;
        if archive.len() > MAX_FILES * 2 {
            return Err("the package has too many entries".to_owned());
        }
        let mut files: Vec<PackedFile> = Vec::new();
        let mut total: u64 = 0;
        let mut seen = HashSet::new();
        for index in 0..archive.len() {
            let mut entry = archive
                .by_index(index)
                .map_err(|err| format!("the package is damaged: {err}"))?;
            let path = clean_path(entry.name())?;
            if entry.is_dir() {
                continue;
            }
            if path.is_empty() {
                return Err("a path in the package is not allowed".to_owned());
            }
            if let Some(mode) = entry.unix_mode() {
                // 0o120000: a symbolic link, which could point anywhere.
                if mode & 0o170000 == 0o120000 {
                    return Err("the package contains a link, which is not allowed".to_owned());
                }
            }
            let executable = entry.unix_mode().is_some_and(|mode| mode & 0o111 != 0);
            if files.len() >= MAX_FILES {
                return Err("the package has too many files".to_owned());
            }
            if entry.size() > MAX_FILE_BYTES {
                return Err("a file in the package is too large".to_owned());
            }
            if !seen.insert(path.to_lowercase()) {
                return Err(format!("the package lists {path} twice"));
            }
            let mut data = Vec::new();
            (&mut entry)
                .take(MAX_FILE_BYTES + 1)
                .read_to_end(&mut data)
                .map_err(|err| format!("the package is damaged: {err}"))?;
            if data.len() as u64 > MAX_FILE_BYTES {
                return Err("a file in the package is too large".to_owned());
            }
            total += data.len() as u64;
            if total > MAX_UNPACKED_BYTES {
                return Err("the package is too large when unpacked".to_owned());
            }
            files.push(PackedFile {
                path,
                data,
                executable,
            });
        }

        let checksums_at = files
            .iter()
            .position(|file| file.path == CHECKSUMS_FILE)
            .ok_or_else(|| format!("the package has no {CHECKSUMS_FILE}"))?;
        let checksums_file = files.remove(checksums_at);
        let checksums = String::from_utf8(checksums_file.data)
            .map_err(|_| format!("{CHECKSUMS_FILE} is not text"))?;
        verify_checksums(&checksums, &files)?;

        let manifest_text = files
            .iter()
            .find(|file| file.path == MANIFEST_FILE)
            .ok_or_else(|| format!("the package has no {MANIFEST_FILE}"))
            .and_then(|file| {
                String::from_utf8(file.data.clone())
                    .map_err(|_| format!("{MANIFEST_FILE} is not text"))
            })?;
        let summary = summarize(&manifest_text, folder)?;
        if !folder.is_empty() && summary.id != folder {
            return Err(format!(
                "the package's id is script:{}, but it is installed as \"{folder}\"",
                summary.id
            ));
        }
        let native = summary.native;

        // Only the binaries the manifest declares: a program the manifest does
        // not name would travel with the package but never be reviewed.
        let declared: HashSet<&str> = native.binaries.values().map(String::as_str).collect();
        for file in &files {
            let looks_like_program = file.executable
                || file.path.starts_with("bin/")
                || file.path.to_lowercase().ends_with(".exe");
            if looks_like_program && !declared.contains(file.path.as_str()) {
                return Err(format!(
                    "{} looks like a program but plugin.toml does not declare it under \
                     [extension.binaries]",
                    file.path
                ));
            }
        }
        if !files.iter().any(|f| declared.contains(f.path.as_str())) {
            return Err(
                "the package contains none of the binaries plugin.toml declares".to_owned(),
            );
        }
        Ok(Self {
            manifest_text,
            native,
            files,
            checksums,
        })
    }

    /// The platforms whose binary this package carries.
    pub fn platforms(&self) -> Vec<String> {
        self.native
            .binaries
            .iter()
            .filter(|(_, path)| self.files.iter().any(|f| &f.path == *path))
            .map(|(platform, _)| platform.clone())
            .collect()
    }

    /// The program for `platform`, if the package carries one.
    pub fn binary(&self, platform: &str) -> Option<&PackedFile> {
        let path = self.native.binaries.get(platform)?;
        self.files.iter().find(|file| &file.path == path)
    }

    /// The files to install on `platform`: everything except the binaries of
    /// other platforms. Errors when the package has no program for it.
    pub fn files_for(&self, platform: &str) -> Result<Vec<(String, Vec<u8>, bool)>, String> {
        let program = self.binary(platform).ok_or_else(|| {
            format!(
                "this package has no build for {platform} (it carries {})",
                self.platforms().join(", ")
            )
        })?;
        let others: HashSet<&str> = self
            .native
            .binaries
            .iter()
            .filter(|(name, _)| name.as_str() != platform)
            .map(|(_, path)| path.as_str())
            .collect();
        Ok(self
            .files
            .iter()
            .filter(|file| !others.contains(file.path.as_str()))
            .map(|file| {
                (
                    file.path.clone(),
                    file.data.clone(),
                    file.executable || file.path == program.path,
                )
            })
            .collect())
    }
}

/// Validates a native extension's manifest for packaging: it must have an
/// `[extension]` table and parse for every platform that table names. The
/// returned [`Native`] is what it declares.
pub fn check_manifest(text: &str, folder: &str) -> Result<Native, String> {
    // A package must name itself: the folder a gallery entry installs to is not
    // part of the file, so the manifest says which extension this is.
    let has_id = toml::from_str::<toml::Table>(text)
        .map_err(|err| err.to_string())?
        .get("id")
        .and_then(toml::Value::as_str)
        .is_some_and(|id| !id.trim().is_empty());
    if !has_id {
        return Err(
            "plugin.toml needs an `id` (script:<name>, the name the extension installs under)"
                .to_owned(),
        );
    }
    let native = Native::parse_text(text)?.ok_or_else(|| {
        "plugin.toml has no [extension] table; a native extension declares its version, \
         author, licence and binaries there"
            .to_owned()
    })?;
    for platform in native.binaries.keys() {
        Manifest::parse_for(text, folder, platform)
            .map_err(|err| format!("plugin.toml is not valid for {platform}: {err}"))?;
    }
    Ok(native)
}

/// What a native extension's manifest says about it, for tools and listings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// The gallery id: the manifest's `id` without its `script:` prefix.
    pub id: String,
    pub name: String,
    pub description: String,
    pub keyword: String,
    pub native: Native,
}

/// The [`Summary`] of manifest `text` (checked for every platform).
pub fn summarize(text: &str, folder: &str) -> Result<Summary, String> {
    let native = check_manifest(text, folder)?;
    let platform = native
        .binaries
        .keys()
        .next()
        .ok_or_else(|| "plugin.toml declares no binaries".to_owned())?;
    let manifest = Manifest::parse_for(text, folder, platform)?;
    Ok(Summary {
        id: manifest
            .id
            .strip_prefix(crate::script::ID_PREFIX)
            .unwrap_or(&manifest.id)
            .to_owned(),
        name: manifest.name,
        description: manifest.description,
        keyword: manifest.keyword,
        native,
    })
}

/// Things that are not wrong but that users and reviewers will want fixed.
pub fn lint_manifest(text: &str, folder: &str) -> Vec<String> {
    let Ok(native) = check_manifest(text, folder) else {
        return Vec::new();
    };
    let Some(platform) = native.binaries.keys().next() else {
        return Vec::new();
    };
    let Ok(manifest) = Manifest::parse_for(text, folder, platform) else {
        return Vec::new();
    };
    let mut notes = Vec::new();
    if manifest.description.is_empty() {
        notes.push("add a `description`: it is shown next to the extension in Settings".to_owned());
    }
    if native.repository.is_none() {
        notes.push(
            "add `extension.repository` so users and gallery reviewers can read the source"
                .to_owned(),
        );
    }
    if native.min_sevak.is_none() {
        notes.push("set `extension.min_sevak` to the oldest Sevak you tested with".to_owned());
    }
    if native.permissions.is_empty() {
        notes.push(
            "`extension.permissions` is empty: that tells users the program needs none (no \
             network, no files outside its folders, no other programs); check that is true"
                .to_owned(),
        );
    }
    if native
        .binaries
        .values()
        .any(|path| !path.starts_with("bin/"))
    {
        notes.push("keep programs under bin/ (bin/<name>-<platform>)".to_owned());
    }
    notes
}

/// Parses `checksums.sha256` and checks it against `files`: one line per file,
/// `<64 hex digits>  <path>`, no file missing and none extra.
fn verify_checksums(text: &str, files: &[PackedFile]) -> Result<(), String> {
    let mut listed: BTreeMap<String, String> = BTreeMap::new();
    for (number, line) in text.lines().enumerate() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let (hash, path) = line.split_once("  ").ok_or_else(|| {
            format!(
                "{CHECKSUMS_FILE} line {} is not \"<sha256>  <path>\"",
                number + 1
            )
        })?;
        if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!(
                "{CHECKSUMS_FILE} line {} does not start with a SHA-256",
                number + 1
            ));
        }
        let path = clean_path(path)?;
        if listed
            .insert(path.clone(), hash.to_ascii_lowercase())
            .is_some()
        {
            return Err(format!("{CHECKSUMS_FILE} lists {path} twice"));
        }
    }
    for file in files {
        match listed.remove(&file.path) {
            None => {
                return Err(format!(
                    "{} is in the package but not in {CHECKSUMS_FILE}",
                    file.path
                ))
            }
            Some(expected) if sha256_hex(&file.data) != expected => {
                return Err(format!(
                    "{} does not match {CHECKSUMS_FILE}, so the package was discarded",
                    file.path
                ))
            }
            Some(_) => {}
        }
    }
    if let Some(missing) = listed.keys().next() {
        return Err(format!(
            "{CHECKSUMS_FILE} lists {missing}, which is not in the package"
        ));
    }
    Ok(())
}

/// Builds a package.
///
/// ```ignore
/// let mut builder = Builder::new(manifest_text, "rust-hello")?;
/// builder.add_binary("linux-x86_64", linux_bytes)?;
/// builder.add_binary("windows-x86_64", windows_bytes)?;
/// let everything = builder.build(None)?;                    // one package, both programs
/// let linux_only = builder.build(Some("linux-x86_64"))?;    // what the gallery lists
/// ```
pub struct Builder {
    manifest_text: String,
    native: Native,
    extras: BTreeMap<String, Vec<u8>>,
    binaries: BTreeMap<String, Vec<u8>>,
}

impl Builder {
    /// A builder for the manifest `manifest_text`, which is checked for every
    /// platform it declares.
    pub fn new(manifest_text: &str, folder: &str) -> Result<Self, String> {
        let native = check_manifest(manifest_text, folder)?;
        Ok(Self {
            manifest_text: manifest_text.to_owned(),
            native,
            extras: BTreeMap::new(),
            binaries: BTreeMap::new(),
        })
    }

    /// What the manifest declares.
    pub fn native(&self) -> &Native {
        &self.native
    }

    /// Adds the program for `platform`, which the manifest must declare.
    pub fn add_binary(&mut self, platform: &str, bytes: Vec<u8>) -> Result<(), String> {
        if !self.native.binaries.contains_key(platform) {
            return Err(format!(
                "plugin.toml declares no binary for {platform} (it declares {})",
                self.native
                    .binaries
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if bytes.is_empty() {
            return Err(format!("the binary for {platform} is empty"));
        }
        self.binaries.insert(platform.to_owned(), bytes);
        Ok(())
    }

    /// Adds a data file (an icon, a text file the program reads) at `path`
    /// inside the extension's folder.
    pub fn add_file(&mut self, path: &str, bytes: Vec<u8>) -> Result<(), String> {
        let path = clean_path(path)?;
        if path.is_empty() || path == MANIFEST_FILE || path == CHECKSUMS_FILE {
            return Err(format!("{path} cannot be added as a data file"));
        }
        if self
            .native
            .binaries
            .values()
            .any(|declared| declared == &path)
        {
            return Err(format!(
                "{path} is a declared binary; add it with its platform"
            ));
        }
        self.extras.insert(path, bytes);
        Ok(())
    }

    /// The platforms whose program was added.
    pub fn platforms(&self) -> Vec<String> {
        self.binaries.keys().cloned().collect()
    }

    /// The package: with the programs of every added platform, or only
    /// `only`'s. Reading the result back is part of building it, so what is
    /// returned is a package this Sevak accepts.
    pub fn build(&self, only: Option<&str>) -> Result<Vec<u8>, String> {
        let mut files: BTreeMap<String, (Vec<u8>, bool)> = BTreeMap::new();
        files.insert(
            MANIFEST_FILE.to_owned(),
            (self.manifest_text.clone().into_bytes(), false),
        );
        for (path, bytes) in &self.extras {
            files.insert(path.clone(), (bytes.clone(), false));
        }
        let mut any = false;
        for (platform, bytes) in &self.binaries {
            if only.is_some_and(|only| only != platform) {
                continue;
            }
            let path = &self.native.binaries[platform];
            files.insert(path.clone(), (bytes.clone(), true));
            any = true;
        }
        if !any {
            return Err(match only {
                Some(platform) => format!("no binary was added for {platform}"),
                None => "no binary was added".to_owned(),
            });
        }
        let mut checksums = String::new();
        for (path, (bytes, _)) in &files {
            checksums.push_str(&format!("{}  {path}\n", sha256_hex(bytes)));
        }

        let mut out = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut out);
            // No `time` feature: entries get the format's earliest date, 1980-01-01.
            let options = |mode: u32| {
                SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated)
                    .unix_permissions(mode)
            };
            for (path, (bytes, executable)) in &files {
                writer
                    .start_file(path, options(if *executable { 0o755 } else { 0o644 }))
                    .map_err(|err| err.to_string())?;
                writer.write_all(bytes).map_err(|err| err.to_string())?;
            }
            writer
                .start_file(CHECKSUMS_FILE, options(0o644))
                .map_err(|err| err.to_string())?;
            writer
                .write_all(checksums.as_bytes())
                .map_err(|err| err.to_string())?;
            writer.finish().map_err(|err| err.to_string())?;
        }
        let bytes = out.into_inner();
        ExtensionPackage::read(&bytes, "")?;
        Ok(bytes)
    }
}

/// The platforms a package for `native` could carry, in the order they are
/// listed (a convenience for tools that print them).
pub fn declared_platforms(native: &Native) -> Vec<&str> {
    PLATFORMS
        .iter()
        .copied()
        .filter(|platform| native.binaries.contains_key(*platform))
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const MANIFEST: &str = r#"protocol = 1
id = "script:rust-hello"
keyword = "rh"
name = "Rust hello"
description = "Says hello."

[extension]
version = "0.3.0"
author = "Ada"
license = "Apache-2.0"
min_sevak = "0.1.0"
permissions = ["network"]
repository = "https://github.com/example/rust-hello"

[extension.binaries]
windows-x86_64 = "bin/rh-windows-x86_64.exe"
macos-aarch64 = "bin/rh-macos-aarch64"
linux-x86_64 = "bin/rh-linux-x86_64"
"#;

    pub(crate) fn built() -> Builder {
        let mut builder = Builder::new(MANIFEST, "rust-hello").unwrap();
        builder
            .add_binary("windows-x86_64", b"MZ windows".to_vec())
            .unwrap();
        builder
            .add_binary("macos-aarch64", b"macho mac".to_vec())
            .unwrap();
        builder
            .add_binary("linux-x86_64", b"ELF linux".to_vec())
            .unwrap();
        builder
    }

    /// A zip with the given `(name, data)` entries, no checks of any kind.
    pub(crate) fn raw_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(&mut out);
        for (name, data) in entries {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap();
        out.into_inner()
    }

    fn checksums_for(entries: &[(&str, &[u8])]) -> String {
        entries
            .iter()
            .map(|(name, data)| format!("{}  {name}\n", sha256_hex(data)))
            .collect()
    }

    /// A package with the given files and a correct checksums file.
    fn with_checksums(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let sums = checksums_for(entries);
        let mut all: Vec<(&str, &[u8])> = entries.to_vec();
        all.push((CHECKSUMS_FILE, sums.as_bytes()));
        raw_zip(&all)
    }

    #[test]
    fn a_built_package_reads_back() {
        let builder = built();
        let bytes = builder.build(None).unwrap();
        let package = ExtensionPackage::read(&bytes, "rust-hello").unwrap();
        assert_eq!(package.native.version, "0.3.0");
        assert_eq!(package.manifest_text, MANIFEST);
        assert_eq!(
            package.platforms(),
            ["linux-x86_64", "macos-aarch64", "windows-x86_64"]
        );
        assert_eq!(
            package.binary("linux-x86_64").unwrap().data,
            b"ELF linux".to_vec()
        );
        assert!(package.binary("linux-aarch64").is_none());
        // The shipped checksums name every file with its SHA-256.
        assert!(package.checksums.contains(&format!(
            "{}  bin/rh-linux-x86_64",
            sha256_hex(b"ELF linux")
        )));
    }

    #[test]
    fn packing_is_deterministic() {
        assert_eq!(built().build(None).unwrap(), built().build(None).unwrap());
    }

    #[test]
    fn one_platform_packages_carry_one_program() {
        let bytes = built().build(Some("macos-aarch64")).unwrap();
        let package = ExtensionPackage::read(&bytes, "").unwrap();
        assert_eq!(package.platforms(), ["macos-aarch64"]);
        let err = package.files_for("linux-x86_64").unwrap_err();
        assert!(err.contains("no build for linux-x86_64"), "{err}");
        assert!(err.contains("macos-aarch64"), "{err}");
        let files = package.files_for("macos-aarch64").unwrap();
        let names: Vec<&str> = files.iter().map(|(name, _, _)| name.as_str()).collect();
        assert_eq!(names, ["bin/rh-macos-aarch64", "plugin.toml"]);
        assert!(
            files
                .iter()
                .find(|f| f.0 == "bin/rh-macos-aarch64")
                .unwrap()
                .2
        );
        assert!(!files.iter().find(|f| f.0 == "plugin.toml").unwrap().2);
    }

    #[test]
    fn installing_one_platform_leaves_the_other_programs_out() {
        let bytes = built().build(None).unwrap();
        let package = ExtensionPackage::read(&bytes, "").unwrap();
        let files = package.files_for("windows-x86_64").unwrap();
        let names: Vec<&str> = files.iter().map(|(name, _, _)| name.as_str()).collect();
        assert_eq!(names, ["bin/rh-windows-x86_64.exe", "plugin.toml"]);
    }

    #[test]
    fn building_needs_a_declared_binary_and_a_valid_manifest() {
        let mut builder = Builder::new(MANIFEST, "x").unwrap();
        assert!(builder.build(None).unwrap_err().contains("no binary"));
        let err = builder
            .add_binary("linux-aarch64", b"x".to_vec())
            .unwrap_err();
        assert!(
            err.contains("declares no binary for linux-aarch64"),
            "{err}"
        );
        assert!(builder.add_binary("linux-x86_64", Vec::new()).is_err());
        builder.add_binary("linux-x86_64", b"x".to_vec()).unwrap();
        assert!(builder
            .build(Some("windows-x86_64"))
            .unwrap_err()
            .contains("no binary was added for windows-x86_64"));

        let script =
            "protocol = 1\nid = \"script:x\"\nkeyword = \"x\"\ncommand = [\"python\", \"x.py\"]\n";
        assert!(Builder::new(script, "x")
            .err()
            .unwrap()
            .contains("[extension]"));
        let no_license = MANIFEST.replace("license = \"Apache-2.0\"\n", "");
        assert!(Builder::new(&no_license, "x")
            .err()
            .unwrap()
            .contains("license"));
    }

    #[test]
    fn data_files_cannot_shadow_the_manifest_or_a_binary() {
        let mut builder = built();
        for path in [
            "plugin.toml",
            CHECKSUMS_FILE,
            "bin/rh-linux-x86_64",
            "../x",
            "/x",
        ] {
            assert!(builder.add_file(path, b"x".to_vec()).is_err(), "{path}");
        }
        builder.add_file("icon.png", b"png".to_vec()).unwrap();
        let package = ExtensionPackage::read(&builder.build(None).unwrap(), "").unwrap();
        assert!(package.files.iter().any(|f| f.path == "icon.png"));
    }

    #[test]
    fn a_changed_file_fails_its_checksum() {
        let good: Vec<(&str, &[u8])> = vec![
            ("plugin.toml", MANIFEST.as_bytes()),
            ("bin/rh-linux-x86_64", b"ELF linux"),
        ];
        assert!(ExtensionPackage::read(&with_checksums(&good), "").is_ok());
        let sums = checksums_for(&good);
        let tampered = raw_zip(&[
            ("plugin.toml", MANIFEST.as_bytes()),
            ("bin/rh-linux-x86_64", b"ELF EVIL!"),
            (CHECKSUMS_FILE, sums.as_bytes()),
        ]);
        let err = ExtensionPackage::read(&tampered, "").unwrap_err();
        assert!(err.contains("bin/rh-linux-x86_64 does not match"), "{err}");
    }

    #[test]
    fn the_package_must_name_itself_and_match_its_folder() {
        let bytes = built().build(None).unwrap();
        assert!(ExtensionPackage::read(&bytes, "rust-hello").is_ok());
        let err = ExtensionPackage::read(&bytes, "other-name").unwrap_err();
        assert!(err.contains("script:rust-hello"), "{err}");
        let no_id = MANIFEST.replace("id = \"script:rust-hello\"\n", "");
        assert!(Builder::new(&no_id, "x").err().unwrap().contains("`id`"));
    }

    #[test]
    fn the_checksums_file_must_cover_exactly_the_files() {
        let manifest = MANIFEST.as_bytes();
        let program: &[u8] = b"ELF linux";
        let sums_of = |names: &[(&str, &[u8])]| checksums_for(names);

        // No checksums file at all.
        let err = ExtensionPackage::read(
            &raw_zip(&[("plugin.toml", manifest), ("bin/rh-linux-x86_64", program)]),
            "",
        )
        .unwrap_err();
        assert!(err.contains(CHECKSUMS_FILE), "{err}");

        // A file the list does not mention.
        let partial = sums_of(&[("plugin.toml", manifest)]);
        let err = ExtensionPackage::read(
            &raw_zip(&[
                ("plugin.toml", manifest),
                ("bin/rh-linux-x86_64", program),
                (CHECKSUMS_FILE, partial.as_bytes()),
            ]),
            "",
        )
        .unwrap_err();
        assert!(err.contains("not in checksums.sha256"), "{err}");

        // A line for a file that is not there.
        let extra = sums_of(&[
            ("plugin.toml", manifest),
            ("bin/rh-linux-x86_64", program),
            ("ghost", b"boo"),
        ]);
        let err = ExtensionPackage::read(
            &raw_zip(&[
                ("plugin.toml", manifest),
                ("bin/rh-linux-x86_64", program),
                (CHECKSUMS_FILE, extra.as_bytes()),
            ]),
            "",
        )
        .unwrap_err();
        assert!(err.contains("ghost"), "{err}");

        // Malformed lines.
        for bad in ["nothex  plugin.toml\n", "garbage line\n"] {
            let err = ExtensionPackage::read(
                &raw_zip(&[
                    ("plugin.toml", manifest),
                    ("bin/rh-linux-x86_64", program),
                    (CHECKSUMS_FILE, bad.as_bytes()),
                ]),
                "",
            )
            .unwrap_err();
            assert!(err.contains(CHECKSUMS_FILE), "{bad}: {err}");
        }
    }

    #[test]
    fn hostile_paths_are_refused() {
        for name in [
            "../evil",
            "bin/../../evil",
            "/abs/evil",
            "C:/evil",
            "bin\\..\\evil",
            "nul",
            "bin/con.exe",
            "trailing.",
            "a\u{7}b",
        ] {
            let program: &[u8] = b"x";
            let bytes = with_checksums(&[
                ("plugin.toml", MANIFEST.as_bytes()),
                ("bin/rh-linux-x86_64", program),
                (name, b"evil"),
            ]);
            let err = ExtensionPackage::read(&bytes, "").unwrap_err();
            assert!(
                err.contains("not allowed") || err.contains("leaves"),
                "{name:?}: {err}"
            );
        }
    }

    #[test]
    fn duplicate_names_are_refused_case_insensitively() {
        let bytes = raw_zip(&[
            ("plugin.toml", MANIFEST.as_bytes()),
            ("Plugin.toml", b"other"),
        ]);
        let err = ExtensionPackage::read(&bytes, "").unwrap_err();
        assert!(err.contains("twice"), "{err}");
    }

    #[test]
    fn links_are_refused() {
        let mut out = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut out);
            writer
                .add_symlink(
                    "bin/rh-linux-x86_64",
                    "/etc/passwd",
                    SimpleFileOptions::default(),
                )
                .unwrap();
            writer.finish().unwrap();
        }
        let err = ExtensionPackage::read(&out.into_inner(), "").unwrap_err();
        assert!(err.contains("link"), "{err}");
    }

    #[test]
    fn undeclared_programs_are_refused() {
        let manifest = MANIFEST.as_bytes();
        let bytes = with_checksums(&[
            ("plugin.toml", manifest),
            ("bin/rh-linux-x86_64", b"ELF linux"),
            ("bin/sneaky", b"ELF extra"),
        ]);
        let err = ExtensionPackage::read(&bytes, "").unwrap_err();
        assert!(err.contains("bin/sneaky"), "{err}");
        assert!(err.contains("does not declare"), "{err}");
        let exe = with_checksums(&[
            ("plugin.toml", manifest),
            ("bin/rh-linux-x86_64", b"ELF linux"),
            ("helper.exe", b"MZ"),
        ]);
        assert!(ExtensionPackage::read(&exe, "")
            .unwrap_err()
            .contains("helper.exe"));
    }

    #[test]
    fn a_package_with_none_of_the_declared_binaries_is_refused() {
        let bytes = with_checksums(&[("plugin.toml", MANIFEST.as_bytes())]);
        let err = ExtensionPackage::read(&bytes, "").unwrap_err();
        assert!(err.contains("none of the binaries"), "{err}");
    }

    #[test]
    fn a_package_without_a_native_manifest_is_refused() {
        let script =
            "protocol = 1\nid = \"script:x\"\nkeyword = \"x\"\ncommand = [\"python\", \"x.py\"]\n";
        let bytes = with_checksums(&[("plugin.toml", script.as_bytes())]);
        let err = ExtensionPackage::read(&bytes, "").unwrap_err();
        assert!(err.contains("[extension]"), "{err}");
        let bytes = with_checksums(&[("x.txt", b"x")]);
        assert!(ExtensionPackage::read(&bytes, "")
            .unwrap_err()
            .contains("plugin.toml"));
    }

    #[test]
    fn not_a_zip_and_oversized_input() {
        assert!(ExtensionPackage::read(b"not a zip", "")
            .unwrap_err()
            .contains("not a valid zip"));
        assert!(
            ExtensionPackage::read(&vec![0u8; MAX_PACKAGE_BYTES + 1], "")
                .unwrap_err()
                .contains("larger")
        );
    }

    #[test]
    fn an_oversized_file_is_refused_before_it_is_unpacked() {
        // Highly compressible, so the package itself is small.
        let big = vec![0u8; MAX_FILE_BYTES as usize + 1];
        let bytes = raw_zip(&[("plugin.toml", MANIFEST.as_bytes()), ("big.bin", &big)]);
        assert!(bytes.len() < MAX_PACKAGE_BYTES);
        let err = ExtensionPackage::read(&bytes, "").unwrap_err();
        assert!(err.contains("too large"), "{err}");
    }

    #[test]
    fn the_unpacked_total_is_capped() {
        let part = vec![0u8; (MAX_FILE_BYTES as usize) - 1024];
        let bytes = raw_zip(&[
            ("plugin.toml", MANIFEST.as_bytes()),
            ("a.bin", &part),
            ("b.bin", &part),
            ("c.bin", &part),
        ]);
        assert!(bytes.len() < MAX_PACKAGE_BYTES);
        let err = ExtensionPackage::read(&bytes, "").unwrap_err();
        assert!(err.contains("too large when unpacked"), "{err}");
    }

    #[test]
    fn too_many_files_are_refused() {
        let names: Vec<String> = (0..=MAX_FILES).map(|n| format!("f{n}.txt")).collect();
        let entries: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &b"x"[..])).collect();
        let err = ExtensionPackage::read(&raw_zip(&entries), "").unwrap_err();
        assert!(err.contains("too many"), "{err}");
    }

    #[test]
    fn lint_notes_what_reviewers_will_ask_for() {
        assert!(
            lint_manifest(MANIFEST, "x").is_empty(),
            "{:?}",
            lint_manifest(MANIFEST, "x")
        );
        let bare =
            "protocol = 1\nid = \"script:x\"\nkeyword = \"x\"\n[extension]\nversion = \"1.0.0\"\nauthor = \"a\"\n\
                    license = \"MIT\"\n[extension.binaries]\nlinux-x86_64 = \"tool\"\n";
        let notes = lint_manifest(bare, "x").join("\n");
        for wanted in [
            "description",
            "repository",
            "min_sevak",
            "permissions",
            "bin/",
        ] {
            assert!(notes.contains(wanted), "{wanted}: {notes}");
        }
    }

    #[test]
    fn declared_platforms_follow_the_standard_order() {
        let native = Native::parse_text(MANIFEST).unwrap().unwrap();
        assert_eq!(
            declared_platforms(&native),
            ["windows-x86_64", "macos-aarch64", "linux-x86_64"]
        );
    }
}
