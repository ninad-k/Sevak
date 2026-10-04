//! What can go wrong when asking, as sentences for the person who asked.
//!
//! None of these carries a request, a header or a key. Text that came from the
//! server (an error message) is cleaned by [`super::keys::scrub`] and cut short
//! before it is stored here.

use std::fmt;

use sevak_core::AiProvider;

/// Longest server-written message kept in an error.
pub(crate) const MAX_SERVER_MESSAGE_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiError {
    /// The provider needs an API key and none is stored or in the environment.
    NoKey { provider: AiProvider },
    /// The settings cannot be used (an unusable or unsafe base URL).
    BadSettings(String),
    /// The question is empty or too long to send.
    BadPrompt(String),
    /// Nothing answered at the address (not running, no network, unknown host).
    Unreachable {
        provider: AiProvider,
        target: String,
        local: bool,
    },
    /// No answer arrived in time.
    Timeout { secs: u64 },
    /// 401 or 403: the key is wrong, revoked or lacks access.
    Unauthorized { provider: AiProvider },
    /// 429.
    RateLimited,
    /// The server does not know the model.
    ModelNotFound { model: String, provider: AiProvider },
    /// Any other 4xx, with the server's own (cleaned) explanation if it gave one.
    Rejected { status: u16, message: String },
    /// 5xx.
    Server { status: u16 },
    /// The reply was larger than Sevak accepts.
    TooLarge,
    /// The reply was not the JSON that provider sends.
    Malformed,
    /// The reply held no text.
    Empty { cut_off: bool },
    /// The stored key could not be read back.
    KeyUnreadable,
}

impl AiError {
    /// A short heading for a result row.
    pub fn title(&self) -> String {
        match self {
            Self::NoKey { provider } => format!("No API key for {}", provider.label()),
            Self::BadSettings(_) => "The AI assistant settings need attention".to_owned(),
            Self::BadPrompt(_) => "That question cannot be sent".to_owned(),
            Self::Unreachable {
                provider: AiProvider::Ollama,
                local: true,
                ..
            } => "Ollama is not running".to_owned(),
            Self::Unreachable { .. } => "Could not reach the AI service".to_owned(),
            Self::Timeout { .. } => "The AI service took too long".to_owned(),
            Self::Unauthorized { .. } => "The AI service did not accept the API key".to_owned(),
            Self::RateLimited => "The AI service is busy or over its limit".to_owned(),
            Self::ModelNotFound { .. } => "That model was not found".to_owned(),
            Self::Rejected { .. } => "The AI service refused the request".to_owned(),
            Self::Server { .. } => "The AI service had a problem".to_owned(),
            Self::TooLarge => "The answer was too large".to_owned(),
            Self::Malformed => "The AI service sent a reply Sevak cannot read".to_owned(),
            Self::Empty { .. } => "The AI service sent an empty answer".to_owned(),
            Self::KeyUnreadable => "The stored API key could not be read".to_owned(),
        }
    }
}

impl fmt::Display for AiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoKey { provider } => {
                write!(
                    f,
                    "Add an API key for {} in Settings > AI assistant",
                    provider.label()
                )?;
                if let Some(var) = provider.key_env_var() {
                    write!(f, ", or set the {var} environment variable")?;
                }
                f.write_str(".")
            }
            Self::BadSettings(reason) | Self::BadPrompt(reason) => f.write_str(reason),
            Self::Unreachable {
                provider: AiProvider::Ollama,
                target,
                local: true,
            } => write!(
                f,
                "Nothing answered at {target}. Start Ollama (ollama serve) or fix the base URL in Settings > AI assistant."
            ),
            Self::Unreachable { target, .. } => write!(
                f,
                "Nothing answered at {target}. Check your internet connection and the base URL in Settings > AI assistant."
            ),
            Self::Timeout { secs } => write!(
                f,
                "No answer within {secs} seconds. A local model may still be loading; try again, or raise the timeout in Settings."
            ),
            Self::Unauthorized { provider } => write!(
                f,
                "{} rejected the API key. Check it in Settings > AI assistant.",
                provider.label()
            ),
            Self::RateLimited => f.write_str("Too many requests or no credit left. Try again later."),
            Self::ModelNotFound { model, provider } => {
                write!(f, "The model \u{201c}{model}\u{201d} is not available.")?;
                if *provider == AiProvider::Ollama {
                    write!(f, " Install it with: ollama pull {model}")
                } else {
                    f.write_str(" Check the model name in Settings > AI assistant.")
                }
            }
            Self::Rejected { status, message } if message.is_empty() => {
                write!(f, "The service answered with status {status}.")
            }
            Self::Rejected { message, .. } => write!(f, "The service said: {message}"),
            Self::Server { status } => {
                write!(f, "The service answered with status {status}. Try again later.")
            }
            Self::TooLarge => f.write_str(
                "The reply was larger than Sevak accepts and was discarded. Lower the maximum answer length.",
            ),
            Self::Malformed => f.write_str(
                "The reply was not in the expected format. Is the base URL right for this provider?",
            ),
            Self::Empty { cut_off: true } => f.write_str(
                "The model used its whole token budget before answering. Raise the maximum answer length in Settings.",
            ),
            Self::Empty { cut_off: false } => f.write_str("The model returned no text."),
            Self::KeyUnreadable => f.write_str(
                "The key was saved by another user or computer, or the file is damaged. Enter it again in Settings > AI assistant.",
            ),
        }
    }
}

impl std::error::Error for AiError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_actionable_and_name_no_secret() {
        let cases = [
            AiError::NoKey {
                provider: AiProvider::OpenAi,
            },
            AiError::Unreachable {
                provider: AiProvider::Ollama,
                target: "localhost:11434".into(),
                local: true,
            },
            AiError::Timeout { secs: 60 },
            AiError::Unauthorized {
                provider: AiProvider::Anthropic,
            },
            AiError::ModelNotFound {
                model: "llama9".into(),
                provider: AiProvider::Ollama,
            },
            AiError::TooLarge,
            AiError::Malformed,
            AiError::Empty { cut_off: true },
        ];
        for error in cases {
            assert!(!error.title().is_empty());
            assert!(error.to_string().len() > 20);
        }
        let text = AiError::NoKey {
            provider: AiProvider::OpenAi,
        }
        .to_string();
        assert!(text.contains("OPENAI_API_KEY"), "{text}");
        let local = AiError::Unreachable {
            provider: AiProvider::Ollama,
            target: "localhost:11434".into(),
            local: true,
        };
        assert!(local.to_string().contains("ollama serve"));
        assert_eq!(local.title(), "Ollama is not running");
    }
}
