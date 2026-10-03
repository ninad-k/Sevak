use std::io;

use thiserror::Error;

pub type Result<T, E = PlatformError> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum PlatformError {
    #[error("could not determine the user's {0} directory")]
    MissingDirectory(&'static str),

    #[error("invalid hotkey \"{hotkey}\": {reason}")]
    InvalidHotkey { hotkey: String, reason: String },

    #[error("`{command}` failed: {message}")]
    CommandFailed { command: String, message: String },

    #[error("unexpected output from `{command}`: {output}")]
    UnexpectedOutput { command: String, output: String },

    #[error("{operation} failed: {message}")]
    Os {
        operation: &'static str,
        message: String,
    },

    #[error("{0} is not supported on this platform")]
    Unsupported(&'static str),

    #[error(transparent)]
    Io(#[from] io::Error),
}
