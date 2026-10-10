//! End-to-end preference/registration transactions with a fake OS boundary.
//! Every config and registration lives under a temporary directory; these
//! tests deliberately never construct the real per-user startup backend.

use std::cell::Cell;
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
}

impl Backend for FakeOs {
    type Snapshot = Status;
    fn snapshot(&self) -> Result<Status, String> {
        Ok(self.state.get())
    }
    fn status(&self, snapshot: &Status) -> Result<Status, String> {
        Ok(*snapshot)
    }
    fn set_enabled(&self, enabled: bool, _explicit: bool) -> Result<(), String> {
        self.sets.set(self.sets.get() + 1);
        if !self.refuse_write.get() {
            self.state.set(Status {
                registered: enabled,
                enabled,
            });
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
    fn refresh_target(&self, _snapshot: &Status) -> Result<(), String> {
        self.sets.set(self.sets.get() + 1);
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
    });
    startup::refresh(&os).unwrap();
    assert_eq!(os.sets.get(), 1);
    assert_eq!(
        os.state.get(),
        Status {
            registered: true,
            enabled: false
        }
    );
}
