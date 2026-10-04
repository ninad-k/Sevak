//! Install pipeline tests: temporary folders, a fake network, no real downloads.

use serde_json::json;
use sevak_core::config::Config;

use super::*;
use crate::extensions::package::Builder;
use crate::extensions::testing::*;
use crate::net::FetchError;
use crate::script::ScriptPluginHost;

// ---- the catalog ---------------------------------------------------------------

#[test]
fn nothing_is_requested_until_the_catalog_is_loaded() {
    let world = World::new();
    assert!(world.store.current().is_none());
    assert!(world.store.load_cached().is_none());
    assert!(world.store.installed().is_empty());
    assert!(
        world.net.asked().is_empty(),
        "reading local state never uses the network"
    );
    let err = world.store.install("anything").unwrap_err();
    assert!(err.contains("Load the extension list"), "{err}");
    assert!(world.net.asked().is_empty());
}

#[test]
fn loading_the_catalog_reads_both_lists_from_the_builds_release() {
    let world = World::new();
    let zip = workflow_zip("docs", &[]);
    let (_, theme) = theme_fixture();
    let entry = workflow_entry(&world, "docs", "1.0", &zip);
    world.publish(
        vec![entry, native_entry(&world, "0.2.0", b"elf")],
        vec![theme],
    );

    let view = world.store.refresh().unwrap();
    assert_eq!(
        world.net.asked(),
        [url("gallery/index.json"), url("gallery/themes.json")]
    );
    assert_eq!(view.source, url("gallery/index.json"));
    assert!(!view.from_cache);
    let ids: Vec<&str> = view.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["docs", "nord", "tool"], "sorted by name");
    let tool = view.items.iter().find(|i| i.id == "tool").unwrap();
    assert_eq!(tool.kind, ItemKind::Native);
    assert_eq!(tool.state, State::Available);
    assert_eq!(tool.permissions, ["network"]);
    // The file shown is the one for this computer.
    assert_eq!(
        tool.source.as_deref(),
        Some(url("gallery/extensions/tool/tool-0.2.0-linux-x86_64.sevakext").as_str())
    );
    assert_eq!(tool.platforms.len(), 3);
}

#[test]
fn a_loaded_catalog_is_cached_for_offline_use() {
    let world = World::new();
    let zip = workflow_zip("docs", &[]);
    let entry = workflow_entry(&world, "docs", "1.0", &zip);
    world.publish(vec![entry], vec![]);
    world.store.refresh().unwrap();
    assert!(world.store.dirs().state.join(CACHE_FILE).is_file());

    // A new store (Sevak restarted) with no network: the cache answers.
    let again = World::new();
    std::fs::create_dir_all(&again.store.dirs().state).unwrap();
    std::fs::copy(
        world.store.dirs().state.join(CACHE_FILE),
        again.store.dirs().state.join(CACHE_FILE),
    )
    .unwrap();
    let view = again.store.load_cached().unwrap();
    assert!(view.from_cache);
    assert_eq!(view.items.len(), 1);
    assert!(again.net.asked().is_empty());
}

#[test]
fn offline_the_installed_list_still_works_and_the_error_says_why() {
    let world = World::new();
    let zip = workflow_zip("docs", &[]);
    let entry = workflow_entry(&world, "docs", "1.0", &zip);
    world.publish(vec![entry], vec![]);
    world.store.refresh().unwrap();
    world.store.install("docs").unwrap();

    world.net.down();
    let err = world.store.refresh().unwrap_err();
    assert!(err.contains("Could not load the gallery"), "{err}");
    // What was loaded before is still there, and so is what is installed.
    assert_eq!(world.store.current_view().unwrap().items.len(), 1);
    let installed = world.store.installed();
    assert_eq!(installed.len(), 1);
    assert_eq!(installed[0].id, "docs");
    // Installing now fails cleanly and changes nothing.
    world.store.uninstall("docs").unwrap();
    let err = world.store.install("docs").unwrap_err();
    assert!(err.contains("Could not download it"), "{err}");
    assert!(!world.workflows().join("docs").exists());
}

#[test]
fn a_missing_theme_list_does_not_take_the_packages_away() {
    let world = World::new();
    let zip = workflow_zip("docs", &[]);
    let entry = workflow_entry(&world, "docs", "1.0", &zip);
    world.publish(vec![entry], vec![]);
    world.net.fail("gallery/themes.json", FetchError::NotFound);
    let view = world.store.refresh().unwrap();
    assert_eq!(view.items.len(), 1);
    assert!(view.themes_error.is_some());
}

#[test]
fn hostile_or_broken_indexes_are_errors_not_panics() {
    let world = World::new();
    for body in [
        "not json".to_owned(),
        "{}".to_owned(),
        json!({"format": 99, "entries": []}).to_string(),
        json!({"format": 2, "entries": "nope"}).to_string(),
        json!({"format": 2, "entries": [{"id": "x"}, 5, null, []]}).to_string(),
        "\u{0}\u{0}\u{0}".to_owned(),
    ] {
        world.net.put("gallery/index.json", body.into_bytes());
        world.net.put("gallery/themes.json", b"{}".to_vec());
        match world.store.refresh() {
            Ok(view) => assert!(view.items.is_empty()),
            Err(err) => assert!(!err.is_empty()),
        }
    }
    // An index of the wrong size is refused by the download itself.
    world.net.put(
        "gallery/index.json",
        vec![b' '; gallery::MAX_INDEX_BYTES + 1],
    );
    assert!(world.store.refresh().is_err());
    assert!(world.store.installed().is_empty());
}

#[test]
fn entries_pointing_outside_the_release_are_skipped_and_counted() {
    let world = World::new();
    let zip = workflow_zip("docs", &[]);
    let mut evil = workflow_entry(&world, "evil", "1.0", &zip);
    evil["source"] = json!("https://example.com/evil.zip");
    let mut other = workflow_entry(&world, "other", "1.0", &zip);
    other["source"] =
        json!("https://raw.githubusercontent.com/ninad-k/Sevak/main/gallery/packages/x.zip");
    let fine = workflow_entry(&world, "docs", "1.0", &zip);
    world.publish(vec![evil, other, fine], vec![]);
    let view = world.store.refresh().unwrap();
    assert_eq!(view.items.len(), 1);
    assert_eq!(view.skipped, 2);
}

// ---- installing ------------------------------------------------------------------

#[test]
fn installing_a_workflow_places_a_new_unapproved_folder() {
    let world = World::new();
    let zip = workflow_zip("docs", &[("notes.txt", b"hello")]);
    let entry = workflow_entry(&world, "docs", "1.0", &zip);
    world.publish(vec![entry], vec![]);
    world.store.refresh().unwrap();

    let outcome = world.store.install("docs").unwrap();
    assert_eq!(outcome.folder, "docs");
    assert!(!outcome.replaced);
    assert!(world.workflows().join("docs/workflow.toml").is_file());
    assert_eq!(
        std::fs::read_to_string(world.workflows().join("docs/notes.txt")).unwrap(),
        "hello"
    );
    // No approval was recorded: the Allow dialog still decides.
    assert!(!world.store.dirs().approvals.exists());
    // The receipt remembers what was installed, and the page now says so.
    let installed = world.store.installed();
    assert_eq!(installed.len(), 1);
    assert_eq!(installed[0].version, "1.0");
    let view = world.store.current_view().unwrap();
    assert_eq!(view.items[0].state, State::Installed);
    assert!(view.items[0].removable);
    // Never over an install, never twice.
    assert!(world
        .store
        .install("docs")
        .unwrap_err()
        .contains("already installed"));
}

#[test]
fn only_catalog_ids_can_be_installed() {
    let world = World::new();
    world.publish(vec![], vec![]);
    world.store.refresh().unwrap();
    for id in ["", "../../etc", "https://example.com/x.zip", "docs"] {
        let err = world.store.install(id).unwrap_err();
        assert!(err.contains("not in the loaded list"), "{id}: {err}");
    }
    assert_eq!(
        world.net.asked().len(),
        2,
        "naming something outside the list requests nothing"
    );
}

#[test]
fn a_checksum_mismatch_is_rejected_and_nothing_is_written() {
    let world = World::new();
    let zip = workflow_zip("docs", &[]);
    let entry = workflow_entry(&world, "docs", "1.0", &zip);
    // The list promises these bytes; the network serves others.
    world.net.put(
        "gallery/packages/docs.zip",
        workflow_zip("docs", &[("evil.txt", b"x")]),
    );
    world.publish(vec![entry], vec![]);
    world.store.refresh().unwrap();
    let err = world.store.install("docs").unwrap_err();
    assert!(err.contains("checksum"), "{err}");
    assert!(!world.workflows().join("docs").exists());
    assert!(world.store.installed().is_empty());
    assert!(
        !world.store.dirs().state.join(RECEIPTS_FILE).exists(),
        "no receipt for a failed install"
    );
    let leftovers: Vec<_> = std::fs::read_dir(world.workflows())
        .map(|d| d.flatten().collect())
        .unwrap_or_default();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn a_zip_slip_package_is_rejected_even_with_a_matching_checksum() {
    let world = World::new();
    for evil in [
        "docs/../../escaped.txt",
        "../escaped.txt",
        "/abs/escaped.txt",
        "docs/C:/x",
    ] {
        let zip = zip_of(&[
            ("docs/workflow.toml", WORKFLOW.as_bytes()),
            (evil, b"owned"),
        ]);
        let entry = workflow_entry(&world, "docs", "1.0", &zip);
        world.publish(vec![entry], vec![]);
        world.store.refresh().unwrap();
        let err = world.store.install("docs").unwrap_err();
        assert!(
            err.contains("not allowed") || err.contains("leaves") || err.contains("single folder"),
            "{evil}: {err}"
        );
        assert!(!world.workflows().join("docs").exists(), "{evil}");
        assert!(!world.tmp.path().join("escaped.txt").exists());
        assert!(!world.tmp.path().join("config").join("escaped.txt").exists());
    }
}

#[test]
fn oversized_downloads_and_oversized_files_are_rejected() {
    let world = World::new();
    // A download over the cap never reaches the checksum.
    let huge = vec![0u8; gallery::MAX_PACKAGE_BYTES + 1];
    let entry = workflow_entry(&world, "docs", "1.0", &huge);
    world.publish(vec![entry], vec![]);
    world.store.refresh().unwrap();
    let err = world.store.install("docs").unwrap_err();
    assert!(err.contains("larger than expected"), "{err}");

    // A small package that unpacks to a file over the per-file cap.
    let big = vec![0u8; 3 * 1024 * 1024];
    let zip = workflow_zip("docs", &[("big.bin", &big)]);
    assert!(zip.len() < gallery::MAX_PACKAGE_BYTES);
    let entry = workflow_entry(&world, "docs", "1.0", &zip);
    world.publish(vec![entry], vec![]);
    world.store.refresh().unwrap();
    let err = world.store.install("docs").unwrap_err();
    assert!(err.contains("too large"), "{err}");
    assert!(!world.workflows().join("docs").exists());
}

#[test]
fn a_folder_the_store_did_not_make_is_never_replaced() {
    let world = World::new();
    let zip = workflow_zip("docs", &[]);
    let entry = workflow_entry(&world, "docs", "1.0", &zip);
    world.publish(vec![entry], vec![]);
    world.store.refresh().unwrap();
    let mine = world.workflows().join("docs");
    std::fs::create_dir_all(&mine).unwrap();
    std::fs::write(mine.join("workflow.toml"), "name = \"mine\"").unwrap();
    let err = world.store.install("docs").unwrap_err();
    assert!(err.contains("already installed"), "{err}");
    assert_eq!(
        std::fs::read_to_string(mine.join("workflow.toml")).unwrap(),
        "name = \"mine\""
    );
    // And it cannot be removed through the store either.
    assert!(world
        .store
        .uninstall("docs")
        .unwrap_err()
        .contains("did not install"));
    assert!(mine.exists());
}

#[test]
fn only_one_change_runs_at_a_time() {
    let world = World::new();
    world.publish(vec![], vec![]);
    world.store.refresh().unwrap();
    let _held = world.store.busy.lock().unwrap();
    assert!(world
        .store
        .install("x")
        .unwrap_err()
        .contains("still running"));
    assert!(world
        .store
        .uninstall("x")
        .unwrap_err()
        .contains("still running"));
}

// ---- updating ------------------------------------------------------------------------

#[test]
fn an_update_replaces_the_folder_as_one_step() {
    let world = World::new();
    let v1 = workflow_zip("docs", &[("old.txt", b"old")]);
    world.publish(vec![workflow_entry(&world, "docs", "1.0", &v1)], vec![]);
    world.store.refresh().unwrap();
    world.store.install("docs").unwrap();
    assert!(world.workflows().join("docs/old.txt").is_file());

    let v2 = workflow_zip("docs", &[("new.txt", b"new")]);
    world.publish(vec![workflow_entry(&world, "docs", "1.1", &v2)], vec![]);
    let view = world.store.refresh().unwrap();
    assert_eq!(view.items[0].state, State::UpdateAvailable);
    assert_eq!(view.items[0].installed_version.as_deref(), Some("1.0"));
    let installed = world.store.installed();
    assert_eq!(installed[0].update_to.as_deref(), Some("1.1"));

    let outcome = world.store.update("docs").unwrap();
    assert!(outcome.replaced);
    let dir = world.workflows().join("docs");
    assert!(dir.join("new.txt").is_file());
    assert!(
        !dir.join("old.txt").exists(),
        "nothing of the old version is left behind"
    );
    let names: Vec<String> = std::fs::read_dir(world.workflows())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["docs"], "no staging or backup folders remain");
    assert_eq!(world.store.installed()[0].version, "1.1");
    assert!(world.store.installed()[0].update_to.is_none());
    assert_eq!(
        *world.stopped.lock().unwrap(),
        ["workflow:docs"],
        "what runs from the folder is stopped right before the swap"
    );
    assert!(world
        .store
        .update("docs")
        .unwrap_err()
        .contains("up to date"));
}

#[test]
fn a_bad_update_leaves_the_old_version_exactly_as_it_was() {
    let world = World::new();
    let v1 = workflow_zip("docs", &[("old.txt", b"old")]);
    world.publish(vec![workflow_entry(&world, "docs", "1.0", &v1)], vec![]);
    world.store.refresh().unwrap();
    world.store.install("docs").unwrap();

    // 1.1 whose bytes do not match its checksum.
    let v2 = workflow_zip("docs", &[("new.txt", b"new")]);
    let entry = workflow_entry(&world, "docs", "1.1", &v2);
    world.net.put(
        "gallery/packages/docs.zip",
        workflow_zip("docs", &[("x", b"y")]),
    );
    world.publish(vec![entry], vec![]);
    world.store.refresh().unwrap();
    assert!(world.store.update("docs").unwrap_err().contains("checksum"));

    // 1.2 that is not a valid workflow (the checksum is right).
    let broken = zip_of(&[("docs/workflow.toml", b"this is = not [valid")]);
    world.publish(vec![workflow_entry(&world, "docs", "1.2", &broken)], vec![]);
    world.store.refresh().unwrap();
    assert!(world.store.update("docs").is_err());

    let dir = world.workflows().join("docs");
    assert_eq!(std::fs::read_to_string(dir.join("old.txt")).unwrap(), "old");
    assert!(!dir.join("new.txt").exists());
    assert_eq!(world.store.installed()[0].version, "1.0");
    assert!(
        world.stopped.lock().unwrap().is_empty(),
        "a refused update never stops the running version"
    );
}

#[test]
fn update_needs_an_install_and_a_newer_version() {
    let world = World::new();
    let zip = workflow_zip("docs", &[]);
    world.publish(vec![workflow_entry(&world, "docs", "1.0", &zip)], vec![]);
    world.store.refresh().unwrap();
    assert!(world
        .store
        .update("docs")
        .unwrap_err()
        .contains("not installed"));
    world.store.install("docs").unwrap();
    assert!(world
        .store
        .update("docs")
        .unwrap_err()
        .contains("up to date"));
}

#[test]
fn version_comparison() {
    assert!(is_newer("1.1", "1.0"));
    assert!(is_newer("1.10", "1.9"));
    assert!(is_newer("2", "1.99.99"));
    assert!(!is_newer("1.0", "1.0.0"));
    assert!(!is_newer("1.0.0", "1.0"));
    assert!(!is_newer("1.0", "1.1"));
    assert!(is_newer("v1.2.0", "1.1.9"));
    assert!(!is_newer("1.0.0-beta.1", "1.0.0"));
    // Not numbers: different is newer; the same, or nothing, is not.
    assert!(is_newer("spring", "winter"));
    assert!(!is_newer("spring", "spring"));
    assert!(!is_newer("", "1.0"));
}

// ---- uninstalling ------------------------------------------------------------------------

#[test]
fn uninstalling_removes_everything_the_install_made_and_nothing_else() {
    let world = World::new();
    let zip = workflow_zip("docs", &[]);
    let other = workflow_zip("other", &[]);
    world.publish(
        vec![
            workflow_entry(&world, "docs", "1.0", &zip),
            workflow_entry(&world, "other", "1.0", &other),
        ],
        vec![],
    );
    world.store.refresh().unwrap();
    world.store.install("docs").unwrap();
    world.store.install("other").unwrap();
    // The workflow has run and was allowed: its data folder and approval exist.
    let data = world.store.dirs().workflow_data.join("docs");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("state.json"), "{}").unwrap();
    let approvals = ApprovalStore::new(world.store.dirs().approvals.clone());
    approvals.approve("workflow:docs", "key-1").unwrap();
    approvals.approve("workflow:other", "key-2").unwrap();
    // A file the user keeps next to it.
    std::fs::write(world.workflows().join("notes.txt"), "mine").unwrap();

    let outcome = world.store.uninstall("docs").unwrap();
    assert_eq!(outcome.id, "docs");
    assert!(!world.workflows().join("docs").exists());
    assert!(!data.exists(), "the extension's own data goes with it");
    assert!(!approvals.has_record("workflow:docs"));
    assert!(
        approvals.has_record("workflow:other"),
        "other approvals stay"
    );
    assert!(world.workflows().join("other").exists());
    assert!(world.workflows().join("notes.txt").is_file());
    let ids: Vec<String> = world.store.installed().into_iter().map(|i| i.id).collect();
    assert_eq!(ids, ["other"]);
    assert_eq!(
        world.store.current_view().unwrap().items[0].state,
        State::Available
    );
    // And it can be installed again.
    world.store.install("docs").unwrap();
}

#[test]
fn a_folder_turned_into_something_else_is_not_deleted() {
    let world = World::new();
    let zip = workflow_zip("docs", &[]);
    world.publish(vec![workflow_entry(&world, "docs", "1.0", &zip)], vec![]);
    world.store.refresh().unwrap();
    world.store.install("docs").unwrap();
    let dir = world.workflows().join("docs");
    std::fs::remove_file(dir.join("workflow.toml")).unwrap();
    std::fs::write(dir.join("precious.txt"), "x").unwrap();
    let err = world.store.uninstall("docs").unwrap_err();
    assert!(err.contains("no longer looks like"), "{err}");
    assert!(dir.join("precious.txt").is_file());
}

#[test]
fn a_receipt_cannot_point_outside_the_managed_folders() {
    let world = World::new();
    let victim = world.tmp.path().join("precious");
    std::fs::create_dir_all(&victim).unwrap();
    std::fs::write(victim.join("plugin.toml"), "x").unwrap();
    std::fs::create_dir_all(&world.store.dirs().state).unwrap();
    let evil = |folder: &str, kind: &str| {
        json!({"version": 1, "items": [{
            "id": "evil", "kind": kind, "folder": folder, "name": "Evil", "version": "1",
            "author": "x", "installed_at": 1, "source": "s", "sha256": "h"
        }]})
        .to_string()
    };
    for (folder, kind) in [
        ("../../precious", "plugin"),
        ("..", "workflow"),
        ("/etc", "native"),
        ("a/b", "plugin"),
        ("../x.toml", "theme"),
        ("themes/../../x.toml", "theme"),
        ("themes/a/b.toml", "theme"),
        ("other/x.toml", "theme"),
    ] {
        std::fs::write(
            world.store.dirs().state.join(RECEIPTS_FILE),
            evil(folder, kind),
        )
        .unwrap();
        let err = world.store.uninstall("evil").unwrap_err();
        assert!(err.contains("not valid"), "{folder}: {err}");
        assert!(victim.join("plugin.toml").is_file());
    }
}

#[test]
fn damaged_receipts_are_treated_as_none() {
    let world = World::new();
    std::fs::create_dir_all(&world.store.dirs().state).unwrap();
    for text in ["not json", "{\"items\": 5}", "", "\u{0}"] {
        std::fs::write(world.store.dirs().state.join(RECEIPTS_FILE), text).unwrap();
        assert!(world.store.installed().is_empty());
    }
}

// ---- native extensions ------------------------------------------------------------------------

#[test]
fn a_native_extension_installs_only_this_platforms_program_and_never_auto_runs() {
    let world = World::new();
    world.publish(
        vec![native_entry(&world, "0.2.0", b"ELF linux build")],
        vec![],
    );
    world.store.refresh().unwrap();
    let outcome = world.store.install("tool").unwrap();
    assert_eq!(outcome.kind, ItemKind::Native);

    let dir = world.plugins().join("tool");
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(&dir).into_iter().flatten() {
        if entry.file_type().is_file() {
            let relative = entry.path().strip_prefix(&dir).unwrap();
            files.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    files.sort();
    assert_eq!(files, ["bin/tool-linux", "plugin.toml"]);
    assert_eq!(
        std::fs::read(dir.join("bin/tool-linux")).unwrap(),
        b"ELF linux build"
    );
    assert!(!dir.join("bin/tool-macos").exists());
    assert!(!dir.join("bin/tool.exe").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.join("bin/tool-linux"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
    }

    // The receipt records the program's hash.
    let installed = world.store.installed();
    assert_eq!(
        installed[0].program_sha256.as_deref(),
        Some(sha256_hex(b"ELF linux build").as_str())
    );

    // The host sees a new, unapproved native extension: nothing runs.
    let host = ScriptPluginHost::new(
        world.plugins(),
        world.store.dirs().plugin_data.clone(),
        world.store.dirs().approvals.clone(),
    );
    // The manifest declares the platform this test pretends to run on only if
    // the real platform matches; the scan still lists it as pending or broken,
    // never approved.
    assert!(!world.store.dirs().approvals.exists());
    let config = Config::default();
    for candidate in host.pending(&config) {
        assert!(!candidate.approved);
        assert!(candidate.manifest.native.is_some());
        assert!(candidate.prompt().contains("NATIVE EXTENSION"));
    }
}

#[test]
fn the_package_must_say_what_the_list_says() {
    let world = World::new();
    for (field, value) in [
        ("version", json!("0.2.1")),
        ("author", json!("Mallory")),
        ("license", json!("GPL-3.0")),
        ("permissions", json!(["network", "filesystem"])),
        ("permissions", json!([])),
    ] {
        let mut entry = native_entry(&world, "0.2.0", b"elf");
        entry[field] = value;
        world.publish(vec![entry], vec![]);
        world.store.refresh().unwrap();
        let err = world.store.install("tool").unwrap_err();
        assert!(err.contains("does not match the list"), "{field}: {err}");
        assert!(!world.plugins().join("tool").exists(), "{field}");
    }
}

#[test]
fn a_native_package_with_the_wrong_checksum_or_id_is_rejected() {
    let world = World::new();
    let mut entry = native_entry(&world, "0.2.0", b"elf");
    entry["platforms"]["linux-x86_64"]["sha256"] = json!("0".repeat(64));
    world.publish(vec![entry], vec![]);
    world.store.refresh().unwrap();
    assert!(world
        .store
        .install("tool")
        .unwrap_err()
        .contains("checksum"));

    // A package whose manifest claims another id than the entry.
    let manifest = native_manifest("0.2.0", "\"network\"").replace("script:tool", "script:other");
    let mut builder = Builder::new(&manifest, "").unwrap();
    builder.add_binary("linux-x86_64", b"elf".to_vec()).unwrap();
    let bytes = builder.build(Some("linux-x86_64")).unwrap();
    let mut entry = native_entry(&world, "0.2.0", b"elf");
    world.net.put(
        "gallery/extensions/tool/tool-0.2.0-linux-x86_64.sevakext",
        bytes.clone(),
    );
    entry["platforms"]["linux-x86_64"]["sha256"] = json!(sha256_hex(&bytes));
    world.publish(vec![entry], vec![]);
    world.store.refresh().unwrap();
    let err = world.store.install("tool").unwrap_err();
    assert!(err.contains("script:other"), "{err}");
    assert!(!world.plugins().join("tool").exists());
}

#[test]
fn a_native_extension_without_a_build_for_this_computer_is_unavailable() {
    let world = World::new();
    let mut entry = native_entry(&world, "0.2.0", b"elf");
    entry["platforms"]
        .as_object_mut()
        .unwrap()
        .remove("linux-x86_64");
    world.publish(vec![entry], vec![]);
    let view = world.store.refresh().unwrap();
    assert_eq!(view.items[0].state, State::Unavailable);
    assert!(view.items[0]
        .unavailable
        .as_deref()
        .unwrap()
        .contains("No build for this computer"));
    assert!(view.items[0].source.is_none());
    assert!(world.store.install("tool").is_err());
}

#[test]
fn an_extension_needing_a_newer_sevak_is_unavailable() {
    let world = World::on("0.1.0");
    let mut entry = native_entry(&world, "0.2.0", b"elf");
    entry["min_sevak"] = json!("0.9.0");
    world.publish(vec![entry], vec![]);
    let view = world.store.refresh().unwrap();
    assert_eq!(view.items[0].state, State::Unavailable);
    let why = view.items[0].unavailable.clone().unwrap();
    assert!(why.contains("0.9.0") && why.contains("0.1.0"), "{why}");
    assert!(world.store.install("tool").unwrap_err().contains("0.9.0"));
    // A newer Sevak is offered it.
    let newer = World::on("1.0.0");
    let mut entry = native_entry(&newer, "0.2.0", b"elf");
    entry["min_sevak"] = json!("0.9.0");
    newer.publish(vec![entry], vec![]);
    assert_eq!(
        newer.store.refresh().unwrap().items[0].state,
        State::Available
    );
}

#[test]
fn updating_a_native_extension_replaces_the_program_and_asks_for_approval_again() {
    let world = World::new();
    world.publish(vec![native_entry(&world, "0.2.0", b"first build")], vec![]);
    world.store.refresh().unwrap();
    world.store.install("tool").unwrap();
    let receipt_v1 = world.store.installed().remove(0);

    world.publish(vec![native_entry(&world, "0.3.0", b"second build")], vec![]);
    let view = world.store.refresh().unwrap();
    assert_eq!(view.items[0].state, State::UpdateAvailable);
    world.store.update("tool").unwrap();

    assert_eq!(
        std::fs::read(world.plugins().join("tool/bin/tool-linux")).unwrap(),
        b"second build"
    );
    let now = world.store.installed().remove(0);
    assert_eq!(now.version, "0.3.0");
    assert_ne!(now.program_sha256, receipt_v1.program_sha256);
    assert_eq!(*world.stopped.lock().unwrap(), ["script:tool"]);
}

#[test]
fn uninstalling_a_native_extension_removes_its_folder_data_and_approval() {
    let world = World::new();
    world.publish(vec![native_entry(&world, "0.2.0", b"elf")], vec![]);
    world.store.refresh().unwrap();
    world.store.install("tool").unwrap();
    let data = world.store.dirs().plugin_data.join("tool");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("names.txt"), "Ada").unwrap();
    let approvals = ApprovalStore::new(world.store.dirs().approvals.clone());
    approvals.approve("script:tool", "key").unwrap();

    world.store.uninstall("tool").unwrap();
    assert!(!world.plugins().join("tool").exists());
    assert!(!data.exists());
    assert!(!approvals.has_record("script:tool"));
    assert!(world.store.installed().is_empty());
    assert_eq!(*world.stopped.lock().unwrap(), ["script:tool"]);
}

// ---- themes ---------------------------------------------------------------------------------------

#[test]
fn a_theme_installs_and_uninstalls() {
    let world = World::new();
    let (nord, entry) = theme_fixture();
    world.net.put("gallery/themes/Nord.toml", nord);
    world.publish(vec![], vec![entry]);
    world.store.refresh().unwrap();
    let outcome = world.store.install("nord").unwrap();
    assert_eq!(outcome.kind, ItemKind::Theme);
    let file = world.store.dirs().config_dir.join("themes/Nord.toml");
    assert!(file.is_file());
    assert_eq!(
        world.store.current_view().unwrap().items[0].state,
        State::Installed
    );
    assert!(world
        .store
        .install("nord")
        .unwrap_err()
        .contains("already installed"));
    assert!(world.store.update("nord").is_err());

    world.store.uninstall("nord").unwrap();
    assert!(!file.exists());
    assert_eq!(
        world.store.current_view().unwrap().items[0].state,
        State::Available
    );
}

#[test]
fn a_theme_with_the_wrong_checksum_is_not_installed() {
    let world = World::new();
    let (_, entry) = theme_fixture();
    world.net.put(
        "gallery/themes/Nord.toml",
        b"[colors]\nbackground = \"#000000\"\n".to_vec(),
    );
    world.publish(vec![], vec![entry]);
    world.store.refresh().unwrap();
    let err = world.store.install("nord").unwrap_err();
    assert!(err.contains("checksum"), "{err}");
    assert!(!world.store.dirs().config_dir.join("themes").exists());
}

#[test]
fn a_theme_the_user_already_has_counts_as_installed_but_is_not_removable() {
    let world = World::new();
    let (nord, entry) = theme_fixture();
    world.net.put("gallery/themes/Nord.toml", nord.clone());
    let themes = world.store.dirs().config_dir.join("themes");
    std::fs::create_dir_all(&themes).unwrap();
    std::fs::write(themes.join("Nord.toml"), nord).unwrap();
    world.publish(vec![], vec![entry]);
    let view = world.store.refresh().unwrap();
    assert_eq!(view.items[0].state, State::Installed);
    assert!(!view.items[0].removable);
    assert!(world
        .store
        .uninstall("nord")
        .unwrap_err()
        .contains("did not install"));
    assert!(themes.join("Nord.toml").is_file());
}
