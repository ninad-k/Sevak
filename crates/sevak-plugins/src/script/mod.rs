//! Script-based external plugins.
//!
//! A user drops a folder with a `plugin.toml` and a script into
//! `<config dir>/plugins/`; Sevak shows it as a keyword plugin without being
//! rebuilt. `docs/plugins.md` ("External plugins") is the user-facing guide;
//! this module is the implementation:
//!
//! | File | Concern |
//! |---|---|
//! | `manifest.rs` | `plugin.toml`: parsing, validation, command resolution |
//! | `protocol.rs` | the persistent-process JSON lines (version 1) |
//! | `items.rs` | validating and mapping a script's items to `ResultItem`s |
//! | `alfred.rs` | Alfred Script Filter compatibility (one-shot mode) |
//! | `delivery.rs` | query generations, the answer cache, late-answer notification |
//! | `runner.rs`, `oneshot.rs` | process lifecycle: start, restart policy, idle stop |
//! | `plugin.rs` | [`ScriptPlugin`], the `Plugin` over a runner |
//! | `host.rs`, `approvals.rs` | discovery, the disabled list, first-run approval |

mod alfred;
mod approvals;
pub(crate) mod delivery;
mod host;
mod items;
mod manifest;
mod oneshot;
mod plugin;
mod protocol;
mod runner;

pub use host::{Candidate, Scanned, ScriptPluginHost, FAMILY};
pub use manifest::{Format, Launch, Manifest, Mode, ID_PREFIX, MANIFEST_FILE, PROTOCOL};
pub use plugin::ScriptPlugin;
pub use runner::Spec;
