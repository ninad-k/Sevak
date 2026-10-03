//! The search engine: routes a query to plugins, merges and ranks the results,
//! and learns from what the user picks.
//!
//! # Routing
//!
//! 1. Input that is only whitespace yields no results.
//! 2. Keyword routing: if the input (leading whitespace trimmed) is
//!    `<kw><whitespace><rest>` and one or more plugins have `keyword() == kw`
//!    (case-sensitive), only those plugins are queried, with
//!    `rest.trim_start()`. `rest` may be empty (`"g "`): the plugin decides what
//!    to show, e.g. a "type to search Google" hint. No fallbacks run in this
//!    mode. A keyword without a following whitespace character (`"g"`) is an
//!    ordinary global query.
//! 3. Otherwise every plugin with `global() == true` is queried with the
//!    trimmed input. Results from a plugin that *has* a keyword but answered
//!    globally (e.g. files) have their score multiplied by
//!    [`GLOBAL_SECONDARY_WEIGHT`]: dedicated sources such as installed apps
//!    should stay on top of the long tail of loosely related files, while an
//!    exact filename match can still surface when no app matches.
//!
//! # Ranking
//!
//! 4. [`UsageStore::boost`] is added to every score, scaled by the item's score
//!    relative to the best one (so usage reorders comparable matches but cannot
//!    lift a weak match over a strong one), except for items whose score is at
//!    least [`score::KEYWORD`]: exact answers and keyword-triggered results keep
//!    the order their plugin gave them (and are not down-weighted by step 3
//!    either).
//! 5. Results are deduplicated by `id` (best score wins), sorted by score
//!    descending (ties: title case-insensitively, then id) and truncated to
//!    `max_results`.
//! 6. If step 3 found nothing and the input is non-empty, each configured
//!    fallback plugin is queried with the full trimmed input and its results
//!    are returned (also truncated, not boosted).
//! 7. Query time is logged at `debug`; a query over [`LATENCY_BUDGET`] logs a
//!    `warn` naming the slowest plugin.
//!
//! Plugin panics are not caught: the release profile uses `panic = "abort"`, so
//! a panicking plugin takes the whole process down. Plugins must not panic.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::model::{score, ResultItem};
use crate::plugin::{Plugin, PluginError, PluginResult};
use crate::usage::{normalize_query, UsageStore};

/// Score multiplier for results of keyword plugins that answer global queries.
pub const GLOBAL_SECONDARY_WEIGHT: f64 = 0.5;

/// Time a single query should stay under to feel instantaneous.
pub const LATENCY_BUDGET: Duration = Duration::from_millis(16);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineOptions {
    /// Maximum number of results returned by a query.
    pub max_results: usize,
    /// Ids of plugins queried with the full input, only when nothing else
    /// matched (e.g. `web:g`).
    pub fallback_plugins: Vec<String>,
}

impl Default for EngineOptions {
    fn default() -> Self {
        Self {
            max_results: 8,
            fallback_plugins: Vec::new(),
        }
    }
}

pub struct SearchEngine {
    plugins: Vec<Arc<dyn Plugin>>,
    usage: RwLock<UsageStore>,
    options: EngineOptions,
}

/// Seconds since the unix epoch (0 if the clock is before it).
fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Splits `<kw><whitespace><rest>`; `None` when there is no whitespace after
/// the first word.
fn split_keyword(input: &str) -> Option<(&str, &str)> {
    let input = input.trim_start();
    let idx = input.find(char::is_whitespace)?;
    Some((&input[..idx], input[idx..].trim_start()))
}

fn rank_order(a: &ResultItem, b: &ResultItem) -> Ordering {
    b.score
        .total_cmp(&a.score)
        .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
        .then_with(|| a.id.cmp(&b.id))
}

/// Keeps the best-scored item per id, sorts and truncates.
fn finalize(items: Vec<ResultItem>, max_results: usize) -> Vec<ResultItem> {
    let mut best: HashMap<String, ResultItem> = HashMap::with_capacity(items.len());
    for item in items {
        match best.get(&item.id) {
            Some(existing) if existing.score.total_cmp(&item.score) != Ordering::Less => {}
            _ => {
                best.insert(item.id.clone(), item);
            }
        }
    }
    let mut out: Vec<ResultItem> = best.into_values().collect();
    out.sort_by(rank_order);
    out.truncate(max_results);
    out
}

impl SearchEngine {
    pub fn new(plugins: Vec<Arc<dyn Plugin>>, usage: UsageStore, options: EngineOptions) -> Self {
        Self {
            plugins,
            usage: RwLock::new(usage),
            options,
        }
    }

    pub fn plugins(&self) -> &[Arc<dyn Plugin>] {
        &self.plugins
    }

    pub fn plugin(&self, id: &str) -> Option<&Arc<dyn Plugin>> {
        self.plugins.iter().find(|p| p.id() == id)
    }

    // A poisoned lock only means another thread panicked while holding it; the
    // usage data is still structurally valid, so keep going.
    fn usage_read(&self) -> RwLockReadGuard<'_, UsageStore> {
        self.usage.read().unwrap_or_else(|e| e.into_inner())
    }

    fn usage_write(&self) -> RwLockWriteGuard<'_, UsageStore> {
        self.usage.write().unwrap_or_else(|e| e.into_inner())
    }

    pub fn query(&self, input: &str) -> Vec<ResultItem> {
        self.query_at(input, unix_now())
    }

    /// Like [`SearchEngine::query`] with an explicit unix time (for tests).
    pub fn query_at(&self, input: &str, now: u64) -> Vec<ResultItem> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }
        let started = Instant::now();
        let mut slowest: (Duration, String) = (Duration::ZERO, String::new());
        let mut run = |plugin: &Arc<dyn Plugin>, text: &str| {
            let t = Instant::now();
            let items = plugin.query(text);
            let elapsed = t.elapsed();
            if elapsed >= slowest.0 {
                slowest = (elapsed, plugin.id().to_owned());
            }
            items
        };

        let mut collected: Vec<ResultItem> = Vec::new();
        let mut keyword_mode = false;

        let keyword_route = split_keyword(input).and_then(|(kw, rest)| {
            let plugins: Vec<&Arc<dyn Plugin>> = self
                .plugins
                .iter()
                .filter(|p| p.keyword() == Some(kw))
                .collect();
            (!plugins.is_empty()).then_some((plugins, rest))
        });

        if let Some((keyword_plugins, rest)) = keyword_route {
            keyword_mode = true;
            for plugin in keyword_plugins {
                collected.extend(run(plugin, rest));
            }
        } else {
            for plugin in self.plugins.iter().filter(|p| p.global()) {
                let secondary = plugin.keyword().is_some();
                let mut items = run(plugin, trimmed);
                if secondary {
                    for item in &mut items {
                        if item.score < score::KEYWORD {
                            item.score *= GLOBAL_SECONDARY_WEIGHT;
                        }
                    }
                }
                collected.extend(items);
            }
        }

        let mut results = if collected.is_empty() {
            Vec::new()
        } else {
            let usage = self.usage_read();
            let query = normalize_query(trimmed);
            // Scale each boost by how well the item matched relative to the best
            // match: habits should reorder comparable matches, not lift a
            // one-letter-overlap result above a strong one.
            let best = collected
                .iter()
                .map(|item| item.score)
                .filter(|&s| s < score::KEYWORD)
                .fold(0.0_f64, f64::max);
            for item in &mut collected {
                if item.score < score::KEYWORD {
                    let quality = if best > 0.0 {
                        (item.score / best).clamp(0.0, 1.0)
                    } else {
                        1.0
                    };
                    item.score += quality * usage.boost(&item.id, &query, now);
                }
            }
            drop(usage);
            finalize(collected, self.options.max_results)
        };

        if results.is_empty() && !keyword_mode {
            let mut fallback = Vec::new();
            for id in &self.options.fallback_plugins {
                if let Some(plugin) = self.plugin(id) {
                    fallback.extend(run(plugin, trimmed));
                }
            }
            results = finalize(fallback, self.options.max_results);
        }

        let total = started.elapsed();
        if total > LATENCY_BUDGET {
            tracing::warn!(
                query = trimmed,
                elapsed_ms = total.as_secs_f64() * 1000.0,
                slowest_plugin = slowest.1.as_str(),
                slowest_ms = slowest.0.as_secs_f64() * 1000.0,
                "query exceeded the {} ms latency budget",
                LATENCY_BUDGET.as_millis()
            );
        } else {
            tracing::debug!(
                query = trimmed,
                elapsed_us = total.as_micros() as u64,
                results = results.len(),
                "query"
            );
        }
        results
    }

    /// Executes `item` through its plugin and, on success, records the launch.
    /// `query` is the text the user had typed (the whole input, keyword
    /// included, so it matches what [`SearchEngine::query`] later receives).
    pub fn execute(&self, item: &ResultItem, query: &str) -> PluginResult<()> {
        self.execute_at(item, query, unix_now())
    }

    pub fn execute_at(&self, item: &ResultItem, query: &str, now: u64) -> PluginResult<()> {
        let plugin = self
            .plugin(&item.plugin_id)
            .ok_or_else(|| PluginError::Unsupported(item.id.clone()))?;
        plugin.execute(item)?;
        self.usage_write().record(&item.id, query, now);
        Ok(())
    }

    /// A copy of the usage statistics, to save from another thread or to carry
    /// into a rebuilt engine after a config reload.
    pub fn usage_snapshot(&self) -> UsageStore {
        self.usage_read().clone()
    }

    /// Refreshes every plugin one after another; returns the failures.
    pub fn refresh_all(&self) -> Vec<(String, PluginError)> {
        let mut failures = Vec::new();
        for plugin in &self.plugins {
            let started = Instant::now();
            match plugin.refresh() {
                Ok(()) => tracing::debug!(
                    plugin = plugin.id(),
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "plugin refreshed"
                ),
                Err(err) => {
                    tracing::error!(plugin = plugin.id(), error = %err, "plugin refresh failed");
                    failures.push((plugin.id().to_owned(), err));
                }
            }
        }
        failures
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::sync::Mutex;

    use super::*;
    use crate::model::Action;

    const T0: u64 = 1_700_000_000;

    type QueryFn = Box<dyn Fn(&str) -> Vec<ResultItem> + Send + Sync>;

    struct Mock {
        id: String,
        keyword: Option<String>,
        global: Option<bool>,
        query_fn: QueryFn,
        fail_execute: bool,
        executed: AtomicUsize,
        refresh_error: Option<String>,
        inputs: Mutex<Vec<String>>,
    }

    impl Mock {
        fn new(
            id: &str,
            query_fn: impl Fn(&str) -> Vec<ResultItem> + Send + Sync + 'static,
        ) -> Self {
            Self {
                id: id.to_owned(),
                keyword: None,
                global: None,
                query_fn: Box::new(query_fn),
                fail_execute: false,
                executed: AtomicUsize::new(0),
                refresh_error: None,
                inputs: Mutex::new(Vec::new()),
            }
        }

        /// A plugin that always returns `(key, title, score)` items.
        fn fixed(id: &str, items: &[(&str, &str, f64)]) -> Self {
            let id_owned = id.to_owned();
            let items: Vec<(String, String, f64)> = items
                .iter()
                .map(|(k, t, s)| ((*k).to_owned(), (*t).to_owned(), *s))
                .collect();
            Self::new(id, move |_| {
                items
                    .iter()
                    .map(|(k, t, s)| item(&id_owned, k, t, *s))
                    .collect()
            })
        }

        fn keyword(mut self, kw: &str) -> Self {
            self.keyword = Some(kw.to_owned());
            self
        }

        fn global(mut self, global: bool) -> Self {
            self.global = Some(global);
            self
        }

        fn failing(mut self) -> Self {
            self.fail_execute = true;
            self
        }

        fn arc(self) -> Arc<Mock> {
            Arc::new(self)
        }
    }

    impl Plugin for Mock {
        fn id(&self) -> &str {
            &self.id
        }
        fn name(&self) -> &str {
            &self.id
        }
        fn keyword(&self) -> Option<&str> {
            self.keyword.as_deref()
        }
        fn global(&self) -> bool {
            self.global.unwrap_or_else(|| self.keyword.is_none())
        }
        fn query(&self, input: &str) -> Vec<ResultItem> {
            self.inputs.lock().unwrap().push(input.to_owned());
            (self.query_fn)(input)
        }
        fn execute(&self, item: &ResultItem) -> PluginResult<()> {
            if self.fail_execute {
                return Err(PluginError::Message("boom".into()));
            }
            let _ = item;
            self.executed.fetch_add(1, AtomicOrdering::SeqCst);
            Ok(())
        }
        fn refresh(&self) -> PluginResult<()> {
            match &self.refresh_error {
                Some(msg) => Err(PluginError::Message(msg.clone())),
                None => Ok(()),
            }
        }
    }

    fn item(plugin: &str, key: &str, title: &str, score: f64) -> ResultItem {
        ResultItem::new(
            plugin,
            key,
            title,
            Action::CopyText {
                text: title.to_owned(),
            },
        )
        .with_score(score)
    }

    fn engine(plugins: Vec<Arc<Mock>>, max_results: usize, fallbacks: &[&str]) -> SearchEngine {
        engine_with(plugins, UsageStore::default(), max_results, fallbacks)
    }

    fn engine_with(
        plugins: Vec<Arc<Mock>>,
        usage: UsageStore,
        max_results: usize,
        fallbacks: &[&str],
    ) -> SearchEngine {
        SearchEngine::new(
            plugins.into_iter().map(|p| p as Arc<dyn Plugin>).collect(),
            usage,
            EngineOptions {
                max_results,
                fallback_plugins: fallbacks.iter().map(|s| (*s).to_owned()).collect(),
            },
        )
    }

    fn ids(results: &[ResultItem]) -> Vec<&str> {
        results.iter().map(|r| r.id.as_str()).collect()
    }

    #[test]
    fn engine_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SearchEngine>();
    }

    #[test]
    fn whitespace_only_input_is_empty() {
        let app = Mock::fixed("app", &[("a", "A", 50.0)]).arc();
        let e = engine(vec![app.clone()], 8, &[]);
        assert!(e.query_at("", T0).is_empty());
        assert!(e.query_at("  \t ", T0).is_empty());
        assert!(app.inputs.lock().unwrap().is_empty());
    }

    #[test]
    fn keyword_routes_only_to_keyword_plugins_with_rest() {
        let app = Mock::fixed("app", &[("a", "App", 50.0)]).arc();
        let web = Mock::fixed("web:g", &[("q", "Search Google", score::KEYWORD)])
            .keyword("g")
            .arc();
        let e = engine(vec![app.clone(), web.clone()], 8, &[]);

        let r = e.query_at("  g   rust traits ", T0);
        assert_eq!(ids(&r), vec!["web:g:q"]);
        assert_eq!(*web.inputs.lock().unwrap(), vec!["rust traits ".to_owned()]);
        assert!(app.inputs.lock().unwrap().is_empty());
    }

    #[test]
    fn keyword_with_empty_rest_still_routes() {
        let web = Mock::fixed("web:g", &[("hint", "Type to search", score::KEYWORD)])
            .keyword("g")
            .arc();
        let app = Mock::fixed("app", &[("a", "App", 50.0)]).arc();
        let e = engine(vec![app.clone(), web.clone()], 8, &[]);

        let r = e.query_at("g ", T0);
        assert_eq!(ids(&r), vec!["web:g:hint"]);
        assert_eq!(*web.inputs.lock().unwrap(), vec![String::new()]);
        assert!(app.inputs.lock().unwrap().is_empty());
    }

    #[test]
    fn keyword_without_space_is_a_global_query() {
        let app = Mock::fixed("app", &[("a", "App", 50.0)]).arc();
        let web = Mock::fixed("web:g", &[("q", "Search Google", score::KEYWORD)])
            .keyword("g")
            .arc();
        let e = engine(vec![app.clone(), web.clone()], 8, &[]);

        let r = e.query_at("g", T0);
        assert_eq!(ids(&r), vec!["app:a"]);
        assert_eq!(*app.inputs.lock().unwrap(), vec!["g".to_owned()]);
        assert!(web.inputs.lock().unwrap().is_empty());
    }

    #[test]
    fn keyword_is_case_sensitive_and_unknown_keyword_goes_global() {
        let app = Mock::fixed("app", &[("a", "App", 50.0)]).arc();
        let web = Mock::fixed("web:g", &[("q", "Search", score::KEYWORD)])
            .keyword("g")
            .arc();
        let e = engine(vec![app.clone(), web.clone()], 8, &[]);

        let r = e.query_at("G rust", T0);
        assert_eq!(ids(&r), vec!["app:a"]);
        assert_eq!(*app.inputs.lock().unwrap(), vec!["G rust".to_owned()]);
    }

    #[test]
    fn multiple_plugins_may_share_a_keyword() {
        let a = Mock::fixed("x1", &[("a", "A", 10.0)]).keyword("k").arc();
        let b = Mock::fixed("x2", &[("b", "B", 20.0)]).keyword("k").arc();
        let e = engine(vec![a, b], 8, &[]);
        assert_eq!(ids(&e.query_at("k z", T0)), vec!["x2:b", "x1:a"]);
    }

    #[test]
    fn only_global_plugins_answer_plain_queries() {
        let app = Mock::fixed("app", &[("a", "App", 50.0)]).arc();
        let hidden = Mock::fixed("hidden", &[("h", "Hidden", 90.0)])
            .keyword("h")
            .global(false)
            .arc();
        let e = engine(vec![app, hidden.clone()], 8, &[]);
        assert_eq!(ids(&e.query_at("anything", T0)), vec!["app:a"]);
        assert!(hidden.inputs.lock().unwrap().is_empty());
    }

    #[test]
    fn keyword_plugin_answering_globally_is_down_weighted() {
        let app = Mock::fixed("app", &[("a", "App", 60.0)]).arc();
        let files = Mock::fixed("files", &[("f", "File", 100.0)])
            .keyword("f")
            .global(true)
            .arc();
        let e = engine(vec![app, files], 8, &[]);
        let r = e.query_at("thing", T0);
        assert_eq!(ids(&r), vec!["app:a", "files:f"]);
        assert_eq!(r[1].score, 100.0 * GLOBAL_SECONDARY_WEIGHT);
    }

    #[test]
    fn usage_boost_reorders_similar_scores() {
        let app = Mock::fixed("app", &[("a", "Alpha", 60.0), ("b", "Beta", 62.0)]).arc();
        let mut usage = UsageStore::default();
        for _ in 0..5 {
            usage.record("app:a", "", T0);
        }
        let e = engine_with(vec![app], usage, 8, &[]);
        assert_eq!(ids(&e.query_at("x", T0)), vec!["app:a", "app:b"]);
    }

    #[test]
    fn usage_cannot_lift_a_weak_match_over_a_strong_one() {
        // "note": Notepad matches strongly; Character Map only scrapes by.
        let app = Mock::fixed(
            "app",
            &[("np", "Notepad", 120.0), ("cm", "Character Map", 20.0)],
        )
        .arc();
        let mut usage = UsageStore::default();
        // Unscaled, 30 recent launches (+~126) would put it at ~146 > 120.
        for _ in 0..30 {
            usage.record("app:cm", "chr", T0);
        }
        let e = engine_with(vec![app], usage, 8, &[]);
        assert_eq!(ids(&e.query_at("note", T0)), vec!["app:np", "app:cm"]);
    }

    #[test]
    fn query_memory_picks_previously_chosen_item() {
        let app = Mock::fixed("app", &[("fx", "Firefox", 70.0), ("fz", "FileZilla", 68.0)]).arc();
        let e = engine(vec![app], 8, &[]);
        let before = e.query_at("f", T0);
        assert_eq!(ids(&before), vec!["app:fx", "app:fz"]);

        let chosen = before[1].clone();
        e.execute_at(&chosen, "f", T0).unwrap();
        let after = e.query_at("f", T0 + 60);
        assert_eq!(ids(&after), vec!["app:fz", "app:fx"]);
    }

    #[test]
    fn high_score_items_are_not_boosted() {
        let calc = Mock::fixed("calc", &[("c", "4", score::EXACT_ANSWER)]).arc();
        let web = Mock::fixed("web:g", &[("q", "Search", score::KEYWORD)])
            .keyword("g")
            .arc();
        let mut usage = UsageStore::default();
        for _ in 0..50 {
            usage.record("calc:c", "2+2", T0);
            usage.record("web:g:q", "g x", T0);
        }
        let e = engine_with(vec![calc, web], usage, 8, &[]);
        assert_eq!(e.query_at("2+2", T0)[0].score, score::EXACT_ANSWER);
        assert_eq!(e.query_at("g x", T0)[0].score, score::KEYWORD);
    }

    #[test]
    fn duplicates_keep_best_score() {
        let a = Mock::fixed("app", &[("same", "Same", 30.0)]).arc();
        let b = Mock::new("app", |_| vec![item("app", "same", "Same", 90.0)]).arc();
        let e = engine(vec![a, b], 8, &[]);
        let r = e.query_at("s", T0);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].score, 90.0);
    }

    #[test]
    fn results_are_truncated_and_sorted() {
        let app = Mock::new("app", |_| {
            (0..20)
                .map(|i| item("app", &i.to_string(), &format!("Item {i}"), f64::from(i)))
                .collect()
        })
        .arc();
        let e = engine(vec![app], 5, &[]);
        let r = e.query_at("i", T0);
        assert_eq!(r.len(), 5);
        assert_eq!(r[0].id, "app:19");
        assert!(r.windows(2).all(|w| w[0].score >= w[1].score));
    }

    #[test]
    fn ties_break_by_title_then_id() {
        let app = Mock::fixed(
            "app",
            &[
                ("2", "banana", 10.0),
                ("1", "Apple", 10.0),
                ("3", "Apple", 10.0),
            ],
        )
        .arc();
        let e = engine(vec![app], 8, &[]);
        assert_eq!(ids(&e.query_at("x", T0)), vec!["app:1", "app:3", "app:2"]);
    }

    fn fallback_web() -> Arc<Mock> {
        Mock::new("web:g", |input| {
            vec![item(
                "web:g",
                "q",
                &format!("Search for {input}"),
                score::FALLBACK,
            )]
        })
        .keyword("g")
        .arc()
    }

    #[test]
    fn fallback_runs_only_when_nothing_matched() {
        let app = Mock::new("app", |input| {
            if input == "firefox" {
                vec![item("app", "fx", "Firefox", 80.0)]
            } else {
                Vec::new()
            }
        })
        .arc();
        let e = engine(vec![app, fallback_web()], 8, &["web:g"]);

        assert_eq!(ids(&e.query_at("firefox", T0)), vec!["app:fx"]);
        let r = e.query_at("  zzzz qqq ", T0);
        assert_eq!(ids(&r), vec!["web:g:q"]);
        assert_eq!(r[0].title, "Search for zzzz qqq");
    }

    #[test]
    fn fallback_does_not_run_in_keyword_mode() {
        let other = Mock::new("x", |_| Vec::new()).keyword("x").arc();
        let e = engine(vec![other, fallback_web()], 8, &["web:g"]);
        assert!(e.query_at("x nothing", T0).is_empty());
    }

    #[test]
    fn missing_fallback_plugin_is_ignored() {
        let app = Mock::new("app", |_| Vec::new()).arc();
        let e = engine(vec![app], 8, &["web:nope"]);
        assert!(e.query_at("zzz", T0).is_empty());
    }

    #[test]
    fn execute_records_usage_only_on_success() {
        let ok = Mock::fixed("ok", &[("a", "A", 1.0)]).arc();
        let bad = Mock::fixed("bad", &[("b", "B", 1.0)]).failing().arc();
        let e = engine(vec![ok.clone(), bad], 8, &[]);

        let good_item = item("ok", "a", "A", 1.0);
        e.execute_at(&good_item, " Foo ", T0).unwrap();
        assert_eq!(ok.executed.load(AtomicOrdering::SeqCst), 1);
        let snap = e.usage_snapshot();
        let entry = snap.get("ok:a").unwrap();
        assert_eq!(entry.count, 1);
        assert_eq!(entry.queries, vec!["foo"]);

        let bad_item = item("bad", "b", "B", 1.0);
        assert!(e.execute_at(&bad_item, "x", T0).is_err());
        assert!(e.usage_snapshot().get("bad:b").is_none());
    }

    #[test]
    fn execute_unknown_plugin_is_unsupported() {
        let e = engine(vec![], 8, &[]);
        let err = e
            .execute_at(&item("ghost", "a", "A", 1.0), "x", T0)
            .unwrap_err();
        assert!(matches!(err, PluginError::Unsupported(id) if id == "ghost:a"));
        assert!(e.usage_snapshot().is_empty());
    }

    #[test]
    fn usage_snapshot_carries_into_new_engine() {
        let app = Mock::fixed("app", &[("a", "A", 1.0)]).arc();
        let e = engine(vec![app.clone()], 8, &[]);
        e.execute_at(&item("app", "a", "A", 1.0), "a", T0).unwrap();
        let e2 = engine_with(vec![app], e.usage_snapshot(), 8, &[]);
        assert_eq!(e2.usage_snapshot().len(), 1);
    }

    #[test]
    fn refresh_all_collects_failures() {
        let mut broken = Mock::new("broken", |_| Vec::new());
        broken.refresh_error = Some("nope".into());
        let fine = Mock::new("fine", |_| Vec::new());
        let e = engine(vec![broken.arc(), fine.arc()], 8, &[]);
        let failures = e.refresh_all();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].0, "broken");
    }

    #[test]
    fn plugin_lookup_by_id() {
        let e = engine(vec![Mock::new("app", |_| Vec::new()).arc()], 8, &[]);
        assert!(e.plugin("app").is_some());
        assert!(e.plugin("nope").is_none());
        assert_eq!(e.plugins().len(), 1);
    }

    #[test]
    fn split_keyword_cases() {
        assert_eq!(split_keyword("g rust"), Some(("g", "rust")));
        assert_eq!(split_keyword("  g   rust  x"), Some(("g", "rust  x")));
        assert_eq!(split_keyword("g "), Some(("g", "")));
        assert_eq!(split_keyword("g"), None);
        assert_eq!(split_keyword("  g"), None);
    }
}
