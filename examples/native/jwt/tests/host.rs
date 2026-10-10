//! The jwt extension, built and run through Sevak's real script-plugin host:
//! approval first, then a real token decoded over the real protocol, and a
//! row's action performed by Sevak itself. The program is the one cargo builds
//! for this package; no network, everything in a temporary folder.

#[path = "../../_support/host.rs"]
mod host;

use host::{query_until_results, titles, World};

fn world() -> World {
    World::new(env!("CARGO_MANIFEST_DIR"), env!("CARGO_BIN_EXE_jwt"), "jwt")
}

/// {"alg":"HS256","typ":"JWT"} . {"sub":"1234567890","name":"John Doe","iat":1516239022}
const TOKEN: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";

#[test]
fn the_extension_waits_for_approval_then_decodes_a_token() {
    let world = world();
    let prompt = world.assert_waits_for_approval();
    assert!(prompt.contains("NATIVE EXTENSION"), "{prompt}");
    assert!(prompt.contains("Publisher: Sevak"), "{prompt}");
    assert!(prompt.contains("Declared permissions: none"), "{prompt}");
    assert!(prompt.contains(&world.program_sha256()), "{prompt}");
    world.approve();

    let (engine, rx) = world.engine();
    let items = query_until_results(&engine, &rx, &format!("jwt {TOKEN}"));
    let shown = titles(&items);
    assert!(shown.contains(&"Algorithm HS256"), "{shown:?}");
    assert!(shown.contains(&"name: John Doe"), "{shown:?}");
    assert!(
        shown.iter().any(|t| t.starts_with("No expiry")),
        "{shown:?}"
    );

    // Enter on a claim copies its value, performed by Sevak itself.
    let name = items.iter().find(|i| i.title == "name: John Doe").unwrap();
    engine.execute(name, &format!("jwt {TOKEN}")).unwrap();
    assert_eq!(*world.platform.clipboard.lock().unwrap(), ["John Doe"]);

    // Bad input is an answer, not a failure.
    let bad = query_until_results(&engine, &rx, "jwt not-a-token");
    assert!(
        titles(&bad)[0].starts_with("Not a JWT"),
        "{:?}",
        titles(&bad)
    );
}

#[test]
fn the_keyword_alone_shows_usage() {
    let world = world();
    world.assert_waits_for_approval();
    world.approve();
    let (engine, rx) = world.engine();
    let items = query_until_results(&engine, &rx, "jwt ");
    assert_eq!(titles(&items)[0], "Paste a JWT after the keyword");
}

#[test]
fn a_changed_program_asks_again() {
    let world = world();
    world.assert_waits_for_approval();
    world.approve();

    // Same manifest, one more byte in the program: not what was allowed.
    let program = world.program();
    let mut bytes = std::fs::read(&program).unwrap();
    bytes.push(0);
    std::fs::write(&program, bytes).unwrap();

    let prompt = world.assert_waits_for_approval();
    assert!(prompt.contains("review again"), "{prompt}");
}

#[test]
fn the_keyword_is_free() {
    host::assert_keyword_is_free(env!("CARGO_MANIFEST_DIR"));
}
