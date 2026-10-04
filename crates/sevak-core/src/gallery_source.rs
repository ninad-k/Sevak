//! Where the galleries may be read from.
//!
//! Both online galleries (workflows and script plugins, and themes) are lists
//! kept in Sevak's repository. This module is the one place that says which
//! addresses they may use, so the workflow gallery, the theme gallery and the
//! download code all enforce the same rule:
//!
//! * **Allow-list.** An index, a package or a theme file is read only from
//!   `https://raw.githubusercontent.com/ninad-k/Sevak/` or from
//!   `https://github.com/ninad-k/Sevak/releases/download/`. Nothing else is
//!   accepted, as the first request or as a redirect target.
//! * **Pinned to a release.** A build reads the lists from the tag of its own
//!   version (`v<version>`), not from `main`, so a later change on `main`
//!   cannot alter what an installed build offers. Entries in an index name
//!   their files by a path relative to the repository root at that tag.
//!
//! The pure parts live here (no network); `sevak_plugins::net` does the
//! requests. See `docs/security/gallery-trust.md` for the design and its limits.

use url::Url;

/// The repository's raw-file address, with a trailing slash.
pub const RAW_PREFIX: &str = "https://raw.githubusercontent.com/ninad-k/Sevak/";
/// Where release assets (future packages) are downloaded from.
pub const RELEASE_PREFIX: &str = "https://github.com/ninad-k/Sevak/releases/download/";
/// Redirects to the newest stable (not draft, not pre-release) release's page.
pub const LATEST_RELEASE_URL: &str = "https://github.com/ninad-k/Sevak/releases/latest";

const RAW_HOST: &str = "raw.githubusercontent.com";
const GITHUB_HOST: &str = "github.com";
const REPO_PATH: &str = "/ninad-k/Sevak/";
const RELEASE_PATH: &str = "/ninad-k/Sevak/releases/download/";
const TAG_PAGE_PATH: &str = "/ninad-k/Sevak/releases/tag/";
/// The hosts a release download is redirected to by GitHub.
const RELEASE_CDN_HOSTS: [&str; 2] = [
    "release-assets.githubusercontent.com",
    "objects.githubusercontent.com",
];
/// The longest relative path an index entry may use.
const MAX_RELATIVE_BYTES: usize = 200;

/// A release tag the galleries are read from: `v` and three numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    tag: String,
}

impl Pin {
    /// The pin for `tag` (`v1.2.3`), or `None` for anything else.
    pub fn new(tag: &str) -> Option<Self> {
        let digits = tag.strip_prefix('v')?;
        let mut parts = digits.split('.');
        let plain = |part: Option<&str>| {
            part.is_some_and(|p| {
                !p.is_empty()
                    && p.len() <= 6
                    && p.bytes().all(|b| b.is_ascii_digit())
                    && (p == "0" || !p.starts_with('0'))
            })
        };
        let valid = plain(parts.next()) && plain(parts.next()) && plain(parts.next());
        (valid && parts.next().is_none()).then(|| Self {
            tag: tag.to_owned(),
        })
    }

    /// The pin for a release build of `version` (`1.2.3`): `None` for versions
    /// that have no release tag of their own (`1.2.3-beta.1`, build metadata).
    pub fn for_version(version: &str) -> Option<Self> {
        Self::new(&format!("v{version}"))
    }

    /// The pin for the running build.
    pub fn for_build() -> Option<Self> {
        Self::for_version(env!("CARGO_PKG_VERSION"))
    }

    pub fn tag(&self) -> &str {
        &self.tag
    }

    /// `https://raw.githubusercontent.com/ninad-k/Sevak/<tag>/`.
    pub fn root(&self) -> String {
        format!("{RAW_PREFIX}{}/", self.tag)
    }

    /// The address of `gallery/<file>` at this tag.
    pub fn index_url(&self, file: &str) -> String {
        format!("{}gallery/{file}", self.root())
    }

    /// The address of the file an index entry names.
    ///
    /// `source` is normally a path relative to the repository root at this tag
    /// (`gallery/packages/x.zip`): letters, digits, `.`, `_`, `-` and `/`, at
    /// most [`MAX_RELATIVE_BYTES`], no empty, `.` or `..` parts. An absolute
    /// `https://` address is accepted only when it lies below this tag's own
    /// raw address or release-download folder. Everything else (another host,
    /// another branch or tag, a query, an encoded path) is refused.
    pub fn resolve(&self, source: &str) -> Result<String, &'static str> {
        let source = source.trim();
        if source.is_empty() {
            return Err("the file address is empty");
        }
        if source.contains("://") || source.starts_with("//") {
            return self.check_absolute(source);
        }
        if source.len() > MAX_RELATIVE_BYTES
            || source.starts_with('/')
            || !source
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/'))
            || source
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err("the file address is not a plain path inside the release");
        }
        Ok(format!("{}{source}", self.root()))
    }

    fn check_absolute(&self, source: &str) -> Result<String, &'static str> {
        let url = Url::parse(source).map_err(|_| "the file address is not valid")?;
        if !is_allowed_url(&url) {
            return Err("the file address is not on the gallery's allow-list");
        }
        let path = url.path();
        let own = match url.host_str() {
            Some(RAW_HOST) => path.starts_with(&format!("{REPO_PATH}{}/", self.tag)),
            Some(GITHUB_HOST) => path.starts_with(&format!("{RELEASE_PATH}{}/", self.tag)),
            _ => false,
        };
        if own {
            Ok(url.to_string())
        } else {
            Err("the file address is not from this release")
        }
    }
}

/// Whether `url` may be requested at all: https, no user name, password or
/// port, no query or fragment, and a path below this repository on
/// `raw.githubusercontent.com` or below its release downloads on `github.com`.
/// The path is judged after the URL parser has resolved `.`, `..` and encoded
/// dots, so `/ninad-k/Sevak/../Other/x` is not below the repository.
pub fn is_allowed(url: &str) -> bool {
    Url::parse(url).is_ok_and(|url| is_allowed_url(&url))
}

fn is_allowed_url(url: &Url) -> bool {
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return false;
    }
    match url.host_str() {
        Some(RAW_HOST) => url.path().starts_with(REPO_PATH) && url.path().len() > REPO_PATH.len(),
        Some(GITHUB_HOST) => url.path().starts_with(RELEASE_PATH),
        _ => false,
    }
}

/// Whether a redirect from the request for `first` to `hop` may be followed:
/// `hop` must itself be on the allow-list, or, for a download that started at a
/// release asset, be GitHub's release storage (`release-assets.` and
/// `objects.githubusercontent.com`, https only, no user name or port).
pub fn is_allowed_redirect(first: &str, hop: &Url) -> bool {
    if is_allowed_url(hop) {
        return true;
    }
    first.starts_with(RELEASE_PREFIX)
        && hop.scheme() == "https"
        && hop.username().is_empty()
        && hop.password().is_none()
        && hop.port().is_none()
        && hop
            .host_str()
            .is_some_and(|h| RELEASE_CDN_HOSTS.contains(&h))
}

/// The tag in the address the "latest release" link redirected to
/// (`https://github.com/ninad-k/Sevak/releases/tag/v1.2.3`), or `None` if the
/// address is anything else.
pub fn tag_from_release_page(location: &str) -> Option<Pin> {
    let url = Url::parse(location).ok()?;
    if url.scheme() != "https"
        || url.host_str() != Some(GITHUB_HOST)
        || url.port().is_some()
        || !url.username().is_empty()
        || url.query().is_some()
    {
        return None;
    }
    Pin::new(url.path().strip_prefix(TAG_PAGE_PATH)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pin() -> Pin {
        Pin::new("v1.2.3").unwrap()
    }

    #[test]
    fn only_plain_release_tags_are_pins() {
        for good in ["v0.0.0", "v1.2.3", "v10.20.30", "v0.1.205"] {
            assert!(Pin::new(good).is_some(), "{good}");
        }
        for bad in [
            "",
            "v",
            "1.2.3",
            "v1.2",
            "v1.2.3.4",
            "v1.2.3-beta.1",
            "v1.2.3+meta",
            "v01.2.3",
            "v1.2.x",
            "v1.2.3/../main",
            "main",
            "V1.2.3",
            "v1.2.3 ",
            "v-1.2.3",
            "v1..3",
            "v1234567.0.0",
        ] {
            assert!(Pin::new(bad).is_none(), "{bad}");
        }
        assert_eq!(Pin::for_version("1.2.3").unwrap().tag(), "v1.2.3");
        assert!(Pin::for_version("1.2.3-beta.1").is_none());
        // The running build's own version is a plain release number.
        assert!(Pin::for_build().is_some());
    }

    #[test]
    fn addresses_are_built_at_the_tag() {
        assert_eq!(
            pin().root(),
            "https://raw.githubusercontent.com/ninad-k/Sevak/v1.2.3/"
        );
        assert_eq!(
            pin().index_url("index.json"),
            "https://raw.githubusercontent.com/ninad-k/Sevak/v1.2.3/gallery/index.json"
        );
        assert_eq!(
            pin().resolve("gallery/packages/x.zip").unwrap(),
            "https://raw.githubusercontent.com/ninad-k/Sevak/v1.2.3/gallery/packages/x.zip"
        );
    }

    #[test]
    fn relative_paths_stay_inside_the_release() {
        for bad in [
            "",
            "/gallery/x.zip",
            "../x.zip",
            "gallery/../../x.zip",
            "gallery//x.zip",
            "gallery/./x.zip",
            "gallery/x.zip?raw=1",
            "gallery/x.zip#a",
            "gallery/%2e%2e/x.zip",
            "gallery\\x.zip",
            "gallery/x y.zip",
            "gallery/x\n.zip",
            "gallery/x.zip/",
            "C:/x.zip",
        ] {
            assert!(pin().resolve(bad).is_err(), "{bad:?}");
        }
        assert!(pin()
            .resolve(&format!("gallery/{}", "a".repeat(250)))
            .is_err());
    }

    #[test]
    fn absolute_addresses_must_be_from_this_release() {
        let ok = [
            "https://raw.githubusercontent.com/ninad-k/Sevak/v1.2.3/gallery/x.zip",
            "https://github.com/ninad-k/Sevak/releases/download/v1.2.3/x.zip",
        ];
        for url in ok {
            assert_eq!(pin().resolve(url).unwrap(), url);
        }
        for bad in [
            // A branch, another tag, another repository, another host.
            "https://raw.githubusercontent.com/ninad-k/Sevak/main/gallery/x.zip",
            "https://raw.githubusercontent.com/ninad-k/Sevak/v1.2.4/gallery/x.zip",
            "https://raw.githubusercontent.com/ninad-k/Sevak/v1.2.30/gallery/x.zip",
            "https://raw.githubusercontent.com/ninad-k/Other/v1.2.3/gallery/x.zip",
            "https://raw.githubusercontent.com/evil/Sevak/v1.2.3/gallery/x.zip",
            "https://github.com/ninad-k/Sevak/releases/download/v1.2.4/x.zip",
            "https://github.com/ninad-k/Sevak/archive/v1.2.3.zip",
            "https://example.com/ninad-k/Sevak/v1.2.3/gallery/x.zip",
            "http://raw.githubusercontent.com/ninad-k/Sevak/v1.2.3/gallery/x.zip",
            // Tricks: dot segments, credentials, ports, queries, look-alike hosts.
            "https://raw.githubusercontent.com/ninad-k/Sevak/v1.2.3/../main/gallery/x.zip",
            "https://raw.githubusercontent.com/ninad-k/Sevak/v1.2.3/%2e%2e/main/x.zip",
            "https://raw.githubusercontent.com/ninad-k/Sevak/v1.2.3/%2E%2E/main/x.zip",
            "https://raw.githubusercontent.com@evil.example/ninad-k/Sevak/v1.2.3/x.zip",
            "https://user:pw@raw.githubusercontent.com/ninad-k/Sevak/v1.2.3/x.zip",
            "https://raw.githubusercontent.com:8443/ninad-k/Sevak/v1.2.3/x.zip",
            "https://raw.githubusercontent.com/ninad-k/Sevak/v1.2.3/x.zip?token=1",
            "https://raw.githubusercontent.com.evil.example/ninad-k/Sevak/v1.2.3/x.zip",
            "https://evilraw.githubusercontent.com/ninad-k/Sevak/v1.2.3/x.zip",
            "//raw.githubusercontent.com/ninad-k/Sevak/v1.2.3/x.zip",
            "ftp://raw.githubusercontent.com/ninad-k/Sevak/v1.2.3/x.zip",
        ] {
            assert!(pin().resolve(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_allow_list_is_exact() {
        for good in [
            "https://raw.githubusercontent.com/ninad-k/Sevak/main/gallery/index.json",
            "https://raw.githubusercontent.com/ninad-k/Sevak/v1.0.0/gallery/index.json",
            "https://github.com/ninad-k/Sevak/releases/download/v1.0.0/pack.zip",
        ] {
            assert!(is_allowed(good), "{good}");
        }
        for bad in [
            "",
            "not a url",
            "https://raw.githubusercontent.com/ninad-k/Sevak/",
            "https://raw.githubusercontent.com/ninad-k/Sevak",
            "https://raw.githubusercontent.com/ninad-k/sevak/main/x",
            "https://raw.githubusercontent.com/ninad-k/Sevak-evil/main/x",
            "https://raw.githubusercontent.com/ninad-k/Sevak/../Other/x",
            "https://raw.githubusercontent.com/ninad-k/Sevak/%2e%2e/Other/x",
            "https://github.com/ninad-k/Sevak/raw/main/gallery/index.json",
            "https://github.com/ninad-k/Sevak/releases/latest",
            "https://github.com/ninad-k/Sevak/releases/downloads/v1/x",
            "https://gist.githubusercontent.com/ninad-k/Sevak/x",
            "https://objects.githubusercontent.com/ninad-k/Sevak/x",
            "http://github.com/ninad-k/Sevak/releases/download/v1/x",
            "file:///ninad-k/Sevak/x",
        ] {
            assert!(!is_allowed(bad), "{bad:?}");
        }
    }

    #[test]
    fn redirects_stay_on_the_allow_list() {
        let hop = |s: &str| Url::parse(s).unwrap();
        let raw = "https://raw.githubusercontent.com/ninad-k/Sevak/v1.2.3/gallery/x.zip";
        let release = "https://github.com/ninad-k/Sevak/releases/download/v1.2.3/x.zip";
        // Between allowed addresses: fine.
        assert!(is_allowed_redirect(
            raw,
            &hop("https://raw.githubusercontent.com/ninad-k/Sevak/v1.2.3/gallery/y.zip")
        ));
        // Anywhere else is not, for a raw download...
        for bad in [
            "https://example.com/x.zip",
            "http://raw.githubusercontent.com/ninad-k/Sevak/v1.2.3/x",
            "https://release-assets.githubusercontent.com/github-production-release-asset/1",
            "https://raw.githubusercontent.com/evil/Sevak/v1.2.3/x",
        ] {
            assert!(!is_allowed_redirect(raw, &hop(bad)), "{bad}");
        }
        // ...and a release asset may also be served from GitHub's storage.
        for good in [
            "https://release-assets.githubusercontent.com/github-production-release-asset/1?sig=2",
            "https://objects.githubusercontent.com/github-production-release-asset-2e65be/1?sig=2",
        ] {
            assert!(is_allowed_redirect(release, &hop(good)), "{good}");
        }
        for bad in [
            "http://objects.githubusercontent.com/x",
            "https://objects.githubusercontent.com:444/x",
            "https://user@objects.githubusercontent.com/x",
            "https://evil.example/x",
            "https://objects.githubusercontent.com.evil.example/x",
        ] {
            assert!(!is_allowed_redirect(release, &hop(bad)), "{bad}");
        }
    }

    #[test]
    fn the_latest_release_page_names_a_plain_tag() {
        let tag = |s: &str| tag_from_release_page(s).map(|p| p.tag().to_owned());
        assert_eq!(
            tag("https://github.com/ninad-k/Sevak/releases/tag/v1.4.0").as_deref(),
            Some("v1.4.0")
        );
        for bad in [
            "https://github.com/ninad-k/Sevak/releases/tag/v1.4.0-beta.1",
            "https://github.com/ninad-k/Sevak/releases/tag/main",
            "https://github.com/ninad-k/Sevak/releases/tag/v1.4.0/extra",
            "https://github.com/ninad-k/Sevak/releases",
            "https://github.com/ninad-k/Other/releases/tag/v1.4.0",
            "https://example.com/ninad-k/Sevak/releases/tag/v1.4.0",
            "http://github.com/ninad-k/Sevak/releases/tag/v1.4.0",
            "https://github.com/ninad-k/Sevak/releases/tag/v1.4.0?x=1",
            "/ninad-k/Sevak/releases/tag/v1.4.0",
            "",
        ] {
            assert_eq!(tag(bad), None, "{bad}");
        }
    }
}
