//! Usage statistics: how often, how recently and for which queries the user
//! picked each result. The engine turns them into a score boost so the things
//! you actually launch float to the top.
//!
//! Persisted as JSON (`<data dir>/sevak/usage.json`):
//!
//! ```json
//! { "version": 1, "entries": { "app:firefox": { "count": 12, "last_used": 1700000000, "queries": ["fi"] } } }
//! ```
//!
//! The same file also keeps the last [`MAX_HISTORY`] executed queries (as typed)
//! for Up-arrow recall: `"history": ["g rust", "firefox"]`, most recent first.
//!
//! Saving is atomic (write a sibling `.tmp` file, then rename over the target)
//! so a crash mid-write never leaves a truncated file behind.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Current on-disk format version.
pub const FORMAT_VERSION: u32 = 1;

/// How many remembered queries each entry keeps.
pub const MAX_QUERIES_PER_ENTRY: usize = 5;

/// [`UsageStore::record`] prunes the store to this many entries when it grows
/// past it, dropping the least recently used.
pub const MAX_ENTRIES: usize = 2000;

/// How many executed queries are kept for recall.
pub const MAX_HISTORY: usize = 50;

// ---------------------------------------------------------------------------
// Boost tuning
//
// Scores from `nucleo` are roughly 16-20 points per matched character plus
// bonuses for word starts and consecutive runs, so two candidates for the same
// short query typically differ by 10-60 points. The boost has to be able to
// reorder near-ties and moderately different matches, but a perfect match for
// what the user typed must not be buried by a merely popular item.
//
//   * frequency: 25 * ln(1 + n) gives +17 for 1 launch, +60 for 10, +115 for
//     100 launches. The logarithm means the hundredth launch barely matters
//     compared to the tenth, so habits can still shift.
//   * recency: up to +40, halving every 72 h; after a week it is ~ +8. This is
//     what lets this week's project outrank last year's.
//   * query memory: a flat +80 when what you typed is a prefix of a query you
//     previously used to pick this item. That outweighs typical match
//     differences (so "f" opens the item you chose for "f" last time) yet stays
//     far below `score::KEYWORD` (5000), which is never boosted at all.
// ---------------------------------------------------------------------------

/// Multiplier of `ln(1 + count)`.
pub const FREQUENCY_WEIGHT: f64 = 25.0;
/// Maximum recency bonus (an item used right now).
pub const RECENCY_WEIGHT: f64 = 40.0;
/// Hours after which the recency bonus halves.
pub const RECENCY_HALF_LIFE_HOURS: f64 = 72.0;
/// Bonus when the current query is a prefix of a remembered query.
pub const QUERY_MEMORY_BONUS: f64 = 80.0;

#[derive(Debug, Error)]
pub enum UsageError {
    #[error("cannot access usage file {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("usage file {} is not valid: {source}", path.display())]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

/// Statistics for one result id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageEntry {
    /// Times the result was launched.
    pub count: u32,
    /// Unix seconds of the last launch.
    pub last_used: u64,
    /// Queries typed when the result was picked, most recent first
    /// (lowercased, trimmed, at most [`MAX_QUERIES_PER_ENTRY`]).
    pub queries: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UsageStore {
    version: u32,
    entries: BTreeMap<String, UsageEntry>,
    /// Executed queries as typed, most recent first.
    history: Vec<String>,
}

impl Default for UsageStore {
    fn default() -> Self {
        Self {
            version: FORMAT_VERSION,
            entries: BTreeMap::new(),
            history: Vec::new(),
        }
    }
}

/// Lowercase + trim: the form in which queries are stored and compared.
pub(crate) fn normalize_query(query: &str) -> String {
    query.trim().to_lowercase()
}

impl UsageStore {
    /// Reads the store from `path`; a missing file yields an empty store.
    pub fn load(path: &Path) -> Result<Self, UsageError> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(source) => {
                return Err(UsageError::Io {
                    path: path.to_owned(),
                    source,
                })
            }
        };
        serde_json::from_str(&text).map_err(|source| UsageError::Parse {
            path: path.to_owned(),
            source,
        })
    }

    /// Writes the store to `path` atomically, creating parent directories.
    pub fn save(&self, path: &Path) -> Result<(), UsageError> {
        let io_err = |source| UsageError::Io {
            path: path.to_owned(),
            source,
        };
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent).map_err(io_err)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| io_err(io::Error::other(e)))?;
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);
        fs::write(&tmp, json).map_err(io_err)?;
        fs::rename(&tmp, path).map_err(|source| {
            let _ = fs::remove_file(&tmp);
            io_err(source)
        })
    }

    /// Notes that result `id` was launched at unix time `now` after the user
    /// typed `query`.
    pub fn record(&mut self, id: &str, query: &str, now: u64) {
        let entry = self.entries.entry(id.to_owned()).or_insert(UsageEntry {
            count: 0,
            last_used: now,
            queries: Vec::new(),
        });
        entry.count = entry.count.saturating_add(1);
        entry.last_used = now;

        let query = normalize_query(query);
        if !query.is_empty() {
            entry.queries.retain(|q| *q != query);
            entry.queries.insert(0, query);
            entry.queries.truncate(MAX_QUERIES_PER_ENTRY);
        }

        if self.entries.len() > MAX_ENTRIES {
            self.prune(MAX_ENTRIES);
        }
    }

    /// Remembers an executed `query` for recall: trimmed, moved to the front if
    /// already present (ignoring case), at most [`MAX_HISTORY`] kept.
    pub fn record_history(&mut self, query: &str) {
        let query = query.trim();
        if query.is_empty() {
            return;
        }
        let lower = query.to_lowercase();
        self.history.retain(|q| q.to_lowercase() != lower);
        self.history.insert(0, query.to_owned());
        self.history.truncate(MAX_HISTORY);
    }

    /// Executed queries, most recent first.
    pub fn history(&self) -> &[String] {
        &self.history
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    /// Score bonus for `id` given the current `query` at unix time `now`.
    /// 0.0 for ids never launched. See the tuning notes at the top of the file.
    pub fn boost(&self, id: &str, query: &str, now: u64) -> f64 {
        let Some(entry) = self.entries.get(id) else {
            return 0.0;
        };
        let frequency = FREQUENCY_WEIGHT * f64::from(entry.count).ln_1p();

        // Clock skew (last_used in the future) counts as "just now".
        let age_hours = now.saturating_sub(entry.last_used) as f64 / 3600.0;
        let recency = RECENCY_WEIGHT * 0.5_f64.powf(age_hours / RECENCY_HALF_LIFE_HOURS);

        let query = normalize_query(query);
        let memory = if !query.is_empty() && entry.queries.iter().any(|q| q.starts_with(&query)) {
            QUERY_MEMORY_BONUS
        } else {
            0.0
        };

        frequency + recency + memory
    }

    pub fn get(&self, id: &str) -> Option<&UsageEntry> {
        self.entries.get(id)
    }

    /// Keys of the entries recorded for `plugin_id` (ids of the form
    /// `<plugin_id>:<key>`), most recently used first, at most `limit`. Entries
    /// with an empty key are skipped.
    pub fn recent_keys(&self, plugin_id: &str, limit: usize) -> Vec<String> {
        let prefix = format!("{plugin_id}:");
        let mut found: Vec<(&str, &UsageEntry)> = self
            .entries
            .iter()
            .filter_map(|(id, entry)| {
                let key = id.strip_prefix(&prefix)?;
                (!key.is_empty()).then_some((key, entry))
            })
            .collect();
        found.sort_by(|(ka, a), (kb, b)| b.last_used.cmp(&a.last_used).then(ka.cmp(kb)));
        found
            .into_iter()
            .take(limit)
            .map(|(key, _)| key.to_owned())
            .collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Keeps only the `max_entries` most recently used entries.
    pub fn prune(&mut self, max_entries: usize) {
        if self.entries.len() <= max_entries {
            return;
        }
        let mut order: Vec<(&String, &UsageEntry)> = self.entries.iter().collect();
        // Newest first; ties broken by count then id so the result is deterministic.
        order.sort_by(|(ia, a), (ib, b)| {
            b.last_used
                .cmp(&a.last_used)
                .then(b.count.cmp(&a.count))
                .then(ia.cmp(ib))
        });
        let drop: Vec<String> = order
            .into_iter()
            .skip(max_entries)
            .map(|(id, _)| id.clone())
            .collect();
        for id in drop {
            self.entries.remove(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: u64 = 1_700_000_000;
    const HOUR: u64 = 3600;

    #[test]
    fn record_counts_and_remembers_queries() {
        let mut s = UsageStore::default();
        s.record("a", "  Fi ", T0);
        s.record("a", "fire", T0 + 1);
        s.record("a", "FI", T0 + 2);
        s.record("a", "   ", T0 + 3);
        let e = s.get("a").unwrap();
        assert_eq!(e.count, 4);
        assert_eq!(e.last_used, T0 + 3);
        assert_eq!(e.queries, vec!["fi", "fire"]);
    }

    #[test]
    fn queries_are_capped() {
        let mut s = UsageStore::default();
        for q in ["a", "b", "c", "d", "e", "f", "g"] {
            s.record("x", q, T0);
        }
        assert_eq!(s.get("x").unwrap().queries, vec!["g", "f", "e", "d", "c"]);
    }

    #[test]
    fn history_is_recent_first_deduped_and_capped() {
        let mut s = UsageStore::default();
        s.record_history("  firefox ");
        s.record_history("g rust");
        s.record_history("   ");
        s.record_history("FireFox");
        assert_eq!(s.history(), ["FireFox", "g rust"]);

        for i in 0..(MAX_HISTORY + 5) {
            s.record_history(&format!("q{i}"));
        }
        assert_eq!(s.history().len(), MAX_HISTORY);
        assert_eq!(s.history()[0], format!("q{}", MAX_HISTORY + 4));
        s.clear_history();
        assert!(s.history().is_empty());
    }

    #[test]
    fn files_without_history_still_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("usage.json");
        fs::write(&path, r#"{"version":1,"entries":{}}"#).unwrap();
        assert!(UsageStore::load(&path).unwrap().history().is_empty());

        let mut s = UsageStore::default();
        s.record_history("a b");
        s.save(&path).unwrap();
        assert_eq!(UsageStore::load(&path).unwrap().history(), ["a b"]);
    }

    #[test]
    fn unknown_id_has_no_boost() {
        assert_eq!(UsageStore::default().boost("nope", "q", T0), 0.0);
    }

    #[test]
    fn frequency_increases_boost() {
        let mut s = UsageStore::default();
        s.record("a", "", T0);
        let one = s.boost("a", "zzz", T0);
        for _ in 0..9 {
            s.record("a", "", T0);
        }
        let ten = s.boost("a", "zzz", T0);
        assert!(ten > one + 30.0, "{one} vs {ten}");
    }

    #[test]
    fn recency_decays() {
        let mut s = UsageStore::default();
        s.record("a", "", T0);
        let now = s.boost("a", "zzz", T0);
        let day = s.boost("a", "zzz", T0 + 24 * HOUR);
        let week = s.boost("a", "zzz", T0 + 7 * 24 * HOUR);
        assert!(now > day && day > week, "{now} {day} {week}");
    }

    #[test]
    fn clock_skew_is_clamped() {
        let mut s = UsageStore::default();
        s.record("a", "", T0);
        assert_eq!(s.boost("a", "q", T0 - 10_000), s.boost("a", "q", T0));
    }

    #[test]
    fn query_memory_matches_prefixes_only() {
        let mut s = UsageStore::default();
        s.record("a", "fire", T0);
        let base = s.boost("a", "zzz", T0);
        assert_eq!(s.boost("a", "f", T0), base + QUERY_MEMORY_BONUS);
        assert_eq!(s.boost("a", "FIRE", T0), base + QUERY_MEMORY_BONUS);
        assert_eq!(s.boost("a", "firefox", T0), base);
        assert_eq!(s.boost("a", "", T0), base);
    }

    #[test]
    fn prune_keeps_most_recent() {
        let mut s = UsageStore::default();
        for i in 0..10u64 {
            s.record(&format!("id{i}"), "", T0 + i);
        }
        s.prune(3);
        assert_eq!(s.len(), 3);
        for i in 7..10 {
            assert!(s.get(&format!("id{i}")).is_some());
        }
        assert!(s.get("id0").is_none());
    }

    #[test]
    fn record_prunes_beyond_cap() {
        let mut s = UsageStore::default();
        for i in 0..(MAX_ENTRIES as u64 + 5) {
            s.record(&format!("id{i}"), "", T0 + i);
        }
        assert_eq!(s.len(), MAX_ENTRIES);
        assert!(s.get("id0").is_none());
        assert!(s.get(&format!("id{}", MAX_ENTRIES + 4)).is_some());
    }

    #[test]
    fn save_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("usage.json");
        let mut s = UsageStore::default();
        s.record("app:fx", "fi", T0);
        s.record("app:fx", "fire", T0 + 5);
        s.record("file:x", "", T0 + 9);
        s.save(&path).unwrap();
        assert!(!dir.path().join("nested").join("usage.json.tmp").exists());
        assert_eq!(UsageStore::load(&path).unwrap(), s);
        // Overwrite works too.
        s.record("app:fx", "fi", T0 + 20);
        s.save(&path).unwrap();
        assert_eq!(UsageStore::load(&path).unwrap(), s);
    }

    #[test]
    fn recent_keys_are_newest_first_and_scoped_to_the_plugin() {
        let mut s = UsageStore::default();
        s.record("shell:ls -la", "> ls", T0);
        s.record("shell:git status", "> git", T0 + 20);
        s.record("shell:ls -la", "> ls", T0 + 30);
        s.record("shell:", "> ", T0 + 40);
        s.record("shellfish:x", "", T0 + 50);
        s.record("app:fx", "", T0 + 60);
        assert_eq!(s.recent_keys("shell", 10), vec!["ls -la", "git status"]);
        assert_eq!(s.recent_keys("shell", 1), vec!["ls -la"]);
        assert!(s.recent_keys("nope", 10).is_empty());
    }

    #[test]
    fn missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let s = UsageStore::load(&dir.path().join("nope.json")).unwrap();
        assert!(s.is_empty());
    }

    #[test]
    fn corrupt_file_is_parse_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("usage.json");
        fs::write(&path, "{ not json").unwrap();
        assert!(matches!(
            UsageStore::load(&path),
            Err(UsageError::Parse { .. })
        ));
    }
}
