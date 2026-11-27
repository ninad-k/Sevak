//! The Windows "Recent" folder: one `.lnk` per recently opened file.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::com::ComGuard;
use super::shortcuts::LinkResolver;

/// Most shortcuts resolved, however many files were asked for (folders and
/// vanished files use up some).
const MAX_LINKS: usize = 1_500;

/// The target of each recent `.lnk`, with the shortcut's modified time (when
/// the file was last opened).
pub(crate) fn candidates(limit: usize) -> Vec<(PathBuf, Option<SystemTime>)> {
    let Some(folder) = std::env::var_os("APPDATA")
        .map(|base| PathBuf::from(base).join(r"Microsoft\Windows\Recent"))
    else {
        return Vec::new();
    };
    let _com = ComGuard::new();
    let resolver = match LinkResolver::new() {
        Ok(resolver) => resolver,
        Err(err) => {
            tracing::warn!(%err, "cannot create IShellLink; recent files are unavailable");
            return Vec::new();
        }
    };
    let mut links = links_in(&folder);
    links.sort_by(|a, b| b.1.cmp(&a.1));
    links.truncate(limit.saturating_mul(3).clamp(100, MAX_LINKS));
    links
        .into_iter()
        .filter_map(|(link, modified)| {
            let info = resolver.resolve(&link).ok()?;
            (!info.target.is_empty()).then(|| (PathBuf::from(info.target), modified))
        })
        .collect()
}

fn links_in(folder: &Path) -> Vec<(PathBuf, Option<SystemTime>)> {
    let Ok(read) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    read.flatten()
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"))
        })
        .map(|entry| {
            let modified = entry.metadata().and_then(|m| m.modified()).ok();
            (entry.path(), modified)
        })
        .collect()
}
