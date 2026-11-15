use super::*;
use crate::extensions::store::CACHE_FILE;
use crate::extensions::testing::{url, Fake, World};
use crate::net::fetch_pinned;
use crate::workflow::gallery::{self, INDEX_FILE, MAX_INDEX_BYTES};
use sevak_core::theme_store::{self, GALLERY_INDEX_FILE as THEMES_FILE};

const KEY: &str = include_str!("fixtures/public-key.txt");
const INDEX: &[u8] = include_bytes!("fixtures/index.json");
const INDEX_SIG: &[u8] = include_bytes!("fixtures/index.json.sig");
const THEMES: &[u8] = include_bytes!("fixtures/themes.json");
const THEMES_SIG: &[u8] = include_bytes!("fixtures/themes.json.sig");
const OTHER_KEY_SIG: &[u8] = include_bytes!("fixtures/index.json.other-key.sig");
const OTHER_TAG_SIG: &[u8] = include_bytes!("fixtures/index.json.other-tag.sig");
const AS_THEMES_SIG: &[u8] = include_bytes!("fixtures/index.json.as-themes.sig");
const UNBOUND_SIG: &[u8] = include_bytes!("fixtures/index.json.unbound.sig");
const ADVISORY: &[u8] = include_bytes!("../extensions/revocations/fixtures/valid.json");
const ADVISORY_SIG: &[u8] = include_bytes!("../extensions/revocations/fixtures/valid.json.sig");
const TAG: &str = "v1.2.3";

/// The fixtures' TEST key, with lists signed from v1.0.0 on.
fn test_trust() -> Trust {
    Trust {
        key: KEY.trim().to_owned(),
        signed_since: (1, 0, 0),
    }
}

fn check(tag: &str, file: &str, body: &[u8], sig: Option<&[u8]>) -> Result<IndexSignature, String> {
    verify_with(&test_trust(), tag, file, body, sig)
}

fn pin() -> Pin {
    Pin::new(TAG).unwrap()
}

#[test]
fn valid_signatures_verify_the_exact_bytes() {
    assert_eq!(
        check(TAG, INDEX_FILE, INDEX, Some(INDEX_SIG)),
        Ok(IndexSignature::Verified)
    );
    assert_eq!(
        check(TAG, THEMES_FILE, THEMES, Some(THEMES_SIG)),
        Ok(IndexSignature::Verified)
    );
    // And the verified bytes are lists the app parses.
    let index = gallery::parse_index(std::str::from_utf8(INDEX).unwrap(), &pin()).unwrap();
    assert_eq!(index.entries.len(), 1);
    let themes = theme_store::parse_index(std::str::from_utf8(THEMES).unwrap(), &pin()).unwrap();
    assert_eq!(themes.len(), 1);
}

#[test]
fn tampered_lists_are_refused() {
    let mut changed = INDEX.to_vec();
    let at = changed.iter().position(|&b| b == b'1').unwrap();
    changed[at] = b'2';
    // A different byte, a byte more (even whitespace) or a byte less.
    let mut longer = INDEX.to_vec();
    longer.push(b'\n');
    let shorter = &INDEX[..INDEX.len() - 1];
    for body in [&changed[..], &longer[..], shorter] {
        let err = check(TAG, INDEX_FILE, body, Some(INDEX_SIG)).unwrap_err();
        assert!(err.contains("does not match"), "{err}");
    }
    // The theme list's signature does not cover the package list.
    assert!(check(TAG, INDEX_FILE, INDEX, Some(THEMES_SIG)).is_err());
}

#[test]
fn invalid_signatures_are_refused() {
    let err = check(TAG, INDEX_FILE, INDEX, Some(OTHER_KEY_SIG)).unwrap_err();
    assert!(err.contains("does not match"), "{err}");
    for garbage in [&b""[..], b"not base64!", b"aGVsbG8=", b"\xff\xfe"] {
        let err = check(TAG, INDEX_FILE, INDEX, Some(garbage)).unwrap_err();
        assert!(err.contains("not a valid signature"), "{garbage:?}: {err}");
    }
    let huge = vec![b'A'; MAX_SIGNATURE_BYTES + 1];
    let err = check(TAG, INDEX_FILE, INDEX, Some(&huge)).unwrap_err();
    assert!(err.contains("larger than allowed"), "{err}");
}

#[test]
fn signatures_are_bound_to_the_list_and_the_release() {
    // Authentic signatures by the trusted key, made for something else.
    for (sig, what) in [
        (OTHER_TAG_SIG, "another release"),
        (AS_THEMES_SIG, "the theme list"),
        (UNBOUND_SIG, "a list without its release"),
    ] {
        let err = check(TAG, INDEX_FILE, INDEX, Some(sig)).unwrap_err();
        assert!(err.contains("made for another"), "{what}: {err}");
    }
    // The v1.2.2 signature is fine at v1.2.2, and only there.
    assert_eq!(
        check("v1.2.2", INDEX_FILE, INDEX, Some(OTHER_TAG_SIG)),
        Ok(IndexSignature::Verified)
    );
    assert!(check("v1.2.4", INDEX_FILE, INDEX, Some(INDEX_SIG)).is_err());
    // The advisory feed is signed with the same key, but is not a list.
    let err = check(TAG, INDEX_FILE, ADVISORY, Some(ADVISORY_SIG)).unwrap_err();
    assert!(err.contains("made for another"), "{err}");
}

#[test]
fn a_missing_signature_is_accepted_only_for_releases_before_signing() {
    assert_eq!(
        check("v0.9.9", INDEX_FILE, INDEX, None),
        Ok(IndexSignature::LegacyUnsigned)
    );
    assert_eq!(
        check("v0.99.99", THEMES_FILE, THEMES, None),
        Ok(IndexSignature::LegacyUnsigned)
    );
    for tag in ["v1.0.0", "v1.0.1", TAG, "v2.0.0"] {
        let err = check(tag, INDEX_FILE, INDEX, None).unwrap_err();
        assert!(err.contains("publishes no signature"), "{tag}: {err}");
    }
}

#[test]
fn an_invalid_signature_is_refused_even_for_old_releases() {
    for sig in [OTHER_KEY_SIG, UNBOUND_SIG, b"junk"] {
        assert!(check("v0.1.0", INDEX_FILE, INDEX, Some(sig)).is_err());
    }
    let mut changed = INDEX.to_vec();
    changed[0] = b' ';
    assert!(check("v0.1.0", INDEX_FILE, &changed, Some(OTHER_TAG_SIG)).is_err());
}

#[test]
fn only_the_two_lists_within_their_size_limits_are_accepted() {
    let err = check(TAG, "revocations.json", INDEX, Some(INDEX_SIG)).unwrap_err();
    assert!(err.contains("not a signed gallery list"), "{err}");
    let big = vec![b' '; MAX_INDEX_BYTES + 1];
    assert!(check("v0.1.0", INDEX_FILE, &big, None)
        .unwrap_err()
        .contains("larger"));
    let big = vec![b' '; theme_store::MAX_INDEX_BYTES as usize + 1];
    assert!(check("v0.1.0", THEMES_FILE, &big, None).is_err());
    assert!(check("main", INDEX_FILE, INDEX, None).is_err());
}

#[test]
fn the_shipped_policy() {
    let release = Trust::release();
    assert_eq!(release.key, trusted_key());
    assert_eq!(numbers(SIGNED_SINCE), Some((0, 14, 1)));
    // v0.14.0 and older were published before signing; v0.14.1 on are signed.
    for old in ["v0.1.0", "v0.13.9", "v0.14.0"] {
        assert_eq!(
            verify_with(&release, old, INDEX_FILE, INDEX, None),
            Ok(IndexSignature::LegacyUnsigned),
            "{old}"
        );
    }
    for new in ["v0.14.1", "v0.15.0", "v1.0.0"] {
        assert!(verify_with(&release, new, INDEX_FILE, INDEX, None).is_err());
    }
    // The fixtures' TEST key is not trusted by the shipped app or the tooling.
    assert!(verify_with(&release, TAG, INDEX_FILE, INDEX, Some(INDEX_SIG)).is_err());
    assert!(verify_for_build(TAG, INDEX_FILE, INDEX, INDEX_SIG).is_err());
    assert_ne!(release.key, KEY.trim(), "test key must never be trusted");
}

#[test]
fn tags_compare_as_versions() {
    assert_eq!(numbers("v1.2.3"), Some((1, 2, 3)));
    assert_eq!(numbers("v0.15.0-beta.2"), Some((0, 15, 0)));
    assert!(numbers("v0.10.0") > numbers("v0.9.9"));
    for bad in ["", "1.2.3", "v1.2", "v1.2.3.4", "vx.2.3", "main"] {
        assert_eq!(numbers(bad), None, "{bad}");
    }
    assert_eq!(
        signature_url(&pin(), INDEX_FILE),
        "https://github.com/ninad-k/Sevak/releases/download/v1.2.3/index.json.sig"
    );
    assert_eq!(signed_name(TAG, THEMES_FILE), "v1.2.3-themes.json");
}

// ---- downloads -----------------------------------------------------------

fn serve_sig(net: &Fake, file: &str, sig: Result<&[u8], FetchError>) {
    net.files
        .lock()
        .unwrap()
        .insert(signature_url(&pin(), file), sig.map(<[u8]>::to_vec));
}

fn serving(sig: Option<&[u8]>) -> Fake {
    let net = Fake::default();
    net.put("gallery/index.json", INDEX.to_vec());
    if let Some(sig) = sig {
        serve_sig(&net, INDEX_FILE, Ok(sig));
    }
    net
}

#[test]
fn the_signature_is_downloaded_next_to_the_list_and_checked() {
    with_trust(test_trust(), || {
        let net = serving(Some(INDEX_SIG));
        let got = fetch_pinned(&net, Some(&pin()), INDEX_FILE, MAX_INDEX_BYTES).unwrap();
        assert_eq!(got.status, IndexSignature::Verified);
        assert_eq!(got.body, INDEX);
        assert_eq!(got.signature.as_deref(), Some(INDEX_SIG));
        assert_eq!(
            net.asked(),
            [url("gallery/index.json"), signature_url(&pin(), INDEX_FILE)]
        );

        for bad in [OTHER_KEY_SIG, OTHER_TAG_SIG, b"junk"] {
            let net = serving(Some(bad));
            let err = fetch_pinned(&net, Some(&pin()), INDEX_FILE, MAX_INDEX_BYTES).unwrap_err();
            assert!(err.starts_with("Could not load the gallery"), "{err}");
        }
        let err =
            fetch_pinned(&serving(None), Some(&pin()), INDEX_FILE, MAX_INDEX_BYTES).unwrap_err();
        assert!(err.contains("publishes no signature"), "{err}");
    });
}

#[test]
fn a_failed_signature_download_is_not_treated_as_unsigned() {
    let old = Trust {
        signed_since: (9, 0, 0),
        ..test_trust()
    };
    with_trust(old, || {
        let net = serving(None);
        serve_sig(
            &net,
            INDEX_FILE,
            Err(FetchError::Failed("offline".to_owned())),
        );
        let err = fetch_pinned(&net, Some(&pin()), INDEX_FILE, MAX_INDEX_BYTES).unwrap_err();
        assert!(err.contains("signature could not be downloaded"), "{err}");
        // A real 404 for an old release is fine.
        let got = fetch_pinned(&serving(None), Some(&pin()), INDEX_FILE, MAX_INDEX_BYTES).unwrap();
        assert_eq!(got.status, IndexSignature::LegacyUnsigned);
        assert_eq!(got.signature, None);
    });
}

// ---- the extension store ------------------------------------------------

fn store_world(themes_sig: Option<&[u8]>) -> World {
    let world = World::new();
    world.net.put("gallery/index.json", INDEX.to_vec());
    world.net.put("gallery/themes.json", THEMES.to_vec());
    serve_sig(&world.net, INDEX_FILE, Ok(INDEX_SIG));
    if let Some(sig) = themes_sig {
        serve_sig(&world.net, THEMES_FILE, Ok(sig));
    }
    world
}

#[test]
fn the_store_checks_both_lists_and_rechecks_its_saved_copy() {
    with_trust(test_trust(), || {
        let world = store_world(Some(THEMES_SIG));
        let view = world.store.refresh().unwrap();
        assert_eq!(view.index_signature, IndexSignature::Verified);
        assert_eq!(view.themes_error, None);
        let ids: Vec<&str> = view.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["fixture-dark", "fixture-search"]);

        // The saved copy carries the signatures and is checked again.
        let cached = world.store.load_cached().unwrap();
        assert_eq!(cached.index_signature, IndexSignature::Verified);
        assert_eq!(cached.items.len(), 2);

        // A saved list altered on disk is not loaded.
        let path = world.store.dirs().state.join(CACHE_FILE);
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, text.replace("Fixture search", "Fixture Search")).unwrap();
        assert!(world.store.load_cached().is_none());
    });
}

#[test]
fn the_store_refuses_an_unsigned_package_list_and_drops_an_unsigned_theme_list() {
    with_trust(test_trust(), || {
        // No theme signature: the packages stay, the themes are not listed.
        let world = store_world(None);
        let view = world.store.refresh().unwrap();
        assert_eq!(view.items.len(), 1);
        let err = view.themes_error.unwrap();
        assert!(err.contains("publishes no signature"), "{err}");

        // A theme signature for the wrong list.
        let world = store_world(Some(INDEX_SIG));
        let view = world.store.refresh().unwrap();
        assert!(view.themes_error.unwrap().contains("does not match"));

        // No package signature: nothing is loaded or saved.
        let world = World::new();
        world.net.put("gallery/index.json", INDEX.to_vec());
        let err = world.store.refresh().unwrap_err();
        assert!(err.contains("publishes no signature"), "{err}");
        assert!(world.store.load_cached().is_none());
    });
}
