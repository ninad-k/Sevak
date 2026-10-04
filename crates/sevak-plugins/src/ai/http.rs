//! The one place the assistant touches the network.
//!
//! [`HttpTransport`] is what the providers are written against, so tests can
//! stand a canned transport or a loopback server in for the real thing.
//! [`ReqwestTransport`] is the real one: HTTP(S) only, no redirects (a
//! redirect could carry an API key to another host), no cookies, a total time
//! limit, and a hard cap on how much of the reply is read.

use std::fmt;
use std::io::Read;
use std::time::Duration;

use sevak_core::ai::parse_base_url;

/// How long connecting may take, at most (the total limit still applies).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

/// One request. Headers can carry an API key, so `Debug` never prints them.
#[derive(Clone)]
pub struct HttpRequest {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    /// The whole exchange, connecting and reading the reply included.
    pub timeout: Duration,
    /// A reply longer than this is an error, not a truncated success.
    pub max_response_bytes: usize,
}

impl HttpRequest {
    /// Whether a header that carries a credential is set.
    pub fn has_credentials(&self) -> bool {
        self.headers.iter().any(|(name, _)| {
            name.eq_ignore_ascii_case("authorization") || name.eq_ignore_ascii_case("x-api-key")
        })
    }
}

impl fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &self.url)
            .field(
                "headers",
                &format_args!("[{} redacted]", self.headers.len()),
            )
            .field("body_bytes", &self.body.as_ref().map(Vec::len))
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// The total time limit passed.
    Timeout,
    /// Nothing answered: refused, unknown host, no route, TLS failure.
    Connect,
    /// The reply is longer than `max_response_bytes`.
    TooLarge,
    /// Anything else (a reply that broke off, an address that is not a URL).
    Other,
}

pub trait HttpTransport: Send + Sync {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError>;
}

/// The real transport (blocking `reqwest`, rustls). Call it from a background
/// thread, never the typing path.
#[derive(Debug, Default, Clone, Copy)]
pub struct ReqwestTransport;

impl HttpTransport for ReqwestTransport {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError> {
        // reqwest builds its TLS config from the process-wide rustls provider.
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
        let loopback = parse_base_url(&request.url).is_ok_and(|url| url.is_loopback());
        let mut builder = reqwest::blocking::Client::builder()
            .timeout(request.timeout)
            .connect_timeout(CONNECT_TIMEOUT.min(request.timeout))
            .user_agent(concat!("Sevak/", env!("CARGO_PKG_VERSION"), " (ai)"))
            .redirect(reqwest::redirect::Policy::none());
        if loopback {
            // A model on this computer is not reached through a proxy.
            builder = builder.no_proxy();
        }
        let client = builder.build().map_err(|_| TransportError::Other)?;

        let mut call = match request.method {
            Method::Get => client.get(&request.url),
            Method::Post => client.post(&request.url),
        };
        for (name, value) in &request.headers {
            call = call.header(name.as_str(), value.as_str());
        }
        if let Some(body) = &request.body {
            call = call.body(body.clone());
        }
        let response = call.send().map_err(classify)?;

        let status = response.status().as_u16();
        if response
            .content_length()
            .is_some_and(|length| length > request.max_response_bytes as u64)
        {
            return Err(TransportError::TooLarge);
        }
        let mut body = Vec::new();
        response
            .take(request.max_response_bytes as u64 + 1)
            .read_to_end(&mut body)
            .map_err(|err| {
                let timed_out = err.kind() == std::io::ErrorKind::TimedOut
                    || err
                        .get_ref()
                        .and_then(|inner| inner.downcast_ref::<reqwest::Error>())
                        .is_some_and(reqwest::Error::is_timeout);
                if timed_out {
                    TransportError::Timeout
                } else {
                    TransportError::Other
                }
            })?;
        if body.len() > request.max_response_bytes {
            return Err(TransportError::TooLarge);
        }
        Ok(HttpResponse { status, body })
    }
}

fn classify(err: reqwest::Error) -> TransportError {
    if err.is_timeout() {
        TransportError::Timeout
    } else if err.is_connect() {
        TransportError::Connect
    } else if err.is_builder() || err.is_request() && !err.is_connect() {
        TransportError::Other
    } else {
        // The connection broke off after it was made, or TLS failed.
        TransportError::Connect
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::testserver::{Reply, TestServer};

    fn request(url: &str) -> HttpRequest {
        HttpRequest {
            method: Method::Post,
            url: url.to_owned(),
            headers: vec![("Authorization".into(), "Bearer sk-secret-value".into())],
            body: Some(b"{}".to_vec()),
            timeout: Duration::from_secs(5),
            max_response_bytes: 1024,
        }
    }

    #[test]
    fn debug_output_hides_headers() {
        let text = format!("{:?}", request("http://localhost/x"));
        assert!(!text.contains("sk-secret-value"), "{text}");
        assert!(!text.contains("Bearer"), "{text}");
        assert!(request("http://localhost/x").has_credentials());
    }

    #[test]
    fn a_reply_is_returned_with_its_status() {
        let server = TestServer::start(Reply::json(200, r#"{"ok":true}"#));
        let response = ReqwestTransport.send(&request(&server.url("/x"))).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, br#"{"ok":true}"#);
        let seen = server.requests();
        assert_eq!(seen.len(), 1);
        assert!(seen[0].head.starts_with("POST /x "));
        assert_eq!(seen[0].body, b"{}");
    }

    #[test]
    fn an_oversized_reply_is_an_error_not_a_truncated_success() {
        let server = TestServer::start(Reply::json(200, &"x".repeat(5_000)));
        assert_eq!(
            ReqwestTransport.send(&request(&server.url("/x"))),
            Err(TransportError::TooLarge)
        );
    }

    #[test]
    fn an_oversized_reply_without_a_length_is_caught_while_reading() {
        let server = TestServer::start(Reply::json(200, &"x".repeat(5_000)).chunked());
        assert_eq!(
            ReqwestTransport.send(&request(&server.url("/x"))),
            Err(TransportError::TooLarge)
        );
    }

    #[test]
    fn a_silent_server_times_out() {
        let server = TestServer::start(Reply::json(200, "{}").after(Duration::from_secs(3)));
        let mut slow = request(&server.url("/x"));
        slow.timeout = Duration::from_millis(300);
        assert_eq!(ReqwestTransport.send(&slow), Err(TransportError::Timeout));
    }

    #[test]
    fn a_stalled_body_times_out() {
        let server = TestServer::start(Reply::json(200, &"x".repeat(500)).stall_body());
        let mut slow = request(&server.url("/x"));
        slow.timeout = Duration::from_millis(400);
        assert_eq!(ReqwestTransport.send(&slow), Err(TransportError::Timeout));
    }

    #[test]
    fn nothing_listening_is_a_connect_error() {
        let port = TestServer::unused_port();
        assert_eq!(
            ReqwestTransport.send(&request(&format!("http://127.0.0.1:{port}/x"))),
            Err(TransportError::Connect)
        );
    }

    #[test]
    fn redirects_are_not_followed() {
        let server = TestServer::start(Reply::redirect("http://example.invalid/steal"));
        let response = ReqwestTransport.send(&request(&server.url("/x"))).unwrap();
        assert_eq!(response.status, 302);
        assert_eq!(server.requests().len(), 1);
    }

    #[test]
    fn an_address_that_is_not_a_url_is_refused() {
        assert_eq!(
            ReqwestTransport.send(&request("not a url")),
            Err(TransportError::Other)
        );
    }
}
