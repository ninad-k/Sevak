//! The cron extension, built and run through Sevak's real script-plugin host:
//! approval first, then an expression explained over the real protocol and a
//! row's action performed by Sevak itself. No network, a temporary folder.

#[path = "../../_support/host.rs"]
mod host;

use host::{query_until_results, titles, World};

fn world() -> World {
    World::new(
        env!("CARGO_MANIFEST_DIR"),
        env!("CARGO_BIN_EXE_cron"),
        "cron",
    )
}

#[test]
fn the_extension_waits_for_approval_then_explains_an_expression() {
    let world = world();
    let prompt = world.assert_waits_for_approval();
    assert!(prompt.contains("NATIVE EXTENSION"), "{prompt}");
    assert!(prompt.contains("Publisher: Sevak"), "{prompt}");
    assert!(prompt.contains("Declared permissions: none"), "{prompt}");
    assert!(prompt.contains(&world.program_sha256()), "{prompt}");
    world.approve();

    let (engine, rx) = world.engine();
    let items = query_until_results(&engine, &rx, "cron 0 9 * * 1-5");
    let shown = titles(&items);
    assert_eq!(shown[0], "At 09:00, on Monday through Friday", "{shown:?}");
    // The meaning plus the next five runs, all on weekdays at 09:00 UTC.
    assert_eq!(shown.len(), 6, "{shown:?}");
    assert!(
        shown[1..].iter().all(|t| t.ends_with("09:00 UTC")),
        "{shown:?}"
    );
    assert!(shown[1..]
        .iter()
        .all(|t| !t.starts_with("Sat") && !t.starts_with("Sun")));

    // Enter on the meaning copies the normalized expression.
    engine.execute(&items[0], "cron 0 9 * * 1-5").unwrap();
    assert_eq!(*world.platform.clipboard.lock().unwrap(), ["0 9 * * 1-5"]);

    // A bad expression is an answer, not a failure.
    let bad = query_until_results(&engine, &rx, "cron 61 * * * *");
    assert!(titles(&bad)[0].starts_with("Not a cron expression"));
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
