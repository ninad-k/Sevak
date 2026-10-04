//! # sevak-extension-sdk
//!
//! Write a [Sevak](https://github.com/ninad-k/Sevak) extension as a native
//! program in Rust. Sevak starts your program, sends it what the user typed
//! after your keyword, and shows the [`Item`]s you answer with. This crate is
//! the plumbing: it speaks Sevak's script-plugin protocol (newline-delimited
//! JSON on stdin and stdout, protocol version 1) so that you only write the
//! function from a [`Query`] to a list of items.
//!
//! ```no_run
//! use sevak_extension_sdk::{run, Item, Query};
//!
//! fn main() {
//!     run(|query: &Query| {
//!         if query.is_empty() {
//!             return Ok(vec![Item::new("Type your name").subtitle("Say hello")]);
//!         }
//!         Ok(vec![Item::new(format!("Hello, {}!", query.text())).copy_on_enter()])
//!     });
//! }
//! ```
//!
//! # What an extension can and cannot do
//!
//! An item's action comes from a **closed set**: copy text, open a web or mail
//! link, open a path, hand a payload back to your program ([`Action::Custom`])
//! and, only if the manifest declares the `launch` capability, start an
//! application. Sevak performs the action itself; it validates everything you
//! send and drops what it does not allow. See [`Action`].
//!
//! Your program is a normal native process: Sevak runs it with the user's
//! account permissions and a scrubbed environment, **not** in a sandbox. What it
//! does beyond answering queries (network, files) is up to you, and the
//! permissions you declare in the manifest are shown to the user but are not
//! enforced. See `docs/writing-extensions-in-rust.md` in the Sevak repository.
//!
//! # Entry points
//!
//! | Function | Use |
//! |---|---|
//! | [`run`] | the common case: a persistent extension that only answers queries |
//! | [`Extension`] | also handle [`Action::Custom`] executions, or the initialize message |
//! | [`run_oneshot`] | a one-shot extension (`mode = "oneshot"`): one process per query |
//!
//! The protocol loop is a plain function of a reader and a writer
//! ([`Extension::serve`]), which is how this crate's own tests drive it, and
//! how you can test your extension without Sevak.
//!
//! # Rules the SDK applies for you
//!
//! * stdout carries the protocol only. Print diagnostics with [`log`] (stderr);
//!   Sevak captures stderr into its log, tagged with your extension's id.
//! * Titles, subtitles and keys are cut to the lengths Sevak keeps, scores are
//!   clamped, an answer holds at most [`MAX_ITEMS`] items and a line never
//!   exceeds the host's limit.
//! * If several queries are waiting, only the newest is answered (Sevak drops
//!   answers to older ones anyway).
//! * A panic in your function is reported as an error for that query; the
//!   extension keeps running.

mod env;
mod error;
mod item;
mod protocol;
mod query;
mod serve;

pub use env::{plugin_data_dir, plugin_dir, plugin_id, sevak_version};
pub use error::Error;
pub use item::{Action, Icon, Item, LaunchTarget, Problem, MAX_ITEMS};
pub use protocol::{Execute, Initialize, PROTOCOL_VERSION};
pub use query::Query;
pub use serve::{log, run, run_oneshot, Extension};
