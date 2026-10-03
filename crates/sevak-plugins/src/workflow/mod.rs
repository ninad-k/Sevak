//! Workflows: small graphs of triggers, actions and outputs.
//!
//! A workflow is a folder `<config dir>/workflows/<name>/workflow.toml` (plus
//! the scripts it uses). `docs/workflows.md` is the user guide; this module is
//! the implementation:
//!
//! | File | Concern |
//! |---|---|
//! | `model.rs` | the file format: [`Workflow`], [`Node`], [`NodeKind`], [`Connection`] |
//! | `validate.rs` | cycles, dangling connections, bad fields: [`Problem`]s |
//! | `template.rs` | `{query}` / `{var:name}` placeholders and their filters |
//! | `exec.rs` | running a workflow: [`Runtime`], [`OutputSink`], timeouts |
//! | `plugins.rs` | the `Plugin`s a workflow becomes: keyword, script filter, Universal Actions, hotkey |
//! | `host.rs` | discovery, approval, the settings page's operations: [`WorkflowHost`] |
//! | `templates.rs` | the "New from template" starting points |
//! | `gallery.rs` | the opt-in gallery: index, download, checksum, safe unpacking |
//!
//! A script filter node reuses the script plugin machinery (`crate::script`)
//! for its process handling, query generations and late answers; everything
//! else runs on the [`Runtime`]'s own thread per run.

pub mod exec;
pub mod gallery;
mod host;
pub mod model;
mod plugins;
pub mod template;
pub mod templates;
pub mod validate;

#[cfg(test)]
pub(crate) mod testing;

pub use exec::{Ctx, NoSink, NodeFailure, OutputSink, RunReport, Runtime};
pub use host::{approval_key, describe, Candidate, Loaded, Saved, Scanned, Summary, WorkflowHost};
pub use model::{
    Accepts, Argument, Category, Connection, Node, NodeKind, Test, TransformOp, Workflow, FAMILY,
    FILE, FORMAT,
};
pub use plugins::{run_id, FilterPlugin, KeywordPlugin, TriggersPlugin};
pub use validate::{Problem, Severity};
