//! Asking a question: key lookup, safety checks, the request, the reply.
//!
//! [`Asker`] is what the plugin talks to, so its tests need no network at all.
//! [`Assistant`] is the real one: a [`Provider`] (wire format), a
//! [`KeyProvider`] (where the API key is) and an [`HttpTransport`] (the network),
//! each replaceable. Before anything is sent it checks that
//!
//! * the question is not empty or longer than [`MAX_PROMPT_CHARS`],
//! * the base URL is `http(s)://host` without credentials, and
//! * an API key is never about to travel over plain `http://` to another computer.

use std::sync::Arc;
use std::time::Instant;

use sevak_core::ai::{parse_base_url, BaseUrl};
use sevak_core::{AiConfig, AiProvider};

use super::error::AiError;
use super::http::{HttpRequest, HttpResponse, HttpTransport, ReqwestTransport, TransportError};
use super::keys::{scrub, ApiKey, KeyProvider, StoredOrEnv};
use super::provider::{provider_for, Answer, Provider, Settings};

/// Longest question that is sent, in characters.
pub const MAX_PROMPT_CHARS: usize = 8_000;
/// Largest request body that is sent, in bytes.
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;

/// Where a question would go: what the interface tells the user before they
/// press Enter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub provider: AiProvider,
    /// `host` or `host:port` of the base URL.
    pub host: String,
    pub model: String,
    /// The address is this computer: nothing leaves it.
    pub local: bool,
}

impl Target {
    /// "gpt-4o-mini at api.openai.com".
    pub fn describe(&self) -> String {
        format!("{} at {}", self.model, self.host)
    }
}

/// Something that can answer a question. The real one is [`Assistant`].
pub trait Asker: Send + Sync {
    /// Blocks until the answer arrives or fails; call it from a worker thread.
    fn ask(&self, prompt: &str) -> Result<Answer, AiError>;

    /// Where [`Asker::ask`] sends the question.
    fn target(&self) -> Target;
}

pub struct Assistant {
    settings: Settings,
    provider: Box<dyn Provider>,
    keys: Arc<dyn KeyProvider>,
    transport: Arc<dyn HttpTransport>,
}

impl Assistant {
    /// The real assistant for `config`: the stored or environment key, the real network.
    pub fn new(config: &AiConfig) -> Self {
        Self::with_parts(
            Settings::from_config(config),
            Arc::new(StoredOrEnv::system()),
            Arc::new(ReqwestTransport),
        )
    }

    pub fn with_parts(
        settings: Settings,
        keys: Arc<dyn KeyProvider>,
        transport: Arc<dyn HttpTransport>,
    ) -> Self {
        Self {
            provider: provider_for(settings.provider),
            settings,
            keys,
            transport,
        }
    }

    fn base(&self) -> Result<BaseUrl, AiError> {
        parse_base_url(&self.settings.base_url)
            .map_err(|reason| AiError::BadSettings(format!("The base URL {reason}.")))
    }

    /// The key to send, if the provider takes one, after the safety checks.
    fn key_for(&self, base: &BaseUrl) -> Result<Option<ApiKey>, AiError> {
        let key = self.keys.key(self.settings.provider)?;
        match &key {
            None if self.provider.needs_key(base) => Err(AiError::NoKey {
                provider: self.settings.provider,
            }),
            Some(_) if base.scheme != "https" && !base.is_loopback() => Err(AiError::BadSettings(
                "An API key is never sent over plain http:// to another computer. \
                     Use an https:// base URL."
                    .to_owned(),
            )),
            _ => Ok(key),
        }
    }

    fn send(&self, request: &HttpRequest, base: &BaseUrl) -> Result<HttpResponse, AiError> {
        if request
            .body
            .as_ref()
            .is_some_and(|b| b.len() > MAX_REQUEST_BYTES)
        {
            return Err(AiError::BadPrompt(
                "That request is too large to send.".to_owned(),
            ));
        }
        self.transport.send(request).map_err(|err| match err {
            TransportError::Timeout => AiError::Timeout {
                secs: self.settings.timeout.as_secs(),
            },
            TransportError::Connect => AiError::Unreachable {
                provider: self.settings.provider,
                target: base.authority(),
                local: base.is_loopback(),
            },
            TransportError::TooLarge => AiError::TooLarge,
            TransportError::Other => AiError::Malformed,
        })
    }

    /// Sends `prompt` and reads the answer.
    pub fn ask(&self, prompt: &str) -> Result<Answer, AiError> {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Err(AiError::BadPrompt(
                "Type a question after the keyword.".to_owned(),
            ));
        }
        let length = prompt.chars().count();
        if length > MAX_PROMPT_CHARS {
            return Err(AiError::BadPrompt(format!(
                "That question has {length} characters; the limit is {MAX_PROMPT_CHARS}."
            )));
        }
        let base = self.base()?;
        let key = self.key_for(&base)?;
        let request = self
            .provider
            .chat_request(&self.settings, key.as_ref(), prompt);

        let started = Instant::now();
        let outcome = self
            .send(&request, &base)
            .and_then(|response| self.provider.parse_chat(&self.settings, &response))
            .map_err(|err| scrub_error(err, key.as_ref()));
        // The question, the answer, the address and the key stay out of the log.
        tracing::info!(
            provider = self.settings.provider.id(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            ok = outcome.is_ok(),
            "asked the AI assistant"
        );
        outcome
    }

    /// Checks the address, the key and (for Ollama) that the model is installed,
    /// without sending any question. The answer is a sentence to show.
    pub fn test_connection(&self) -> Result<String, AiError> {
        let base = self.base()?;
        let key = self.key_for(&base)?;
        let request = self.provider.probe_request(&self.settings, key.as_ref());
        self.send(&request, &base)
            .and_then(|response| self.provider.parse_probe(&self.settings, &response))
            .map_err(|err| scrub_error(err, key.as_ref()))
    }

    pub fn target(&self) -> Target {
        let base = parse_base_url(&self.settings.base_url);
        Target {
            provider: self.settings.provider,
            host: base
                .as_ref()
                .map_or_else(|_| self.settings.base_url.clone(), BaseUrl::authority),
            model: self.settings.model.clone(),
            local: base.is_ok_and(|base| base.is_loopback()),
        }
    }
}

impl Asker for Assistant {
    fn ask(&self, prompt: &str) -> Result<Answer, AiError> {
        Assistant::ask(self, prompt)
    }

    fn target(&self) -> Target {
        Assistant::target(self)
    }
}

/// Removes the key (and key-shaped text) from the one error that quotes the server.
fn scrub_error(error: AiError, key: Option<&ApiKey>) -> AiError {
    match error {
        AiError::Rejected { status, message } => AiError::Rejected {
            status,
            message: scrub(&message, key),
        },
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use super::*;
    use crate::ai::keys::{KeyStore, KEY_FILE};
    use crate::ai::testserver::{Reply, TestServer};

    /// Keys that are not found in any environment or file.
    struct FixedKey(Option<&'static str>);

    impl KeyProvider for FixedKey {
        fn key(&self, provider: AiProvider) -> Result<Option<ApiKey>, AiError> {
            Ok(self
                .0
                .filter(|_| provider.key_env_var().is_some())
                .and_then(ApiKey::new))
        }
    }

    fn assistant(provider: AiProvider, base: &str, key: Option<&'static str>) -> Assistant {
        let mut settings = Settings::from_config(&AiConfig {
            provider,
            base_url: base.to_owned(),
            ..AiConfig::default()
        });
        settings.timeout = Duration::from_secs(5);
        Assistant::with_parts(
            settings,
            Arc::new(FixedKey(key)),
            Arc::new(ReqwestTransport),
        )
    }

    const OPENAI_OK: &str =
        r#"{"choices":[{"message":{"content":"The sky scatters blue."},"finish_reason":"stop"}]}"#;
    const ANTHROPIC_OK: &str =
        r#"{"content":[{"type":"text","text":"The sky scatters blue."}],"stop_reason":"end_turn"}"#;
    const OLLAMA_OK: &str =
        r#"{"message":{"role":"assistant","content":"The sky scatters blue."},"done":true}"#;

    /// Every provider, with a success body and the key it needs.
    fn providers() -> [(AiProvider, &'static str, Option<&'static str>); 3] {
        [
            (AiProvider::OpenAi, OPENAI_OK, Some("sk-test-openai-123456")),
            (
                AiProvider::Anthropic,
                ANTHROPIC_OK,
                Some("sk-ant-test-123456"),
            ),
            (AiProvider::Ollama, OLLAMA_OK, None),
        ]
    }

    #[test]
    fn each_provider_returns_the_answer() {
        for (provider, body, key) in providers() {
            let server = TestServer::start(Reply::json(200, body));
            let answer = assistant(provider, &server.base(), key)
                .ask("Why is the sky blue?")
                .unwrap_or_else(|err| panic!("{provider:?}: {err}"));
            assert_eq!(answer.text, "The sky scatters blue.", "{provider:?}");
            assert!(!answer.cut_off);

            let seen = server.requests();
            assert_eq!(seen.len(), 1, "{provider:?}");
            let sent = &seen[0];
            assert!(sent.head.starts_with("POST "), "{provider:?}");
            let json = sent.json();
            let prompt = match provider {
                AiProvider::Anthropic => json["messages"][0]["content"].clone(),
                _ => json["messages"][1]["content"].clone(),
            };
            assert_eq!(prompt, "Why is the sky blue?", "{provider:?}");
        }
    }

    #[test]
    fn the_key_travels_in_the_header_the_provider_expects_and_nowhere_else() {
        let server = TestServer::start(Reply::json(200, OPENAI_OK));
        assistant(
            AiProvider::OpenAi,
            &server.base(),
            Some("sk-test-openai-123456"),
        )
        .ask("hi")
        .unwrap();
        let sent = &server.requests()[0];
        assert_eq!(
            sent.header("authorization").as_deref(),
            Some("Bearer sk-test-openai-123456")
        );
        assert!(!String::from_utf8_lossy(&sent.body).contains("sk-test-openai"));
        assert!(!sent.path().contains("sk-test"));

        let server = TestServer::start(Reply::json(200, ANTHROPIC_OK));
        assistant(
            AiProvider::Anthropic,
            &server.base(),
            Some("sk-ant-test-123456"),
        )
        .ask("hi")
        .unwrap();
        let sent = &server.requests()[0];
        assert_eq!(
            sent.header("x-api-key").as_deref(),
            Some("sk-ant-test-123456")
        );
        assert_eq!(
            sent.header("anthropic-version").as_deref(),
            Some("2023-06-01")
        );
        assert!(!String::from_utf8_lossy(&sent.body).contains("sk-ant-test"));
    }

    #[test]
    fn a_wrong_key_is_a_friendly_error_that_does_not_echo_the_key() {
        for (provider, _, key) in providers().into_iter().take(2) {
            let server = TestServer::start(Reply::json(
                401,
                r#"{"error":{"message":"Incorrect API key provided: sk-test-openai-123456."}}"#,
            ));
            let err = assistant(provider, &server.base(), key)
                .ask("hi")
                .unwrap_err();
            assert_eq!(err, AiError::Unauthorized { provider }, "{provider:?}");
            let shown = format!("{} {err}", err.title());
            assert!(!shown.contains("sk-"), "{shown}");
        }
    }

    #[test]
    fn a_server_message_that_quotes_the_key_is_scrubbed() {
        let server = TestServer::start(Reply::json(
            400,
            r#"{"error":{"message":"bad request for token my-odd-token-value"}}"#,
        ));
        let err = assistant(
            AiProvider::OpenAi,
            &server.base(),
            Some("my-odd-token-value"),
        )
        .ask("hi")
        .unwrap_err();
        let shown = err.to_string();
        assert!(shown.contains("[key]"), "{shown}");
        assert!(!shown.contains("my-odd-token-value"), "{shown}");
    }

    #[test]
    fn a_missing_key_is_reported_before_anything_is_sent() {
        let server = TestServer::start(Reply::json(200, ANTHROPIC_OK));
        let err = assistant(AiProvider::Anthropic, &server.base(), None)
            .ask("hi")
            .unwrap_err();
        assert_eq!(
            err,
            AiError::NoKey {
                provider: AiProvider::Anthropic
            }
        );
        assert!(server.requests().is_empty(), "nothing may be sent");

        // A remote OpenAI-compatible server needs one; a local one does not.
        let remote = assistant(AiProvider::OpenAi, "https://api.openai.com/v1", None)
            .ask("hi")
            .unwrap_err();
        assert!(matches!(remote, AiError::NoKey { .. }));
        let local = TestServer::start(Reply::json(200, OPENAI_OK));
        assert!(assistant(AiProvider::OpenAi, &local.base(), None)
            .ask("hi")
            .is_ok());
    }

    #[test]
    fn a_key_is_never_sent_over_plain_http_to_another_computer() {
        let err = assistant(
            AiProvider::OpenAi,
            "http://192.168.1.50:8000/v1",
            Some("sk-test-openai-123456"),
        )
        .ask("hi")
        .unwrap_err();
        assert!(matches!(err, AiError::BadSettings(_)), "{err:?}");
        assert!(err.to_string().contains("https://"));
        // Without a key a LAN server over http is the user's own business.
        let no_key = assistant(AiProvider::Ollama, "http://192.168.1.50:11434", None);
        assert!(no_key.target().host.contains("192.168.1.50"));
    }

    #[test]
    fn unusable_base_urls_are_refused_before_anything_is_sent() {
        for bad in [
            "ftp://example.com",
            "https://user:pw@example.com",
            "nonsense",
        ] {
            let err = assistant(AiProvider::Ollama, bad, None)
                .ask("hi")
                .unwrap_err();
            assert!(matches!(err, AiError::BadSettings(_)), "{bad}: {err:?}");
        }
    }

    #[test]
    fn a_slow_server_times_out_with_a_friendly_message() {
        for (provider, body, key) in providers() {
            let server = TestServer::start(Reply::json(200, body).after(Duration::from_secs(3)));
            let mut settings = Settings::from_config(&AiConfig {
                provider,
                base_url: server.base(),
                ..AiConfig::default()
            });
            settings.timeout = Duration::from_millis(300);
            let assistant = Assistant::with_parts(
                settings,
                Arc::new(FixedKey(key)),
                Arc::new(ReqwestTransport),
            );
            let err = assistant.ask("hi").unwrap_err();
            assert_eq!(err, AiError::Timeout { secs: 0 }, "{provider:?}");
        }
        assert!(AiError::Timeout { secs: 60 }
            .to_string()
            .contains("60 seconds"));
    }

    #[test]
    fn malformed_json_is_reported() {
        for (provider, _, key) in providers() {
            let server = TestServer::start(Reply::json(200, "{ this is not json"));
            let err = assistant(provider, &server.base(), key)
                .ask("hi")
                .unwrap_err();
            assert_eq!(err, AiError::Malformed, "{provider:?}");
        }
    }

    #[test]
    fn an_oversized_reply_is_refused() {
        for (provider, _, key) in providers() {
            let huge = format!(r#"{{"junk":"{}"}}"#, "x".repeat(2 * 1024 * 1024));
            let server = TestServer::start(Reply::json(200, &huge));
            let err = assistant(provider, &server.base(), key)
                .ask("hi")
                .unwrap_err();
            assert_eq!(err, AiError::TooLarge, "{provider:?}");
        }
    }

    #[test]
    fn an_unreachable_ollama_says_how_to_start_it() {
        let port = TestServer::unused_port();
        let err = assistant(
            AiProvider::Ollama,
            &format!("http://127.0.0.1:{port}"),
            None,
        )
        .ask("hi")
        .unwrap_err();
        assert!(matches!(
            err,
            AiError::Unreachable {
                provider: AiProvider::Ollama,
                local: true,
                ..
            }
        ));
        assert!(err.to_string().contains("ollama serve"));
        assert_eq!(err.title(), "Ollama is not running");
    }

    #[test]
    fn rate_limits_and_server_errors_are_told_apart() {
        let busy = TestServer::start(Reply::json(429, "{}"));
        assert_eq!(
            assistant(AiProvider::Ollama, &busy.base(), None).ask("hi"),
            Err(AiError::RateLimited)
        );
        let down = TestServer::start(Reply::json(500, "oops"));
        assert_eq!(
            assistant(AiProvider::Ollama, &down.base(), None).ask("hi"),
            Err(AiError::Server { status: 500 })
        );
    }

    #[test]
    fn a_redirect_is_not_followed_and_the_key_stays_put() {
        let target = TestServer::start(Reply::json(200, OPENAI_OK));
        let redirector = TestServer::start(Reply::redirect(&target.url("/v1/chat/completions")));
        let err = assistant(
            AiProvider::OpenAi,
            &redirector.base(),
            Some("sk-test-openai-123456"),
        )
        .ask("hi")
        .unwrap_err();
        assert!(
            matches!(err, AiError::Rejected { status: 302, .. }),
            "{err:?}"
        );
        assert!(
            target.requests().is_empty(),
            "the redirect must not be followed"
        );
    }

    #[test]
    fn bad_questions_are_refused_without_a_request() {
        let server = TestServer::start(Reply::json(200, OLLAMA_OK));
        let assistant = assistant(AiProvider::Ollama, &server.base(), None);
        assert!(matches!(assistant.ask("   "), Err(AiError::BadPrompt(_))));
        let long = "q".repeat(MAX_PROMPT_CHARS + 1);
        let err = assistant.ask(&long).unwrap_err();
        assert!(err.to_string().contains("limit"), "{err}");
        assert!(server.requests().is_empty());
        assert!(assistant.ask(&"q".repeat(MAX_PROMPT_CHARS)).is_ok());
    }

    #[test]
    fn the_connection_test_sends_no_question() {
        let server = TestServer::start(Reply::json(
            200,
            r#"{"models":[{"name":"llama3.2:latest"}]}"#,
        ));
        let message = assistant(AiProvider::Ollama, &server.base(), None)
            .test_connection()
            .unwrap();
        assert!(message.contains("llama3.2"), "{message}");
        let seen = server.requests();
        assert!(seen[0].head.starts_with("GET /api/tags"));
        assert!(seen[0].body.is_empty());

        let openai = TestServer::start(Reply::json(200, r#"{"data":[{"id":"gpt-4o-mini"}]}"#));
        let message = assistant(
            AiProvider::OpenAi,
            &openai.base(),
            Some("sk-test-openai-123456"),
        )
        .test_connection()
        .unwrap();
        assert!(message.starts_with("Connected to 127.0.0.1"), "{message}");
        assert!(openai.requests()[0].head.starts_with("GET /models"));

        let rejected = TestServer::start(Reply::json(401, "{}"));
        assert!(matches!(
            assistant(
                AiProvider::Anthropic,
                &rejected.base(),
                Some("sk-ant-test-123456")
            )
            .test_connection(),
            Err(AiError::Unauthorized { .. })
        ));
    }

    #[test]
    fn the_stored_key_is_what_gets_sent() {
        let dir = tempfile::tempdir().unwrap();
        let store = KeyStore::new(dir.path().join(KEY_FILE));
        store
            .set(
                AiProvider::OpenAi,
                &ApiKey::new("sk-stored-in-settings").unwrap(),
            )
            .unwrap();
        let keys = StoredOrEnv::with_parts(Some(store), Arc::new(|_| None));
        let server = TestServer::start(Reply::json(200, OPENAI_OK));
        let settings = Settings::from_config(&AiConfig {
            provider: AiProvider::OpenAi,
            base_url: server.base(),
            ..AiConfig::default()
        });
        Assistant::with_parts(settings, Arc::new(keys), Arc::new(ReqwestTransport))
            .ask("hi")
            .unwrap();
        assert_eq!(
            server.requests()[0].header("authorization").as_deref(),
            Some("Bearer sk-stored-in-settings")
        );
    }

    #[test]
    fn the_target_names_the_host_and_whether_it_is_local() {
        let local = assistant(AiProvider::Ollama, "http://localhost:11434", None).target();
        assert_eq!(local.host, "localhost:11434");
        assert!(local.local);
        assert_eq!(local.describe(), "llama3.2 at localhost:11434");
        let cloud = assistant(
            AiProvider::OpenAi,
            "https://api.openai.com/v1",
            Some("sk-x-x-x-x-x"),
        )
        .target();
        assert_eq!(cloud.host, "api.openai.com");
        assert!(!cloud.local);
    }

    /// A transport that records the requests it is given (no network at all).
    struct Recording(Mutex<Vec<HttpRequest>>);

    impl HttpTransport for Recording {
        fn send(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError> {
            self.0.lock().unwrap().push(request.clone());
            Ok(HttpResponse {
                status: 200,
                body: OLLAMA_OK.as_bytes().to_vec(),
            })
        }
    }

    #[test]
    fn a_mock_transport_can_stand_in_for_the_network() {
        let transport = Arc::new(Recording(Mutex::new(Vec::new())));
        let settings = Settings::from_config(&AiConfig::default());
        let assistant =
            Assistant::with_parts(settings, Arc::new(FixedKey(None)), transport.clone());
        assert_eq!(
            assistant.ask("hello").unwrap().text,
            "The sky scatters blue."
        );
        let sent = transport.0.lock().unwrap();
        assert_eq!(sent[0].url, "http://localhost:11434/api/chat");
        assert_eq!(sent[0].timeout, Duration::from_secs(60));
    }
}
