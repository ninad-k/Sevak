//! The error an extension reports for a query.

use std::fmt;

/// Why a query could not be answered. Sevak logs the message (tagged with the
/// extension's id) and shows no results for that query; it is not shown to the
/// user, so put a row in the list instead when the user should see something.
///
/// Any [`std::error::Error`] converts into it, so `?` works on I/O and JSON
/// errors; build one from text with [`Error::new`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    message: String,
}

impl Error {
    /// An error with this message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// The message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

// Like `anyhow::Error`, `Error` does not implement `std::error::Error` itself:
// that is what lets every other error type convert into it.
impl<E: std::error::Error> From<E> for Error {
    fn from(err: E) -> Self {
        Self::new(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<u32, Error> {
        Ok(text.parse::<u32>()?)
    }

    #[test]
    fn question_mark_converts_std_errors() {
        let err = parse("x").unwrap_err();
        assert!(err.message().contains("invalid digit"), "{err}");
        assert_eq!(parse("7").unwrap(), 7);
    }

    #[test]
    fn built_from_text() {
        assert_eq!(Error::new("nope").to_string(), "nope");
    }
}
