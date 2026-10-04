//! macOS: letting Sevak have Cmd+Space, which belongs to Spotlight.
//!
//! Spotlight's shortcut is the system symbolic hotkey 64 in
//! `com.apple.symbolichotkeys`. While it is on, registering Cmd+Space fails.
//! With the user's permission (asked by the caller, never here) Sevak turns
//! that one entry off with `defaults write`, asks the system to re-read its
//! settings, and can turn it back on again.
//!
//! Every call to the system goes through [`CommandRunner`], so the decisions
//! and the commands are unit-tested with a fake on any OS. Nothing in here runs
//! unless a caller on macOS asks it to.

use std::process::Command;

/// The preferences domain of the system keyboard shortcuts.
pub const DOMAIN: &str = "com.apple.symbolichotkeys";
/// The dictionary inside it that holds one entry per shortcut.
const HOTKEYS: &str = "AppleSymbolicHotKeys";
/// Spotlight's "Show Spotlight search" is entry 64.
const SPOTLIGHT_KEY: &str = "64";
/// Asks the system to re-read the shortcut settings without logging out.
const ACTIVATE_SETTINGS: &str =
    "/System/Library/PrivateFrameworks/SystemAdministration.framework/Resources/activateSettings";
/// The Keyboard Shortcuts pane of System Settings.
const SHORTCUTS_PANE: &str = "x-apple.systempreferences:com.apple.Keyboard-Settings.extension";

/// Key code of Space and the modifier mask of Cmd in a symbolic hotkey.
const KEY_CODE_SPACE: i64 = 49;
const MASK_COMMAND: i64 = 1_048_576;

/// What is wrong, as one line for a log or a message box.
pub type Error = String;

/// Runs a program and returns what it printed. A failing program is an error
/// carrying its message.
pub trait CommandRunner {
    fn run(&self, program: &str, args: &[String]) -> Result<String, Error>;
}

/// Runs the real programs.
pub struct SystemRunner;

impl CommandRunner for SystemRunner {
    fn run(&self, program: &str, args: &[String]) -> Result<String, Error> {
        let output = Command::new(program)
            .args(args)
            .output()
            .map_err(|err| format!("{program}: {err}"))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(format!(
                "{program} failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }
}

/// What Spotlight's shortcut is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpotlightShortcut {
    /// On, and bound to Cmd+Space (the default): Sevak cannot have the key.
    Active,
    /// Off: Cmd+Space is free as far as Spotlight goes.
    Disabled,
    /// On, but the user gave it another key: Cmd+Space is free.
    Remapped,
}

/// Reads Spotlight's shortcut from the output of `defaults export <domain> -`
/// (an XML property list). An entry that is missing means the factory default:
/// on, Cmd+Space. `None` if the text is not a symbolic-hotkeys list at all.
pub fn parse_shortcut(xml: &str) -> Option<SpotlightShortcut> {
    if !xml.contains(HOTKEYS) {
        // No customised shortcuts at all: everything is at its default.
        return xml.contains("<plist").then_some(SpotlightShortcut::Active);
    }
    // No entry 64 in a customised list: the factory default.
    let Some((_, after_key)) = xml.split_once(&format!("<key>{SPOTLIGHT_KEY}</key>")) else {
        return Some(SpotlightShortcut::Active);
    };
    // The entry ends where the next shortcut's number starts.
    let entry = next_number_key(after_key).map_or(after_key, |end| &after_key[..end]);

    let enabled = entry
        .split_once("<key>enabled</key>")
        .map(|(_, rest)| rest.trim_start())
        .map(|rest| rest.starts_with("<true"));
    if enabled == Some(false) {
        return Some(SpotlightShortcut::Disabled);
    }

    // `parameters` is [character, key code, modifier mask].
    let numbers: Vec<i64> = entry
        .split_once("<key>parameters</key>")
        .map(|(_, rest)| rest)
        .and_then(|rest| rest.split_once("</array>"))
        .map(|(array, _)| {
            array
                .split("<integer>")
                .skip(1)
                .filter_map(|part| part.split_once("</integer>"))
                .filter_map(|(number, _)| number.trim().parse().ok())
                .collect()
        })
        .unwrap_or_default();
    if let [_, code, mask, ..] = numbers[..] {
        if code != KEY_CODE_SPACE || mask != MASK_COMMAND {
            return Some(SpotlightShortcut::Remapped);
        }
    }
    Some(SpotlightShortcut::Active)
}

/// Where the next `<key>NN</key>` (a number: another shortcut) starts.
fn next_number_key(text: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(found) = text[from..].find("<key>") {
        let start = from + found;
        let name = &text[start + 5..];
        if let Some((name, _)) = name.split_once("</key>") {
            if !name.is_empty() && name.bytes().all(|b| b.is_ascii_digit()) {
                return Some(start);
            }
        }
        from = start + 5;
    }
    None
}

/// What Spotlight's shortcut is doing right now.
pub fn shortcut_state(runner: &dyn CommandRunner) -> Result<SpotlightShortcut, Error> {
    let xml = runner.run(
        "defaults",
        &["export".to_owned(), DOMAIN.to_owned(), "-".to_owned()],
    )?;
    parse_shortcut(&xml).ok_or_else(|| "could not read the keyboard shortcut settings".to_owned())
}

/// The commands that turn Spotlight's shortcut on or off: one `defaults write`
/// replacing entry 64 (keeping its default key, Cmd+Space), then the system is
/// told to re-read its settings. Each is `(program, arguments)`.
pub fn set_enabled_commands(enabled: bool) -> Vec<(String, Vec<String>)> {
    let entry = format!(
        "<dict><key>enabled</key><{flag}/><key>value</key><dict><key>parameters</key>\
         <array><integer>32</integer><integer>{KEY_CODE_SPACE}</integer>\
         <integer>{MASK_COMMAND}</integer></array><key>type</key><string>standard</string>\
         </dict></dict>",
        flag = if enabled { "true" } else { "false" },
    );
    vec![
        (
            "defaults".to_owned(),
            [
                "write",
                DOMAIN,
                HOTKEYS,
                "-dict-add",
                SPOTLIGHT_KEY,
                entry.as_str(),
            ]
            .map(str::to_owned)
            .to_vec(),
        ),
        (ACTIVATE_SETTINGS.to_owned(), vec!["-u".to_owned()]),
    ]
}

fn set_enabled(runner: &dyn CommandRunner, enabled: bool) -> Result<(), Error> {
    let commands = set_enabled_commands(enabled);
    let (write, activate) = (&commands[0], &commands[1]);
    runner.run(&write.0, &write.1)?;
    // Without this the change only applies after the next login; if it is
    // refused the setting is still written, so it is a note, not a failure.
    if let Err(err) = runner.run(&activate.0, &activate.1) {
        tracing::warn!("could not reload the keyboard shortcut settings: {err}");
    }
    Ok(())
}

/// Turns Spotlight's Cmd+Space shortcut off. Only call this with the user's
/// permission.
pub fn disable(runner: &dyn CommandRunner) -> Result<(), Error> {
    set_enabled(runner, false)
}

/// Turns Spotlight's Cmd+Space shortcut back on.
pub fn restore(runner: &dyn CommandRunner) -> Result<(), Error> {
    set_enabled(runner, true)
}

/// Opens System Settings at Keyboard Shortcuts, for the user to switch
/// Spotlight's shortcut off by hand when `defaults` is not enough.
pub fn open_keyboard_shortcuts(runner: &dyn CommandRunner) -> Result<(), Error> {
    runner.run("open", &[SHORTCUTS_PANE.to_owned()]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Records the commands and answers from a script.
    #[derive(Default)]
    struct Fake {
        calls: RefCell<Vec<(String, Vec<String>)>>,
        output: String,
        fail_on: Option<&'static str>,
    }

    impl CommandRunner for Fake {
        fn run(&self, program: &str, args: &[String]) -> Result<String, Error> {
            self.calls
                .borrow_mut()
                .push((program.to_owned(), args.to_vec()));
            if self.fail_on.is_some_and(|name| program.contains(name)) {
                return Err(format!("{program} refused"));
            }
            Ok(self.output.clone())
        }
    }

    fn plist(entry_64: &str) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict>\
             <key>AppleSymbolicHotKeys</key><dict>\
             <key>60</key><dict><key>enabled</key><true/><key>value</key><dict>\
             <key>parameters</key><array><integer>32</integer><integer>49</integer>\
             <integer>262144</integer></array></dict></dict>\
             {entry_64}\
             <key>65</key><dict><key>enabled</key><false/></dict>\
             </dict></dict></plist>"
        )
    }

    fn entry(enabled: bool, code: i64, mask: i64) -> String {
        format!(
            "<key>64</key><dict><key>enabled</key><{}/><key>value</key><dict>\
             <key>parameters</key><array><integer>32</integer><integer>{code}</integer>\
             <integer>{mask}</integer></array><key>type</key><string>standard</string>\
             </dict></dict>",
            if enabled { "true" } else { "false" }
        )
    }

    #[test]
    fn an_enabled_cmd_space_entry_is_active() {
        let xml = plist(&entry(true, 49, 1_048_576));
        assert_eq!(parse_shortcut(&xml), Some(SpotlightShortcut::Active));
    }

    #[test]
    fn a_disabled_entry_is_disabled_whatever_its_key() {
        let xml = plist(&entry(false, 49, 1_048_576));
        assert_eq!(parse_shortcut(&xml), Some(SpotlightShortcut::Disabled));
    }

    #[test]
    fn another_key_means_cmd_space_is_free() {
        // Cmd+Option+Space (a different mask) and Cmd+K (another key code).
        for (code, mask) in [(49, 1_572_864), (40, 1_048_576)] {
            let xml = plist(&entry(true, code, mask));
            assert_eq!(parse_shortcut(&xml), Some(SpotlightShortcut::Remapped));
        }
    }

    #[test]
    fn neighbouring_entries_are_not_mixed_up() {
        // 65 is disabled and 60 is Ctrl+Space, but 64 is untouched.
        let xml = plist(&entry(true, 49, 1_048_576));
        assert_eq!(parse_shortcut(&xml), Some(SpotlightShortcut::Active));
        // No entry 64 in a customised list: the default (on).
        assert_eq!(parse_shortcut(&plist("")), Some(SpotlightShortcut::Active));
    }

    #[test]
    fn nothing_customised_is_the_default() {
        assert_eq!(
            parse_shortcut("<?xml version=\"1.0\"?><plist version=\"1.0\"><dict/></plist>"),
            Some(SpotlightShortcut::Active)
        );
        assert_eq!(parse_shortcut("garbage"), None);
        assert_eq!(parse_shortcut(""), None);
    }

    #[test]
    fn disabling_writes_entry_64_and_reloads_the_settings() {
        let runner = Fake::default();
        disable(&runner).unwrap();
        let calls = runner.calls.borrow();
        assert_eq!(calls.len(), 2);
        let (program, args) = &calls[0];
        assert_eq!(program, "defaults");
        assert_eq!(
            &args[..5],
            ["write", DOMAIN, "AppleSymbolicHotKeys", "-dict-add", "64"]
        );
        assert!(
            args[5].contains("<key>enabled</key><false/>"),
            "{}",
            args[5]
        );
        assert!(args[5].contains("<integer>49</integer>"));
        assert!(args[5].contains("<integer>1048576</integer>"));
        assert!(calls[1].0.ends_with("/activateSettings"));
        assert_eq!(calls[1].1, ["-u"]);
    }

    #[test]
    fn restoring_writes_the_same_entry_enabled() {
        let runner = Fake::default();
        restore(&runner).unwrap();
        let calls = runner.calls.borrow();
        assert!(calls[0].1[5].contains("<key>enabled</key><true/>"));
    }

    #[test]
    fn what_is_written_reads_back_as_the_state_it_means() {
        for (enabled, expected) in [
            (false, SpotlightShortcut::Disabled),
            (true, SpotlightShortcut::Active),
        ] {
            let written = set_enabled_commands(enabled)[0].1[5].clone();
            let xml = plist(&format!("<key>64</key>{written}"));
            assert_eq!(parse_shortcut(&xml), Some(expected), "{written}");
        }
    }

    #[test]
    fn a_failed_write_is_an_error_but_a_refused_reload_is_not() {
        let runner = Fake {
            fail_on: Some("defaults"),
            ..Fake::default()
        };
        assert!(disable(&runner).unwrap_err().contains("refused"));
        // The reload is not attempted after a failed write.
        assert_eq!(runner.calls.borrow().len(), 1);

        let runner = Fake {
            fail_on: Some("activateSettings"),
            ..Fake::default()
        };
        assert_eq!(disable(&runner), Ok(()));
    }

    #[test]
    fn the_state_is_read_from_defaults_export() {
        let runner = Fake {
            output: plist(&entry(true, 49, 1_048_576)),
            ..Fake::default()
        };
        assert_eq!(shortcut_state(&runner), Ok(SpotlightShortcut::Active));
        let calls = runner.calls.borrow();
        assert_eq!(calls[0].0, "defaults");
        assert_eq!(calls[0].1, ["export", DOMAIN, "-"]);

        let runner = Fake {
            output: "nonsense".to_owned(),
            ..Fake::default()
        };
        assert!(shortcut_state(&runner).is_err());
    }

    #[test]
    fn the_settings_pane_is_opened_with_open() {
        let runner = Fake::default();
        open_keyboard_shortcuts(&runner).unwrap();
        let calls = runner.calls.borrow();
        assert_eq!(calls[0].0, "open");
        assert!(calls[0].1[0].starts_with("x-apple.systempreferences:"));
    }
}
