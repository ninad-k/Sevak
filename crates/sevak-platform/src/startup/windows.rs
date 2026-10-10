use std::io;

use winreg::enums::{
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_BINARY,
};
use winreg::{RegKey, RegValue};

use super::{Backend, LaunchCommand, Status, ENTRY_NAME};

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

pub struct Native {
    command: LaunchCommand,
}

pub struct Snapshot {
    run: Option<RegValue>,
    approved: Option<RegValue>,
}

impl Native {
    pub fn new(command: LaunchCommand) -> Result<Self, String> {
        Ok(Self { command })
    }
}

fn read_value(key: &str) -> Result<Option<RegValue>, String> {
    read_value_at(&RegKey::predef(HKEY_CURRENT_USER), key)
}

fn read_value_at(root: &RegKey, key: &str) -> Result<Option<RegValue>, String> {
    match root
        .open_subkey_with_flags(key, KEY_QUERY_VALUE)
        .and_then(|key| key.get_raw_value(ENTRY_NAME))
    {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("cannot read startup registration: {error}")),
    }
}

fn write_value(key: &str, value: Option<&RegValue>) -> Result<(), String> {
    let root = RegKey::predef(HKEY_CURRENT_USER);
    let result = if let Some(value) = value {
        root.create_subkey(key)
            .and_then(|(key, _)| key.set_raw_value(ENTRY_NAME, value))
    } else {
        root.open_subkey_with_flags(key, KEY_SET_VALUE)
            .and_then(|key| key.delete_value(ENTRY_NAME))
    };
    match result {
        Ok(()) => Ok(()),
        Err(error) if value.is_none() && error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("cannot update startup registration: {error}")),
    }
}

fn enabled_by_windows(value: Option<&RegValue>) -> Result<bool, String> {
    let Some(value) = value else { return Ok(true) };
    // StartupApproved values begin with a DWORD state. Windows uses 2/6 for
    // enabled and 3/7 for disabled; leave unknown future states alone.
    if value.vtype != REG_BINARY || value.bytes.len() < 4 {
        return Err("Windows returned an invalid startup approval value".to_owned());
    }
    let state = u32::from_le_bytes(value.bytes[..4].try_into().unwrap());
    Ok(matches!(state, 2 | 6))
}

/// Quote each argument using the Windows CommandLineToArgvW conventions.
fn quote(arg: &str) -> String {
    let mut result = String::from("\"");
    let mut slashes = 0;
    for ch in arg.chars() {
        if ch == '\\' {
            slashes += 1;
        } else {
            result.extend(std::iter::repeat_n(
                '\\',
                if ch == '"' { slashes * 2 + 1 } else { slashes },
            ));
            slashes = 0;
            result.push(ch);
        }
    }
    result.extend(std::iter::repeat_n('\\', slashes * 2));
    result.push('"');
    result
}

fn command_line(command: &LaunchCommand) -> String {
    let mut words = vec![quote(&command.executable.to_string_lossy())];
    words.extend(command.args().iter().map(|arg| quote(arg)));
    words.join(" ")
}

fn owned_by(value: &RegValue, command: &LaunchCommand) -> bool {
    use winreg::types::FromRegValue;
    let Ok(text) = String::from_reg_value(value) else {
        return false;
    };
    let expected = quote(&command.executable.to_string_lossy());
    let text = text.to_lowercase();
    let expected = expected.to_lowercase();
    text == expected
        || text
            .strip_prefix(&expected)
            .is_some_and(|rest| rest.starts_with(' '))
        // auto-launch 0.6 wrote the literal, unquoted executable followed by
        // --background. Match that entire legacy command, never a basename
        // or a prefix that could belong to another installed copy.
        || text == command.executable.to_string_lossy().to_lowercase()
        || text == format!("{} --background", command.executable.to_string_lossy().to_lowercase())
}

fn check_legacy_machine_entry(command: &LaunchCommand) -> Result<(), String> {
    if read_value_at(&RegKey::predef(HKEY_LOCAL_MACHINE), RUN)?
        .as_ref()
        .is_some_and(|value| owned_by(value, command))
    {
        return Err(format!("An older Sevak version registered startup for all users. Ask an administrator to remove only the Sevak value from HKLM\\{RUN}, then enable startup here for your account."));
    }
    Ok(())
}

fn refreshed_command(value: &RegValue, command: &LaunchCommand) -> Result<RegValue, String> {
    use winreg::types::{FromRegValue, ToRegValue};
    let text = String::from_reg_value(value).map_err(|error| error.to_string())?;
    let (old_executable, suffix) = if let Some(quoted) = text.strip_prefix('"') {
        let end = quoted
            .find('"')
            .ok_or("invalid quoted startup executable")?;
        (&quoted[..end], &quoted[end + 1..])
    } else if let Some((exe, _)) = text.split_once(" --background") {
        (exe, &text[exe.len()..])
    } else {
        (text.as_str(), "")
    };
    if !std::path::Path::new(old_executable)
        .file_name()
        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("sevak.exe"))
    {
        return Err("the startup entry points to a different application; its executable was left unchanged".to_owned());
    }
    Ok(format!("{}{suffix}", quote(&command.executable.to_string_lossy())).to_reg_value())
}

impl Backend for Native {
    type Snapshot = Snapshot;

    fn snapshot(&self) -> Result<Snapshot, String> {
        check_legacy_machine_entry(&self.command)?;
        Ok(Snapshot {
            run: read_value(RUN)?,
            approved: read_value(APPROVED)?,
        })
    }

    fn status(&self, snapshot: &Snapshot) -> Result<Status, String> {
        Ok(Status {
            registered: snapshot.run.is_some(),
            enabled: snapshot.run.is_some() && enabled_by_windows(snapshot.approved.as_ref())?,
        })
    }

    fn set_enabled(&self, enabled: bool, explicit: bool) -> Result<(), String> {
        use winreg::types::ToRegValue;
        if enabled {
            write_value(RUN, Some(&command_line(&self.command).to_reg_value()))?;
            if explicit {
                // Explicit opt-in clears a previous Task Manager disable.
                let mut bytes = vec![0; 12];
                bytes[0] = 2;
                write_value(
                    APPROVED,
                    Some(&RegValue {
                        bytes,
                        vtype: REG_BINARY,
                    }),
                )?;
            }
        } else {
            write_value(RUN, None)?;
            write_value(APPROVED, None)?;
        }
        Ok(())
    }

    fn restore(&self, snapshot: &Snapshot) -> Result<(), String> {
        write_value(RUN, snapshot.run.as_ref())?;
        write_value(APPROVED, snapshot.approved.as_ref())
    }

    fn remove_owned(&self) -> Result<(), String> {
        check_legacy_machine_entry(&self.command)?;
        if read_value(RUN)?
            .as_ref()
            .is_some_and(|value| owned_by(value, &self.command))
        {
            self.set_enabled(false, false)?;
        }
        Ok(())
    }

    fn refresh_target(&self, snapshot: &Snapshot) -> Result<(), String> {
        if let Some(value) = &snapshot.run {
            write_value(RUN, Some(&refreshed_command(value, &self.command)?))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winreg::types::ToRegValue;

    #[test]
    fn disabled_approval_is_never_mistaken_for_enabled_even_without_timestamp() {
        for (state, enabled) in [(2, true), (3, false), (6, true), (7, false), (99, false)] {
            let mut bytes = vec![0; 12];
            bytes[0] = state;
            assert_eq!(
                enabled_by_windows(Some(&RegValue {
                    bytes,
                    vtype: REG_BINARY
                }))
                .unwrap(),
                enabled
            );
        }
        assert!(enabled_by_windows(None).unwrap());
    }

    #[test]
    fn command_is_quoted_background_and_preserves_the_config_path() {
        let command = LaunchCommand::new(
            r"C:\Program Files\Sevak\sevak.exe".into(),
            Some(r"C:\Users\Test User\config.toml".into()),
        )
        .unwrap();
        assert_eq!(
            command_line(&command),
            r#""C:\Program Files\Sevak\sevak.exe" "--background" "--config" "C:\Users\Test User\config.toml""#
        );
        assert_eq!(quote("a\\\"b\\"), "\"a\\\\\\\"b\\\\\"");
    }

    #[test]
    fn uninstall_only_owns_the_exact_executable_not_another_installation() {
        let command = LaunchCommand::new(r"C:\Apps\Sevak\sevak.exe".into(), None).unwrap();
        assert!(owned_by(
            &r#""c:\apps\sevak\SEVAK.EXE" --background"#.to_reg_value(),
            &command
        ));
        assert!(!owned_by(
            &r#""C:\Apps\Sevak\sevak.exe.old" --background"#.to_reg_value(),
            &command
        ));
        assert!(!owned_by(
            &r#""C:\New\Sevak\sevak.exe" --background"#.to_reg_value(),
            &command
        ));
        assert!(owned_by(
            &r"c:\apps\sevak\sevak.exe --background".to_reg_value(),
            &command
        ));
        assert!(!owned_by(
            &r"c:\apps\sevak\sevak.exe.other --background".to_reg_value(),
            &command
        ));
        assert!(!owned_by(
            &r"c:\new\sevak\sevak.exe --background".to_reg_value(),
            &command
        ));
    }

    #[test]
    fn moved_installation_refreshes_only_target_and_retains_custom_arguments() {
        use winreg::types::FromRegValue;
        let command = LaunchCommand::new(r"C:\New App\sevak.exe".into(), None).unwrap();
        let old =
            r#""C:\Old App\sevak.exe" "--background" "--config" "D:\Custom Profile\config.toml""#;
        let result = refreshed_command(&old.to_reg_value(), &command).unwrap();
        assert_eq!(
            String::from_reg_value(&result).unwrap(),
            old.replace("Old App", "New App")
        );
        let legacy = r"C:\Old App\sevak.exe --background";
        assert_eq!(
            String::from_reg_value(&refreshed_command(&legacy.to_reg_value(), &command).unwrap())
                .unwrap(),
            r#""C:\New App\sevak.exe" --background"#
        );
        assert!(refreshed_command(
            &r#""C:\Other\other.exe" --background"#.to_reg_value(),
            &command
        )
        .is_err());
    }
}
