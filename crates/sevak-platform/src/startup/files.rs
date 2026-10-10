use std::fs;
use std::io;
use std::path::PathBuf;

use super::{write_atomic, Backend, LaunchCommand, Status, ENTRY_NAME};

#[derive(Clone, Copy)]
enum Kind {
    #[cfg(any(target_os = "linux", test))]
    Linux,
    #[cfg(any(target_os = "macos", test))]
    Macos,
}

pub struct Native {
    file: PathBuf,
    command: LaunchCommand,
    kind: Kind,
    #[cfg(target_os = "macos")]
    system_override: bool,
}

pub struct Snapshot {
    contents: Option<Vec<u8>>,
    disabled: bool,
}

impl Native {
    #[cfg(not(windows))]
    pub fn new(command: LaunchCommand) -> Result<Self, String> {
        #[cfg(target_os = "linux")]
        let (file, kind) = (
            dirs::config_dir()
                .ok_or("cannot determine the autostart directory")?
                .join("autostart")
                .join(format!("{ENTRY_NAME}.desktop")),
            Kind::Linux,
        );
        #[cfg(target_os = "macos")]
        let (file, kind) = (
            dirs::home_dir()
                .ok_or("cannot determine the LaunchAgents directory")?
                .join("Library/LaunchAgents")
                .join(format!("{ENTRY_NAME}.plist")),
            Kind::Macos,
        );
        Ok(Self {
            file,
            command,
            kind,
            #[cfg(target_os = "macos")]
            system_override: true,
        })
    }

    fn content(&self) -> Result<Vec<u8>, String> {
        match self.kind {
            #[cfg(any(target_os = "linux", test))]
            Kind::Linux => {
                let mut args = vec![self.command.executable.to_string_lossy().into_owned()];
                args.extend(self.command.args());
                let exec = args
                    .iter()
                    .map(|arg| desktop_quote(arg))
                    .collect::<Vec<_>>()
                    .join(" ");
                Ok(format!("[Desktop Entry]\nType=Application\nVersion=1.0\nName=Sevak\nComment=Keyboard-first launcher\nExec={exec}\nStartupNotify=false\nTerminal=false\nX-GNOME-Autostart-enabled=true\n").into_bytes())
            }
            #[cfg(any(target_os = "macos", test))]
            Kind::Macos => {
                let mut args = vec![plist::Value::String(
                    self.command.executable.to_string_lossy().into_owned(),
                )];
                args.extend(self.command.args().into_iter().map(plist::Value::String));
                let mut dictionary = plist::Dictionary::new();
                dictionary.insert("Label".into(), ENTRY_NAME.into());
                dictionary.insert("ProgramArguments".into(), plist::Value::Array(args));
                dictionary.insert(
                    "AssociatedBundleIdentifiers".into(),
                    plist::Value::Array(vec!["com.ninad.sevak".into()]),
                );
                dictionary.insert("RunAtLoad".into(), true.into());
                let mut bytes = Vec::new();
                plist::Value::Dictionary(dictionary)
                    .to_writer_xml(&mut bytes)
                    .map_err(|error| error.to_string())?;
                Ok(bytes)
            }
        }
    }

    fn inspect(&self, contents: &[u8]) -> Result<(bool, bool), String> {
        match self.kind {
            #[cfg(any(target_os = "linux", test))]
            Kind::Linux => {
                let text = std::str::from_utf8(contents).map_err(|error| error.to_string())?;
                let groups = crate::desktop_entry::parse_groups(text);
                let group = groups
                    .iter()
                    .find(|group| group.name == "Desktop Entry")
                    .ok_or("startup file is missing [Desktop Entry]")?;
                let enabled = group.get("Hidden") != Some("true")
                    && group.get("X-GNOME-Autostart-enabled") != Some("false");
                // A Hidden-only file is a valid XDG override/tombstone.
                // Desktop startup managers may remove Exec when disabling it.
                let args = group
                    .get("Exec")
                    .ok_or_else(|| "startup file is missing Exec".to_owned())
                    .and_then(|exec| {
                        crate::desktop_entry::parse_exec(exec).map_err(|error| error.to_string())
                    });
                let owned = match args {
                    Ok(args) => args
                        .first()
                        .is_some_and(|path| std::path::Path::new(path) == self.command.executable),
                    Err(error) if enabled => return Err(error),
                    Err(_) => false,
                };
                Ok((owned, enabled))
            }
            #[cfg(any(target_os = "macos", test))]
            Kind::Macos => {
                let value = plist::Value::from_reader(std::io::Cursor::new(contents))
                    .map_err(|error| error.to_string())?;
                let dictionary = value
                    .as_dictionary()
                    .ok_or("startup plist must be a dictionary")?;
                let executable = dictionary
                    .get("Program")
                    .and_then(plist::Value::as_string)
                    .or_else(|| {
                        dictionary
                            .get("ProgramArguments")
                            .and_then(plist::Value::as_array)
                            .and_then(|args| args.first())
                            .and_then(plist::Value::as_string)
                    });
                let owned = executable
                    .is_some_and(|path| std::path::Path::new(path) == self.command.executable);
                let enabled = dictionary
                    .get("Disabled")
                    .and_then(plist::Value::as_boolean)
                    != Some(true);
                Ok((owned, enabled))
            }
        }
    }

    fn disabled_by_system(&self) -> Result<bool, String> {
        #[cfg(target_os = "macos")]
        if self.system_override {
            let output = std::process::Command::new("/bin/launchctl")
                .args(["print-disabled", &login_domain()])
                .output()
                .map_err(|error| error.to_string())?;
            if !output.status.success() {
                return Err("macOS could not report the login item's status".to_owned());
            }
            let text = String::from_utf8_lossy(&output.stdout);
            return Ok(text.lines().any(|line| {
                line.trim()
                    .starts_with(&format!("\"{ENTRY_NAME}\" => true"))
            }));
        }
        Ok(false)
    }

    fn set_system_disabled(&self, disabled: bool) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        if self.system_override {
            let target = format!("{}/{ENTRY_NAME}", login_domain());
            let status = std::process::Command::new("/bin/launchctl")
                .args([if disabled { "disable" } else { "enable" }, &target])
                .status()
                .map_err(|error| error.to_string())?;
            if !status.success() {
                return Err("macOS could not update the login item's status".to_owned());
            }
        }
        let _ = disabled;
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn login_domain() -> String {
    // geteuid has no preconditions and does not access memory through pointers.
    format!("gui/{}", unsafe { libc::geteuid() })
}

impl Backend for Native {
    type Snapshot = Snapshot;

    fn snapshot(&self) -> Result<Snapshot, String> {
        let contents = match sevak_core::bounded_read::read_capped(&self.file, 1024 * 1024) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(format!("cannot read startup entry: {error}")),
        };
        let disabled = self.disabled_by_system()?;
        Ok(Snapshot { contents, disabled })
    }

    fn status(&self, snapshot: &Snapshot) -> Result<Status, String> {
        match &snapshot.contents {
            None => Ok(Status::default()),
            Some(contents) => Ok(Status {
                registered: true,
                enabled: self.inspect(contents)?.1 && !snapshot.disabled,
            }),
        }
    }

    fn set_enabled(&self, enabled: bool, explicit: bool) -> Result<(), String> {
        if enabled {
            write_atomic(&self.file, &self.content()?).map_err(|error| error.to_string())?;
            if explicit {
                self.set_system_disabled(false)?;
            }
            Ok(())
        } else {
            remove_file(&self.file)
        }
    }

    fn restore(&self, snapshot: &Snapshot) -> Result<(), String> {
        match &snapshot.contents {
            Some(contents) => {
                write_atomic(&self.file, contents).map_err(|error| error.to_string())?
            }
            None => remove_file(&self.file)?,
        }
        self.set_system_disabled(snapshot.disabled)
    }

    fn remove_owned(&self) -> Result<(), String> {
        if let Some(contents) = self.snapshot()?.contents {
            if self.inspect(&contents)?.0 {
                remove_file(&self.file)?;
            }
        }
        Ok(())
    }

    fn refresh_target(&self, snapshot: &Snapshot) -> Result<(), String> {
        let Some(contents) = &snapshot.contents else {
            return Ok(());
        };
        let updated = match self.kind {
            #[cfg(any(target_os = "linux", test))]
            Kind::Linux => {
                let text = std::str::from_utf8(contents).map_err(|error| error.to_string())?;
                let groups = crate::desktop_entry::parse_groups(text);
                let group = groups
                    .iter()
                    .find(|group| group.name == "Desktop Entry")
                    .ok_or("startup file is missing [Desktop Entry]")?;
                let Some(exec) = group.get("Exec") else {
                    return Ok(());
                };
                let mut args =
                    crate::desktop_entry::parse_exec(exec).map_err(|error| error.to_string())?;
                require_sevak(args.first().map(String::as_str))?;
                args[0] = self.command.executable.to_string_lossy().into_owned();
                let exec = args
                    .iter()
                    .map(|arg| desktop_quote(arg))
                    .collect::<Vec<_>>()
                    .join(" ");
                let mut in_entry = false;
                let mut replaced = false;
                let mut updated = String::new();
                for line in text.split_inclusive('\n') {
                    let trimmed = line.trim();
                    if trimmed.starts_with('[') {
                        in_entry = trimmed == "[Desktop Entry]";
                    }
                    if in_entry
                        && !replaced
                        && trimmed
                            .split_once('=')
                            .is_some_and(|(key, _)| key.trim() == "Exec")
                    {
                        let ending = if line.ends_with("\r\n") {
                            "\r\n"
                        } else if line.ends_with('\n') {
                            "\n"
                        } else {
                            ""
                        };
                        updated.push_str(&format!("Exec={exec}{ending}"));
                        replaced = true;
                    } else {
                        updated.push_str(line);
                    }
                }
                updated.into_bytes()
            }
            #[cfg(any(target_os = "macos", test))]
            Kind::Macos => {
                let mut value = plist::Value::from_reader(std::io::Cursor::new(contents))
                    .map_err(|error| error.to_string())?;
                let dictionary = value
                    .as_dictionary_mut()
                    .ok_or("startup plist must be a dictionary")?;
                if let Some(program) = dictionary.get_mut("Program") {
                    require_sevak(program.as_string())?;
                    *program = self
                        .command
                        .executable
                        .to_string_lossy()
                        .into_owned()
                        .into();
                } else if let Some(args) = dictionary
                    .get_mut("ProgramArguments")
                    .and_then(plist::Value::as_array_mut)
                {
                    require_sevak(args.first().and_then(plist::Value::as_string))?;
                    args[0] = self
                        .command
                        .executable
                        .to_string_lossy()
                        .into_owned()
                        .into();
                } else {
                    return Ok(());
                }
                let mut updated = Vec::new();
                value
                    .to_writer_xml(&mut updated)
                    .map_err(|error| error.to_string())?;
                updated
            }
        };
        write_atomic(&self.file, &updated).map_err(|error| error.to_string())
    }
}

fn require_sevak(executable: Option<&str>) -> Result<(), String> {
    if executable
        .and_then(|path| std::path::Path::new(path).file_name())
        .is_some_and(|name| {
            let name = name.to_string_lossy();
            name.eq_ignore_ascii_case("sevak") || name.eq_ignore_ascii_case("sevak.exe")
        })
    {
        Ok(())
    } else {
        Err("the startup entry points to a different application; its executable was left unchanged".to_owned())
    }
}

fn remove_file(path: &std::path::Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("cannot remove startup entry: {error}")),
    }
}

#[cfg(any(target_os = "linux", test))]
fn desktop_quote(value: &str) -> String {
    let mut quoted = String::from("\"");
    for ch in value.chars() {
        match ch {
            '%' => quoted.push_str("%%"),
            '\\' | '"' | '$' | '`' => {
                quoted.push('\\');
                quoted.push(ch);
            }
            _ => quoted.push(ch),
        }
    }
    quoted.push('"');
    // Desktop files unescape general values before parsing Exec arguments.
    quoted.replace('\\', "\\\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backend(temp: &tempfile::TempDir, kind: Kind) -> Native {
        Native {
            file: temp.path().join("startup-entry"),
            command: LaunchCommand::new(
                temp.path().join("Sevak & friends.exe"),
                Some(temp.path().join("settings file.toml")),
            )
            .unwrap(),
            kind,
            #[cfg(target_os = "macos")]
            system_override: false,
        }
    }

    #[test]
    fn linux_and_macos_entries_round_trip_paths_and_background_arguments() {
        let temp = tempfile::tempdir().unwrap();
        for kind in [Kind::Linux, Kind::Macos] {
            let backend = backend(&temp, kind);
            backend.set_enabled(true, true).unwrap();
            let saved = fs::read(&backend.file).unwrap();
            assert_eq!(backend.inspect(&saved).unwrap(), (true, true));
            assert!(String::from_utf8_lossy(&saved).contains("--background"));
            assert!(String::from_utf8_lossy(&saved).contains("--config"));
            assert!(super::super::status(&backend).unwrap().enabled);
            backend.remove_owned().unwrap();
            assert!(!backend.file.exists());
        }
    }

    #[test]
    fn linux_exec_quotes_every_special_character() {
        let path = "/home/person/Sevak's \"100%\" $`\\.AppImage";
        assert_eq!(
            crate::desktop_entry::parse_exec(&desktop_quote(path)).unwrap(),
            vec![path]
        );
    }

    #[test]
    fn disabled_file_is_preserved_by_sync_and_reenabled_only_explicitly() {
        let temp = tempfile::tempdir().unwrap();
        let backend = backend(&temp, Kind::Linux);
        backend.set_enabled(true, true).unwrap();
        let hidden = format!(
            "{}Hidden=true\n",
            fs::read_to_string(&backend.file).unwrap()
        );
        fs::write(&backend.file, &hidden).unwrap();
        super::super::update(&backend, true, false, || Ok(())).unwrap();
        assert_eq!(fs::read_to_string(&backend.file).unwrap(), hidden);
        super::super::update(&backend, true, true, || Ok(())).unwrap();
        assert!(super::super::status(&backend).unwrap().enabled);
    }

    #[test]
    fn linux_hidden_only_override_can_be_preserved_then_explicitly_reenabled() {
        let temp = tempfile::tempdir().unwrap();
        let backend = backend(&temp, Kind::Linux);
        for disabled in ["Hidden=true", "X-GNOME-Autostart-enabled=false"] {
            let content = format!("[Desktop Entry]\n{disabled}\n");
            fs::write(&backend.file, &content).unwrap();
            assert_eq!(
                super::super::status(&backend).unwrap(),
                Status {
                    registered: true,
                    enabled: false
                }
            );
            super::super::update(&backend, true, false, || Ok(())).unwrap();
            assert_eq!(fs::read_to_string(&backend.file).unwrap(), content);
            super::super::update(&backend, true, true, || Ok(())).unwrap();
            assert!(super::super::status(&backend).unwrap().enabled);
        }
    }

    #[test]
    fn removing_an_old_install_keeps_the_new_install_entry() {
        let temp = tempfile::tempdir().unwrap();
        for kind in [Kind::Linux, Kind::Macos] {
            let mut backend = backend(&temp, kind);
            backend.set_enabled(true, true).unwrap();
            backend.command.executable = temp.path().join("old-sevak");
            backend.remove_owned().unwrap();
            assert!(backend.file.exists());
        }
    }

    #[test]
    fn refresh_preserves_disabled_state_and_original_profile_arguments() {
        let temp = tempfile::tempdir().unwrap();
        for kind in [Kind::Linux, Kind::Macos] {
            let mut backend = backend(&temp, kind);
            backend.command.executable = temp.path().join("old/sevak");
            backend.set_enabled(true, true).unwrap();
            match kind {
                Kind::Linux => {
                    let mut content = fs::read_to_string(&backend.file).unwrap();
                    content.push_str("Hidden=true\n");
                    fs::write(&backend.file, content).unwrap();
                }
                Kind::Macos => {
                    let mut value = plist::Value::from_file(&backend.file).unwrap();
                    value
                        .as_dictionary_mut()
                        .unwrap()
                        .insert("Disabled".into(), true.into());
                    value.to_file_xml(&backend.file).unwrap();
                }
            }
            let original_profile = backend.command.config_file.take().unwrap();
            backend.command.executable = temp.path().join("new/sevak");
            super::super::refresh(&backend).unwrap();
            let saved = fs::read(&backend.file).unwrap();
            assert_eq!(backend.inspect(&saved).unwrap(), (true, false));
            // Restore the known original profile and compare parsed argument
            // values so both XML and desktop escaping are exercised.
            let args = match kind {
                Kind::Linux => {
                    let text = std::str::from_utf8(&saved).unwrap();
                    let groups = crate::desktop_entry::parse_groups(text);
                    crate::desktop_entry::parse_exec(groups[0].get("Exec").unwrap()).unwrap()
                }
                Kind::Macos => plist::Value::from_reader(std::io::Cursor::new(saved))
                    .unwrap()
                    .as_dictionary()
                    .unwrap()
                    .get("ProgramArguments")
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_string().unwrap().to_owned())
                    .collect(),
            };
            assert_eq!(
                args,
                vec![
                    backend.command.executable.to_string_lossy().into_owned(),
                    "--background".into(),
                    "--config".into(),
                    original_profile.to_string_lossy().into_owned()
                ]
            );
        }
    }
}
