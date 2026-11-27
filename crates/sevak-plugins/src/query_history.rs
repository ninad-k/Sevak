//! Query history: `! ` lists the searches you ran, newest first.
//!
//! Type text after the `!` to filter them fuzzily. Enter puts the query back
//! into the search box (the row's [`ViewHint::Complete`]; nothing runs); Ctrl+Enter
//! forgets that one query and Alt+Enter forgets them all (after a question).
//!
//! The queries come from the engine's in-memory usage store through a
//! [`HistoryHandle`], so `query` never touches the disk; `usage.json` is
//! rewritten by the shell's usual debounced save after a deletion. History
//! rows are never recorded themselves (`tracks_usage` is false).

use std::sync::{Mutex, RwLock};

use sevak_core::{
    Action, FuzzyQuery, HistoryHandle, IconSource, Modifier, Plugin, PluginError, PluginResult,
    ResultItem, ResultsNotifier, ViewHint,
};

const KEYWORD: &str = "!";
const ID: &str = "history";

const DELETE: &str = "delete:";
const CLEAR: &str = "clear";

/// Newest-first ranking base when nothing is typed after the keyword.
const LISTING_BASE: f64 = 1000.0;

#[derive(Default)]
pub struct QueryHistoryPlugin {
    history: RwLock<Option<HistoryHandle>>,
    notifier: Mutex<Option<ResultsNotifier>>,
}

impl QueryHistoryPlugin {
    pub fn new() -> Self {
        Self::default()
    }

    fn snapshot(&self) -> Vec<String> {
        self.history
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(HistoryHandle::list)
            .unwrap_or_default()
    }

    fn row(&self, query: &str, score: f64) -> ResultItem {
        ResultItem::new(
            ID,
            query,
            query,
            Action::Custom {
                payload: format!("recall:{query}"),
            },
        )
        .with_subtitle("Enter to put it back in the search box")
        .with_icon(IconSource::builtin("clock"))
        .with_autocomplete(query)
        .with_view(ViewHint::Complete)
        .with_secondary(
            "Delete from history",
            Some(Modifier::Ctrl),
            Action::Custom {
                payload: format!("{DELETE}{query}"),
            },
        )
        .with_secondary(
            "Clear all history",
            Some(Modifier::Alt),
            Action::Custom {
                payload: CLEAR.to_owned(),
            },
        )
        .with_score(score)
    }

    fn payload(item: &ResultItem) -> Option<&str> {
        match &item.action {
            Action::Custom { payload } => Some(payload),
            _ => None,
        }
    }

    fn notify(&self) {
        let notifier = self
            .notifier
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(notifier) = notifier {
            notifier(ID);
        }
    }
}

impl Plugin for QueryHistoryPlugin {
    fn id(&self) -> &str {
        ID
    }

    fn name(&self) -> &str {
        "Query history"
    }

    fn description(&self) -> &str {
        "Type `!` to list the searches you ran; Enter puts one back in the search box."
    }

    fn keyword(&self) -> Option<&str> {
        Some(KEYWORD)
    }

    fn global(&self) -> bool {
        false
    }

    fn query(&self, input: &str) -> Vec<ResultItem> {
        let history = self.snapshot();
        let input = input.trim();
        let mut query = FuzzyQuery::new(input);
        if query.is_empty() {
            return history
                .iter()
                .enumerate()
                .map(|(i, q)| self.row(q, LISTING_BASE - i as f64))
                .collect();
        }
        history
            .iter()
            .enumerate()
            .filter_map(|(i, q)| {
                // Newer entries win ties between equal matches.
                let score = f64::from(query.score(q)?) - i as f64 * 0.001;
                Some(self.row(q, score))
            })
            .collect()
    }

    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        let payload = Self::payload(item)
            .ok_or_else(|| PluginError::Unsupported(item.id.clone()))?
            .to_owned();
        let guard = self.history.read().unwrap_or_else(|e| e.into_inner());
        let Some(history) = guard.as_ref() else {
            return Err(PluginError::Message("history is not ready yet".into()));
        };
        if let Some(query) = payload.strip_prefix(DELETE) {
            history.remove(query);
        } else if payload == CLEAR {
            history.clear();
        } else {
            // `recall:` rows are completed by the UI and never reach execute.
            return Err(PluginError::Unsupported(item.id.clone()));
        }
        drop(guard);
        self.notify();
        Ok(())
    }

    fn confirmation(&self, item: &ResultItem) -> Option<String> {
        (Self::payload(item) == Some(CLEAR))
            .then(|| "Forget every search in the history?".to_owned())
    }

    fn tracks_usage(&self) -> bool {
        false
    }

    fn keeps_open(&self, item: &ResultItem) -> bool {
        // Deleting leaves the list open, shorter (the notifier re-runs it).
        Self::payload(item).is_some_and(|p| p == CLEAR || p.starts_with(DELETE))
    }

    fn attach_history(&self, history: HistoryHandle) {
        *self.history.write().unwrap_or_else(|e| e.into_inner()) = Some(history);
    }

    fn attach_notifier(&self, notifier: ResultsNotifier) {
        *self.notifier.lock().unwrap_or_else(|e| e.into_inner()) = Some(notifier);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use sevak_core::{EngineOptions, SearchEngine, UsageStore};

    use super::*;

    /// An engine over just this plugin, with `queries` in its history (oldest
    /// first, so the last is the newest).
    fn engine_with(queries: &[&str]) -> (SearchEngine, Arc<QueryHistoryPlugin>) {
        let plugin = Arc::new(QueryHistoryPlugin::new());
        let mut usage = UsageStore::default();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        for query in queries {
            usage.record_history(query, now);
        }
        let engine = SearchEngine::new(
            vec![plugin.clone() as Arc<dyn Plugin>],
            usage,
            EngineOptions::default(),
        );
        (engine, plugin)
    }

    fn titles(items: &[ResultItem]) -> Vec<&str> {
        items.iter().map(|i| i.title.as_str()).collect()
    }

    #[test]
    fn metadata() {
        let plugin = QueryHistoryPlugin::new();
        assert_eq!(plugin.id(), "history");
        assert_eq!(plugin.keyword(), Some("!"));
        assert!(!plugin.global());
        assert!(!plugin.tracks_usage());
    }

    #[test]
    fn lists_newest_first_and_filters_fuzzily() {
        let (engine, _) = engine_with(&["firefox", "g rust traits", "calc 2+2", "g rust"]);
        assert_eq!(
            titles(&engine.query("!")),
            ["g rust", "calc 2+2", "g rust traits", "firefox"]
        );
        assert_eq!(titles(&engine.query("! rust")), ["g rust", "g rust traits"]);
        assert!(engine.query("! zzzz").is_empty());
    }

    #[test]
    fn enter_completes_the_bare_query_without_the_keyword() {
        let (engine, _) = engine_with(&["g rust"]);
        let rows = engine.query("! rust");
        assert_eq!(rows[0].autocomplete.as_deref(), Some("g rust"));
        assert_eq!(rows[0].view, Some(ViewHint::Complete));
    }

    #[test]
    fn no_history_means_no_rows() {
        let (engine, _) = engine_with(&[]);
        assert!(engine.query("!").is_empty());
    }

    #[test]
    fn delete_forgets_one_query_and_clear_asks_first() {
        let (engine, plugin) = engine_with(&["one", "two"]);
        let rows = engine.query("!");
        let delete = rows[0].secondary_as_primary(0).unwrap();
        assert_eq!(plugin.confirmation(&delete), None);
        assert!(plugin.keeps_open(&delete));
        engine.execute(&delete, "!").unwrap();
        assert_eq!(titles(&engine.query("!")), ["one"]);
        // Running a history row never records the query that found it.
        assert_eq!(engine.history(), ["one"]);

        let clear = engine.query("!")[0].secondary_as_primary(1).unwrap();
        assert!(plugin.confirmation(&clear).is_some());
        engine.execute(&clear, "!").unwrap();
        assert!(engine.query("!").is_empty());
        assert!(engine.history().is_empty());
    }

    #[test]
    fn deleting_notifies_the_shell() {
        let (engine, plugin) = engine_with(&["one"]);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        plugin.attach_notifier(Arc::new(move |id: &str| {
            sink.lock().unwrap().push(id.to_owned());
        }));
        let delete = engine.query("!")[0].secondary_as_primary(0).unwrap();
        engine.execute(&delete, "!").unwrap();
        assert_eq!(*seen.lock().unwrap(), ["history"]);
    }

    #[test]
    fn history_off_lists_nothing() {
        let plugin = Arc::new(QueryHistoryPlugin::new());
        let mut usage = UsageStore::default();
        usage.record_history("x", 1);
        let engine = SearchEngine::new(
            vec![plugin as Arc<dyn Plugin>],
            usage,
            EngineOptions {
                query_history: false,
                ..EngineOptions::default()
            },
        );
        assert!(engine.query("!").is_empty());
    }
}
