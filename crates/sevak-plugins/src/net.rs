//! Downloading a file on the user's request: what the workflow gallery
//! ([`crate::workflow::gallery`]) and the theme gallery (Settings → Appearance)
//! share. Both fetch an index and then one file, only when the user clicks,
//! and check the file against the SHA-256 in the index
//! ([`sevak_core::checksum`]) before anything is written.
//!
//! The README's "Privacy and updates" lists every request Sevak makes; a new
//! caller of [`fetch_https`] belongs there.

use std::io::Read;
use std::time::Duration;

pub use sevak_core::checksum::{sha256_hex, verify_sha256};

/// How long one download may take.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
/// Redirects followed, all of them to `https://` addresses.
const MAX_REDIRECTS: usize = 5;

/// Downloads `url` (HTTPS only, redirects included) into memory, refusing
/// anything over `max_bytes`. One `GET`: no cookies, no identifiers beyond the
/// program name in the user agent. Blocking; call it from a background thread.
pub fn fetch_https(url: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
    if !url.to_ascii_lowercase().starts_with("https://") {
        return Err("only https:// addresses are downloaded".to_owned());
    }
    // reqwest builds its TLS config from the process-wide rustls provider.
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent(concat!("Sevak/", env!("CARGO_PKG_VERSION"), " (gallery)"))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.url().scheme() != "https" {
                attempt.error("a redirect to a non-https address")
            } else if attempt.previous().len() >= MAX_REDIRECTS {
                attempt.error("too many redirects")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|err| err.to_string())?;
    let response = client
        .get(url)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|err| format!("could not download it: {}", err.without_url()))?;
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err("the download is larger than expected".to_owned());
    }
    let mut body = Vec::new();
    response
        .take(max_bytes as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|err| format!("the download was interrupted: {err}"))?;
    if body.len() > max_bytes {
        return Err("the download is larger than expected".to_owned());
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_is_ever_requested() {
        for url in [
            "http://example.com/x.zip",
            "ftp://example.com/x",
            "file:///etc/passwd",
            "//example.com/x",
            "example.com/x",
        ] {
            let err = fetch_https(url, 1024).unwrap_err();
            assert!(err.contains("https"), "{url}: {err}");
        }
    }
}
