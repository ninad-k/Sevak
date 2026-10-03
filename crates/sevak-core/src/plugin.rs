//! The plugin contract.
//!
//! Every source of results — installed apps, the calculator, web search, files
//! and third-party plugins — implements [`Plugin`]. The search engine routes a
//! query to plugins, merges and ranks their results, and hands the chosen one
//! back to its plugin's [`Plugin::execute`].

use std::error::Error as StdError;
use std::sync::Arc;

use thiserror::Error;

use crate::model::{PreviewHint, ResultItem};
use crate::selection::Selection;

pub type PluginResult<T> = Result<T, PluginError>;

/// Tells the shell that the plugin with the given id has results that arrived
/// after its [`Plugin::query`] returned, so the current query should run again.
/// See [`Plugin::attach_notifier`].
pub type ResultsNotifier = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Debug, Error)]
pub enum PluginError {
    #[error("{0}")]
    Message(String),
    #[error("result {0} is not handled by this plugin")]
    Unsupported(String),
    #[error(transparent)]
    Other(Box<dyn StdError + Send + Sync>),
}

impl PluginError {
    pub fn other(err: impl StdError + Send + Sync + 'static) -> Self {
        Self::Other(Box::new(err))
    }
}

pub trait Plugin: Send + Sync {
    /// Unique, stable id (`app`, `calculator`, `web:g`, `files`, ...). Used in
    /// result ids, usage statistics and the `[plugins] disabled` config list.
    fn id(&self) -> &str;

    /// Human-readable name shown in settings.
    fn name(&self) -> &str;

    /// One-line, human-readable summary of what the plugin does, shown next to
    /// its enable/disable toggle in settings. Empty by default.
    fn description(&self) -> &str {
        ""
    }

    /// When set, typing `<keyword> <text>` sends only `<text>` to this plugin
    /// and suppresses all others (e.g. `g rust traits` for web search).
    fn keyword(&self) -> Option<&str>;

    /// Whether the plugin also answers queries typed without its keyword.
    fn global(&self) -> bool {
        self.keyword().is_none()
    }

    /// A row offered when the user has typed exactly this plugin's keyword
    /// without a trailing space (`g`), so Tab can complete it to `g `. Shown
    /// below real matches; its `autocomplete` is the full replacement input.
    /// `None` by default.
    fn keyword_row(&self) -> Option<ResultItem> {
        None
    }

    /// Returns results for `input`. Called on a worker thread for every
    /// keystroke, so it must be fast (well under a millisecond for typical
    /// inputs) and must never block on I/O; keep indexes in memory.
    fn query(&self, input: &str) -> Vec<ResultItem>;

    /// Performs a result previously returned by [`Plugin::query`].
    fn execute(&self, item: &ResultItem) -> PluginResult<()>;

    /// A question the shell must put to the user before [`Plugin::execute`]
    /// runs `item` (it is skipped if they decline), or `None` to run at once.
    /// For actions that cannot be undone, such as shutting the computer down.
    /// Cheap and side-effect free; the shell calls it once per activation.
    fn confirmation(&self, _item: &ResultItem) -> Option<String> {
        None
    }

    /// Whether running this plugin's results is recorded in the usage
    /// statistics and the search history. True by default; a plugin whose
    /// results contain what the user typed or selected elsewhere (Universal
    /// Actions) returns false, so none of it reaches `usage.json`.
    fn tracks_usage(&self) -> bool {
        true
    }

    /// Actions this plugin offers for what the user selected in another app
    /// (the Universal Actions hotkey), in the order they are listed. The
    /// engine collects them from every plugin. Each result must be handled by
    /// this plugin's own [`Plugin::execute`], and must not put the selection in
    /// its `id` (ids are stable keys, the selection is private). Empty by
    /// default. Same cost rules as [`Plugin::query`].
    fn selection_actions(&self, _selection: &Selection) -> Vec<ResultItem> {
        Vec::new()
    }

    /// What the preview pane (and Text View) should show for `item`, when the
    /// plugin can say it better than the item's action does: a snippet with
    /// its placeholders filled in, an emoji with its keywords. Called lazily,
    /// on a worker thread, only for the one row the user asks to preview, so
    /// unlike [`Plugin::query`] it may do a little work (it must still not
    /// block for long). `None` by default: the shell then uses the item's own
    /// [`ResultItem::preview`] or derives one from its action.
    fn preview(&self, _item: &ResultItem) -> Option<PreviewHint> {
        None
    }

    /// Hands the plugin the keys (the part of a result id after `<plugin id>:`)
    /// of its results that were run before, most recently used first, taken from
    /// the usage statistics when the engine is built. Plugins whose results are
    /// text the user typed (the shell plugin) use it to offer recent entries
    /// again; everyone else ignores it.
    fn restore_history(&self, _keys: &[String]) {}

    /// Rebuilds the result with id `id` (the full id, `<plugin id>:<key>`)
    /// without a query, so a `[[hotkey]] run = "<id>"` can execute it directly.
    /// Plugins whose results can be named by a stable id implement this; the
    /// default says "not resolvable". Same cost rules as [`Plugin::query`].
    fn resolve(&self, _id: &str) -> Option<ResultItem> {
        None
    }

    /// Rebuilds any index the plugin keeps. Called on a background thread at
    /// startup, on demand and periodically. May be slow.
    fn refresh(&self) -> PluginResult<()> {
        Ok(())
    }

    /// Receives the shell's "results updated" callback. Only plugins that answer
    /// slowly (script plugins) use it: when a late answer is ready for the query
    /// they last served, they call it with their id and the shell re-runs the
    /// current query, which then finds the answer in the plugin's cache. Called
    /// once per plugin instance, before the first query.
    fn attach_notifier(&self, _notifier: ResultsNotifier) {}

    /// Stops background work (child processes, threads). Called when Sevak
    /// quits; plugins are also dropped when the config reloads, so anything
    /// that must not outlive the instance should be stopped in `Drop` too.
    fn shutdown(&self) {}
}
