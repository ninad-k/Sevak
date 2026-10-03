//! The plugin contract.
//!
//! Every source of results — installed apps, the calculator, web search, files
//! and third-party plugins — implements [`Plugin`]. The search engine routes a
//! query to plugins, merges and ranks their results, and hands the chosen one
//! back to its plugin's [`Plugin::execute`].

use std::error::Error as StdError;

use thiserror::Error;

use crate::model::ResultItem;

pub type PluginResult<T> = Result<T, PluginError>;

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

    /// A question the shell must put to the user before [`Plugin::execute`]
    /// runs `item` (it is skipped if they decline), or `None` to run at once.
    /// For actions that cannot be undone, such as shutting the computer down.
    /// Cheap and side-effect free; the shell calls it once per activation.
    fn confirmation(&self, _item: &ResultItem) -> Option<String> {
        None
    }

    /// Rebuilds any index the plugin keeps. Called on a background thread at
    /// startup, on demand and periodically. May be slow.
    fn refresh(&self) -> PluginResult<()> {
        Ok(())
    }
}
