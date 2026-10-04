//! The opt-in gallery: a curated list of workflows and script plugins that can
//! be installed with one click.
//!
//! Nothing here runs by itself. The settings window asks for the index when the
//! user presses "Load gallery" and for a package when they press "Install";
//! those two requests are the only network use (see the README's "Privacy and
//! updates"). Each package is checked against the SHA-256 the index lists
//! before anything is written, unpacked with strict path rules, and installed
//! into the workflows or plugins folder as a *new, unapproved* folder: the
//! normal Allow dialog still decides whether it may run.
//!
//! The index is a JSON file, `gallery/index.json` in the Sevak repository,
//! read from the tag of the running build (see [`sevak_core::gallery_source`]
//! and `docs/security/gallery-trust.md`). Entries name their package by a path
//! relative to the repository root at that tag:
//!
//! ```json
//! {"format": 2, "name": "Sevak gallery", "entries": [
//!   {"id": "search-docs", "kind": "workflow", "name": "Search docs",
//!    "description": "...", "author": "...", "version": "1.0",
//!    "source": "gallery/packages/search-docs.zip",
//!    "sha256": "<64 hex digits>"}
//! ]}
//! ```
//!
//! The download and the checksum are the generic [`crate::net::fetch_https`]
//! and [`sevak_core::checksum`], shared with the theme gallery; only [`Kind`]
//! is specific to workflows and script plugins, and entries of kinds this Sevak
//! does not know are skipped rather than failing the index.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::model::{valid_folder_name, Workflow, FILE as WORKFLOW_FILE};
use super::validate::error_summary;
use crate::net::{fetch_https, fetch_pinned, verify_sha256, Https, Transport};
use crate::script::{Manifest, MANIFEST_FILE as PLUGIN_FILE};
use sevak_core::gallery_source::Pin;
use sevak_core::safe_names::is_reserved_device_name;

/// The index file inside the repository's `gallery` folder. The settings window
/// fetches it only on request.
pub const INDEX_FILE: &str = "index.json";
/// The index format this Sevak reads. Format 2 names packages by a path
/// relative to the release (format 1 used absolute `main` addresses).
pub const INDEX_FORMAT: u32 = 2;

pub const MAX_INDEX_BYTES: usize = 512 * 1024;
pub const MAX_PACKAGE_BYTES: usize = 5 * 1024 * 1024;
const MAX_FILES: usize = 200;
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_UNPACKED_BYTES: u64 = 10 * 1024 * 1024;
const MAX_PATH_BYTES: usize = 200;

/// What an entry installs as.
///
/// Older Sevaks do not know [`Kind::Native`]: an index entry of an unknown kind
/// is skipped, so adding native extensions to the index does not change the
/// index format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A folder for `<config dir>/workflows`.
    Workflow,
    /// A script plugin folder for `<config dir>/plugins`.
    Plugin,
    /// A native extension (a compiled program, one `.sevakext` package per
    /// platform) for `<config dir>/plugins`; see [`crate::extensions`].
    Native,
}

impl Kind {
    /// The file that makes a folder one of these.
    pub fn manifest_file(self) -> &'static str {
        match self {
            Self::Workflow => WORKFLOW_FILE,
            Self::Plugin | Self::Native => PLUGIN_FILE,
        }
    }
}

/// One platform's package of a native extension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    /// The `.sevakext` package: a path relative to the repository root at the
    /// release in the index file; after parsing, the full `https://` address.
    pub source: String,
    /// SHA-256 of the package, as 64 hex digits.
    pub sha256: String,
}

/// One installable thing in the index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Unique, lower case letters, digits and dashes.
    pub id: String,
    pub kind: Kind,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub version: String,
    /// Short lower-case labels for browsing (`search`, `needs-python`, ...).
    /// Only informational; invalid ones are dropped when the index is parsed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// The zip package. In the index file a path relative to the repository
    /// root at the release (`gallery/packages/x.zip`); after parsing, the full
    /// `https://` address at that release. Empty for native extensions, which
    /// have one package per platform in [`Entry::platforms`].
    #[serde(default)]
    pub source: String,
    /// SHA-256 of the zip, as 64 hex digits (empty for native extensions).
    #[serde(default)]
    pub sha256: String,
    /// Where to read more (shown as text, never opened automatically).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// The folder the package installs to; the `id` when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    /// Native extensions: platform (`windows-x86_64`, `macos-aarch64`,
    /// `linux-x86_64`, ...) to that platform's package.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub platforms: BTreeMap<String, Artifact>,
    /// Native extensions: the licence (SPDX), shown before installing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// Native extensions: where the source code is (shown, never opened).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    /// The oldest Sevak that can run it; the extensions page does not offer it
    /// to older ones.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_sevak: Option<String>,
    /// Native extensions: what the author says the program does beyond
    /// answering queries (`network`, `filesystem`, ...). Not enforced.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permissions: Vec<String>,
}

impl Entry {
    /// The folder name this entry installs to.
    pub fn folder_name(&self) -> &str {
        self.folder.as_deref().unwrap_or(&self.id)
    }
}

/// A parsed index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Index {
    pub name: String,
    /// Where the index was read from (shown, so the request is no secret).
    pub source: String,
    /// The release the index and its packages belong to.
    pub tag: String,
    /// Set when that is not this build's own release, and why.
    pub note: Option<String>,
    pub entries: Vec<Entry>,
    /// Entries that were left out (unknown kind, invalid), with the reason.
    pub skipped: Vec<String>,
}

#[derive(Deserialize)]
struct RawIndex {
    format: u32,
    #[serde(default)]
    name: String,
    #[serde(default)]
    entries: Vec<serde_json::Value>,
}

/// Parses the index file. A bad entry is skipped, not fatal: one typo should
/// not take the whole gallery away.
///
/// Each entry's `source` is resolved against `pin` (the release the index was
/// read from) and must lie inside it: an entry that names another host, branch
/// or release is skipped.
pub fn parse_index(text: &str, pin: &Pin) -> Result<Index, String> {
    let raw: RawIndex = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|err| format!("the gallery index is not valid: {err}"))?;
    if raw.format != INDEX_FORMAT {
        return Err(format!(
            "the gallery uses index format {}, but this Sevak reads format {INDEX_FORMAT}; \
             update Sevak",
            raw.format
        ));
    }
    let mut entries: Vec<Entry> = Vec::new();
    let mut skipped = Vec::new();
    let mut ids: HashSet<String> = HashSet::new();
    for (position, value) in raw.entries.into_iter().enumerate() {
        let label = value
            .get("id")
            .and_then(|id| id.as_str())
            .map_or_else(|| format!("entry {}", position + 1), str::to_owned);
        let mut entry: Entry = match serde_json::from_value(value) {
            Ok(entry) => entry,
            Err(err) => {
                skipped.push(format!("{label}: {err}"));
                continue;
            }
        };
        entry.sha256 = entry.sha256.trim().to_ascii_lowercase();
        clean_tags(&mut entry.tags);
        if let Err(reason) = check_entry(&mut entry, pin) {
            skipped.push(format!("{label}: {reason}"));
            continue;
        }
        if !ids.insert(entry.id.clone()) {
            skipped.push(format!("{label}: the id is used twice"));
            continue;
        }
        entries.push(entry);
    }
    Ok(Index {
        name: raw.name,
        source: pin.index_url(INDEX_FILE),
        tag: pin.tag().to_owned(),
        note: None,
        entries,
        skipped,
    })
}

/// Most tags an entry keeps, and the longest one.
const MAX_TAGS: usize = 8;
const MAX_TAG_CHARS: usize = 24;

/// Keeps the tags that are lower case letters, digits and dashes, once each.
fn clean_tags(tags: &mut Vec<String>) {
    let mut seen = HashSet::new();
    tags.retain(|tag| {
        !tag.is_empty()
            && tag.len() <= MAX_TAG_CHARS
            && tag
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            && seen.insert(tag.clone())
    });
    tags.truncate(MAX_TAGS);
}

fn check_entry(entry: &mut Entry, pin: &Pin) -> Result<(), String> {
    let valid_id = |id: &str| {
        !id.is_empty()
            && id.len() <= 48
            && id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            && !is_reserved_device_name(id)
    };
    if !valid_id(&entry.id) {
        return Err("the id may only use a-z, 0-9 and -".to_owned());
    }
    if let Some(folder) = &entry.folder {
        if !valid_folder_name(folder) {
            return Err("the folder name is not valid".to_owned());
        }
    }
    if entry.name.trim().is_empty() {
        return Err("the name is empty".to_owned());
    }
    // Windows device names cannot be a folder, whatever the package says.
    if is_reserved_device_name(entry.folder_name()) {
        return Err("the folder name is not valid".to_owned());
    }
    if entry.kind == Kind::Native {
        return check_native(entry, pin);
    }
    if !entry.platforms.is_empty() {
        return Err("only native extensions have `platforms`".to_owned());
    }
    entry.source = pin
        .resolve(&entry.source)
        .map_err(|why| format!("the source is refused: {why}"))?;
    if !is_sha256(&entry.sha256) {
        return Err("sha256 must be 64 hex digits".to_owned());
    }
    Ok(())
}

fn is_sha256(text: &str) -> bool {
    text.len() == 64 && text.chars().all(|c| c.is_ascii_hexdigit())
}

/// The extra rules of a native extension's entry: one package per platform,
/// each inside the release, and the facts the page shows before installing.
fn check_native(entry: &mut Entry, pin: &Pin) -> Result<(), String> {
    if entry.platforms.is_empty() {
        return Err("a native extension needs `platforms`".to_owned());
    }
    if !entry.source.is_empty() || !entry.sha256.is_empty() {
        return Err("a native extension has `platforms`, not `source` and `sha256`".to_owned());
    }
    if entry
        .folder
        .as_deref()
        .is_some_and(|folder| folder != entry.id)
    {
        return Err("a native extension installs under its id, so it has no `folder`".to_owned());
    }
    if semver::Version::parse(entry.version.trim()).is_err() {
        return Err("`version` must be like 1.2.3".to_owned());
    }
    entry.version = entry.version.trim().to_owned();
    if entry.author.trim().is_empty() {
        return Err("a native extension needs an `author` (the publisher)".to_owned());
    }
    if entry.license.as_deref().is_none_or(|l| l.trim().is_empty()) {
        return Err("a native extension needs a `license`".to_owned());
    }
    if let Some(min) = &entry.min_sevak {
        if semver::Version::parse(min.trim()).is_err() {
            return Err("`min_sevak` must be like 1.2.3".to_owned());
        }
    }
    if let Some(repository) = &entry.repository {
        if !repository.to_ascii_lowercase().starts_with("https://") {
            return Err("`repository` must be an https:// address".to_owned());
        }
    }
    // Not silently trimmed like tags: the page and the package must agree.
    let listed = entry.permissions.len();
    clean_tags(&mut entry.permissions);
    if entry.permissions.len() != listed {
        return Err(
            "permissions must be unique lower case words (a-z, 0-9, -), at most 8, each at most              24 characters"
                .to_owned(),
        );
    }
    for (platform, artifact) in &mut entry.platforms {
        if !crate::script::PLATFORMS.contains(&platform.as_str()) {
            return Err(format!("the platform \"{platform}\" is not known"));
        }
        artifact.source = pin
            .resolve(&artifact.source)
            .map_err(|why| format!("the source for {platform} is refused: {why}"))?;
        artifact.sha256 = artifact.sha256.trim().to_ascii_lowercase();
        if !is_sha256(&artifact.sha256) {
            return Err(format!("sha256 for {platform} must be 64 hex digits"));
        }
    }
    Ok(())
}

/// Fetches and parses the index of this build's release (or, for a build
/// without one, of the latest stable release, which [`Index::note`] then says).
pub fn fetch_index() -> Result<Index, String> {
    fetch_index_with(&Https, Pin::for_build().as_ref())
}

/// [`fetch_index`] over `transport`, for a build that is `build`'s release.
pub fn fetch_index_with(transport: &dyn Transport, build: Option<&Pin>) -> Result<Index, String> {
    let pinned = fetch_pinned(transport, build, INDEX_FILE, MAX_INDEX_BYTES)?;
    let text =
        String::from_utf8(pinned.body).map_err(|_| "the gallery index is not text".to_owned())?;
    let mut index = parse_index(&text, &pinned.pin)?;
    index.note = pinned.note;
    Ok(index)
}

/// Where installs go.
#[derive(Debug, Clone, Copy)]
pub struct Dirs<'a> {
    pub workflows: &'a Path,
    pub plugins: &'a Path,
}

impl Dirs<'_> {
    pub(crate) fn root(&self, kind: Kind) -> &Path {
        match kind {
            Kind::Workflow => self.workflows,
            Kind::Plugin | Kind::Native => self.plugins,
        }
    }

    /// Whether the folder for `entry` already exists.
    pub fn is_installed(&self, entry: &Entry) -> bool {
        self.root(entry.kind).join(entry.folder_name()).exists()
    }
}

/// What an install put on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub kind: Kind,
    pub folder: String,
    pub path: PathBuf,
    pub files: usize,
}

const NATIVE_ELSEWHERE: &str =
    "native extensions are installed from Settings > Extensions, which shows what they declare first";

/// Downloads `entry` and installs it (see [`install_bytes`]).
pub fn install(entry: &Entry, dirs: &Dirs<'_>) -> Result<Installed, String> {
    if entry.kind == Kind::Native {
        return Err(NATIVE_ELSEWHERE.to_owned());
    }
    let bytes = fetch_https(&entry.source, MAX_PACKAGE_BYTES)?;
    install_bytes(entry, &bytes, dirs)
}

/// Verifies `bytes` against the entry's checksum, checks the package and
/// unpacks it into a new folder below the workflows or plugins folder. An
/// existing folder of that name is never touched.
pub fn install_bytes(entry: &Entry, bytes: &[u8], dirs: &Dirs<'_>) -> Result<Installed, String> {
    install_bytes_as(entry, bytes, dirs, Mode::New, &|| {})
}

/// Whether an install may replace what is there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// A new folder; an existing one of that name is never touched.
    New,
    /// An update: the folder is replaced as one step. Nothing is replaced when
    /// the package is refused, and a failure while swapping puts the old folder
    /// back.
    Replace,
}

/// [`install_bytes`] with a choice of [`Mode`]. `before_swap` runs once the new
/// package is checked and written aside, right before it replaces the old
/// folder: the place to stop a program that still runs from it.
pub fn install_bytes_as(
    entry: &Entry,
    bytes: &[u8],
    dirs: &Dirs<'_>,
    mode: Mode,
    before_swap: &dyn Fn(),
) -> Result<Installed, String> {
    if entry.kind == Kind::Native {
        return Err(NATIVE_ELSEWHERE.to_owned());
    }
    verify_sha256(bytes, &entry.sha256)?;
    let folder = entry.folder_name();
    if !valid_folder_name(folder) {
        return Err("the folder name is not valid".to_owned());
    }
    let root = dirs.root(entry.kind);
    let target = root.join(folder);
    let replace = mode == Mode::Replace;
    if target.exists() && !replace {
        return Err(format!(
            "\"{folder}\" is already installed. To reinstall it, remove that folder first: {}",
            target.display()
        ));
    }
    let package = Package::read(bytes, folder, entry.kind)?;
    check_content(&package, entry.kind, folder)?;

    fs::create_dir_all(root).map_err(|err| format!("could not create the folder: {err}"))?;
    let staging = root.join(format!(".installing-{folder}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&staging);
    let result = write_package(&package, &staging)
        .and_then(|()| place(&staging, &target, replace && target.exists(), before_swap));
    if let Err(err) = result {
        let _ = fs::remove_dir_all(&staging);
        return Err(err);
    }
    Ok(Installed {
        kind: entry.kind,
        folder: folder.to_owned(),
        path: target,
        files: package.files.len(),
    })
}

/// Moves the finished `staging` folder to `target`. With `replace`, the old
/// folder is moved aside first and put back if the new one cannot take its
/// place, so an update either happens whole or not at all.
///
/// A folder whose program is still running cannot be moved on Windows, so the
/// first move is retried for a moment (the program has just been stopped).
pub(crate) fn place(
    staging: &Path,
    target: &Path,
    replace: bool,
    before_swap: &dyn Fn(),
) -> Result<(), String> {
    if !replace {
        return fs::rename(staging, target)
            .map_err(|err| format!("could not finish the install: {err}"));
    }
    before_swap();
    let backup = staging.with_file_name(format!(
        ".replaced-{}-{}",
        target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&backup);
    retry(|| fs::rename(target, &backup))
        .map_err(|err| format!("could not replace the old version (is it running?): {err}"))?;
    if let Err(err) = fs::rename(staging, target) {
        // Put the old version back; it is the only copy.
        let restored = fs::rename(&backup, target);
        return Err(match restored {
            Ok(()) => format!("could not finish the update, the old version is back: {err}"),
            Err(back) => format!(
                "could not finish the update ({err}) and could not put the old version back \
                 ({back}); it is in {}",
                backup.display()
            ),
        });
    }
    let _ = fs::remove_dir_all(&backup);
    Ok(())
}

/// Runs `action`, retrying for about two seconds while it fails (Windows holds
/// a folder until the program that ran from it has really exited).
pub(crate) fn retry<T>(mut action: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    let mut attempts = 0;
    loop {
        match action() {
            Ok(done) => return Ok(done),
            Err(err) => {
                attempts += 1;
                if attempts >= 20 {
                    return Err(err);
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }
}

/// A package's files, checked and held in memory.
struct Package {
    /// `(path relative to the folder, contents, executable)`.
    files: Vec<(String, Vec<u8>, bool)>,
}

impl Package {
    fn read(bytes: &[u8], folder: &str, kind: Kind) -> Result<Self, String> {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|err| format!("the package is not a valid zip file: {err}"))?;
        if archive.len() > MAX_FILES * 2 {
            return Err("the package has too many entries".to_owned());
        }
        let mut files = Vec::new();
        let mut total: u64 = 0;
        // The package's single top-level folder, when it has one.
        let mut top: Option<String> = None;
        let mut rooted = 0usize;
        for index in 0..archive.len() {
            let mut entry = archive
                .by_index(index)
                .map_err(|err| format!("the package is damaged: {err}"))?;
            let name = clean_path(entry.name())?;
            if entry.is_dir() {
                continue;
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
            match name.split_once('/') {
                Some((first, _)) => match &top {
                    None => top = Some(first.to_owned()),
                    Some(seen) if seen == first => {}
                    Some(_) => {
                        return Err(
                            "the package must contain a single folder, or only files".to_owned()
                        )
                    }
                },
                None => rooted += 1,
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
            files.push((name, data, executable));
        }
        if files.is_empty() {
            return Err("the package is empty".to_owned());
        }
        if top.is_some() && rooted > 0 {
            return Err("the package must contain a single folder, or only files".to_owned());
        }
        if let Some(top) = &top {
            if top != folder {
                return Err(format!(
                    "the package's folder is called \"{top}\", but the gallery says \"{folder}\""
                ));
            }
            for (name, _, _) in &mut files {
                *name = name[top.len() + 1..].to_owned();
            }
        }
        let manifest = kind.manifest_file();
        if !files.iter().any(|(name, _, _)| name == manifest) {
            return Err(format!("the package has no {manifest}"));
        }
        let mut seen = HashSet::new();
        for (name, _, _) in &files {
            if !seen.insert(name.to_lowercase()) {
                return Err(format!("the package lists {name} twice"));
            }
        }
        Ok(Self { files })
    }

    fn file(&self, name: &str) -> Option<&[u8]> {
        self.files
            .iter()
            .find(|(path, _, _)| path == name)
            .map(|(_, data, _)| data.as_slice())
    }
}

/// A path from a zip entry as forward-slash components, or why it is refused:
/// no leading `/`, drive letter, `..`, empty or hidden-looking tricks.
pub(crate) fn clean_path(raw: &str) -> Result<String, String> {
    let name = raw.replace('\\', "/");
    if name.len() > MAX_PATH_BYTES {
        return Err("a path in the package is too long".to_owned());
    }
    if name.starts_with('/') || name.contains(':') || name.contains('\0') {
        return Err("a path in the package is not allowed".to_owned());
    }
    let mut parts = Vec::new();
    for part in name.split('/') {
        match part {
            "" | "." => continue,
            ".." => return Err("a path in the package leaves its folder".to_owned()),
            part if part.chars().any(char::is_control) => {
                return Err("a path in the package is not allowed".to_owned())
            }
            // Windows trims a trailing dot or space, so `a.` and `a` collide.
            part if part.ends_with('.') || part.ends_with(' ') => {
                return Err("a path in the package is not allowed".to_owned())
            }
            // `CON`, `NUL`, `COM1`... (also `nul.txt`) are devices on Windows.
            part if is_reserved_device_name(part) => {
                return Err("a path in the package is not allowed".to_owned())
            }
            part => parts.push(part),
        }
    }
    Ok(parts.join("/"))
}

/// Checks that the package's manifest is one Sevak can load.
fn check_content(package: &Package, kind: Kind, folder: &str) -> Result<(), String> {
    let manifest = kind.manifest_file();
    let text = package
        .file(manifest)
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .ok_or_else(|| format!("{manifest} is not text"))?;
    match kind {
        Kind::Workflow => {
            let workflow = Workflow::from_toml(text)
                .map_err(|err| format!("the package's workflow is not valid: {err}"))?;
            if let Some(summary) = error_summary(&workflow.validate()) {
                return Err(format!("the package's workflow is not valid: {summary}"));
            }
        }
        Kind::Plugin | Kind::Native => {
            Manifest::parse(text, folder)
                .map_err(|err| format!("the package's plugin is not valid: {err}"))?;
        }
    }
    Ok(())
}

fn write_package(package: &Package, dir: &Path) -> Result<(), String> {
    write_files(&package.files, dir)
}

/// Writes `(relative path, contents, executable)` files below `dir`, creating
/// folders as needed. Only the owner's execute bit survives on Unix.
pub(crate) fn write_files(files: &[(String, Vec<u8>, bool)], dir: &Path) -> Result<(), String> {
    for (name, data, executable) in files {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| format!("could not write a file: {err}"))?;
        }
        let mut file =
            fs::File::create(&path).map_err(|err| format!("could not write a file: {err}"))?;
        file.write_all(data)
            .map_err(|err| format!("could not write a file: {err}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // Only the owner's execute bit survives: never setuid and friends.
            let mode = if *executable { 0o755 } else { 0o644 };
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(mode));
        }
        #[cfg(not(unix))]
        let _ = executable;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use zip::write::SimpleFileOptions;

    use super::*;
    use crate::net::{sha256_hex, FetchError};

    fn pin() -> Pin {
        Pin::new("v1.2.3").unwrap()
    }

    fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(&mut out);
        for (name, data) in files {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap();
        out.into_inner()
    }

    const WORKFLOW: &str = r#"
        name = "Docs"
        [[node]]
        id = "k"
        type = "keyword"
        keyword = "docs"
        [[node]]
        id = "o"
        type = "open_url"
        url = "https://example.com/?q={query}"
        [[connection]]
        from = "k"
        to = "o"
    "#;

    fn entry_for(bytes: &[u8], kind: Kind, id: &str) -> Entry {
        Entry {
            id: id.to_owned(),
            kind,
            name: id.to_owned(),
            description: String::new(),
            author: String::new(),
            version: String::new(),
            tags: Vec::new(),
            source: "https://example.com/p.zip".to_owned(),
            sha256: sha256_hex(bytes),
            homepage: None,
            folder: None,
            platforms: BTreeMap::new(),
            license: None,
            repository: None,
            min_sevak: None,
            permissions: Vec::new(),
        }
    }

    struct Roots {
        _tmp: tempfile::TempDir,
        workflows: PathBuf,
        plugins: PathBuf,
    }

    fn roots() -> Roots {
        let tmp = tempfile::tempdir().unwrap();
        Roots {
            workflows: tmp.path().join("workflows"),
            plugins: tmp.path().join("plugins"),
            _tmp: tmp,
        }
    }

    impl Roots {
        fn dirs(&self) -> Dirs<'_> {
            Dirs {
                workflows: &self.workflows,
                plugins: &self.plugins,
            }
        }
    }

    // ---- the index ----------------------------------------------------

    fn good_entry(id: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id, "kind": "workflow", "name": id, "description": "d", "author": "a",
            "version": "1", "source": "gallery/packages/x.zip",
            "sha256": "A".repeat(64)
        })
    }

    #[test]
    fn parses_an_index_and_skips_bad_entries() {
        let text = serde_json::json!({
            "format": 2, "name": "Test gallery",
            "entries": [
                good_entry("one"),
                {"id": "future", "kind": "theme", "name": "T", "source": "gallery/t.zip", "sha256": "0".repeat(64)},
                {"id": "elsewhere", "kind": "plugin", "name": "T", "source": "https://x.test/t.zip", "sha256": "0".repeat(64)},
                {"id": "short-hash", "kind": "plugin", "name": "T", "source": "gallery/t.zip", "sha256": "abc"},
                {"id": "Bad Id", "kind": "plugin", "name": "T", "source": "gallery/t.zip", "sha256": "0".repeat(64)},
                good_entry("one"),
                {"kind": "plugin"},
                "not an object",
                {"id": "plug", "kind": "plugin", "name": "P", "source": "gallery/packages/p.zip", "sha256": "f".repeat(64), "folder": "plug-dir"}
            ]
        })
        .to_string();
        let index = parse_index(&text, &pin()).unwrap();
        assert_eq!(index.name, "Test gallery");
        assert_eq!(index.tag, "v1.2.3");
        assert_eq!(
            index.source,
            "https://raw.githubusercontent.com/ninad-k/Sevak/v1.2.3/gallery/index.json"
        );
        let ids: Vec<_> = index.entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["one", "plug"]);
        // Hashes are normalized to lower case.
        assert_eq!(index.entries[0].sha256, "a".repeat(64));
        assert_eq!(index.entries[1].folder_name(), "plug-dir");
        assert_eq!(index.entries[0].folder_name(), "one");
        // Sources are the release's own addresses.
        assert_eq!(
            index.entries[0].source,
            "https://raw.githubusercontent.com/ninad-k/Sevak/v1.2.3/gallery/packages/x.zip"
        );
        assert_eq!(index.skipped.len(), 7, "{:?}", index.skipped);
        assert!(index.skipped.iter().any(|s| s.starts_with("future")));
    }

    #[test]
    fn tags_are_kept_only_when_they_are_plain_labels() {
        let mut entry = good_entry("tagged");
        entry["tags"] = serde_json::json!([
            "search",
            "Needs Python",
            "search",
            "",
            "x".repeat(25),
            "no-code",
            "a",
            "b",
            "c",
            "d",
            "e",
            "f"
        ]);
        let text = serde_json::json!({"format": 2, "entries": [entry, good_entry("plain")]});
        let index = parse_index(&text.to_string(), &pin()).unwrap();
        assert_eq!(
            index.entries[0].tags,
            ["search", "no-code", "a", "b", "c", "d", "e", "f"]
        );
        assert!(index.entries[1].tags.is_empty());
        // Entries without tags serialize without the field.
        let json = serde_json::to_value(&index.entries[1]).unwrap();
        assert!(json.get("tags").is_none());
    }

    #[test]
    fn sources_outside_the_release_are_skipped() {
        let entry = |id: &str, source: &str| {
            serde_json::json!({
                "id": id, "kind": "workflow", "name": id, "source": source,
                "sha256": "0".repeat(64)
            })
        };
        let text = serde_json::json!({
            "format": 2,
            "entries": [
                entry("relative", "gallery/packages/a.zip"),
                entry("own-absolute", "https://raw.githubusercontent.com/ninad-k/Sevak/v1.2.3/gallery/packages/b.zip"),
                entry("own-release", "https://github.com/ninad-k/Sevak/releases/download/v1.2.3/c.zip"),
                entry("main-branch", "https://raw.githubusercontent.com/ninad-k/Sevak/main/gallery/packages/d.zip"),
                entry("other-tag", "https://raw.githubusercontent.com/ninad-k/Sevak/v1.2.4/gallery/packages/e.zip"),
                entry("other-host", "https://example.com/f.zip"),
                entry("other-repo", "https://raw.githubusercontent.com/evil/Sevak/v1.2.3/gallery/g.zip"),
                entry("plain-http", "http://raw.githubusercontent.com/ninad-k/Sevak/v1.2.3/gallery/h.zip"),
                entry("dot-dot", "gallery/../../../main/i.zip"),
                entry("credentials", "https://u@raw.githubusercontent.com/ninad-k/Sevak/v1.2.3/gallery/j.zip"),
                entry("empty", "")
            ]
        })
        .to_string();
        let index = parse_index(&text, &pin()).unwrap();
        let ids: Vec<_> = index.entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["relative", "own-absolute", "own-release"]);
        assert_eq!(index.skipped.len(), 8, "{:?}", index.skipped);
        assert!(index
            .skipped
            .iter()
            .all(|s| s.contains("source is refused")));
    }

    #[test]
    fn windows_device_names_are_not_ids_or_folders() {
        let entry = |id: &str, folder: Option<&str>| {
            let mut value = serde_json::json!({
                "id": id, "kind": "workflow", "name": "N", "source": "gallery/x.zip",
                "sha256": "0".repeat(64)
            });
            if let Some(folder) = folder {
                value["folder"] = folder.into();
            }
            value
        };
        let text = serde_json::json!({
            "format": 2,
            "entries": [
                entry("con", None), entry("nul", None), entry("com1", None), entry("lpt9", None),
                entry("ok-id", Some("CON")), entry("ok-id-2", Some("aux.txt")),
                entry("fine", None), entry("console", None)
            ]
        })
        .to_string();
        let index = parse_index(&text, &pin()).unwrap();
        let ids: Vec<_> = index.entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["fine", "console"]);
    }

    fn native_entry(id: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id, "kind": "native", "name": "Tool", "description": "d",
            "author": "Ada", "version": "1.2.3", "license": "MIT",
            "min_sevak": "0.1.0", "permissions": ["network"],
            "repository": "https://github.com/example/tool",
            "platforms": {
                "linux-x86_64": {
                    "source": "gallery/extensions/tool/tool-1.2.3-linux-x86_64.sevakext",
                    "sha256": "A".repeat(64)
                },
                "windows-x86_64": {
                    "source": "gallery/extensions/tool/tool-1.2.3-windows-x86_64.sevakext",
                    "sha256": "b".repeat(64)
                }
            }
        })
    }

    #[test]
    fn native_entries_have_one_package_per_platform() {
        let text = serde_json::json!({"format": 2, "entries": [native_entry("tool")]}).to_string();
        let index = parse_index(&text, &pin()).unwrap();
        assert!(index.skipped.is_empty(), "{:?}", index.skipped);
        let entry = &index.entries[0];
        assert_eq!(entry.kind, Kind::Native);
        assert_eq!(entry.folder_name(), "tool");
        assert_eq!(entry.permissions, ["network"]);
        let linux = &entry.platforms["linux-x86_64"];
        assert_eq!(
            linux.source,
            "https://raw.githubusercontent.com/ninad-k/Sevak/v1.2.3/gallery/extensions/tool/tool-1.2.3-linux-x86_64.sevakext"
        );
        assert_eq!(linux.sha256, "a".repeat(64), "hashes are lower case");
        assert!(entry.source.is_empty() && entry.sha256.is_empty());
    }

    #[test]
    fn bad_native_entries_are_skipped_not_fatal() {
        let with = |change: &dyn Fn(&mut serde_json::Value)| {
            let mut entry = native_entry("tool");
            change(&mut entry);
            entry
        };
        let bad = vec![
            with(&|e| {
                e.as_object_mut().unwrap().remove("platforms");
            }),
            with(&|e| e["platforms"] = serde_json::json!({})),
            with(&|e| e["platforms"]["beos-x86_64"] = e["platforms"]["linux-x86_64"].clone()),
            with(&|e| e["platforms"]["linux-x86_64"]["sha256"] = "abc".into()),
            with(&|e| e["platforms"]["linux-x86_64"]["source"] = "https://example.com/x".into()),
            with(&|e| {
                e["platforms"]["linux-x86_64"]["source"] = "gallery/../../main/x.sevakext".into()
            }),
            with(&|e| e["source"] = "gallery/x.zip".into()),
            with(&|e| e["version"] = "latest".into()),
            with(&|e| e["author"] = "".into()),
            with(&|e| {
                e.as_object_mut().unwrap().remove("license");
            }),
            with(&|e| e["min_sevak"] = "soon".into()),
            with(&|e| e["repository"] = "http://example.com".into()),
            with(&|e| e["folder"] = "elsewhere".into()),
            // Permissions are never silently trimmed: the page and the package must agree.
            with(&|e| e["permissions"] = serde_json::json!(["network", "Bad Permission"])),
            with(&|e| e["permissions"] = serde_json::json!(["network", "network"])),
            with(&|e| {
                e["permissions"] = serde_json::json!(["a", "b", "c", "d", "e", "f", "g", "h", "i"])
            }),
        ];
        let count = bad.len();
        let mut entries = bad;
        entries.push(native_entry("good"));
        let text = serde_json::json!({"format": 2, "entries": entries}).to_string();
        let index = parse_index(&text, &pin()).unwrap();
        assert_eq!(index.entries.len(), 1, "{:?}", index.skipped);
        assert_eq!(index.entries[0].id, "good");
        assert_eq!(index.skipped.len(), count, "{:?}", index.skipped);
    }

    #[test]
    fn only_native_entries_have_platforms() {
        let mut entry = good_entry("one");
        entry["platforms"] = native_entry("x")["platforms"].clone();
        let text = serde_json::json!({"format": 2, "entries": [entry]}).to_string();
        let index = parse_index(&text, &pin()).unwrap();
        assert!(index.entries.is_empty());
        assert!(
            index.skipped[0].contains("only native"),
            "{:?}",
            index.skipped
        );
    }

    #[test]
    fn a_native_entry_does_not_install_through_the_old_page() {
        let r = roots();
        let text = serde_json::json!({"format": 2, "entries": [native_entry("tool")]}).to_string();
        let entry = parse_index(&text, &pin()).unwrap().entries.remove(0);
        let err = install_bytes(&entry, b"x", &r.dirs()).unwrap_err();
        assert!(err.contains("Settings > Extensions"), "{err}");
        assert!(!r.plugins.exists());
    }

    #[test]
    fn an_update_swaps_the_folder_and_a_failure_puts_the_old_one_back() {
        let r = roots();
        let v1 = zip_of(&[
            ("docs/workflow.toml", WORKFLOW.as_bytes()),
            ("docs/old.txt", b"old"),
        ]);
        let v2 = zip_of(&[
            ("docs/workflow.toml", WORKFLOW.as_bytes()),
            ("docs/new.txt", b"new"),
        ]);
        let one = entry_for(&v1, Kind::Workflow, "docs");
        let two = entry_for(&v2, Kind::Workflow, "docs");
        install_bytes(&one, &v1, &r.dirs()).unwrap();
        // A plain install never replaces.
        assert!(install_bytes(&two, &v2, &r.dirs()).is_err());
        let stopped = std::cell::Cell::new(0);
        install_bytes_as(&two, &v2, &r.dirs(), Mode::Replace, &|| {
            stopped.set(stopped.get() + 1);
        })
        .unwrap();
        assert_eq!(stopped.get(), 1);
        assert!(r.workflows.join("docs/new.txt").is_file());
        assert!(!r.workflows.join("docs/old.txt").exists());
        // A package that is refused leaves the folder alone and never calls the hook.
        let bad = zip_of(&[("docs/readme.txt", b"no manifest")]);
        let refused = entry_for(&bad, Kind::Workflow, "docs");
        stopped.set(0);
        assert!(
            install_bytes_as(&refused, &bad, &r.dirs(), Mode::Replace, &|| {
                stopped.set(1);
            })
            .is_err()
        );
        assert_eq!(stopped.get(), 0);
        assert!(r.workflows.join("docs/new.txt").is_file());
    }

    #[test]
    fn the_index_format_and_json_are_checked() {
        let err = parse_index(r#"{"format": 1, "entries": []}"#, &pin()).unwrap_err();
        assert!(err.contains("format 1"), "{err}");
        let err = parse_index(r#"{"format": 3, "entries": []}"#, &pin()).unwrap_err();
        assert!(err.contains("format 3"), "{err}");
        assert!(parse_index("not json", &pin()).is_err());
        assert!(parse_index(r#"{"entries": []}"#, &pin()).is_err());
        let empty = parse_index("\u{feff}{\"format\": 2}", &pin()).unwrap();
        assert!(empty.entries.is_empty());
    }

    /// A transport that serves one canned index and nothing else.
    struct Canned(String);

    impl Transport for Canned {
        fn get(&self, url: &str, _max: usize) -> Result<Vec<u8>, FetchError> {
            if url.ends_with("/v1.2.3/gallery/index.json") {
                Ok(self.0.clone().into_bytes())
            } else {
                Err(FetchError::NotFound)
            }
        }

        fn latest_release_page(&self) -> Result<String, String> {
            Err("not asked".to_owned())
        }
    }

    #[test]
    fn fetching_the_index_reads_the_builds_release() {
        let text = serde_json::json!({"format": 2, "name": "G", "entries": [good_entry("one")]});
        let index = fetch_index_with(&Canned(text.to_string()), Some(&pin())).unwrap();
        assert_eq!(index.entries.len(), 1);
        assert_eq!(index.note, None);
        assert!(index.entries[0].source.contains("/v1.2.3/"));
    }

    // ---- installing ---------------------------------------------------

    #[test]
    fn installs_a_workflow_package_into_a_new_folder() {
        let r = roots();
        let bytes = zip_of(&[
            ("docs/workflow.toml", WORKFLOW.as_bytes()),
            ("docs/scripts/helper.py", b"print('hi')"),
        ]);
        let entry = entry_for(&bytes, Kind::Workflow, "docs");
        assert!(!r.dirs().is_installed(&entry));
        let done = install_bytes(&entry, &bytes, &r.dirs()).unwrap();
        assert_eq!(done.folder, "docs");
        assert_eq!(done.files, 2);
        assert!(r.workflows.join("docs/workflow.toml").is_file());
        assert!(r.workflows.join("docs/scripts/helper.py").is_file());
        assert!(r.dirs().is_installed(&entry));
        // No staging folder is left behind.
        let left: Vec<_> = fs::read_dir(&r.workflows)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, ["docs"]);
        // The folder is a loadable, unapproved workflow.
        let text = fs::read_to_string(r.workflows.join("docs/workflow.toml")).unwrap();
        assert!(Workflow::from_toml(&text).unwrap().is_valid());
    }

    #[test]
    fn a_package_may_hold_its_files_at_the_top() {
        let r = roots();
        let bytes = zip_of(&[("workflow.toml", WORKFLOW.as_bytes())]);
        let entry = entry_for(&bytes, Kind::Workflow, "flat");
        install_bytes(&entry, &bytes, &r.dirs()).unwrap();
        assert!(r.workflows.join("flat/workflow.toml").is_file());
    }

    #[test]
    fn installs_a_script_plugin_into_the_plugins_folder() {
        let r = roots();
        let manifest = "protocol = 1\nkeyword = \"hi\"\ncommand = [\"python3\", \"main.py\"]\n";
        let bytes = zip_of(&[
            ("hello/plugin.toml", manifest.as_bytes()),
            ("hello/main.py", b"print(1)"),
        ]);
        let entry = entry_for(&bytes, Kind::Plugin, "hello");
        let done = install_bytes(&entry, &bytes, &r.dirs()).unwrap();
        assert_eq!(done.path, r.plugins.join("hello"));
        assert!(!r.workflows.exists(), "a plugin is not a workflow");
    }

    #[test]
    fn a_tampered_download_is_discarded_before_anything_is_written() {
        let r = roots();
        let good = zip_of(&[("docs/workflow.toml", WORKFLOW.as_bytes())]);
        let entry = entry_for(&good, Kind::Workflow, "docs");
        let evil = zip_of(&[("docs/workflow.toml", b"name = \"evil\"")]);
        let err = install_bytes(&entry, &evil, &r.dirs()).unwrap_err();
        assert!(err.contains("checksum"), "{err}");
        assert!(!r.workflows.exists());
    }

    #[test]
    fn an_existing_folder_is_never_overwritten() {
        let r = roots();
        let bytes = zip_of(&[("docs/workflow.toml", WORKFLOW.as_bytes())]);
        let entry = entry_for(&bytes, Kind::Workflow, "docs");
        fs::create_dir_all(r.workflows.join("docs")).unwrap();
        fs::write(r.workflows.join("docs/mine.txt"), "my edits").unwrap();
        let err = install_bytes(&entry, &bytes, &r.dirs()).unwrap_err();
        assert!(err.contains("already installed"), "{err}");
        assert_eq!(
            fs::read_to_string(r.workflows.join("docs/mine.txt")).unwrap(),
            "my edits"
        );
        assert!(r.dirs().is_installed(&entry));
    }

    fn refused(files: &[(&str, &[u8])], needle: &str) {
        let r = roots();
        let bytes = zip_of(files);
        let entry = entry_for(&bytes, Kind::Workflow, "docs");
        let err = install_bytes(&entry, &bytes, &r.dirs()).unwrap_err();
        assert!(err.contains(needle), "{files:?}: {err}");
        // Nothing - not even a staging folder - may be left behind.
        let leftovers = fs::read_dir(&r.workflows).map_or(0, Iterator::count);
        assert_eq!(leftovers, 0, "{files:?}");
    }

    #[test]
    fn paths_that_escape_the_folder_are_refused() {
        let w = WORKFLOW.as_bytes();
        refused(
            &[("docs/workflow.toml", w), ("docs/../../evil.txt", b"x")],
            "leaves its folder",
        );
        refused(
            &[("docs/workflow.toml", w), ("/etc/passwd", b"x")],
            "not allowed",
        );
        refused(
            &[("docs/workflow.toml", w), ("C:/Windows/evil.dll", b"x")],
            "not allowed",
        );
        refused(
            &[("docs/workflow.toml", w), ("docs\\..\\..\\evil.txt", b"x")],
            "leaves its folder",
        );
        refused(
            &[("docs/workflow.toml", w), ("docs/file.", b"x")],
            "not allowed",
        );
        refused(
            &[("docs/workflow.toml", w), ("docs/a\u{7}b", b"x")],
            "not allowed",
        );
    }

    #[test]
    fn windows_device_names_are_refused_as_entry_names() {
        let w = WORKFLOW.as_bytes();
        for name in [
            "docs/NUL",
            "docs/nul.txt",
            "docs/scripts/CON.py",
            "docs/Aux",
            "docs/COM1",
            "docs/com9.json",
            "docs/lpt3.txt",
            "docs/PRN.",
            "docs/COM\u{b9}",
            "docs/con/inner.txt",
        ] {
            refused(&[("docs/workflow.toml", w), (name, b"x")], "not allowed");
        }
        // Names that merely start like one are fine.
        let r = roots();
        let bytes = zip_of(&[
            ("docs/workflow.toml", w),
            ("docs/console.txt", b"x"),
            ("docs/connect.py", b"x"),
        ]);
        let entry = entry_for(&bytes, Kind::Workflow, "docs");
        install_bytes(&entry, &bytes, &r.dirs()).unwrap();
    }

    #[test]
    fn a_package_under_a_device_name_is_refused() {
        let w = WORKFLOW.as_bytes();
        let r = roots();
        let bytes = zip_of(&[("nul/workflow.toml", w)]);
        let mut entry = entry_for(&bytes, Kind::Workflow, "docs");
        entry.folder = Some("nul".to_owned());
        let err = install_bytes(&entry, &bytes, &r.dirs()).unwrap_err();
        assert!(err.contains("folder name is not valid"), "{err}");
        assert!(!r.workflows.exists());
    }

    #[test]
    fn the_layout_is_checked() {
        let w = WORKFLOW.as_bytes();
        refused(&[("other/workflow.toml", w)], "gallery says");
        refused(
            &[("docs/workflow.toml", w), ("elsewhere/x.txt", b"x")],
            "single folder",
        );
        refused(
            &[("docs/workflow.toml", w), ("loose.txt", b"x")],
            "single folder",
        );
        refused(&[("docs/readme.txt", b"x")], "no workflow.toml");
        refused(&[], "empty");
        refused(
            &[
                ("docs/workflow.toml", w),
                ("docs/A.txt", b"1"),
                ("docs/a.txt", b"2"),
            ],
            "twice",
        );
    }

    #[test]
    fn the_content_is_validated_before_it_is_installed() {
        refused(&[("docs/workflow.toml", b"not toml [")], "not valid");
        // A loop is an error even though the file parses.
        let looped = r#"
            name = "Loop"
            [[node]]
            id = "k"
            type = "keyword"
            keyword = "k"
            [[node]]
            id = "a"
            type = "copy"
            [[node]]
            id = "b"
            type = "copy"
            [[connection]]
            from = "k"
            to = "a"
            [[connection]]
            from = "a"
            to = "b"
            [[connection]]
            from = "b"
            to = "a"
        "#;
        refused(&[("docs/workflow.toml", looped.as_bytes())], "loop");
        // A link in a plugin package cannot be written: zip stores the type in
        // the Unix mode.
        let r = roots();
        let mut out = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(&mut out);
        writer
            .start_file("docs/workflow.toml", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(WORKFLOW.as_bytes()).unwrap();
        writer
            .add_symlink("docs/link", "/etc/passwd", SimpleFileOptions::default())
            .unwrap();
        writer.finish().unwrap();
        let bytes = out.into_inner();
        let entry = entry_for(&bytes, Kind::Workflow, "docs");
        let err = install_bytes(&entry, &bytes, &r.dirs()).unwrap_err();
        assert!(err.contains("link"), "{err}");
    }

    #[test]
    fn a_plugin_manifest_must_load() {
        let r = roots();
        let bytes = zip_of(&[("p/plugin.toml", b"protocol = 1\n")]);
        let entry = entry_for(&bytes, Kind::Plugin, "p");
        let err = install_bytes(&entry, &bytes, &r.dirs()).unwrap_err();
        assert!(err.contains("plugin is not valid"), "{err}");
    }

    #[test]
    fn size_limits_hold_even_when_the_header_lies() {
        let r = roots();
        let big = vec![b'a'; MAX_FILE_BYTES as usize + 1];
        let bytes = zip_of(&[
            ("docs/workflow.toml", WORKFLOW.as_bytes()),
            ("docs/big.bin", &big),
        ]);
        let entry = entry_for(&bytes, Kind::Workflow, "docs");
        let err = install_bytes(&entry, &bytes, &r.dirs()).unwrap_err();
        assert!(err.contains("too large"), "{err}");

        let many: Vec<(String, Vec<u8>)> = (0..MAX_FILES + 1)
            .map(|i| (format!("docs/f{i}.txt"), b"x".to_vec()))
            .collect();
        let mut files: Vec<(&str, &[u8])> = many
            .iter()
            .map(|(n, d)| (n.as_str(), d.as_slice()))
            .collect();
        files.push(("docs/workflow.toml", WORKFLOW.as_bytes()));
        let bytes = zip_of(&files);
        let entry = entry_for(&bytes, Kind::Workflow, "docs");
        let err = install_bytes(&entry, &bytes, &r.dirs()).unwrap_err();
        assert!(err.contains("too many"), "{err}");
    }

    #[test]
    fn a_zip_bomb_is_refused_by_total_size() {
        let r = roots();
        // Each file is within its own limit; together they are not.
        let chunk = vec![b'0'; MAX_FILE_BYTES as usize];
        let names: Vec<String> = (0..7).map(|i| format!("docs/part{i}.bin")).collect();
        let mut files: Vec<(&str, &[u8])> = names
            .iter()
            .map(|name| (name.as_str(), chunk.as_slice()))
            .collect();
        files.push(("docs/workflow.toml", WORKFLOW.as_bytes()));
        let mut out = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(&mut out);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in &files {
            writer.start_file(*name, options).unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap();
        let bytes = out.into_inner();
        assert!(bytes.len() < MAX_PACKAGE_BYTES, "the bomb compresses well");
        let entry = entry_for(&bytes, Kind::Workflow, "docs");
        let err = install_bytes(&entry, &bytes, &r.dirs()).unwrap_err();
        assert!(err.contains("too large when unpacked"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn only_the_owners_execute_bit_is_restored() {
        use std::os::unix::fs::PermissionsExt;

        let r = roots();
        let mut out = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(&mut out);
        writer
            .start_file("docs/workflow.toml", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(WORKFLOW.as_bytes()).unwrap();
        writer
            .start_file(
                "docs/run.sh",
                SimpleFileOptions::default().unix_permissions(0o4777),
            )
            .unwrap();
        writer.write_all(b"#!/bin/sh\n").unwrap();
        writer.finish().unwrap();
        let bytes = out.into_inner();
        let entry = entry_for(&bytes, Kind::Workflow, "docs");
        install_bytes(&entry, &bytes, &r.dirs()).unwrap();
        let mode = fs::metadata(r.workflows.join("docs/run.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o7777, 0o755);
        let plain = fs::metadata(r.workflows.join("docs/workflow.toml"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(plain & 0o7777, 0o644);
    }

    // ---- the repository's own gallery ----------------------------------

    fn repo_gallery() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../gallery")
    }

    #[test]
    fn the_shipped_index_matches_the_shipped_packages() {
        let text = fs::read_to_string(repo_gallery().join("index.json")).unwrap();
        let index = parse_index(&text, &pin()).unwrap();
        assert!(index.skipped.is_empty(), "{:?}", index.skipped);
        assert!(index.entries.len() >= 3);
        let kinds: HashSet<_> = index.entries.iter().map(|e| e.kind).collect();
        assert_eq!(kinds.len(), 2, "both workflows and plugins are shown");
        for entry in &index.entries {
            // The source is the release's raw address of the committed package.
            let file = entry.source.rsplit('/').next().unwrap();
            assert!(
                entry.source.starts_with(
                    "https://raw.githubusercontent.com/ninad-k/Sevak/v1.2.3/gallery/packages/"
                ),
                "{}",
                entry.source
            );
            let bytes = fs::read(repo_gallery().join("packages").join(file))
                .unwrap_or_else(|err| panic!("{file}: {err}"));
            verify_sha256(&bytes, &entry.sha256)
                .unwrap_or_else(|err| panic!("{}: {err}", entry.id));
            // And it installs.
            let r = roots();
            let done = install_bytes(entry, &bytes, &r.dirs())
                .unwrap_or_else(|err| panic!("{}: {err}", entry.id));
            assert_eq!(done.folder, entry.folder_name());
        }
    }

    #[test]
    fn the_packages_match_the_examples_they_were_built_from() {
        let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        let text = fs::read_to_string(repo_gallery().join("index.json")).unwrap();
        let index = parse_index(&text, &pin()).unwrap();
        for entry in index.entries.iter().filter(|e| e.kind != Kind::Native) {
            let file = entry.source.rsplit('/').next().unwrap();
            let bytes = fs::read(repo_gallery().join("packages").join(file)).unwrap();
            let r = roots();
            install_bytes(entry, &bytes, &r.dirs()).unwrap();
            let (installed, source) = match entry.kind {
                Kind::Workflow => (
                    r.workflows.join(entry.folder_name()),
                    examples.join("workflows").join(entry.folder_name()),
                ),
                Kind::Plugin => (
                    r.plugins.join(entry.folder_name()),
                    examples.join("plugins").join(entry.folder_name()),
                ),
                Kind::Native => unreachable!("filtered above"),
            };
            assert_same_files(&installed, &source);
        }
    }

    /// Both folders hold the same files with the same text (line endings
    /// aside, which a Windows checkout may change).
    fn assert_same_files(installed: &Path, source: &Path) {
        fn list(root: &Path, dir: &Path, out: &mut Vec<String>) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    list(root, &path, out);
                } else {
                    out.push(
                        path.strip_prefix(root)
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/"),
                    );
                }
            }
        }
        let (mut a, mut b) = (Vec::new(), Vec::new());
        list(installed, installed, &mut a);
        list(source, source, &mut b);
        a.sort();
        b.sort();
        assert_eq!(a, b, "{}", source.display());
        for name in a {
            let normalize = |path: PathBuf| {
                String::from_utf8_lossy(&fs::read(path).unwrap()).replace("\r\n", "\n")
            };
            assert_eq!(
                normalize(installed.join(&name)),
                normalize(source.join(&name)),
                "{name} in {}",
                source.display()
            );
        }
    }
}
