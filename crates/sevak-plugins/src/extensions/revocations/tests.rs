use super::*;
use crate::extensions::testing::Fake;

const KEY: &str = include_str!("fixtures/public-key.txt");
const BODY: &[u8] = include_bytes!("fixtures/valid.json");
const SIGNATURE: &[u8] = include_bytes!("fixtures/valid.json.sig");
const NOW: u64 = 1700000100;

fn network(body: &[u8], signature: &[u8]) -> Fake {
    let net = Fake::default();
    net.files
        .lock()
        .unwrap()
        .insert(SOURCE.into(), Ok(body.to_vec()));
    net.files
        .lock()
        .unwrap()
        .insert(format!("{SOURCE}.sig"), Ok(signature.to_vec()));
    net
}

fn saved(directory: &Path, body: &[u8], signature: &[u8]) {
    let envelope = Envelope {
        body: String::from_utf8(body.to_vec()).unwrap(),
        signature: String::from_utf8(signature.to_vec()).unwrap(),
    };
    std::fs::write(
        directory.join(CACHE),
        serde_json::to_vec(&envelope).unwrap(),
    )
    .unwrap();
}

#[test]
fn authenticates_exact_bytes_and_rejects_other_keys_tampering_and_encoding() {
    let feed = verify(BODY, SIGNATURE, KEY).unwrap();
    assert_eq!(feed.revocations[0].id, "docs");
    assert!(
        verify_for_build(BODY, SIGNATURE, NOW)
            .unwrap_err()
            .contains("not authenticated"),
        "test key must never be trusted by the shipped app"
    );
    let mut altered = BODY.to_vec();
    altered.push(b' ');
    assert!(verify(&altered, SIGNATURE, KEY)
        .unwrap_err()
        .contains("not authenticated"));
    assert!(verify(BODY, b"bad signature", KEY).is_err());
    assert!(verify(BODY, SIGNATURE, "untrusted").is_err());
    assert!(verify(&vec![0; MAX_BYTES + 1], SIGNATURE, KEY).is_err());
    assert!(verify(BODY, &vec![0; MAX_SIGNATURE_BYTES + 1], KEY).is_err());
}

#[test]
fn a_valid_signature_does_not_excuse_an_invalid_schema() {
    assert!(verify(
        include_bytes!("fixtures/invalid.json"),
        include_bytes!("fixtures/invalid.json.sig"),
        KEY
    )
    .unwrap_err()
    .contains("invalid signed advisory"));
}

#[test]
fn rejects_invalid_metadata_and_accepts_exact_or_all_version_notices() {
    let good = verify(BODY, SIGNATURE, KEY).unwrap();
    for id in [
        "../escape",
        "CON",
        "con",
        "bad.id",
        "bad id",
        "",
        "A",
        &"a".repeat(49),
    ] {
        let mut feed = good.clone();
        feed.revocations[0].id = id.into();
        assert!(validate(&feed).is_err(), "{id}");
    }
    for versions in [
        vec![],
        vec!["*", "1.0"],
        vec!["1.0", "1.0"],
        vec!["../1"],
        vec![""],
    ] {
        let mut feed = good.clone();
        feed.revocations[0].versions = versions.into_iter().map(String::from).collect();
        assert!(validate(&feed).is_err());
    }
    for reason in ["", " leading", "trailing ", "line\nbreak"] {
        let mut feed = good.clone();
        feed.revocations[0].reason = reason.into();
        assert!(validate(&feed).is_err());
    }
    for url in [
        "http://github.com/ninad-k/Sevak/security/advisories/GHSA-x",
        "https://evil.example/GHSA-x",
        "https://github.com/ninad-k/Sevak/security/advisories/GHSA-x?q=x",
        "https://github.com/ninad-k/Sevak/security/advisories/GHSA-x/../GHSA-z",
    ] {
        let mut feed = good.clone();
        feed.revocations[0].advisory_url = Some(url.into());
        assert!(validate(&feed).is_err());
    }
    let mut feed = good.clone();
    feed.revocations.push(feed.revocations[0].clone());
    assert!(validate(&feed).is_err());
    let mut feed = good.clone();
    feed.expires_at = feed.issued_at + MAX_LIFETIME + 1;
    assert!(validate(&feed).is_err());
    let mut state = Advisories {
        feed: Some(good),
        ..Default::default()
    };
    assert!(state.notice(Subject::Package, "docs", "1.0").is_some());
    assert!(state.notice(Subject::Package, "docs", "1.0.0").is_some());
    assert!(state.notice(Subject::Package, "docs", "1.0.1").is_none());
    assert!(state.notice(Subject::Package, "another", "1.0").is_none());
    state.feed.as_mut().unwrap().revocations[0].versions = vec!["*".into()];
    assert!(validate(state.feed.as_ref().unwrap()).is_ok());
    assert!(state
        .notice(Subject::Package, "docs", "future-version")
        .is_some());
}

#[test]
fn theme_and_package_notices_use_their_own_namespace_and_id_grammar() {
    let good = verify(BODY, SIGNATURE, KEY).unwrap();
    // Theme ids may contain `_`, like the theme gallery's own ids.
    let mut feed = good.clone();
    feed.revocations[2].id = "solar_dark".into();
    assert!(validate(&feed).is_ok());
    let state = Advisories {
        feed: Some(feed),
        ..Default::default()
    };
    assert!(state.notice(Subject::Theme, "solar_dark", "").is_some());
    // The fixture's `nord` is a theme notice: it never revokes a package `nord`.
    let state = Advisories {
        feed: Some(good.clone()),
        ..Default::default()
    };
    assert!(state.notice(Subject::Theme, "nord", "").is_some());
    assert!(state.notice(Subject::Package, "nord", "1.0").is_none());
    assert!(state.notice(Subject::Theme, "docs", "").is_none());
    // Themes carry no version, so an exact-version theme notice could never match.
    let mut feed = good.clone();
    feed.revocations[2].versions = vec!["1.0".into()];
    assert!(validate(&feed).unwrap_err().contains("theme"));
    // The same id may appear once per namespace.
    let mut feed = good;
    let mut package = feed.revocations[2].clone();
    package.kind = Subject::Package;
    feed.revocations.push(package.clone());
    assert!(validate(&feed).is_ok());
    feed.revocations.push(package);
    assert!(validate(&feed).is_err());
}

#[test]
fn explicit_refresh_persists_signed_bytes_and_offline_restart_keeps_expired_notices() {
    let dir = tempfile::tempdir().unwrap();
    let net = network(BODY, SIGNATURE);
    let mut state = Advisories::default();
    state.load_cached_using(dir.path(), KEY);
    assert_eq!(state.status(NOW).state, "not_checked");
    assert!(net.asked().is_empty());
    state.refresh_using(&net, dir.path(), KEY);
    assert_eq!(state.status(NOW).state, "verified");
    assert_eq!(net.asked(), [SOURCE.to_owned(), format!("{SOURCE}.sig")]);
    let mut restart = Advisories::default();
    restart.load_cached_using(dir.path(), KEY);
    assert_eq!(restart.status(NOW + 100000).state, "stale");
    assert!(restart.notice(Subject::Package, "docs", "1.0").is_some());
    assert!(restart.status(NOW + 100000).message.contains("unknown"));
}

#[test]
fn invalid_expired_future_and_rollback_refreshes_never_clear_known_revocations() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = Advisories::default();
    state.refresh_using(&network(BODY, SIGNATURE), dir.path(), KEY);
    let saved_bytes = std::fs::read(dir.path().join(CACHE)).unwrap();
    for net in [
        Fake::default(),
        network(BODY, b"invalid"),
        network(
            include_bytes!("fixtures/older.json"),
            include_bytes!("fixtures/older.json.sig"),
        ),
    ] {
        state.refresh_using(&net, dir.path(), KEY);
        assert_eq!(state.status(NOW).state, "unavailable");
        assert!(state
            .status(NOW)
            .message
            .contains("Previously authenticated notices still apply"));
        assert!(state.notice(Subject::Package, "docs", "1.0").is_some());
        assert_eq!(std::fs::read(dir.path().join(CACHE)).unwrap(), saved_bytes);
    }
    // A clock far ahead (expired) or behind (future-dated) keeps the authentic
    // feed and labels it stale; it never reports the list as verified.
    for now in [NOW + 100000, NOW - 1000] {
        state.refresh_using(&network(BODY, SIGNATURE), dir.path(), KEY);
        assert_eq!(state.status(now).state, "stale");
        assert!(state.notice(Subject::Package, "docs", "1.0").is_some());
        assert_eq!(std::fs::read(dir.path().join(CACHE)).unwrap(), saved_bytes);
    }
}

#[test]
fn a_wrong_clock_never_drops_saved_notices_or_lets_a_revoked_version_through() {
    let dir = tempfile::tempdir().unwrap();
    saved(dir.path(), BODY, SIGNATURE);
    // Clock behind the feed's issue time (a reset CMOS clock, dual boot).
    let mut behind = Advisories::default();
    behind.load_cached_using(dir.path(), KEY);
    assert!(behind.notice(Subject::Package, "docs", "1.0").is_some());
    let status = behind.status(NOW - 100000);
    assert_eq!(status.state, "stale");
    assert!(status.message.contains("check your clock"));
    // Clock far ahead: expired, still enforced.
    let mut ahead = Advisories::default();
    ahead.load_cached_using(dir.path(), KEY);
    assert!(ahead.notice(Subject::Package, "docs", "1.0").is_some());
    assert_eq!(ahead.status(NOW + 100 * 86400).state, "stale");
}

#[test]
fn unavailable_status_does_not_claim_notices_that_were_never_loaded() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = Advisories::default();
    state.refresh_using(&Fake::default(), dir.path(), KEY);
    let status = state.status(NOW);
    assert_eq!(status.state, "unavailable");
    assert!(status
        .message
        .contains("No authenticated notices are available yet"));
    assert!(!status.message.contains("still apply"));
}

#[test]
fn malformed_oversized_or_untrusted_cache_is_bounded_and_recovers_on_valid_refresh() {
    for bytes in [
        b"invalid".to_vec(),
        vec![b' '; MAX_BYTES * 6 + MAX_SIGNATURE_BYTES * 2 + 1],
    ] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(CACHE), bytes).unwrap();
        let mut state = Advisories::default();
        state.load_cached_using(dir.path(), KEY);
        assert_eq!(state.status(NOW).state, "unavailable");
        assert!(state.notice(Subject::Package, "docs", "1.0").is_none());
        state.refresh_using(&network(BODY, SIGNATURE), dir.path(), KEY);
        assert_eq!(state.status(NOW).state, "verified");
    }
    let dir = tempfile::tempdir().unwrap();
    saved(dir.path(), BODY, SIGNATURE);
    let mut untrusted = Advisories::default();
    untrusted.load_cached(dir.path());
    assert_eq!(untrusted.status(NOW).state, "unavailable");
    assert!(untrusted.notice(Subject::Package, "docs", "1.0").is_none());
}

#[test]
fn cache_write_failure_does_not_report_a_successful_check() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(CACHE)).unwrap();
    let mut state = Advisories::default();
    state.refresh_using(&network(BODY, SIGNATURE), dir.path(), KEY);
    assert_eq!(state.status(NOW).state, "unavailable");
    assert!(state.status(NOW).message.contains("could not save"));
    assert!(
        state.notice(Subject::Package, "docs", "1.0").is_some(),
        "authenticated notices remain actionable even when the disk is unwritable"
    );
}

#[test]
fn versions_match_by_semver_where_both_parse_and_exactly_otherwise() {
    for (a, b) in [
        ("1.0", "1.0.0"),
        ("1", "1.0.0"),
        ("1.0.0+build.7", "1.0"),
        ("1.0-beta.1", "1.0.0-beta.1"),
        ("weird", "weird"),
    ] {
        assert!(same_version(a, b), "{a} / {b}");
        assert!(same_version(b, a), "{b} / {a}");
    }
    for (a, b) in [
        ("1.0", "1.0.1"),
        ("1.0-beta", "1.0"),
        ("1.0.0.0", "1.0.0"),
        ("01.0", "1.0"),
        ("weird", "Weird"),
        ("", "0"),
    ] {
        assert!(!same_version(a, b), "{a} / {b}");
    }
}

#[test]
fn all_version_and_any_version_lookups_cover_unknown_installed_versions() {
    let mut feed = verify(BODY, SIGNATURE, KEY).unwrap();
    feed.revocations[1].versions = vec!["*".into()];
    let state = Advisories {
        feed: Some(feed),
        ..Default::default()
    };
    let all: Vec<_> = state
        .all_version_notices()
        .into_iter()
        .map(|n| n.id)
        .collect();
    assert_eq!(all, ["tool", "nord"]);
    assert!(state.any_notice(Subject::Package, "docs").is_some());
    assert!(state.any_notice(Subject::Package, "nord").is_none());
    // "*" also blocks a later, fixed version.
    assert!(state.notice(Subject::Package, "tool", "9.9.9").is_some());
}

#[test]
fn the_embedded_key_is_the_apps_updater_key() {
    // The crate carries its own copy so it does not depend on the app layout;
    // this keeps the two from drifting apart.
    let config: serde_json::Value = serde_json::from_slice(
        &std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/tauri.conf.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        trusted_key(),
        config["plugins"]["updater"]["pubkey"].as_str().unwrap()
    );
}

#[test]
fn the_embedded_trust_anchor_is_a_usable_minisign_key() {
    // A missing or malformed key would make every refresh fail after release.
    let key = trusted_key();
    assert!(
        !key.is_empty(),
        "tauri.conf.json must carry the updater public key"
    );
    let text = String::from_utf8(STANDARD.decode(key.trim()).unwrap()).unwrap();
    assert!(PublicKey::decode(&text).is_ok());
}
