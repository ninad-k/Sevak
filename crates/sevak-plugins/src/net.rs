//! Downloading a file on the user's request: what the workflow gallery
//! ([`crate::workflow::gallery`]) and the theme gallery (Settings → Appearance)
//! share. Both fetch an index and then one file, only when the user clicks,
//! and check the file against the SHA-256 in the index
//! ([`sevak_core::checksum`]) before anything is written.
//!
//! Only addresses on the allow-list in [`sevak_core::gallery_source`] are
//! requested, as the first request and at every redirect: the Sevak
//! repository's raw files and release downloads, nothing else. The indexes are
//! read from the tag of the running build ([`fetch_pinned`]), so a later change
//! on `main` cannot alter what a shipped build installs.
//!
//! The README's "Privacy and updates" lists every request Sevak makes; a new
//! caller of [`fetch_https`] belongs there.

use std::fmt;
use std::io::Read;
use std::time::Duration;

use sevak_core::gallery_source::{self, Pin};
use url::Url;

pub use sevak_core::checksum::{sha256_hex, verify_sha256};

/// How long one download may take.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
/// Redirects followed, all of them to addresses on the allow-list.
const MAX_REDIRECTS: usize = 5;

/// Why a download failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// The server has no such file (HTTP 404).
    NotFound,
    /// Anything else, in words for the user.
    Failed(String),
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => f.write_str("it is not there (404)"),
            Self::Failed(why) => f.write_str(why),
        }
    }
}

/// What the gallery code needs from the network. [`Https`] is the real one;
/// tests use a fake, so no test touches the network.
pub trait Transport {
    /// Downloads `url` into memory, at most `max_bytes`.
    fn get(&self, url: &str, max_bytes: usize) -> Result<Vec<u8>, FetchError>;
    /// The address the repository's "latest release" link redirects to.
    fn latest_release_page(&self) -> Result<String, String>;
}

/// The real network: HTTPS only, allow-listed addresses only, a timeout, no
/// cookies and no identifiers beyond the program name in the user agent.
pub struct Https;

/// Downloads `url` (an allow-listed HTTPS address, redirects included) into
/// memory, refusing anything over `max_bytes`. One `GET`. Blocking; call it
/// from a background thread.
pub fn fetch_https(url: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
    Https.get(url, max_bytes).map_err(|err| err.to_string())
}

/// Whether a redirect (the `previous` addresses so far, ending in the one that
/// redirected) to `hop` may be followed.
fn follow_redirect(first: &str, hop: &Url, previous: usize) -> Result<(), &'static str> {
    if previous >= MAX_REDIRECTS {
        Err("too many redirects")
    } else if !gallery_source::is_allowed_redirect(first, hop) {
        Err("a redirect to an address outside the Sevak repository")
    } else {
        Ok(())
    }
}

fn client(policy: reqwest::redirect::Policy) -> Result<reqwest::blocking::Client, String> {
    // reqwest builds its TLS config from the process-wide rustls provider.
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    reqwest::blocking::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent(concat!("Sevak/", env!("CARGO_PKG_VERSION"), " (gallery)"))
        .redirect(policy)
        .build()
        .map_err(|err| err.to_string())
}

impl Transport for Https {
    fn get(&self, url: &str, max_bytes: usize) -> Result<Vec<u8>, FetchError> {
        if !gallery_source::is_allowed(url) {
            return Err(FetchError::Failed(
                "only https:// files in Sevak's own repository are downloaded".to_owned(),
            ));
        }
        let first = url.to_owned();
        let policy = reqwest::redirect::Policy::custom(move |attempt| {
            match follow_redirect(&first, attempt.url(), attempt.previous().len()) {
                Ok(()) => attempt.follow(),
                Err(why) => attempt.error(why),
            }
        });
        let response = client(policy)
            .map_err(FetchError::Failed)?
            .get(url)
            .send()
            .map_err(|err| {
                FetchError::Failed(format!("could not download it: {}", err.without_url()))
            })?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(FetchError::NotFound);
        }
        let response = response.error_for_status().map_err(|err| {
            FetchError::Failed(format!("could not download it: {}", err.without_url()))
        })?;
        if response
            .content_length()
            .is_some_and(|length| length > max_bytes as u64)
        {
            return Err(FetchError::Failed(
                "the download is larger than expected".to_owned(),
            ));
        }
        let mut body = Vec::new();
        response
            .take(max_bytes as u64 + 1)
            .read_to_end(&mut body)
            .map_err(|err| FetchError::Failed(format!("the download was interrupted: {err}")))?;
        if body.len() > max_bytes {
            return Err(FetchError::Failed(
                "the download is larger than expected".to_owned(),
            ));
        }
        Ok(body)
    }

    fn latest_release_page(&self) -> Result<String, String> {
        let response = client(reqwest::redirect::Policy::none())?
            .get(gallery_source::LATEST_RELEASE_URL)
            .send()
            .map_err(|err| {
                format!(
                    "could not look up the latest release: {}",
                    err.without_url()
                )
            })?;
        if !response.status().is_redirection() {
            return Err("the repository has no published release yet".to_owned());
        }
        let location = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .ok_or("the latest release link did not say where it leads")?;
        Url::parse(gallery_source::LATEST_RELEASE_URL)
            .and_then(|base| base.join(location))
            .map(|url| url.to_string())
            .map_err(|_| "the latest release link did not lead anywhere valid".to_owned())
    }
}

/// An index fetched from a tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pinned {
    /// The release the index (and so every file it names) is read from.
    pub pin: Pin,
    pub body: Vec<u8>,
    /// Set when the index is not from this build's own release: what happened,
    /// in words for the user.
    pub note: Option<String>,
}

/// Fetches `gallery/<file>` from the tag of the running build (`build`, from
/// [`Pin::for_build`]).
///
/// A build whose tag does not exist (a local or test build, a pre-release) is
/// not an error for the user: the index of the **latest stable release** is
/// used instead and [`Pinned::note`] says so. The fallback is taken only when
/// the tag is missing (404) or the build has none; a network failure is
/// reported as it is, never answered with different content.
pub fn fetch_pinned(
    transport: &dyn Transport,
    build: Option<&Pin>,
    file: &str,
    max_bytes: usize,
) -> Result<Pinned, String> {
    let version = env!("CARGO_PKG_VERSION");
    if let Some(pin) = build {
        match transport.get(&pin.index_url(file), max_bytes) {
            Ok(body) => {
                return Ok(Pinned {
                    pin: pin.clone(),
                    body,
                    note: None,
                });
            }
            Err(FetchError::NotFound) => {}
            Err(err) => return Err(format!("Could not load the gallery: {err}.")),
        }
    }

    let why = match build {
        Some(pin) => format!(
            "This build (version {version}) has no published release {}",
            pin.tag()
        ),
        None => format!("This build (version {version}) is not a released version"),
    };
    let page = transport
        .latest_release_page()
        .map_err(|err| format!("{why}, and the latest release could not be found: {err}."))?;
    let latest = gallery_source::tag_from_release_page(&page).ok_or_else(|| {
        format!("{why}, and the latest release has no version tag Sevak can use.")
    })?;
    if build == Some(&latest) {
        return Err(format!(
            "The gallery is not published for release {}.",
            latest.tag()
        ));
    }
    let body = transport
        .get(&latest.index_url(file), max_bytes)
        .map_err(|err| {
            format!(
                "{why}; the latest release {} has no gallery: {err}.",
                latest.tag()
            )
        })?;
    let note = format!(
        "{why}, so the gallery of the latest release ({}) is shown. Its files are not from this build.",
        latest.tag()
    );
    Ok(Pinned {
        pin: latest,
        body,
        note: Some(note),
    })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn only_allow_listed_https_is_ever_requested() {
        for url in [
            "http://raw.githubusercontent.com/ninad-k/Sevak/main/x.zip",
            "ftp://example.com/x",
            "file:///etc/passwd",
            "//example.com/x",
            "example.com/x",
            "https://example.com/x.zip",
            "https://raw.githubusercontent.com/someone-else/repo/main/x.zip",
            "https://user@raw.githubusercontent.com/ninad-k/Sevak/main/x.zip",
        ] {
            let err = fetch_https(url, 1024).unwrap_err();
            assert!(err.contains("https"), "{url}: {err}");
        }
    }

    #[test]
    fn redirects_are_limited_in_number_and_destination() {
        let hop = |s: &str| Url::parse(s).unwrap();
        let first = "https://raw.githubusercontent.com/ninad-k/Sevak/v1.0.0/gallery/index.json";
        let own = hop("https://raw.githubusercontent.com/ninad-k/Sevak/v1.0.0/gallery/other.json");
        assert!(follow_redirect(first, &own, 1).is_ok());
        assert!(follow_redirect(first, &own, MAX_REDIRECTS).is_err());
        assert!(follow_redirect(first, &hop("https://example.com/x"), 0).is_err());
        assert!(follow_redirect(
            first,
            &hop("http://raw.githubusercontent.com/ninad-k/Sevak/v1/x"),
            0
        )
        .is_err());
    }

    /// Serves canned answers and records what was asked.
    struct Fake {
        files: HashMap<String, Vec<u8>>,
        latest: Result<String, String>,
        asked: RefCell<Vec<String>>,
        failing: bool,
    }

    impl Fake {
        fn new(files: &[(&str, &str)], latest: Result<&str, &str>) -> Self {
            Self {
                files: files
                    .iter()
                    .map(|(url, body)| ((*url).to_owned(), body.as_bytes().to_vec()))
                    .collect(),
                latest: latest.map(str::to_owned).map_err(str::to_owned),
                asked: RefCell::new(Vec::new()),
                failing: false,
            }
        }
    }

    impl Transport for Fake {
        fn get(&self, url: &str, max_bytes: usize) -> Result<Vec<u8>, FetchError> {
            self.asked.borrow_mut().push(url.to_owned());
            if self.failing {
                return Err(FetchError::Failed("no network".to_owned()));
            }
            let body = self.files.get(url).ok_or(FetchError::NotFound)?;
            if body.len() > max_bytes {
                return Err(FetchError::Failed(
                    "the download is larger than expected".into(),
                ));
            }
            Ok(body.clone())
        }

        fn latest_release_page(&self) -> Result<String, String> {
            self.latest.clone()
        }
    }

    const RAW: &str = "https://raw.githubusercontent.com/ninad-k/Sevak";

    fn pin(tag: &str) -> Pin {
        Pin::new(tag).unwrap()
    }

    #[test]
    fn a_build_reads_the_index_of_its_own_tag() {
        let url = format!("{RAW}/v1.2.3/gallery/index.json");
        let fake = Fake::new(&[(&url, "{}")], Err("not asked"));
        let got = fetch_pinned(&fake, Some(&pin("v1.2.3")), "index.json", 100).unwrap();
        assert_eq!(got.pin.tag(), "v1.2.3");
        assert_eq!(got.body, b"{}");
        assert_eq!(got.note, None);
        assert_eq!(*fake.asked.borrow(), [url], "main is never asked for");
    }

    #[test]
    fn a_missing_tag_falls_back_to_the_latest_release_with_a_note() {
        let latest = format!("{RAW}/v1.4.0/gallery/index.json");
        let fake = Fake::new(
            &[(&latest, "latest")],
            Ok("https://github.com/ninad-k/Sevak/releases/tag/v1.4.0"),
        );
        let got = fetch_pinned(&fake, Some(&pin("v9.9.9")), "index.json", 100).unwrap();
        assert_eq!(got.pin.tag(), "v1.4.0");
        assert_eq!(got.body, b"latest");
        let note = got.note.unwrap();
        assert!(note.contains("v9.9.9") && note.contains("v1.4.0"), "{note}");
        assert!(note.contains("latest release"), "{note}");
        assert_eq!(fake.asked.borrow().len(), 2);
    }

    #[test]
    fn a_build_without_a_tag_goes_straight_to_the_latest_release() {
        let latest = format!("{RAW}/v1.4.0/gallery/themes.json");
        let fake = Fake::new(
            &[(&latest, "t")],
            Ok("https://github.com/ninad-k/Sevak/releases/tag/v1.4.0"),
        );
        let got = fetch_pinned(&fake, None, "themes.json", 100).unwrap();
        assert_eq!(got.pin.tag(), "v1.4.0");
        assert!(got.note.unwrap().contains("not a released version"));
        assert_eq!(*fake.asked.borrow(), [latest]);
    }

    #[test]
    fn a_network_failure_is_not_answered_with_other_content() {
        let latest = format!("{RAW}/v1.4.0/gallery/index.json");
        let mut fake = Fake::new(
            &[(&latest, "latest")],
            Ok("https://github.com/ninad-k/Sevak/releases/tag/v1.4.0"),
        );
        fake.failing = true;
        let err = fetch_pinned(&fake, Some(&pin("v1.2.3")), "index.json", 100).unwrap_err();
        assert!(err.contains("no network"), "{err}");
        assert_eq!(fake.asked.borrow().len(), 1, "no fallback after a failure");
    }

    #[test]
    fn the_fallback_fails_clearly() {
        // No published release at all.
        let fake = Fake::new(&[], Err("the repository has no published release yet"));
        let err = fetch_pinned(&fake, None, "index.json", 100).unwrap_err();
        assert!(err.contains("no published release"), "{err}");
        // The latest release redirects somewhere that is not a version tag.
        let fake = Fake::new(
            &[],
            Ok("https://github.com/ninad-k/Sevak/releases/tag/nightly"),
        );
        let err = fetch_pinned(&fake, None, "index.json", 100).unwrap_err();
        assert!(err.contains("no version tag"), "{err}");
        let fake = Fake::new(
            &[],
            Ok("https://evil.example/ninad-k/Sevak/releases/tag/v1.0.0"),
        );
        assert!(fetch_pinned(&fake, None, "index.json", 100).is_err());
        // The latest release exists but publishes no gallery.
        let fake = Fake::new(
            &[],
            Ok("https://github.com/ninad-k/Sevak/releases/tag/v1.0.0"),
        );
        let err = fetch_pinned(&fake, None, "index.json", 100).unwrap_err();
        assert!(err.contains("has no gallery"), "{err}");
        // The build's own tag is the latest one and still has no gallery: stop.
        let fake = Fake::new(
            &[],
            Ok("https://github.com/ninad-k/Sevak/releases/tag/v1.0.0"),
        );
        let err = fetch_pinned(&fake, Some(&pin("v1.0.0")), "index.json", 100).unwrap_err();
        assert!(err.contains("not published"), "{err}");
        assert_eq!(fake.asked.borrow().len(), 1);
    }

    #[test]
    fn the_size_cap_applies_to_the_index() {
        let url = format!("{RAW}/v1.2.3/gallery/index.json");
        let fake = Fake::new(&[(&url, "0123456789")], Err("x"));
        let err = fetch_pinned(&fake, Some(&pin("v1.2.3")), "index.json", 5).unwrap_err();
        assert!(err.contains("larger than expected"), "{err}");
    }
}
