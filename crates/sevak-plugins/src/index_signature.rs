//! Signed gallery indexes.
//!
//! The gallery lists (`gallery/index.json` for workflows, script plugins and
//! native extensions; `gallery/themes.json` for themes) are read from the
//! release tag of the running build (`sevak_core::gallery_source::Pin`). Since
//! [`SIGNED_SINCE`], the release workflow also signs the exact bytes of both
//! files with the updater key (the Tauri signer, as for the advisory feed) and
//! attaches the signature to the release:
//!
//! ```text
//! https://raw.githubusercontent.com/ninad-k/Sevak/<tag>/gallery/index.json
//! https://github.com/ninad-k/Sevak/releases/download/<tag>/index.json.sig
//! ```
//!
//! Sevak downloads the signature next to the list and checks it **before the
//! list is parsed**, with the minisign check and the embedded key the advisory
//! feed uses (`crate::extensions::revocations`). The file is signed staged as
//! `<tag>-<file>` (`v1.2.3-index.json`), so the signed trusted comment names
//! the release and the list: a signature for another release, for the other
//! list, for the advisory feed or for an installer is refused.
//!
//! ## Rollout policy
//!
//! * A signature that is present is always checked. An invalid, altered,
//!   oversized or wrongly bound signature is refused for every tag.
//! * A missing signature (HTTP 404) is accepted only for a tag older than
//!   [`SIGNED_SINCE`]: releases published before signing existed. The page
//!   then does not call the list signed.
//! * For [`SIGNED_SINCE`] and every later tag a missing signature is refused,
//!   so removing the `.sig` from a release cannot downgrade it to unsigned.
//!
//! The SHA-256 of every package and theme in a list is checked as before; a
//! signed list makes those hashes come from the release key's holder.

use serde::{Deserialize, Serialize};
use sevak_core::gallery_source::{Pin, RELEASE_PREFIX};

use crate::extensions::revocations::{authenticate, trusted_key, AuthError};
use crate::net::{FetchError, Transport};

/// The first release whose gallery lists are signed. Releases before it have
/// no `.sig` assets, so their lists are accepted unsigned; from it on a list
/// without a signature is refused. It must be no later than the first release
/// built by a release workflow that signs the lists (see docs/releasing.md,
/// "Gallery index signatures").
pub const SIGNED_SINCE: &str = "v0.14.1";

/// The largest `.sig` accepted (a Tauri-signer signature is about 400 bytes).
pub const MAX_SIGNATURE_BYTES: usize = 8 * 1024;

/// The lists that are signed, with the size each may have. Anything else has
/// no signature policy and is refused.
const SIGNED_FILES: [(&str, usize); 2] = [
    (
        crate::workflow::gallery::INDEX_FILE,
        crate::workflow::gallery::MAX_INDEX_BYTES,
    ),
    (
        sevak_core::theme_store::GALLERY_INDEX_FILE,
        sevak_core::theme_store::MAX_INDEX_BYTES as usize,
    ),
];

/// What the signature check found for a list that was accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexSignature {
    /// Signed by the embedded release key for this list at this release.
    Verified,
    /// No signature, from a release published before lists were signed.
    LegacyUnsigned,
}

/// Where the signature of `gallery/<file>` at `pin` is published.
pub fn signature_url(pin: &Pin, file: &str) -> String {
    format!("{RELEASE_PREFIX}{}/{file}.sig", pin.tag())
}

/// The name the release workflow gives the list when signing it, which the
/// Tauri signer writes into the signed trusted comment as `file:<name>`.
pub fn signed_name(tag: &str, file: &str) -> String {
    format!("{tag}-{file}")
}

/// `vX.Y.Z` as numbers, for comparing a tag with [`SIGNED_SINCE`]. Tags with a
/// pre-release part (`v1.2.0-beta.1`) count as their `X.Y.Z`.
fn numbers(tag: &str) -> Option<(u64, u64, u64)> {
    let core = tag.strip_prefix('v')?;
    let core = core.split_once('-').map_or(core, |(core, _)| core);
    let mut parts = core.split('.').map(|part| part.parse::<u64>().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

/// The key and the rollout threshold a check uses.
#[derive(Debug, Clone)]
pub(crate) struct Trust {
    pub(crate) key: String,
    pub(crate) signed_since: (u64, u64, u64),
}

impl Trust {
    /// What a shipped build uses: the embedded updater key and [`SIGNED_SINCE`].
    pub(crate) fn release() -> Self {
        Self {
            key: trusted_key(),
            signed_since: numbers(SIGNED_SINCE).expect("SIGNED_SINCE is a vX.Y.Z tag"),
        }
    }
}

#[cfg(test)]
thread_local! {
    static TEST_TRUST: std::cell::RefCell<Option<Trust>> = const { std::cell::RefCell::new(None) };
}

/// Runs `f` with `trust` as this thread's trust (tests only).
#[cfg(test)]
pub(crate) fn with_trust<T>(trust: Trust, f: impl FnOnce() -> T) -> T {
    struct Reset(Option<Trust>);
    impl Drop for Reset {
        fn drop(&mut self) {
            let previous = self.0.take();
            TEST_TRUST.with(|t| *t.borrow_mut() = previous);
        }
    }
    let _reset = Reset(TEST_TRUST.with(|t| t.borrow_mut().replace(trust)));
    f()
}

#[cfg(not(test))]
fn current() -> Trust {
    Trust::release()
}

/// In this crate's unit tests the shipped key is replaced by the TEST key of
/// the fixtures, and every tag counts as published before signing unless a
/// test sets a trust ([`with_trust`]); the policy itself is tested with
/// explicit trusts, [`Trust::release`] included.
#[cfg(test)]
fn current() -> Trust {
    TEST_TRUST
        .with(|t| t.borrow().clone())
        .unwrap_or_else(|| Trust {
            key: include_str!("index_signature/fixtures/public-key.txt")
                .trim()
                .to_owned(),
            signed_since: (u64::MAX, 0, 0),
        })
}

/// Checks the exact downloaded `body` of `gallery/<file>` at release `tag`
/// against `signature` (`None`: the release has none), before anything parses
/// it. See the module documentation for the policy.
pub fn verify(
    tag: &str,
    file: &str,
    body: &[u8],
    signature: Option<&[u8]>,
) -> Result<IndexSignature, String> {
    verify_with(&current(), tag, file, body, signature)
}

pub(crate) fn verify_with(
    trust: &Trust,
    tag: &str,
    file: &str,
    body: &[u8],
    signature: Option<&[u8]>,
) -> Result<IndexSignature, String> {
    let Some(&(_, max_bytes)) = SIGNED_FILES.iter().find(|(name, _)| *name == file) else {
        return Err(format!("{file} is not a signed gallery list"));
    };
    let release = numbers(tag).ok_or_else(|| format!("{tag} is not a release tag"))?;
    if body.len() > max_bytes {
        return Err(format!("{file} is larger than a gallery list may be"));
    }
    let Some(signature) = signature else {
        return if release < trust.signed_since {
            Ok(IndexSignature::LegacyUnsigned)
        } else {
            Err(format!(
                "release {tag} publishes no signature for {file}, so the list cannot be trusted"
            ))
        };
    };
    if signature.len() > MAX_SIGNATURE_BYTES {
        return Err(format!("the signature of {file} is larger than allowed"));
    }
    let comment = authenticate(body, signature, &trust.key).map_err(|error| match error {
        AuthError::NotAuthenticated => format!(
            "the signature of {file} at release {tag} does not match the list or Sevak's release key"
        ),
        AuthError::Key => "Sevak's embedded release key is not usable".to_owned(),
        _ => format!("the signature of {file} at release {tag} is not a valid signature file"),
    })?;
    let expected = format!("file:{}", signed_name(tag, file));
    if !comment.split('\t').any(|field| field == expected) {
        return Err(format!(
            "the signature of {file} was made for another list or release, not {file} at {tag}"
        ));
    }
    Ok(IndexSignature::Verified)
}

/// Downloads the signature of `gallery/<file>` at `pin`: `None` when the
/// release has none (HTTP 404). Any other failure is an error, never "unsigned".
pub fn fetch_signature(
    transport: &dyn Transport,
    pin: &Pin,
    file: &str,
) -> Result<Option<Vec<u8>>, String> {
    match transport.get(&signature_url(pin, file), MAX_SIGNATURE_BYTES) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(FetchError::NotFound) => Ok(None),
        Err(err) => Err(format!("its signature could not be downloaded: {err}")),
    }
}

/// A list read from a release and checked against its signature.
#[derive(Debug, Clone)]
pub struct Checked {
    pub body: Vec<u8>,
    /// The signature as downloaded (base64 text), kept so a saved copy can be
    /// checked again.
    pub signature: Option<Vec<u8>>,
    pub status: IndexSignature,
}

/// Fetches the signature for an already downloaded `body` and checks both.
pub fn check_download(
    transport: &dyn Transport,
    pin: &Pin,
    file: &str,
    body: Vec<u8>,
) -> Result<Checked, String> {
    let signature = fetch_signature(transport, pin, file)?;
    let status = verify(pin.tag(), file, &body, signature.as_deref())?;
    Ok(Checked {
        body,
        signature,
        status,
    })
}

/// Downloads `gallery/<file>` at `pin` and its signature, and checks them.
pub fn fetch_checked(
    transport: &dyn Transport,
    pin: &Pin,
    file: &str,
    max_bytes: usize,
) -> Result<Checked, String> {
    let body = transport
        .get(&pin.index_url(file), max_bytes)
        .map_err(|err| err.to_string())?;
    check_download(transport, pin, file, body)
}

/// Release tooling: the shipped key and policy, but a signature is required
/// whatever the tag, so a release can never publish an unsigned list.
pub fn verify_for_build(
    tag: &str,
    file: &str,
    body: &[u8],
    signature: &[u8],
) -> Result<IndexSignature, String> {
    verify_with(&Trust::release(), tag, file, body, Some(signature))
}

#[cfg(test)]
mod tests;
