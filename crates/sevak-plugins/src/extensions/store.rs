//! The extension store: browse the gallery, install, update, uninstall.
//!
//! This is the logic behind Settings > Extensions and the `ext` / `store`
//! launcher keywords. It adds no trust of its own; it reuses what exists:
//!
//! * **The opt-in.** Nothing here touches the network on its own. The caller
//!   asks for the catalog ([`ExtensionStore::refresh`]) when the user presses
//!   the button or Enter on "load", and for an install, update or removal when
//!   the user chooses one. Reading what was loaded earlier
//!   ([`ExtensionStore::load_cached`]) is a local file.
//! * **The sources.** The index is read from the tag of the running build and
//!   every file only from the Sevak repository's allow-list
//!   (`sevak_core::gallery_source`, `crate::net`); a request names an entry by
//!   id, so a caller cannot make Sevak download an address of its choosing.
//! * **The checks before anything is written.** SHA-256 against the index,
//!   then the package rules (`workflow::gallery`, [`super::package`]): safe
//!   paths, no links, size limits, a manifest that parses. A native
//!   extension's own manifest must also agree with the index entry (version,
//!   author, licence, permissions), because the page shows the entry and the
//!   Allow dialog shows the manifest, and they must not tell different stories.
//! * **No auto-run.** An install only places a folder under the plugins or
//!   workflows folder. The folder is *new and unapproved*: Sevak's own Allow
//!   dialog decides whether it may ever run, bound to its contents.
//! * **Records.** Each install writes a receipt (`installed-extensions.json`
//!   in the data folder) so the page can show versions, offer updates and
//!   remove exactly what it installed, nothing else.
//!
//! Errors are sentences for the user.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sevak_core::bounded_read::read_to_string_capped;
use sevak_core::gallery_source::Pin;
use sevak_core::theme_store::{self, Existing, GalleryEntry as ThemeEntry};

use super::package::{self, ExtensionPackage};
use super::revocations::{self, Advisories, Notice, Subject};
use crate::index_signature::{self, IndexSignature};
use crate::net::{fetch_pinned, sha256_hex, verify_sha256, Https, Transport};
use crate::script::{current_platform, ApprovalStore};
use crate::workflow::gallery::{
    self, install_bytes_as, place, retry, write_files, Dirs, Entry, Kind, Mode,
};
use crate::workflow::model::valid_folder_name;

/// The receipts file in the data folder.
pub const RECEIPTS_FILE: &str = "installed-extensions.json";
/// The last catalog loaded, so the page works offline.
pub const CACHE_FILE: &str = "extensions-catalog.json";
const MAX_STATE_BYTES: u64 = 1024 * 1024;

/// What an item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    /// A workflow (runs no code, or a script after the user allows it).
    Workflow,
    /// A script plugin.
    Plugin,
    /// A native extension: a compiled program, never run before the user allows it.
    Native,
    /// A colour theme.
    Theme,
}

impl ItemKind {
    fn from_kind(kind: Kind) -> Self {
        match kind {
            Kind::Workflow => Self::Workflow,
            Kind::Plugin => Self::Plugin,
            Kind::Native => Self::Native,
        }
    }

    /// The word the launcher and the page use.
    pub fn label(self) -> &'static str {
        match self {
            Self::Workflow => "Workflow",
            Self::Plugin => "Script plugin",
            Self::Native => "Native extension",
            Self::Theme => "Theme",
        }
    }
}

/// Where an install lives and what it keeps. All of it is below folders the
/// caller owns; a test points them at a temporary directory.
#[derive(Debug, Clone)]
pub struct StoreDirs {
    /// The config folder (themes live in its `themes` folder).
    pub config_dir: PathBuf,
    /// `<config dir>/workflows`
    pub workflows: PathBuf,
    /// `<config dir>/plugins`
    pub plugins: PathBuf,
    /// Where script plugins keep their own files (`<data dir>/plugins`); removed
    /// with the plugin.
    pub plugin_data: PathBuf,
    /// The same for workflows (`<data dir>/workflows`).
    pub workflow_data: PathBuf,
    /// The data folder: receipts and the catalog cache.
    pub state: PathBuf,
    /// The approvals file shared by script plugins and workflows.
    pub approvals: PathBuf,
}

impl StoreDirs {
    fn dirs(&self) -> Dirs<'_> {
        Dirs {
            workflows: &self.workflows,
            plugins: &self.plugins,
        }
    }
}

/// What the store remembers about one install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    /// The gallery id.
    pub id: String,
    pub kind: ItemKind,
    /// The folder below the workflows or plugins folder; for a theme, its file
    /// relative to the config folder (`themes/Nord.toml`).
    pub folder: String,
    pub name: String,
    pub version: String,
    pub author: String,
    /// Seconds since the Unix epoch.
    pub installed_at: u64,
    /// The address the package was downloaded from.
    pub source: String,
    /// The SHA-256 of what was downloaded.
    pub sha256: String,
    /// A native extension: the SHA-256 of the program that was installed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program_sha256: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Receipts {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    items: Vec<Receipt>,
}

/// An index parsed, with what was left out.
#[derive(Debug, Clone)]
pub struct Catalog {
    pub entries: Vec<Entry>,
    pub themes: Vec<ThemeEntry>,
    /// Where the package index was read from (shown, so the request is no secret).
    pub source: String,
    /// The release the lists belong to.
    pub tag: String,
    /// Set when the lists are not from this build's own release, and why.
    pub note: Option<String>,
    /// Why the theme list is missing, when it could not be read.
    pub themes_error: Option<String>,
    /// Package entries left out (unknown kind, invalid).
    pub skipped: usize,
    /// When it was loaded, seconds since the Unix epoch.
    pub fetched_at: u64,
    /// Read from the local cache, not just now from the network.
    pub from_cache: bool,
    /// What the package list's release signature check found.
    pub index_signature: IndexSignature,
}

/// The cache file: the raw lists as fetched, with their release signatures,
/// checked and parsed again on load so the same rules apply.
#[derive(Debug, Serialize, Deserialize)]
struct CacheFile {
    version: u32,
    fetched_at: u64,
    tag: String,
    note: Option<String>,
    packages: String,
    themes: Option<String>,
    themes_error: Option<String>,
    /// The `.sig` text of `packages` (absent in caches of older builds, and
    /// for releases published before lists were signed).
    #[serde(default)]
    packages_signature: Option<String>,
    #[serde(default)]
    themes_signature: Option<String>,
}

/// Whether an item can be installed, is installed, or has a newer version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Available,
    Installed,
    UpdateAvailable,
    /// Not offered to this computer (see [`CatalogItem::unavailable`]).
    Unavailable,
}

/// One row of the catalog as the page and the launcher show it.
#[derive(Debug, Clone, Serialize)]
pub struct CatalogItem {
    pub deprecated: Option<String>,
    pub replaced_by: Option<String>,
    pub revocation: Option<Notice>,
    pub id: String,
    pub kind: ItemKind,
    pub name: String,
    pub description: String,
    pub author: String,
    pub version: String,
    pub tags: Vec<String>,
    pub homepage: Option<String>,
    /// Native: where the source code is.
    pub repository: Option<String>,
    pub license: Option<String>,
    /// Native: what the author says the program does beyond answering queries.
    /// Not enforced.
    pub permissions: Vec<String>,
    pub min_sevak: Option<String>,
    /// Where the file for this computer is downloaded from (a package, or a
    /// theme file) and its SHA-256.
    pub source: Option<String>,
    pub sha256: Option<String>,
    /// Native: the platforms it has a build for.
    pub platforms: Vec<String>,
    /// Themes: `light` or `dark`.
    pub theme_mode: Option<String>,
    pub state: State,
    pub installed_version: Option<String>,
    /// Why it cannot be installed here.
    pub unavailable: Option<String>,
    /// The store can remove it (it has a receipt).
    pub removable: bool,
}

/// What the page needs from one load.
#[derive(Debug, Clone, Serialize)]
pub struct CatalogView {
    pub source: String,
    pub note: Option<String>,
    pub themes_error: Option<String>,
    pub skipped: usize,
    pub fetched_at: u64,
    pub from_cache: bool,
    /// `verified`, or `legacy_unsigned` for a release from before lists were signed.
    pub index_signature: IndexSignature,
    pub items: Vec<CatalogItem>,
}

/// An installed item.
#[derive(Debug, Clone, Serialize)]
pub struct InstalledItem {
    pub deprecated: Option<String>,
    pub replaced_by: Option<String>,
    pub revocation: Option<Notice>,
    pub id: String,
    pub kind: ItemKind,
    pub name: String,
    pub folder: String,
    pub version: String,
    pub author: String,
    pub installed_at: u64,
    /// A newer version in the catalog, if one is loaded.
    pub update_to: Option<String>,
    pub program_sha256: Option<String>,
}

/// A copy on disk without a receipt that a `"*"` notice revokes
/// ([`ExtensionStore::unrecorded_revoked`]). The store cannot remove it; the
/// page shows where it is so the user can.
#[derive(Debug, Clone, Serialize)]
pub struct UnrecordedCopy {
    pub id: String,
    pub kind: ItemKind,
    pub name: String,
    /// The folder (or theme file) on disk.
    pub path: String,
    pub revocation: Notice,
}

/// What an install, update or removal did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Outcome {
    pub id: String,
    pub kind: ItemKind,
    pub name: String,
    pub version: String,
    pub folder: String,
    /// An update replaced an older install.
    pub replaced: bool,
}

/// The network the store reads through: [`Https`] for real, a fake in tests.
pub type SharedTransport = Arc<dyn Transport + Send + Sync>;

/// Called with the id of a plugin about to be replaced or removed
/// (`script:<folder>`), so the caller can stop its program first.
pub type Quiesce = Arc<dyn Fn(&str) + Send + Sync>;

/// The store. One per app; cheap to share behind an `Arc`.
pub struct ExtensionStore {
    dirs: StoreDirs,
    transport: SharedTransport,
    build: Option<Pin>,
    platform: String,
    sevak_version: String,
    catalog: Mutex<Option<Arc<Catalog>>>,
    advisories: Mutex<Advisories>,
    /// Held while an install, update or removal runs.
    busy: Mutex<()>,
    quiesce: Mutex<Option<Quiesce>>,
}

/// A downloaded `.sig` as the text saved in the cache.
fn signature_text(
    signature: Option<Vec<u8>>,
) -> Result<Option<String>, std::string::FromUtf8Error> {
    signature.map(String::from_utf8).transpose()
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl ExtensionStore {
    /// The store for this build of Sevak, over the real network.
    pub fn new(dirs: StoreDirs) -> Self {
        Self::with(
            dirs,
            Arc::new(Https),
            Pin::for_build(),
            current_platform(),
            env!("CARGO_PKG_VERSION").to_owned(),
        )
    }

    /// A store over `transport`, for a build that is `build`'s release, running
    /// on `platform` as Sevak `sevak_version`. Tests use this.
    pub fn with(
        dirs: StoreDirs,
        transport: SharedTransport,
        build: Option<Pin>,
        platform: String,
        sevak_version: String,
    ) -> Self {
        Self {
            dirs,
            transport,
            build,
            platform,
            sevak_version,
            catalog: Mutex::new(None),
            advisories: Mutex::new(Advisories::default()),
            busy: Mutex::new(()),
            quiesce: Mutex::new(None),
        }
    }

    /// Sets what runs before a plugin's folder is replaced or removed.
    pub fn set_quiesce(&self, quiesce: Quiesce) {
        *self.quiesce.lock().unwrap_or_else(|p| p.into_inner()) = Some(quiesce);
    }

    fn quiesce(&self, plugin_id: &str) {
        let hook = self
            .quiesce
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let Some(hook) = hook {
            hook(plugin_id);
        }
    }

    pub fn dirs(&self) -> &StoreDirs {
        &self.dirs
    }

    pub fn advisory_status(&self) -> revocations::Status {
        let mut state = self.advisories.lock().unwrap_or_else(|p| p.into_inner());
        state.load_cached(&self.dirs.state);
        state.status(now())
    }

    /// Themes pass [`Subject::Theme`] and an empty version.
    pub fn revocation(&self, kind: Subject, id: &str, version: &str) -> Option<Notice> {
        let mut state = self.advisories.lock().unwrap_or_else(|p| p.into_inner());
        state.load_cached(&self.dirs.state);
        state.notice(kind, id, version)
    }

    /// A notice for `id` naming any version, for a copy of unknown version.
    pub fn any_revocation(&self, kind: Subject, id: &str) -> Option<Notice> {
        let mut state = self.advisories.lock().unwrap_or_else(|p| p.into_inner());
        state.load_cached(&self.dirs.state);
        state.any_notice(kind, id)
    }

    /// Only call when the user explicitly loads or refreshes a gallery page.
    ///
    /// The two downloads run without the advisory lock, so lookups (the pages'
    /// lists) never wait on the network. Verification, the rollback check
    /// against the saved copy and the swap then happen under the lock, which
    /// also serializes them with [`Self::commit_if_not_revoked`].
    pub fn refresh_advisories(&self) {
        let fetched = Advisories::fetch(&*self.transport);
        self.advisories
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .apply(fetched, &self.dirs.state);
    }

    pub fn check_revocation(&self, kind: Subject, id: &str, version: &str) -> Result<(), String> {
        if let Some(notice) = self.revocation(kind, id, version) {
            Err(format!(
                "This version has been revoked: {}. Remove any installed copy from Extensions.",
                notice.reason
            ))
        } else {
            Ok(())
        }
    }

    /// Recheck after download, then serialize the local commit with advisory
    /// replacement. The callback must do no network work or advisory lookups.
    pub fn commit_if_not_revoked<T>(
        &self,
        kind: Subject,
        id: &str,
        version: &str,
        commit: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let mut state = self.advisories.lock().unwrap_or_else(|p| p.into_inner());
        state.load_cached(&self.dirs.state);
        if let Some(notice) = state.notice(kind, id, version) {
            return Err(format!("This version has been revoked: {}", notice.reason));
        }
        let result = commit();
        drop(state);
        result
    }

    // ---- the catalog ----------------------------------------------------

    /// Loads the catalog from the network (the user asked) and keeps it, in
    /// memory and in the cache file. Blocking.
    pub fn refresh(&self) -> Result<CatalogView, String> {
        self.refresh_advisories();
        let pinned = fetch_pinned(
            &*self.transport,
            self.build.as_ref(),
            gallery::INDEX_FILE,
            gallery::MAX_INDEX_BYTES,
        )?;
        let packages = String::from_utf8(pinned.body)
            .map_err(|_| "Could not load the gallery: the list is not text.".to_owned())?;
        let packages_signature = signature_text(pinned.signature)
            .map_err(|_| "Could not load the gallery: its signature is not text.".to_owned())?;
        // The theme list is read from the same release and checked against its
        // own signature; failing either does not take the packages away.
        let (themes, themes_signature, themes_error) = match index_signature::fetch_checked(
            &*self.transport,
            &pinned.pin,
            theme_store::GALLERY_INDEX_FILE,
            theme_store::MAX_INDEX_BYTES as usize,
        ) {
            Ok(checked) => match (
                String::from_utf8(checked.body),
                signature_text(checked.signature),
            ) {
                (Ok(text), Ok(signature)) => (Some(text), signature, None),
                _ => (None, None, Some("the theme list is not text".to_owned())),
            },
            Err(err) => (None, None, Some(err)),
        };
        let cache = CacheFile {
            version: 1,
            fetched_at: now(),
            tag: pinned.pin.tag().to_owned(),
            note: pinned.note,
            packages,
            themes,
            themes_error,
            packages_signature,
            themes_signature,
        };
        let catalog = Self::parse_cache(&cache, false)?;
        // A cache that cannot be written only means the next start begins empty.
        if let Err(err) = self.write_cache(&cache) {
            tracing::warn!("extensions: could not save the catalog cache: {err}");
        }
        let view = self.view(&catalog);
        *self.catalog.lock().unwrap_or_else(|p| p.into_inner()) = Some(Arc::new(catalog));
        Ok(view)
    }

    /// Loads the catalog saved by an earlier [`ExtensionStore::refresh`], if
    /// there is one. Local only; never touches the network.
    pub fn load_cached(&self) -> Option<CatalogView> {
        let text =
            read_to_string_capped(&self.dirs.state.join(CACHE_FILE), MAX_STATE_BYTES).ok()?;
        let cache: CacheFile = serde_json::from_str(&text).ok()?;
        if cache.version != 1 {
            return None;
        }
        let catalog = Self::parse_cache(&cache, true).ok()?;
        let view = self.view(&catalog);
        *self.catalog.lock().unwrap_or_else(|p| p.into_inner()) = Some(Arc::new(catalog));
        Some(view)
    }

    /// The catalog in memory (loaded or cached), if any.
    pub fn current(&self) -> Option<Arc<Catalog>> {
        self.catalog
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// The catalog in memory as the page shows it, with the installed state
    /// worked out again (an install since changes it).
    pub fn current_view(&self) -> Option<CatalogView> {
        self.current().map(|catalog| self.view(&catalog))
    }

    fn parse_cache(cache: &CacheFile, from_cache: bool) -> Result<Catalog, String> {
        let pin = Pin::new(&cache.tag)
            .ok_or_else(|| "the saved gallery list names no valid release".to_owned())?;
        // The same signature check as a download, before parsing: a saved list
        // is only as trusted as the release signature saved with it.
        let index_signature = index_signature::verify(
            pin.tag(),
            gallery::INDEX_FILE,
            cache.packages.as_bytes(),
            cache.packages_signature.as_deref().map(str::as_bytes),
        )
        .map_err(|err| format!("Could not load the gallery: {err}."))?;
        let index = gallery::parse_index(&cache.packages, &pin)?;
        let (themes, themes_error) = match &cache.themes {
            Some(text) => match index_signature::verify(
                pin.tag(),
                theme_store::GALLERY_INDEX_FILE,
                text.as_bytes(),
                cache.themes_signature.as_deref().map(str::as_bytes),
            )
            .and_then(|_| theme_store::parse_index(text, &pin))
            {
                Ok(themes) => (themes, None),
                Err(err) => (Vec::new(), Some(err)),
            },
            None => (Vec::new(), cache.themes_error.clone()),
        };
        for skipped in &index.skipped {
            tracing::warn!("extensions: skipped a gallery entry: {skipped}");
        }
        Ok(Catalog {
            skipped: index.skipped.len(),
            entries: index.entries,
            themes,
            source: index.source,
            tag: cache.tag.clone(),
            note: cache.note.clone(),
            themes_error,
            fetched_at: cache.fetched_at,
            from_cache,
            index_signature,
        })
    }

    fn write_cache(&self, cache: &CacheFile) -> std::io::Result<()> {
        let text = serde_json::to_string(cache).map_err(std::io::Error::other)?;
        sevak_platform::private_file::write_atomic(
            &self.dirs.state.join(CACHE_FILE),
            text.as_bytes(),
        )
    }

    // ---- what is installed ------------------------------------------------

    fn receipts_path(&self) -> PathBuf {
        self.dirs.state.join(RECEIPTS_FILE)
    }

    fn read_receipts(&self) -> Receipts {
        #[cfg(test)]
        tests::RECEIPT_READS.with(|reads| reads.set(reads.get() + 1));
        let path = self.receipts_path();
        match read_to_string_capped(&path, MAX_STATE_BYTES) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|err| {
                tracing::warn!(path = %path.display(), %err, "the extension receipts are unreadable");
                Receipts::default()
            }),
            Err(_) => Receipts::default(),
        }
    }

    fn write_receipts(&self, receipts: &Receipts) -> Result<(), String> {
        let text = serde_json::to_string_pretty(receipts).map_err(|err| err.to_string())?;
        sevak_platform::private_file::write_atomic(&self.receipts_path(), text.as_bytes())
            .map_err(|err| format!("could not save what was installed: {err}"))
    }

    fn target_of(&self, receipt: &Receipt) -> Option<PathBuf> {
        match receipt.kind {
            ItemKind::Workflow => valid_folder_name(&receipt.folder)
                .then(|| self.dirs.workflows.join(&receipt.folder)),
            ItemKind::Plugin | ItemKind::Native => {
                valid_folder_name(&receipt.folder).then(|| self.dirs.plugins.join(&receipt.folder))
            }
            ItemKind::Theme => theme_path(&self.dirs.config_dir, &receipt.folder),
        }
    }

    /// The installs whose files are still there, newest first. A receipt whose
    /// folder was deleted by hand is not listed (and is dropped on the next
    /// change).
    pub fn installed(&self) -> Vec<InstalledItem> {
        let catalog = self.current();
        let receipts = self.read_receipts().items;
        let mut items: Vec<InstalledItem> = receipts
            .clone()
            .into_iter()
            .filter(|receipt| self.target_of(receipt).is_some_and(|path| path.exists()))
            .map(|receipt| {
                let update_to = catalog.as_ref().and_then(|catalog| {
                    catalog
                        .entries
                        .iter()
                        .find(|entry| entry.id == receipt.id)
                        .filter(|entry| {
                            ItemKind::from_kind(entry.kind) == receipt.kind
                                && is_newer(&entry.version, &receipt.version)
                                && self.available(entry, &receipts).is_ok()
                        })
                        .map(|entry| entry.version.clone())
                });
                InstalledItem {
                    deprecated: catalog
                        .as_ref()
                        .and_then(|c| c.entries.iter().find(|e| e.id == receipt.id))
                        .and_then(|e| e.deprecated.clone()),
                    replaced_by: catalog
                        .as_ref()
                        .and_then(|c| c.entries.iter().find(|e| e.id == receipt.id))
                        .and_then(|e| e.replaced_by.clone()),
                    revocation: self.revocation(
                        if receipt.kind == ItemKind::Theme {
                            Subject::Theme
                        } else {
                            Subject::Package
                        },
                        &receipt.id,
                        &receipt.version,
                    ),
                    id: receipt.id,
                    kind: receipt.kind,
                    name: receipt.name,
                    folder: receipt.folder,
                    version: receipt.version,
                    author: receipt.author,
                    installed_at: receipt.installed_at,
                    update_to,
                    program_sha256: receipt.program_sha256,
                }
            })
            .collect();
        items.sort_by(|a, b| b.installed_at.cmp(&a.installed_at).then(a.id.cmp(&b.id)));
        items
    }

    fn receipt(&self, id: &str) -> Option<Receipt> {
        self.read_receipts()
            .items
            .into_iter()
            .find(|receipt| receipt.id == id)
    }

    /// Copies on disk that Sevak has no receipt for (a legacy Gallery install,
    /// a theme installed before Appearance kept receipts, a folder copied by
    /// hand) and that a signed notice revokes in every version (`"*"`). Their
    /// version is unknown, so only `"*"` notices are certain to cover them.
    /// Packages are found by the catalog's folder name, or a folder named as
    /// the id; themes only through the loaded catalog (it maps id to file).
    pub fn unrecorded_revoked(&self) -> Vec<UnrecordedCopy> {
        let notices = {
            let mut state = self.advisories.lock().unwrap_or_else(|p| p.into_inner());
            state.load_cached(&self.dirs.state);
            state.all_version_notices()
        };
        if notices.is_empty() {
            return Vec::new();
        }
        let receipts = self.read_receipts().items;
        let recorded = |id: &str, kind: Subject| {
            receipts.iter().any(|r| {
                r.id == id
                    && (r.kind == ItemKind::Theme) == (kind == Subject::Theme)
                    && self.target_of(r).is_some_and(|p| p.exists())
            })
        };
        let catalog = self.current();
        let mut found = Vec::new();
        for notice in notices {
            if recorded(&notice.id, notice.kind) {
                continue;
            }
            match notice.kind {
                Subject::Package => {
                    let listed = catalog
                        .as_ref()
                        .and_then(|c| c.entries.iter().find(|e| e.id == notice.id));
                    let candidates: Vec<(ItemKind, String, String)> = match listed {
                        Some(entry) => vec![(
                            ItemKind::from_kind(entry.kind),
                            entry.folder_name().to_owned(),
                            entry.name.clone(),
                        )],
                        None => [ItemKind::Workflow, ItemKind::Plugin]
                            .into_iter()
                            .map(|kind| (kind, notice.id.clone(), notice.id.clone()))
                            .collect(),
                    };
                    for (kind, folder, name) in candidates {
                        if !valid_folder_name(&folder) {
                            continue;
                        }
                        let (root, manifest) = match kind {
                            ItemKind::Workflow => (&self.dirs.workflows, Kind::Workflow),
                            _ => (&self.dirs.plugins, Kind::Plugin),
                        };
                        let path = root.join(&folder);
                        if path.join(manifest.manifest_file()).is_file() {
                            found.push(UnrecordedCopy {
                                id: notice.id.clone(),
                                kind,
                                name,
                                path: path.display().to_string(),
                                revocation: notice.clone(),
                            });
                        }
                    }
                }
                Subject::Theme => {
                    let Some(theme) = catalog
                        .as_ref()
                        .and_then(|c| c.themes.iter().find(|t| t.id == notice.id))
                    else {
                        continue;
                    };
                    let path = self
                        .dirs
                        .config_dir
                        .join(theme_store::file_for(&theme.name));
                    if path.is_file() {
                        found.push(UnrecordedCopy {
                            id: notice.id.clone(),
                            kind: ItemKind::Theme,
                            name: theme.name.clone(),
                            path: path.display().to_string(),
                            revocation: notice,
                        });
                    }
                }
            }
        }
        found
    }

    // ---- the page's view ----------------------------------------------------

    fn view(&self, catalog: &Catalog) -> CatalogView {
        let receipts = self.read_receipts().items;
        let mut items: Vec<CatalogItem> = Vec::new();
        for entry in &catalog.entries {
            items.push(self.package_item(entry, &receipts));
        }
        for theme in &catalog.themes {
            items.push(self.theme_item(theme, &receipts));
        }
        items.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then(a.id.cmp(&b.id))
        });
        CatalogView {
            source: catalog.source.clone(),
            note: catalog.note.clone(),
            themes_error: catalog.themes_error.clone(),
            skipped: catalog.skipped,
            fetched_at: catalog.fetched_at,
            from_cache: catalog.from_cache,
            index_signature: catalog.index_signature,
            items,
        }
    }

    /// Whether this computer can install `entry`: the Sevak version it needs
    /// and, for a native extension, a build for this platform.
    /// `receipts` is what [`Self::read_receipts`] returned, read once by the
    /// caller rather than once per entry.
    fn available(&self, entry: &Entry, receipts: &[Receipt]) -> Result<(), String> {
        self.check_revocation(Subject::Package, &entry.id, &entry.version)?;
        if let Some(reason) = &entry.deprecated {
            if !receipts
                .iter()
                .find(|r| r.id == entry.id)
                .is_some_and(|r| self.target_of(r).is_some_and(|p| p.exists()))
            {
                return Err(format!(
                    "Deprecated; new installs are unavailable: {reason}"
                ));
            }
        }
        if let Some(min) = &entry.min_sevak {
            if let (Ok(min), Ok(running)) = (
                semver::Version::parse(min),
                semver::Version::parse(&self.sevak_version),
            ) {
                let core = semver::Version::new(running.major, running.minor, running.patch);
                if core < min {
                    return Err(format!(
                        "Needs Sevak {min} or newer; this is Sevak {}.",
                        self.sevak_version
                    ));
                }
            }
        }
        if entry.kind == Kind::Native && !entry.platforms.contains_key(&self.platform) {
            let offered: Vec<&str> = entry.platforms.keys().map(String::as_str).collect();
            return Err(format!(
                "No build for this computer ({}). It offers {}.",
                self.platform,
                offered.join(", ")
            ));
        }
        Ok(())
    }

    fn package_item(&self, entry: &Entry, receipts: &[Receipt]) -> CatalogItem {
        let kind = ItemKind::from_kind(entry.kind);
        let receipt = receipts.iter().find(|r| r.id == entry.id && r.kind == kind);
        let on_disk = receipt.filter(|r| self.target_of(r).is_some_and(|path| path.exists()));
        let unavailable = self.available(entry, receipts).err();
        let (state, installed_version) = match (on_disk, &unavailable) {
            (Some(r), _) if is_newer(&entry.version, &r.version) && unavailable.is_none() => {
                (State::UpdateAvailable, Some(r.version.clone()))
            }
            (Some(r), _) => (State::Installed, Some(r.version.clone())),
            (None, Some(_)) => (State::Unavailable, None),
            (None, None) => (
                // A folder of that name that this store did not make is not
                // ours to touch; the install refuses it with a message.
                State::Available,
                None,
            ),
        };
        let artifact = (entry.kind == Kind::Native)
            .then(|| entry.platforms.get(&self.platform))
            .flatten();
        CatalogItem {
            deprecated: entry.deprecated.clone(),
            replaced_by: entry.replaced_by.clone(),
            revocation: self.revocation(Subject::Package, &entry.id, &entry.version),
            id: entry.id.clone(),
            kind,
            name: entry.name.clone(),
            description: entry.description.clone(),
            author: entry.author.clone(),
            version: entry.version.clone(),
            tags: entry.tags.clone(),
            homepage: entry.homepage.clone(),
            repository: entry.repository.clone(),
            license: entry.license.clone(),
            permissions: entry.permissions.clone(),
            min_sevak: entry.min_sevak.clone(),
            source: match artifact {
                Some(artifact) => Some(artifact.source.clone()),
                None if entry.kind != Kind::Native => Some(entry.source.clone()),
                None => None,
            },
            sha256: match artifact {
                Some(artifact) => Some(artifact.sha256.clone()),
                None if entry.kind != Kind::Native => Some(entry.sha256.clone()),
                None => None,
            },
            platforms: entry.platforms.keys().cloned().collect(),
            theme_mode: None,
            state,
            installed_version,
            unavailable,
            removable: on_disk.is_some(),
        }
    }

    fn theme_item(&self, theme: &ThemeEntry, receipts: &[Receipt]) -> CatalogItem {
        let receipt = receipts
            .iter()
            .find(|r| r.id == theme.id && r.kind == ItemKind::Theme)
            .filter(|r| self.target_of(r).is_some_and(|path| path.exists()));
        // A theme of the same name that the user made or installed another way
        // also counts as installed (it would not be replaced).
        let present = self
            .dirs
            .config_dir
            .join(theme_store::file_for(&theme.name))
            .is_file();
        let revocation = self.revocation(Subject::Theme, &theme.id, "");
        CatalogItem {
            deprecated: None,
            replaced_by: None,
            revocation: revocation.clone(),
            id: theme.id.clone(),
            kind: ItemKind::Theme,
            name: theme.name.clone(),
            description: theme.description.clone(),
            author: theme.author.clone(),
            version: String::new(),
            tags: vec![theme.mode.clone()],
            homepage: None,
            repository: None,
            license: None,
            permissions: Vec::new(),
            min_sevak: None,
            source: Some(theme.url.clone()),
            sha256: Some(theme.sha256.clone()),
            platforms: Vec::new(),
            theme_mode: Some(theme.mode.clone()),
            state: if receipt.is_some() || present {
                State::Installed
            } else if revocation.is_some() {
                State::Unavailable
            } else {
                State::Available
            },
            installed_version: None,
            unavailable: revocation.map(|notice| format!("Revoked: {}", notice.reason)),
            removable: receipt.is_some(),
        }
    }

    // ---- install, update, uninstall -----------------------------------------

    /// Installs catalog item `id`. The item must be in the catalog that was
    /// loaded; nothing else can be named.
    pub fn install(&self, id: &str) -> Result<Outcome, String> {
        self.apply(id, false)
    }

    /// Replaces the installed version of `id` with the catalog's newer one, as
    /// one step: either the new version is in place, or the old one is.
    pub fn update(&self, id: &str) -> Result<Outcome, String> {
        self.apply(id, true)
    }

    /// Legacy Gallery's already-validated, pinned entry. Use the same checked
    /// install and receipt path so new Gallery installs can be uninstalled.
    pub fn install_gallery_entry(&self, entry: &Entry) -> Result<Outcome, String> {
        let _busy = self.busy.try_lock().map_err(|_| {
            "Another extension change is still running. Try again in a moment.".to_owned()
        })?;
        if entry.kind == Kind::Native {
            return Err(
                "Native extensions must be reviewed in Extensions before installing.".into(),
            );
        }
        self.available(entry, &self.read_receipts().items)?;
        self.install_package(entry, false)
    }

    fn apply(&self, id: &str, update: bool) -> Result<Outcome, String> {
        let _busy = self.busy.try_lock().map_err(|_| {
            "Another extension change is still running. Try again in a moment.".to_owned()
        })?;
        let catalog = self
            .current()
            .ok_or_else(|| "Load the extension list first.".to_owned())?;
        self.cleanup_leftovers();
        let receipts = self.read_receipts().items;
        let existing = receipts.iter().find(|receipt| receipt.id == id).cloned();
        let on_disk = existing
            .as_ref()
            .is_some_and(|r| self.target_of(r).is_some_and(|path| path.exists()));

        if let Some(entry) = catalog.entries.iter().find(|entry| entry.id == id) {
            let kind = ItemKind::from_kind(entry.kind);
            match (update, on_disk) {
                (false, true) => {
                    return Err(format!("{} is already installed. Use Update.", entry.name))
                }
                (true, false) => return Err(format!("{} is not installed.", entry.name)),
                (true, true) => {
                    let receipt = existing.as_ref().expect("on_disk implies a receipt");
                    if receipt.kind != kind {
                        return Err(format!(
                            "{} was installed as something else; remove it first.",
                            entry.name
                        ));
                    }
                    if !is_newer(&entry.version, &receipt.version) {
                        return Err(format!("{} is already up to date.", entry.name));
                    }
                }
                (false, false) => {}
            }
            self.available(entry, &receipts)?;
            let outcome = match entry.kind {
                Kind::Native => self.install_native(entry, update)?,
                Kind::Workflow | Kind::Plugin => self.install_package(entry, update)?,
            };
            return Ok(outcome);
        }
        if let Some(theme) = catalog.themes.iter().find(|theme| theme.id == id) {
            self.check_revocation(Subject::Theme, &theme.id, "")?;
            if update {
                return Err(
                    "Themes have no updates; install it again after removing it.".to_owned(),
                );
            }
            if on_disk {
                return Err(format!("{} is already installed.", theme.name));
            }
            return self.install_theme(theme);
        }
        Err("That item is not in the loaded list. Load the list again.".to_owned())
    }

    /// Removes leftovers of an install or update that was cut short.
    fn cleanup_leftovers(&self) {
        for root in [&self.dirs.workflows, &self.dirs.plugins] {
            let Ok(entries) = fs::read_dir(root) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with(".installing-") || name.starts_with(".replaced-") {
                    let _ = fs::remove_dir_all(entry.path());
                }
            }
        }
    }

    fn download(&self, url: &str, limit: usize) -> Result<Vec<u8>, String> {
        self.transport
            .get(url, limit)
            .map_err(|err| format!("Could not download it: {err}."))
    }

    fn install_package(&self, entry: &Entry, update: bool) -> Result<Outcome, String> {
        let bytes = self.download(&entry.source, gallery::MAX_PACKAGE_BYTES)?;
        let mode = if update { Mode::Replace } else { Mode::New };
        let running_id = match entry.kind {
            Kind::Workflow => format!("workflow:{}", entry.folder_name()),
            _ => format!("{}{}", crate::script::ID_PREFIX, entry.folder_name()),
        };
        self.commit_if_not_revoked(Subject::Package, &entry.id, &entry.version, || {
            let installed = install_bytes_as(entry, &bytes, &self.dirs.dirs(), mode, &|| {
                self.quiesce(&running_id);
            })?;
            let receipt = Receipt {
                id: entry.id.clone(),
                kind: ItemKind::from_kind(entry.kind),
                folder: installed.folder.clone(),
                name: entry.name.clone(),
                version: entry.version.clone(),
                author: entry.author.clone(),
                installed_at: now(),
                source: entry.source.clone(),
                sha256: entry.sha256.clone(),
                program_sha256: None,
            };
            self.record(receipt)?;
            Ok(Outcome {
                id: entry.id.clone(),
                kind: ItemKind::from_kind(entry.kind),
                name: entry.name.clone(),
                version: entry.version.clone(),
                folder: installed.folder,
                replaced: update,
            })
        })
    }

    fn install_native(&self, entry: &Entry, update: bool) -> Result<Outcome, String> {
        let artifact = entry
            .platforms
            .get(&self.platform)
            .ok_or_else(|| format!("There is no build of {} for {}.", entry.name, self.platform))?;
        let bytes = self.download(&artifact.source, package::MAX_PACKAGE_BYTES)?;
        verify_sha256(&bytes, &artifact.sha256)?;
        let folder = entry.folder_name();
        if !valid_folder_name(folder) || folder != entry.id {
            return Err("The folder name is not valid.".to_owned());
        }
        let package = ExtensionPackage::read(&bytes, folder)?;
        check_entry_matches(entry, &package)?;
        let files = package.files_for(&self.platform)?;
        let program_sha256 = package
            .binary(&self.platform)
            .map(|file| sha256_hex(&file.data));

        self.commit_if_not_revoked(Subject::Package, &entry.id, &entry.version, || {
            let root = &self.dirs.plugins;
            let target = root.join(folder);
            if target.exists() && !update {
                return Err(format!(
                    "\"{folder}\" is already in your plugins folder, so it was not replaced. \
                 Remove that folder first: {}",
                    target.display()
                ));
            }
            fs::create_dir_all(root)
                .map_err(|err| format!("Could not create the folder: {err}"))?;
            let staging = root.join(format!(".installing-{folder}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&staging);
            let plugin_id = format!("{}{folder}", crate::script::ID_PREFIX);
            let result = write_files(&files, &staging).and_then(|()| {
                place(&staging, &target, update && target.exists(), &|| {
                    self.quiesce(&plugin_id);
                })
            });
            if let Err(err) = result {
                let _ = fs::remove_dir_all(&staging);
                return Err(err);
            }
            self.record(Receipt {
                id: entry.id.clone(),
                kind: ItemKind::Native,
                folder: folder.to_owned(),
                name: entry.name.clone(),
                version: entry.version.clone(),
                author: entry.author.clone(),
                installed_at: now(),
                source: artifact.source.clone(),
                sha256: artifact.sha256.clone(),
                program_sha256,
            })?;
            Ok(Outcome {
                id: entry.id.clone(),
                kind: ItemKind::Native,
                name: entry.name.clone(),
                version: entry.version.clone(),
                folder: folder.to_owned(),
                replaced: update,
            })
        })
    }

    fn install_theme(&self, theme: &ThemeEntry) -> Result<Outcome, String> {
        let bytes = self.download(&theme.url, theme_store::MAX_THEME_BYTES as usize)?;
        let text = theme_store::verify_download(&bytes, &theme.sha256)?;
        let stored = self.commit_theme(theme, &text, &Existing::Refuse)?;
        Ok(Outcome {
            id: theme.id.clone(),
            kind: ItemKind::Theme,
            name: theme.name.clone(),
            version: String::new(),
            folder: stored.file,
            replaced: false,
        })
    }

    /// Appearance's gallery install: the already verified `text` of `theme`
    /// is written under the same final revocation check as an install from
    /// Extensions, and gets the same receipt, so a later notice reaches it and
    /// Extensions can remove it.
    pub fn install_gallery_theme_text(
        &self,
        theme: &ThemeEntry,
        text: &str,
        existing: &Existing,
    ) -> Result<theme_store::StoredTheme, String> {
        let _busy = self.busy.try_lock().map_err(|_| {
            "Another extension change is still running. Try again in a moment.".to_owned()
        })?;
        self.commit_theme(theme, text, existing)
    }

    fn commit_theme(
        &self,
        theme: &ThemeEntry,
        text: &str,
        existing: &Existing,
    ) -> Result<theme_store::StoredTheme, String> {
        self.commit_if_not_revoked(Subject::Theme, &theme.id, "", || {
            let stored =
                theme_store::install_text(&self.dirs.config_dir, text, &theme.name, existing)?;
            self.record(Receipt {
                id: theme.id.clone(),
                kind: ItemKind::Theme,
                folder: stored.file.clone(),
                name: theme.name.clone(),
                version: String::new(),
                author: theme.author.clone(),
                installed_at: now(),
                source: theme.url.clone(),
                sha256: theme.sha256.clone(),
                program_sha256: None,
            })?;
            Ok(stored)
        })
    }

    /// Adds `receipt` (replacing one for the same id) and drops receipts whose
    /// files are gone.
    fn record(&self, receipt: Receipt) -> Result<(), String> {
        let mut receipts = self.read_receipts();
        receipts.version = 1;
        receipts.items.retain(|old| {
            old.id != receipt.id && self.target_of(old).is_some_and(|path| path.exists())
        });
        receipts.items.push(receipt);
        self.write_receipts(&receipts)
    }

    /// Removes what the store installed for `id`: the folder (or theme file),
    /// the extension's own data folder, its recorded approval and its receipt.
    /// Only things with a receipt can be removed here.
    pub fn uninstall(&self, id: &str) -> Result<Outcome, String> {
        let _busy = self.busy.try_lock().map_err(|_| {
            "Another extension change is still running. Try again in a moment.".to_owned()
        })?;
        let receipt = self
            .receipt(id)
            .ok_or_else(|| "Sevak did not install that, so it is not removed here.".to_owned())?;
        let target = self.target_of(&receipt).ok_or_else(|| {
            "The recorded folder is not valid, so nothing was removed.".to_owned()
        })?;
        let approvals = ApprovalStore::new(self.dirs.approvals.clone());
        match receipt.kind {
            ItemKind::Theme => {
                if target.exists() {
                    fs::remove_file(&target)
                        .map_err(|err| format!("Could not remove the theme: {err}"))?;
                }
            }
            ItemKind::Workflow | ItemKind::Plugin | ItemKind::Native => {
                let (manifest, data_root, approval_id) = match receipt.kind {
                    ItemKind::Workflow => (
                        "workflow.toml",
                        &self.dirs.workflow_data,
                        format!("workflow:{}", receipt.folder),
                    ),
                    _ => (
                        "plugin.toml",
                        &self.dirs.plugin_data,
                        format!("{}{}", crate::script::ID_PREFIX, receipt.folder),
                    ),
                };
                if target.exists() {
                    // Only something that is still one of ours: a folder that was
                    // turned into something else is left alone.
                    if !target.join(manifest).is_file() {
                        return Err(format!(
                            "{} no longer looks like what was installed, so it was not removed.",
                            target.display()
                        ));
                    }
                    self.quiesce(&approval_id);
                    retry(|| fs::remove_dir_all(&target)).map_err(|err| {
                        format!(
                            "Could not remove {} (is it running?): {err}",
                            target.display()
                        )
                    })?;
                }
                let data = data_root.join(&receipt.folder);
                if data.exists() {
                    let _ = retry(|| fs::remove_dir_all(&data));
                }
                if let Err(err) = approvals.forget(&approval_id) {
                    tracing::warn!("extensions: could not drop the recorded approval: {err}");
                }
            }
        }
        let mut receipts = self.read_receipts();
        receipts.version = 1;
        receipts
            .items
            .retain(|old| old.id != receipt.id && self.target_of(old).is_some_and(|p| p.exists()));
        self.write_receipts(&receipts)?;
        Ok(Outcome {
            id: receipt.id,
            kind: receipt.kind,
            name: receipt.name,
            version: receipt.version,
            folder: receipt.folder,
            replaced: false,
        })
    }
}

/// The path of an installed theme file: exactly `themes/<name>.toml` below the
/// config folder, nothing that leaves it.
fn theme_path(config_dir: &Path, file: &str) -> Option<PathBuf> {
    let name = file.strip_prefix("themes/")?;
    let plain = !name.is_empty()
        && !name.contains(['/', '\\'])
        && !name.starts_with('.')
        && name.to_ascii_lowercase().ends_with(".toml")
        && !name.chars().any(char::is_control);
    plain.then(|| config_dir.join("themes").join(name))
}

/// The package's own manifest must say what the index said: the page shows the
/// entry and the Allow dialog shows the manifest.
fn check_entry_matches(entry: &Entry, package: &ExtensionPackage) -> Result<(), String> {
    let native = &package.native;
    let mismatch = |what: &str, listed: &str, packaged: &str| {
        Err(format!(
            "The package does not match the list: the list says the {what} is \"{listed}\" but \
             the package says \"{packaged}\". Nothing was installed."
        ))
    };
    if native.version != entry.version {
        return mismatch("version", &entry.version, &native.version);
    }
    if native.author.trim() != entry.author.trim() {
        return mismatch("publisher", &entry.author, &native.author);
    }
    let listed_license = entry.license.as_deref().unwrap_or_default();
    if native.license != listed_license.trim() {
        return mismatch("licence", listed_license, &native.license);
    }
    let listed: HashSet<&str> = entry.permissions.iter().map(String::as_str).collect();
    let packaged: HashSet<&str> = native.permissions.iter().map(String::as_str).collect();
    if listed != packaged {
        return mismatch(
            "permissions",
            &entry.permissions.join(", "),
            &native.permissions.join(", "),
        );
    }
    Ok(())
}

/// Whether `candidate` is a later version than `installed`. Dotted numbers are
/// compared by value (`1.10` is later than `1.9`, `1.0` equals `1.0.0`, an
/// optional leading `v` and a pre-release suffix are ignored); versions that
/// are not numbers count as different, hence newer, only if they differ.
pub fn is_newer(candidate: &str, installed: &str) -> bool {
    fn key(version: &str) -> Option<Vec<u64>> {
        let core = version
            .trim()
            .trim_start_matches('v')
            .split(['-', '+'])
            .next()?;
        let mut parts: Vec<u64> = core
            .split('.')
            .map(|part| part.parse::<u64>().ok())
            .collect::<Option<_>>()?;
        while parts.len() > 1 && parts.last() == Some(&0) {
            parts.pop();
        }
        Some(parts)
    }
    match (key(candidate), key(installed)) {
        (Some(a), Some(b)) => a > b,
        _ => !candidate.trim().is_empty() && candidate.trim() != installed.trim(),
    }
}

#[cfg(test)]
mod tests;
