//! The optional AI assistant: `ai <question>`.
//!
//! Off by default and strictly opt-in. Nothing is sent anywhere until the user
//! turns on `[ai] enabled`, types the keyword and a question, and presses Enter;
//! there is no background traffic, no telemetry and no prefetching. The README
//! and `docs/ai.md` list what is sent to which host.
//!
//! | Module | Job |
//! |---|---|
//! | [`plugin`] | the `ai` keyword: rows, Enter, answer rows, "Ask AI about selection" |
//! | [`assistant`] | one question: safety checks, key lookup, request, reply ([`Asker`] is the seam tests use) |
//! | [`provider`] | the wire formats of OpenAI-compatible servers, Anthropic and Ollama behind one trait |
//! | [`http`] | the only code that touches the network ([`HttpTransport`]; no redirects, size and time limits) |
//! | [`keys`] | API key storage (DPAPI on Windows, an owner-only file elsewhere), environment variables, redaction |
//! | [`error`] | friendly error messages |
//!
//! No test contacts a real provider: [`provider`] is tested on canned replies and
//! [`assistant`] against a loopback server (`testserver`).

pub mod assistant;
pub mod error;
pub mod http;
pub mod keys;
pub mod plugin;
pub mod provider;
#[cfg(test)]
mod testserver;

pub use assistant::{Asker, Assistant, Target, MAX_PROMPT_CHARS};
pub use error::AiError;
pub use http::{HttpRequest, HttpResponse, HttpTransport, ReqwestTransport, TransportError};
pub use keys::{ApiKey, KeyProvider, KeySource, KeyStatus, KeyStore, StoredOrEnv};
pub use plugin::AiPlugin;
pub use provider::{Answer, Provider, Settings};
