//! The plugin contract.
//!
//! Every source of results — installed apps, the calculator, web search, files
//! and third-party plugins — implements [`Plugin`]. The search engine routes a
//! query to plugins, merges and ranks their results, and hands the chosen one
//! back to its plugin's [`Plugin::execute`].

use std::error::Error as StdError;
use std::sync::Arc;

use thiserror::Error;

use crate::model::ResultItem;

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

    /// Returns results for `input`. Called on a worker thread for every
    /// keystroke, so it must be fast (well under a millisecond for typical
    /// inputs) and must never block on I/O; keep indexes in memory.
    fn query(&self, input: &str) -> Vec<ResultItem>;

    /// Performs a result previously returned by [`Plugin::query`].
    fn execute(&self, item: &ResultItem) -> PluginResult<()>;

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
