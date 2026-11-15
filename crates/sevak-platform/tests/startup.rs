//! End-to-end preference/registration transactions with a fake OS boundary.
//! Every config and registration lives under a temporary directory; these
//! tests deliberately never construct the real per-user startup backend.

use std::cell::{Cell, RefCell};
use std::fs;

use sevak_core::Config;
use sevak_platform::startup::{self, Backend, Status};

#[derive(Default)]
struct FakeOs {
    state: Cell<Status>,
    sets: Cell<usize>,
    fail_after_write: Cell<bool>,
    fail_restore: Cell<bool>,
    refuse_write: Cell<bool>,
    fail_read: Cell<bool>,
    on_set: RefCell<Option<Box<dyn FnOnce()>>>,
    refreshes: Cell<usize>,
}

impl Backend for FakeOs {
    type Snapshot = Status;
    fn snapshot(&self) -> Result<Status, String> {
        if self.fail_read.get() {
            return Err("Cannot read startup state".into());
        }
        Ok(self.state.get())
    }
    fn status(&self, snapshot: &Status) -> Result<Status, String> {
        Ok(*snapshot)
    }
    fn set_enabled(&self, enabled: bool, _explicit: bool) -> Result<(), String> {
        self.sets.set(self.sets.get() + 1);
        if !self.refuse_write.get() {
            // Writing the entry points it at this copy.
            self.state.set(Status {
                registered: enabled,
                enabled,
                other_copy: false,
            });
        }
        if let Some(callback) = self.on_set.borrow_mut().take() {
            callback();
        }
        if self.fail_after_write.get() {
            Err("OS denied the change".into())
        } else {
            Ok(())
        }
    }
    fn restore(&self, snapshot: &Status) -> Result<(), String> {
        if self.fail_restore.get() {
            return Err("OS denied rollback".into());
        }
        self.state.set(*snapshot);
        Ok(())
    }
    fn remove_owned(&self) -> Result<(), String> {
        self.state.set(Status::default());
        Ok(())
    }
    fn refresh_target(&self, snapshot: &Status) -> Result<(), String> {
        self.sets.set(self.sets.get() + 1);
        self.refreshes.set(self.refreshes.get() + 1);
        // Only the executable changes; registration and OS approval stay.
        self.state.set(Status {
            other_copy: false,
            ..*snapshot
        });
        Ok(())
    }
}

#[test]
fn first_install_enable_disable_and_reinstall_share_one_persisted_preference() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("new-profile/config.toml");
    let os = FakeOs::default();
    startup::set_preference(&config, true, &os).unwrap();
    assert!(
        Config::load_or_create(&config)
            .unwrap()
            .0
            .general
            .launch_at_login
    );
    assert!(startup::status(&os).unwrap().enabled);
    let saved = fs::read(&config).unwrap();
    os.remove_owned().unwrap();
    assert_eq!(
        fs::read(&config).unwrap(),
        saved,
        "uninstall keeps settings"
    );
    let wanted = Config::load_or_create(&config)
        .unwrap()
        .0
        .general
        .launch_at_login;
    startup::update(&os, wanted, false, || Ok(())).unwrap();
    assert!(
        startup::status(&os).unwrap().enabled,
        "reinstall restores opt-in"
    );
    startup::set_preference(&config, false, &os).unwrap();
    assert!(
        !Config::load_or_create(&config)
            .unwrap()
            .0
            .general
            .launch_at_login
    );
    assert!(!startup::status(&os).unwrap().registered);
    startup::set_preference(&config, false, &os).unwrap();
}

#[test]
fn installer_only_edits_startup_and_keeps_comments_and_unknown_settings() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    let before = "# personal settings\n[general]\nhotkey = 'Alt+K'\nlaunch_at_login = false # my choice\nfuture_key = 42\n[window]\nwidth = 99000\n";
    fs::write(&config, before).unwrap();
    startup::set_preference(&config, true, &FakeOs::default()).unwrap();
    assert_eq!(
        fs::read_to_string(config).unwrap(),
        before.replace("false", "true")
    );
}

#[test]
fn invalid_config_is_never_overwritten_or_registered() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    let os = FakeOs::default();
    for invalid in [
        "not TOML",
        "[general]\nlaunch_at_login = 'yes'",
        "general = 3",
    ] {
        fs::write(&config, invalid).unwrap();
        assert!(startup::set_preference(&config, true, &os).is_err());
        assert_eq!(fs::read_to_string(&config).unwrap(), invalid);
    }
    assert_eq!(os.sets.get(), 0);
}

#[test]
fn partial_os_failure_restores_old_registration_and_preserves_settings() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    fs::write(&config, "[general]\nlaunch_at_login = false\n").unwrap();
    let before = fs::read(&config).unwrap();
    let os = FakeOs::default();
    os.fail_after_write.set(true);
    assert!(startup::set_preference(&config, true, &os)
        .unwrap_err()
        .contains("denied"));
    assert_eq!(fs::read(config).unwrap(), before);
    assert_eq!(os.state.get(), Status::default());
}

#[test]
fn persistence_failure_rolls_registration_back_even_after_disable() {
    let os = FakeOs::default();
    startup::update(&os, true, true, || Ok(())).unwrap();
    let error = startup::update(&os, false, true, || Err("disk full".into())).unwrap_err();
    assert!(error.contains("disk full"));
    assert!(startup::status(&os).unwrap().enabled);
}

#[test]
fn failure_to_create_config_directory_leaves_no_enabled_registration() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("blocked");
    fs::write(&file, "not a folder").unwrap();
    let os = FakeOs::default();
    assert!(startup::set_preference(&file.join("config.toml"), true, &os).is_err());
    assert_eq!(os.state.get(), Status::default());
}

#[test]
fn external_disable_survives_restart_and_unrelated_settings_save() {
    let os = FakeOs::default();
    os.state.set(Status {
        registered: true,
        enabled: false,
        ..Status::default()
    });
    startup::update(&os, true, false, || Ok(())).unwrap();
    assert_eq!(os.sets.get(), 0);
    assert!(!startup::status(&os).unwrap().enabled);
    startup::update(&os, true, true, || Ok(())).unwrap();
    assert!(startup::status(&os).unwrap().enabled);
}

#[test]
fn enabled_entries_are_refreshed_to_follow_a_new_executable_location() {
    let os = FakeOs::default();
    os.state.set(Status {
        registered: true,
        enabled: true,
        ..Status::default()
    });
    startup::update(&os, true, false, || Ok(())).unwrap();
    assert_eq!(
        os.sets.get(),
        1,
        "refresh even when the enabled bit already matches"
    );
}

#[test]
fn rejected_os_write_cannot_be_reported_as_success() {
    let os = FakeOs::default();
    os.refuse_write.set(true);
    let saved = Cell::new(false);
    assert!(startup::update(&os, true, true, || {
        saved.set(true);
        Ok(())
    })
    .is_err());
    assert!(!saved.get());
}

#[test]
fn rollback_failure_is_included_in_the_actionable_error() {
    let os = FakeOs::default();
    os.fail_restore.set(true);
    let error = startup::update(&os, true, true, || Err("disk full".into())).unwrap_err();
    assert!(error.contains("disk full"));
    assert!(error.contains("rollback"));
}

#[test]
fn unrelated_settings_save_succeeds_when_startup_access_is_denied() {
    let saved = Cell::new(false);
    startup::save_settings::<FakeOs>(
        true,
        true,
        || Err("registry access denied".into()),
        || {
            saved.set(true);
            Ok(())
        },
    )
    .unwrap();
    assert!(saved.get());
}

#[test]
fn changed_startup_setting_is_not_saved_when_startup_access_is_denied() {
    let saved = Cell::new(false);
    let error = startup::save_settings::<FakeOs>(
        false,
        true,
        || Err("registry access denied".into()),
        || {
            saved.set(true);
            Ok(())
        },
    )
    .unwrap_err();
    assert!(error.contains("registry access denied"));
    assert!(!saved.get());
}

#[test]
fn installer_refresh_preserves_disabled_state_and_never_creates_registration() {
    let os = FakeOs::default();
    startup::refresh(&os).unwrap();
    assert_eq!(os.sets.get(), 0);
    os.state.set(Status {
        registered: true,
        enabled: false,
        ..Status::default()
    });
    startup::refresh(&os).unwrap();
    assert_eq!(os.sets.get(), 1);
    assert_eq!(
        os.state.get(),
        Status {
            registered: true,
            enabled: false,
            ..Status::default()
        }
    );
}

#[test]
fn explicit_repair_enables_missing_or_disabled_entry_without_rewriting_config() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    let original = "# personal settings\r\n[general]\r\nlaunch_at_login = true # opted in\r\nfuture_setting = 'keep'\r\n";
    fs::write(&config, original).unwrap();
    for registered in [false, true] {
        let os = FakeOs::default();
        os.state.set(Status {
            registered,
            enabled: false,
            ..Status::default()
        });
        assert_eq!(
            startup::repair(&config, &os).unwrap(),
            Status {
                registered: true,
                enabled: true,
                other_copy: false
            }
        );
        assert_eq!(os.sets.get(), 1);
        assert_eq!(fs::read_to_string(&config).unwrap(), original);
    }
}

#[test]
fn repair_requires_valid_saved_opt_in_and_does_not_create_a_missing_config() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    let os = FakeOs::default();
    assert!(startup::repair(&config, &os).is_err());
    assert!(!config.exists());
    for original in [
        "[general]\nlaunch_at_login = false\n",
        "[general]\nlaunch_at_login = 'yes'\n",
    ] {
        fs::write(&config, original).unwrap();
        assert!(startup::repair(&config, &os).is_err());
        assert_eq!(fs::read_to_string(&config).unwrap(), original);
    }
    assert_eq!(os.sets.get(), 0);
}

#[test]
fn failed_repair_restores_external_disable_and_keeps_the_saved_preference() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    let original = "[general]\nlaunch_at_login = true\n";
    fs::write(&config, original).unwrap();
    let os = FakeOs::default();
    let before = Status {
        registered: true,
        enabled: false,
        ..Status::default()
    };
    os.state.set(before);
    os.fail_after_write.set(true);
    assert!(startup::repair(&config, &os)
        .unwrap_err()
        .contains("denied"));
    assert_eq!(os.state.get(), before);
    assert_eq!(fs::read_to_string(&config).unwrap(), original);
    os.fail_read.set(true);
    assert!(startup::repair(&config, &os)
        .unwrap_err()
        .contains("Cannot read"));
    assert_eq!(
        os.sets.get(),
        1,
        "a status error must not attempt another write"
    );
}

#[test]
fn repair_rolls_back_if_saved_opt_in_is_withdrawn_while_registering() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    fs::write(&config, "[general]\nlaunch_at_login = true\n").unwrap();
    let os = FakeOs::default();
    let changed = config.clone();
    *os.on_set.borrow_mut() = Some(Box::new(move || {
        fs::write(changed, "[general]\nlaunch_at_login = false\n").unwrap()
    }));
    assert!(startup::repair(&config, &os)
        .unwrap_err()
        .contains("settings changed"));
    assert_eq!(os.state.get(), Status::default());
    assert!(
        !Config::load_or_create(&config)
            .unwrap()
            .0
            .general
            .launch_at_login
    );
}

#[test]
fn saved_preference_reads_the_file_and_defaults_to_off_when_missing() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    assert_eq!(startup::saved_preference(&config), Ok(false));
    fs::write(&config, "[general]\nlaunch_at_login = true\n").unwrap();
    assert_eq!(startup::saved_preference(&config), Ok(true));
    fs::write(&config, "[general]\nlaunch_at_login = 'yes'\n").unwrap();
    assert!(startup::saved_preference(&config).is_err());
}

#[test]
fn settings_save_keeps_a_preference_changed_outside_the_window() {
    // Settings loaded "on"; the installer or CLI then saved "off".
    // Leaving the switch alone keeps "off" and changes nothing in the OS.
    assert_eq!(
        startup::settings_preference(true, true, Some(false)),
        (false, false)
    );
    // Moving the switch is an explicit choice, compared with what is on disk.
    assert_eq!(
        startup::settings_preference(false, true, Some(false)),
        (false, true)
    );
    assert_eq!(
        startup::settings_preference(true, false, Some(true)),
        (true, false)
    );
    // An unreadable file falls back to what Settings loaded.
    assert_eq!(
        startup::settings_preference(true, false, None),
        (true, false)
    );
    assert_eq!(startup::settings_preference(true, true, None), (true, true));
}

#[test]
fn unrelated_settings_save_never_constructs_the_startup_backend() {
    for preference in [false, true] {
        let saved = Cell::new(false);
        startup::save_settings::<FakeOs>(
            preference,
            preference,
            || panic!("an unrelated save must not touch the OS startup entry"),
            || {
                saved.set(true);
                Ok(())
            },
        )
        .unwrap();
        assert!(saved.get());
    }
}

#[test]
fn routine_sync_points_a_disabled_entry_at_this_copy_without_enabling_it() {
    let os = FakeOs::default();
    os.state.set(Status {
        registered: true,
        enabled: false,
        other_copy: true,
    });
    startup::update(&os, true, false, || Ok(())).unwrap();
    assert_eq!(os.refreshes.get(), 1);
    assert_eq!(
        os.state.get(),
        Status {
            registered: true,
            enabled: false,
            other_copy: false
        },
        "a later OS re-enable must start this executable, not the old path"
    );
    // An entry that already starts this copy is not rewritten on every launch.
    startup::update(&os, true, false, || Ok(())).unwrap();
    assert_eq!(os.refreshes.get(), 1);
    // A preference that is off never refreshes; it removes the entry.
    os.state.set(Status {
        registered: true,
        enabled: false,
        other_copy: true,
    });
    startup::update(&os, false, false, || Ok(())).unwrap();
    assert_eq!(os.refreshes.get(), 1);
    assert!(!os.state.get().registered);
}

#[test]
fn startup_transactions_in_one_process_never_interleave() {
    use std::sync::mpsc;
    use std::time::Duration;
    let (first_inside, wait_first) = mpsc::channel();
    let (release_first, first_released) = mpsc::channel::<()>();
    let first = std::thread::spawn(move || {
        let os = FakeOs::default();
        startup::update(&os, true, true, || {
            first_inside.send(()).unwrap();
            // Hold the transaction open, as a slow registry or disk would.
            first_released.recv().unwrap();
            Ok(())
        })
        .unwrap();
    });
    wait_first.recv().unwrap();
    let (second_inside, wait_second) = mpsc::channel();
    let second = std::thread::spawn(move || {
        let os = FakeOs::default();
        *os.on_set.borrow_mut() = Some(Box::new(move || second_inside.send(()).unwrap()));
        startup::update(&os, true, true, || Ok(())).unwrap();
    });
    assert!(
        wait_second
            .recv_timeout(Duration::from_millis(300))
            .is_err(),
        "a second transaction changed the OS entry while the first could still roll back"
    );
    release_first.send(()).unwrap();
    wait_second.recv_timeout(Duration::from_secs(10)).unwrap();
    first.join().unwrap();
    second.join().unwrap();
}
