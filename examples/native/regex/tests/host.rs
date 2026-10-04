//! The regex extension, built and run through Sevak's real script-plugin host:
//! approval first, then a pattern tested over the real protocol and a row's
//! action performed by Sevak itself. No network, a temporary folder.

#[path = "../../_support/host.rs"]
mod host;

use host::{query_until_results, titles, World};

fn world() -> World {
    World::new(
        env!("CARGO_MANIFEST_DIR"),
        env!("CARGO_BIN_EXE_regex"),
        "regex",
    )
}

#[test]
fn the_extension_waits_for_approval_then_tests_a_pattern() {
    let world = world();
    let prompt = world.assert_waits_for_approval();
    assert!(prompt.contains("NATIVE EXTENSION"), "{prompt}");
    assert!(prompt.contains("Publisher: Sevak"), "{prompt}");
    assert!(prompt.contains("Declared permissions: none"), "{prompt}");
    assert!(prompt.contains(&world.program_sha256()), "{prompt}");
    world.approve();

    let (engine, rx) = world.engine();
    let query = r"regex (\d+)-(\d+) => call 555-1234 now";
    let items = query_until_results(&engine, &rx, query);
    assert_eq!(titles(&items), ["1 match", "\"555-1234\""]);

    // Enter on a match copies it, performed by Sevak itself.
    engine.execute(&items[1], query).unwrap();
    assert_eq!(*world.platform.clipboard.lock().unwrap(), ["555-1234"]);

    // A bad pattern is an answer, not a failure.
    let bad = query_until_results(&engine, &rx, "regex (abc => text");
    assert!(titles(&bad)[0].starts_with("Invalid pattern"));
}

#[test]
fn a_changed_program_asks_again() {
    let world = world();
    world.assert_waits_for_approval();
    world.approve();

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
