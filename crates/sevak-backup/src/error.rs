//! Errors of the backup and restore code. Every message is written for the
//! person who sees it in Settings (or in the terminal), so none of them names a
//! Rust type.

use std::io;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    /// The file is not a usable backup: damaged, hostile or not ours.
    #[error("{0}")]
    Invalid(String),

    /// The backup was written by a newer Sevak than this one understands.
    #[error(
        "this backup was made by a newer version of Sevak (backup format {found}, this version \
         understands up to {supported}); update Sevak to restore it"
    )]
    TooNew { found: u32, supported: u32 },

    /// Something about the current installation stands in the way (for example
    /// a `config.toml` with a mistake in it).
    #[error("{0}")]
    Current(String),

    /// The restore would produce a configuration Sevak would not accept.
    #[error("{0}")]
    Rejected(String),

    /// Nothing to do.
    #[error("{0}")]
    Nothing(String),

    /// The file changed between the preview and the restore.
    #[error("the backup file changed after it was checked; open it again")]
    Changed,

    /// The safety snapshot could not be written, so nothing was touched.
    #[error("could not save a safety copy of your current settings, so nothing was changed: {0}")]
    Snapshot(String),

    /// Applying the restore failed. `rolled_back` says whether the previous
    /// state was put back.
    #[error("{message}")]
    Apply { message: String, rolled_back: bool },

    #[error("{what}: {source}")]
    Io {
        what: String,
        #[source]
        source: io::Error,
    },
}

impl Error {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }

    pub(crate) fn io(what: impl Into<String>, source: io::Error) -> Self {
        Self::Io {
            what: what.into(),
            source,
        }
    }
}
