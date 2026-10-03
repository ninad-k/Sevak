//! [`ScriptPlugin`]: the `Plugin` that fronts a script.

use std::sync::Arc;
use std::time::Duration;

use sevak_core::{Action, Plugin, PluginError, PluginResult, ResultItem, ResultsNotifier};
use sevak_platform::PlatformProvider;

use super::delivery::Begin;
use super::runner::{Runner, Spec};
use crate::actions::execute_action;

/// A keyword plugin whose answers come from a script.
///
/// `query` never waits longer than the manifest's `timeout_ms`: it asks the
/// script and returns whatever arrived in that time. Answers that arrive later
/// are kept and announced through the notifier, and the shell then runs the
/// query again (see `delivery.rs`).
pub struct ScriptPlugin {
    runner: Arc<Runner>,
    platform: Arc<dyn PlatformProvider>,
}

impl ScriptPlugin {
    /// Builds the plugin. Nothing is started: the script launches on the first
    /// query that reaches it.
    pub fn new(spec: Spec, platform: Arc<dyn PlatformProvider>) -> Self {
        Self {
            runner: Runner::new(spec),
            platform,
        }
    }

    fn manifest(&self) -> &super::manifest::Manifest {
        &self.runner.spec.manifest
    }
}

impl Plugin for ScriptPlugin {
    fn id(&self) -> &str {
        &self.manifest().id
    }

    fn name(&self) -> &str {
        &self.manifest().name
    }

    fn description(&self) -> &str {
        &self.manifest().description
    }

    fn keyword(&self) -> Option<&str> {
        Some(&self.manifest().keyword)
    }

    /// Script plugins only answer their keyword: every global query would
    /// otherwise reach every script on every keystroke.
    fn global(&self) -> bool {
        false
    }

    fn query(&self, input: &str) -> Vec<ResultItem> {
        let delivery = &self.runner.delivery;
        let id = match delivery.begin(input) {
            Begin::Cached(items) => return items,
            Begin::InFlight(id) => id,
            Begin::Send(id) => {
                if !self.runner.request(id, input) {
                    delivery.abandon(id);
                    // Nothing to wait for; may still return the previous list.
                    return delivery.wait(id, Duration::ZERO);
                }
                id
            }
        };
        delivery.wait(id, self.manifest().timeout)
    }

    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        match &item.action {
            Action::Custom { payload } => {
                let prefix = format!("{}:", self.id());
                let key = item.id.strip_prefix(&prefix).unwrap_or(&item.id);
                self.runner
                    .send_execute(key, payload)
                    .map_err(PluginError::Message)
            }
            action => execute_action(self.platform.as_ref(), action),
        }
    }

    fn attach_notifier(&self, notifier: ResultsNotifier) {
        self.runner.delivery.attach_notifier(notifier);
    }

    fn shutdown(&self) {
        self.runner.stop(true);
    }
}

impl Drop for ScriptPlugin {
    fn drop(&mut self) {
        self.runner.stop(false);
    }
}
