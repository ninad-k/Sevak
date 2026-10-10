//! Hand-rolled command-line parsing. The surface is tiny, so no clap.

use std::fmt;
use std::path::PathBuf;

pub const USAGE: &str = "\
Usage: sevak [OPTION] [--config PATH]

Options:
  (none)                 Start Sevak and show the search bar
      --toggle           Show the search bar, or hide it if it is visible
                         (bind this to a key on Wayland)
      --query TEXT       Show the search bar with TEXT already typed in
      --run ID           Run the result with this id (e.g. apps:firefox.desktop)
                         without showing the search bar
      --actions          Universal Actions: act on what is selected in the app
                         you are using (bind this to a key on Wayland)
      --trigger WORKFLOW/ID [TEXT]
                         Start the external trigger ID of the workflow in the
                         folder WORKFLOW, with TEXT as its argument (put -- before
                         text that starts with a dash)
      --background       Start without showing the window
      --set-startup on|off
                         Start Sevak in the background after sign-in, or turn
                         startup off, for the current user. Saves the preference.
      --startup-status   Exit 0 if startup is enabled, 1 if disabled, 2 on error
      --remove-startup   Remove this copy's startup entry; keep saved settings
      --refresh-startup  Update an existing entry to this copy's path; keep
                         its arguments, disabled state and saved settings
      --startup-result PATH
                         Write the startup command's numeric exit code to PATH
                         (for installers running it as the signed-in user)
      --settings         Open the settings window
      --quit             Quit the running instance
      --setup-hotkey [KEY]
                         Bind KEY (default: the hotkey from config.toml) to
                         `sevak --toggle` in GNOME, the actions_hotkey to
                         `--actions`, and the [[hotkey]] entries to `--query` /
                         `--run`. Needed on Wayland. On GNOME, if the input-source
                         switcher uses the same key (Super+Space), it offers to
                         move that shortcut, after you confirm.
      --restore-hotkey   Put back what Sevak changed to get its shortcut: GNOME's
                         input-source shortcuts, macOS Spotlight's shortcut
      --diagnostics      Print a report for bug reports (version, system, settings
                         summary, plugin status, recent log lines) with private
                         data removed. Reads files only; sends nothing anywhere.
      --backup PATH      Save a backup of your settings, snippets, web searches,
                         themes, script plugins and workflows to PATH (a file,
                         or an existing folder). Never holds passwords, API keys,
                         clipboard or search history. The file is not encrypted.
      --restore PATH [--replace]
                         Restore a backup made by --backup or by Settings. Adds
                         what is missing and overwrites what has the same name;
                         with --replace it makes everything match the backup.
                         A safety copy is saved first. Scripts and workflows ask
                         for your approval again.
      --undo-restore     Put back what the last --restore (or Settings) replaced
      --config PATH      Use PATH as the config folder (or the config file, if
                         it ends in .toml) instead of the default; overrides
                         SEVAK_CONFIG_DIR. Only used when this process starts
                         Sevak; a running instance keeps its own config.
  -h, --help             Print this help
  -V, --version          Print the version

Environment:
  SEVAK_CONFIG_DIR       Config folder (config.toml, custom themes)
  SEVAK_DATA_DIR         Data folder (usage statistics, logs)";

/// What a launch of Sevak should do once the app is running.
#[derive(Clone, PartialEq, Eq, Default)]
pub enum Launch {
    #[default]
    Show,
    Toggle,
    Background,
    Settings,
    Quit,
    /// Show the search bar with this text typed in.
    Query(String),
    /// Run the result with this id; the window stays hidden.
    Run(String),
    /// Universal Actions: capture the selection in the foreground app and
    /// show the actions for it.
    Actions,
    /// Start a workflow's external trigger: `target` is `<workflow>/<node id>`,
    /// `arg` the text handed to it (may be empty).
    Trigger {
        target: String,
        arg: String,
    },
}

// By hand: what the user typed into a query stays out of the log.
impl fmt::Debug for Launch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Show => f.write_str("Show"),
            Self::Toggle => f.write_str("Toggle"),
            Self::Background => f.write_str("Background"),
            Self::Settings => f.write_str("Settings"),
            Self::Quit => f.write_str("Quit"),
            Self::Query(text) => write!(f, "Query({} chars)", text.chars().count()),
            Self::Run(id) => write!(f, "Run({id})"),
            Self::Actions => f.write_str("Actions"),
            Self::Trigger { target, arg } => {
                write!(f, "Trigger({target}, {} chars)", arg.chars().count())
            }
        }
    }
}

/// What the process was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invocation {
    Run(Launch),
    SetupHotkey(Option<String>),
    /// Undo the system shortcut changes Sevak made with permission.
    RestoreHotkey,
    /// Print the diagnostics report (see `diagnostics`) and exit.
    Diagnostics,
    /// Write a settings backup to this file or folder and exit.
    Backup(PathBuf),
    /// Restore a settings backup and exit; `replace` makes the chosen
    /// categories exactly like the backup instead of merging.
    Restore {
        path: PathBuf,
        replace: bool,
    },
    /// Undo the last restore and exit.
    UndoRestore,
    /// Startup management exits before creating a window or single instance.
    Startup {
        action: StartupAction,
        result: Option<PathBuf>,
    },
    Help,
    Version,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupAction {
    Set(bool),
    Status,
    Remove,
    Refresh,
}

/// A parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub invocation: Invocation,
    /// `--config PATH`, as given.
    pub config: Option<PathBuf>,
}

/// Options that take a value, as `--name VALUE` or `--name=VALUE`.
const VALUE_OPTIONS: [&str; 8] = [
    "--config",
    "--query",
    "--run",
    "--trigger",
    "--backup",
    "--restore",
    "--set-startup",
    "--startup-result",
];

/// Parses the arguments after the executable name.
pub fn parse<I, S>(args: I) -> Result<Command, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_owned())
        .peekable();
    let mut invocation: Option<Invocation> = None;
    let mut config: Option<PathBuf> = None;
    let mut replace = false;
    let mut startup_result = None;

    while let Some(arg) = args.next() {
        let (name, inline) = match arg.split_once('=') {
            Some((name, value)) if VALUE_OPTIONS.contains(&name) => (name, Some(value.to_owned())),
            _ => (arg.as_str(), None),
        };
        let set = |slot: &mut Option<Invocation>, next: Invocation| match slot.replace(next) {
            None => Ok(()),
            Some(_) => Err(format!("unexpected argument \"{arg}\"")),
        };

        if VALUE_OPTIONS.contains(&name) {
            let value = match inline {
                Some(value) => value,
                None => args
                    .next()
                    .ok_or_else(|| format!("{name}: expected a value"))?,
            };
            match name {
                "--set-startup" => {
                    let enabled = match value.as_str() {
                        "on" => true,
                        "off" => false,
                        _ => return Err("--set-startup: expected on or off".to_owned()),
                    };
                    set(
                        &mut invocation,
                        Invocation::Startup {
                            action: StartupAction::Set(enabled),
                            result: None,
                        },
                    )?;
                }
                "--startup-result" => {
                    if value.trim().is_empty() {
                        return Err("--startup-result: expected a path".to_owned());
                    }
                    if startup_result.replace(PathBuf::from(value)).is_some() {
                        return Err("--startup-result was given twice".to_owned());
                    }
                }
                "--config" => {
                    if value.is_empty() {
                        return Err("--config: expected a path".to_owned());
                    }
                    if config.replace(PathBuf::from(value)).is_some() {
                        return Err("--config was given twice".to_owned());
                    }
                }
                "--query" => set(&mut invocation, Invocation::Run(Launch::Query(value)))?,
                "--backup" | "--restore" => {
                    if value.trim().is_empty() {
                        return Err(format!("{name}: expected a path"));
                    }
                    let path = PathBuf::from(value);
                    set(
                        &mut invocation,
                        if name == "--backup" {
                            Invocation::Backup(path)
                        } else {
                            Invocation::Restore {
                                path,
                                replace: false,
                            }
                        },
                    )?;
                }
                "--trigger" => {
                    let target = value.trim().to_owned();
                    if !target.contains('/') || target.starts_with('/') || target.ends_with('/') {
                        return Err(
                            "--trigger: expected <workflow>/<trigger id>, for example my-flow/go"
                                .to_owned(),
                        );
                    }
                    // The words after it are its text, up to the next option;
                    // `--` makes everything after it text.
                    let mut words: Vec<String> = Vec::new();
                    while let Some(next) = args.peek() {
                        if next == "--" {
                            args.next();
                            words.extend(args.by_ref());
                            break;
                        }
                        if next.starts_with('-') {
                            break;
                        }
                        words.extend(args.next());
                    }
                    set(
                        &mut invocation,
                        Invocation::Run(Launch::Trigger {
                            target,
                            arg: words.join(" "),
                        }),
                    )?;
                }
                _ => {
                    if value.trim().is_empty() {
                        return Err("--run: expected a result id".to_owned());
                    }
                    set(
                        &mut invocation,
                        Invocation::Run(Launch::Run(value.trim().to_owned())),
                    )?;
                }
            }
            continue;
        }

        match name {
            "--toggle" => set(&mut invocation, Invocation::Run(Launch::Toggle))?,
            "--background" => set(&mut invocation, Invocation::Run(Launch::Background))?,
            "--settings" => set(&mut invocation, Invocation::Run(Launch::Settings))?,
            "--quit" => set(&mut invocation, Invocation::Run(Launch::Quit))?,
            "--actions" => set(&mut invocation, Invocation::Run(Launch::Actions))?,
            "-h" | "--help" => set(&mut invocation, Invocation::Help)?,
            "-V" | "--version" => set(&mut invocation, Invocation::Version)?,
            "--restore-hotkey" => set(&mut invocation, Invocation::RestoreHotkey)?,
            "--diagnostics" => set(&mut invocation, Invocation::Diagnostics)?,
            "--undo-restore" => set(&mut invocation, Invocation::UndoRestore)?,
            "--startup-status" => set(
                &mut invocation,
                Invocation::Startup {
                    action: StartupAction::Status,
                    result: None,
                },
            )?,
            "--remove-startup" => set(
                &mut invocation,
                Invocation::Startup {
                    action: StartupAction::Remove,
                    result: None,
                },
            )?,
            "--refresh-startup" => set(
                &mut invocation,
                Invocation::Startup {
                    action: StartupAction::Refresh,
                    result: None,
                },
            )?,
            "--replace" => replace = true,
            "--setup-hotkey" => {
                // The key is optional; `--config` after it is not the key.
                let key = match args.peek() {
                    Some(next) if is_value_option(next) => None,
                    Some(next) if next.starts_with('-') => {
                        return Err(format!("--setup-hotkey: expected a key, found \"{next}\""));
                    }
                    Some(_) => args.next(),
                    None => None,
                };
                set(&mut invocation, Invocation::SetupHotkey(key))?;
            }
            other if invocation.is_some() => {
                return Err(format!("unexpected argument \"{other}\""));
            }
            other => return Err(format!("unknown option \"{other}\"")),
        }
    }

    if replace {
        match &mut invocation {
            Some(Invocation::Restore { replace, .. }) => *replace = true,
            _ => return Err("--replace only goes with --restore".to_owned()),
        }
    }

    if let Some(path) = startup_result {
        match &mut invocation {
            Some(Invocation::Startup { result, .. }) => *result = Some(path),
            _ => return Err("--startup-result only goes with a startup command".to_owned()),
        }
    }

    Ok(Command {
        invocation: invocation.unwrap_or(Invocation::Run(Launch::Show)),
        config,
    })
}

fn is_value_option(arg: &str) -> bool {
    VALUE_OPTIONS
        .iter()
        .any(|name| arg == *name || arg.strip_prefix(name).is_some_and(|r| r.starts_with('=')))
}

/// What a second `sevak` invocation asked the running instance to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub launch: Launch,
    /// The `--config` it was started with: ignored by the running instance
    /// (which keeps its own), only noted in the log.
    pub config: Option<PathBuf>,
}

/// Parses the argv a second instance forwarded through single-instance.
/// `argv[0]` is the executable. Anything unusable is treated as a plain launch.
pub fn parse_remote(argv: &[String]) -> Remote {
    match parse(argv.iter().skip(1)) {
        Ok(Command {
            invocation: Invocation::Run(launch),
            config,
        }) => Remote { launch, config },
        Ok(other) => {
            tracing::warn!(?other, "forwarded command is not a launch request; showing");
            Remote {
                launch: Launch::Show,
                config: other.config,
            }
        }
        Err(err) => {
            // The count only: the arguments can carry a query.
            tracing::warn!(%err, args = argv.len(), "could not parse forwarded arguments; showing");
            Remote {
                launch: Launch::Show,
                config: None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_strs(args: &[&str]) -> Result<Invocation, String> {
        parse(args.iter().copied()).map(|command| command.invocation)
    }

    fn config_of(args: &[&str]) -> Option<PathBuf> {
        parse(args.iter().copied()).unwrap().config
    }

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_arguments_shows() {
        assert_eq!(parse_strs(&[]), Ok(Invocation::Run(Launch::Show)));
    }

    #[test]
    fn startup_commands_are_headless_and_accept_installer_receipts() {
        for (args, action) in [
            (vec!["--set-startup", "on"], StartupAction::Set(true)),
            (vec!["--set-startup=off"], StartupAction::Set(false)),
            (vec!["--startup-status"], StartupAction::Status),
            (vec!["--remove-startup"], StartupAction::Remove),
            (vec!["--refresh-startup"], StartupAction::Refresh),
        ] {
            assert_eq!(
                parse_strs(&args),
                Ok(Invocation::Startup {
                    action,
                    result: None
                })
            );
            let mut with_receipt = args;
            with_receipt.extend(["--startup-result", "result file.txt", "--config", "profile"]);
            assert_eq!(
                parse(&with_receipt).unwrap(),
                Command {
                    invocation: Invocation::Startup {
                        action,
                        result: Some("result file.txt".into())
                    },
                    config: Some("profile".into()),
                }
            );
        }
    }

    #[test]
    fn startup_commands_reject_ambiguous_or_incomplete_arguments() {
        for args in [
            vec!["--set-startup"],
            vec!["--set-startup", "yes"],
            vec!["--set-startup="],
            vec!["--set-startup", "on", "--toggle"],
            vec!["--startup-status", "--remove-startup"],
            vec!["--startup-result", "receipt"],
            vec!["--background", "--startup-result", "receipt"],
            vec!["--remove-startup", "--startup-result", ""],
            vec![
                "--remove-startup",
                "--startup-result=a",
                "--startup-result=b",
            ],
        ] {
            assert!(parse(&args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn launch_flags() {
        assert_eq!(
            parse_strs(&["--toggle"]),
            Ok(Invocation::Run(Launch::Toggle))
        );
        assert_eq!(
            parse_strs(&["--background"]),
            Ok(Invocation::Run(Launch::Background))
        );
        assert_eq!(
            parse_strs(&["--settings"]),
            Ok(Invocation::Run(Launch::Settings))
        );
        assert_eq!(parse_strs(&["--quit"]), Ok(Invocation::Run(Launch::Quit)));
        assert_eq!(
            parse_strs(&["--actions"]),
            Ok(Invocation::Run(Launch::Actions))
        );
    }

    #[test]
    fn actions_is_a_launch_of_its_own() {
        assert!(parse_strs(&["--actions", "--toggle"]).is_err());
        assert!(parse_strs(&["--actions", "stray"]).is_err());
        assert_eq!(
            parse(["--config", "d", "--actions"]).unwrap(),
            Command {
                invocation: Invocation::Run(Launch::Actions),
                config: Some("d".into()),
            }
        );
        assert_eq!(format!("{:?}", Launch::Actions), "Actions");
    }

    #[test]
    fn query_and_run() {
        assert_eq!(
            parse_strs(&["--query", "> "]),
            Ok(Invocation::Run(Launch::Query("> ".into())))
        );
        assert_eq!(
            parse_strs(&["--query=g rust"]),
            Ok(Invocation::Run(Launch::Query("g rust".into())))
        );
        // The text may look like an option.
        assert_eq!(
            parse_strs(&["--query", "--toggle"]),
            Ok(Invocation::Run(Launch::Query("--toggle".into())))
        );
        assert_eq!(
            parse_strs(&["--query", ""]),
            Ok(Invocation::Run(Launch::Query(String::new())))
        );
        assert_eq!(
            parse_strs(&["--run", "apps:firefox.desktop"]),
            Ok(Invocation::Run(Launch::Run("apps:firefox.desktop".into())))
        );
        assert_eq!(
            parse_strs(&["--run=files:/tmp/a b.txt"]),
            Ok(Invocation::Run(Launch::Run("files:/tmp/a b.txt".into())))
        );
        assert!(parse_strs(&["--query"]).is_err());
        assert!(parse_strs(&["--run"]).is_err());
        assert!(parse_strs(&["--run", "  "]).is_err());
        assert!(parse_strs(&["--query", "a", "--run", "b"]).is_err());
    }

    fn trigger(target: &str, arg: &str) -> Result<Invocation, String> {
        Ok(Invocation::Run(Launch::Trigger {
            target: target.into(),
            arg: arg.into(),
        }))
    }

    #[test]
    fn trigger_takes_a_target_and_optional_text() {
        assert_eq!(
            parse_strs(&["--trigger", "flow/go"]),
            trigger("flow/go", "")
        );
        assert_eq!(parse_strs(&["--trigger=flow/go"]), trigger("flow/go", ""));
        assert_eq!(
            parse_strs(&["--trigger", "flow/go", "hello"]),
            trigger("flow/go", "hello")
        );
        // Several words are one text.
        assert_eq!(
            parse_strs(&["--trigger", "flow/go", "hello", "big", "world"]),
            trigger("flow/go", "hello big world")
        );
        // The text may be quoted as a single argument.
        assert_eq!(
            parse_strs(&["--trigger", "flow/go", "hello big world"]),
            trigger("flow/go", "hello big world")
        );
        // `--` lets the text start with a dash or look like an option.
        assert_eq!(
            parse_strs(&["--trigger", "flow/go", "--", "-v", "--toggle"]),
            trigger("flow/go", "-v --toggle")
        );
        assert_eq!(
            parse_strs(&["--trigger", "flow/go", "--"]),
            trigger("flow/go", "")
        );
    }

    #[test]
    fn trigger_stops_at_the_next_option_and_needs_a_valid_target() {
        assert_eq!(
            parse(["--trigger", "flow/go", "text", "--config", "d"]).unwrap(),
            Command {
                invocation: Invocation::Run(Launch::Trigger {
                    target: "flow/go".into(),
                    arg: "text".into(),
                }),
                config: Some("d".into()),
            }
        );
        for bad in ["flow", "/go", "flow/", ""] {
            assert!(parse_strs(&["--trigger", bad]).is_err(), "{bad:?}");
        }
        assert!(parse_strs(&["--trigger"]).is_err());
        assert!(parse_strs(&["--toggle", "--trigger", "a/b"]).is_err());
        assert!(parse_strs(&["--trigger", "a/b", "--toggle"]).is_err());
    }

    #[test]
    fn a_forwarded_trigger_keeps_its_text() {
        assert_eq!(
            parse_remote(&argv(&["sevak", "--trigger", "a/b", "some", "text"])).launch,
            Launch::Trigger {
                target: "a/b".into(),
                arg: "some text".into()
            }
        );
    }

    #[test]
    fn debug_output_leaves_the_trigger_text_out() {
        let text = format!(
            "{:?}",
            Launch::Trigger {
                target: "a/b".into(),
                arg: "secret".into()
            }
        );
        assert!(!text.contains("secret"));
        assert!(text.contains("a/b") && text.contains("6 chars"));
    }

    #[test]
    fn config_goes_with_any_action() {
        assert_eq!(config_of(&[]), None);
        assert_eq!(
            config_of(&["--config", "sync/sevak"]),
            Some("sync/sevak".into())
        );
        assert_eq!(config_of(&["--config=/a b"]), Some("/a b".into()));
        assert_eq!(
            parse(["--config", "d", "--toggle"]).unwrap(),
            Command {
                invocation: Invocation::Run(Launch::Toggle),
                config: Some("d".into()),
            }
        );
        assert_eq!(
            parse(["--run", "x:y", "--config", "d"]).unwrap(),
            Command {
                invocation: Invocation::Run(Launch::Run("x:y".into())),
                config: Some("d".into()),
            }
        );
        // On its own it just starts Sevak.
        assert_eq!(
            parse_strs(&["--config", "d"]),
            Ok(Invocation::Run(Launch::Show))
        );
        assert!(parse_strs(&["--config"]).is_err());
        assert!(parse_strs(&["--config", ""]).is_err());
        assert!(parse_strs(&["--config", "a", "--config", "b"]).is_err());
    }

    #[test]
    fn help_and_version() {
        assert_eq!(parse_strs(&["--help"]), Ok(Invocation::Help));
        assert_eq!(parse_strs(&["-h"]), Ok(Invocation::Help));
        assert_eq!(parse_strs(&["--version"]), Ok(Invocation::Version));
        assert_eq!(parse_strs(&["-V"]), Ok(Invocation::Version));
    }

    #[test]
    fn setup_hotkey_with_and_without_key() {
        assert_eq!(
            parse_strs(&["--setup-hotkey"]),
            Ok(Invocation::SetupHotkey(None))
        );
        assert_eq!(
            parse_strs(&["--setup-hotkey", "Super+Space"]),
            Ok(Invocation::SetupHotkey(Some("Super+Space".into())))
        );
        // `--config` after it is not taken for the key, in either order.
        assert_eq!(
            parse(["--setup-hotkey", "--config", "d"]).unwrap(),
            Command {
                invocation: Invocation::SetupHotkey(None),
                config: Some("d".into()),
            }
        );
        assert_eq!(
            parse(["--config", "d", "--setup-hotkey", "F9"]).unwrap(),
            Command {
                invocation: Invocation::SetupHotkey(Some("F9".into())),
                config: Some("d".into()),
            }
        );
    }

    #[test]
    fn restore_hotkey_stands_alone() {
        assert_eq!(
            parse_strs(&["--restore-hotkey"]),
            Ok(Invocation::RestoreHotkey)
        );
        assert_eq!(
            parse(["--config", "d", "--restore-hotkey"]).unwrap(),
            Command {
                invocation: Invocation::RestoreHotkey,
                config: Some("d".into()),
            }
        );
        assert!(parse_strs(&["--restore-hotkey", "--toggle"]).is_err());
        assert!(parse_strs(&["--restore-hotkey", "Alt+Space"]).is_err());
        // A running instance is not asked to restore anything: it is a plain launch.
        let remote = parse_remote(&argv(&["sevak", "--restore-hotkey"]));
        assert_eq!(remote.launch, Launch::Show);
    }

    #[test]
    fn diagnostics_stands_alone() {
        assert_eq!(parse_strs(&["--diagnostics"]), Ok(Invocation::Diagnostics));
        assert_eq!(
            parse(["--config", "d", "--diagnostics"]).unwrap(),
            Command {
                invocation: Invocation::Diagnostics,
                config: Some("d".into()),
            }
        );
        assert!(parse_strs(&["--diagnostics", "--toggle"]).is_err());
        assert!(parse_strs(&["--toggle", "--diagnostics"]).is_err());
        assert!(parse_strs(&["--diagnostics", "stray"]).is_err());
        // A running instance is never asked for a report: a plain launch.
        let remote = parse_remote(&argv(&["sevak", "--diagnostics"]));
        assert_eq!(remote.launch, Launch::Show);
    }

    #[test]
    fn backup_and_restore_take_a_path() {
        assert_eq!(
            parse_strs(&["--backup", "b.sevakbackup"]),
            Ok(Invocation::Backup(PathBuf::from("b.sevakbackup")))
        );
        assert_eq!(
            parse_strs(&["--backup=/tmp/dir"]),
            Ok(Invocation::Backup(PathBuf::from("/tmp/dir")))
        );
        assert_eq!(
            parse_strs(&["--restore", "b.sevakbackup"]),
            Ok(Invocation::Restore {
                path: PathBuf::from("b.sevakbackup"),
                replace: false
            })
        );
        assert_eq!(
            parse_strs(&["--restore", "b.sevakbackup", "--replace"]),
            Ok(Invocation::Restore {
                path: PathBuf::from("b.sevakbackup"),
                replace: true
            })
        );
        assert_eq!(
            parse_strs(&["--replace", "--restore=x"]),
            Ok(Invocation::Restore {
                path: PathBuf::from("x"),
                replace: true
            })
        );
        assert_eq!(
            config_of(&["--config", "c", "--backup", "b"]),
            Some(PathBuf::from("c"))
        );
        assert_eq!(parse_strs(&["--undo-restore"]), Ok(Invocation::UndoRestore));
    }

    #[test]
    fn backup_and_restore_are_strict_about_their_arguments() {
        assert!(parse_strs(&["--backup"]).is_err());
        assert!(parse_strs(&["--restore"]).is_err());
        assert!(parse_strs(&["--backup", ""]).is_err());
        assert!(parse_strs(&["--backup", "a", "--restore", "b"]).is_err());
        assert!(parse_strs(&["--backup", "a", "--toggle"]).is_err());
        assert!(parse_strs(&["--backup", "a", "b"]).is_err());
        // --replace belongs to --restore alone.
        assert!(parse_strs(&["--replace"]).is_err());
        assert!(parse_strs(&["--backup", "a", "--replace"]).is_err());
        assert!(parse_strs(&["--undo-restore", "--replace"]).is_err());
        // And --restore-hotkey is still its own flag.
        assert_eq!(
            parse_strs(&["--restore-hotkey"]),
            Ok(Invocation::RestoreHotkey)
        );
    }

    #[test]
    fn a_forwarded_backup_command_just_shows() {
        let remote = parse_remote(&argv(&["sevak", "--backup", "x"]));
        assert_eq!(remote.launch, Launch::Show);
    }

    #[test]
    fn setup_hotkey_rejects_flag_as_key() {
        assert!(parse_strs(&["--setup-hotkey", "--toggle"]).is_err());
    }

    #[test]
    fn unknown_flag_is_an_error() {
        let err = parse_strs(&["--frobnicate"]).unwrap_err();
        assert!(err.contains("--frobnicate"));
    }

    #[test]
    fn trailing_arguments_are_an_error() {
        assert!(parse_strs(&["--toggle", "--quit"]).is_err());
        assert!(parse_strs(&["--setup-hotkey", "Alt+Space", "x"]).is_err());
        assert!(parse_strs(&["--toggle", "stray"]).is_err());
        assert!(parse_strs(&["stray"]).is_err());
    }

    #[test]
    fn remote_skips_the_executable() {
        let launch = |args: &[&str]| parse_remote(&argv(args)).launch;
        assert_eq!(launch(&["sevak"]), Launch::Show);
        assert_eq!(launch(&["sevak", "--toggle"]), Launch::Toggle);
        assert_eq!(launch(&["sevak", "--quit"]), Launch::Quit);
        assert_eq!(launch(&["sevak", "--settings"]), Launch::Settings);
        assert_eq!(launch(&["sevak", "--background"]), Launch::Background);
        assert_eq!(launch(&["sevak", "--actions"]), Launch::Actions);
        assert_eq!(
            launch(&["sevak", "--query", "> "]),
            Launch::Query("> ".into())
        );
        assert_eq!(
            launch(&["sevak", "--run", "apps:x"]),
            Launch::Run("apps:x".into())
        );
    }

    #[test]
    fn remote_reports_the_config_it_was_given() {
        let remote = parse_remote(&argv(&["sevak", "--config", "other", "--toggle"]));
        assert_eq!(remote.launch, Launch::Toggle);
        assert_eq!(remote.config, Some("other".into()));
        assert_eq!(parse_remote(&argv(&["sevak", "--toggle"])).config, None);
    }

    #[test]
    fn remote_falls_back_to_show() {
        let launch = |args: &[&str]| parse_remote(&argv(args)).launch;
        assert_eq!(launch(&["sevak", "--bogus"]), Launch::Show);
        assert_eq!(launch(&["sevak", "--help"]), Launch::Show);
        assert_eq!(parse_remote(&[]).launch, Launch::Show);
    }

    #[test]
    fn debug_output_leaves_the_query_text_out() {
        let text = format!("{:?}", Launch::Query("secret".into()));
        assert!(!text.contains("secret"));
        assert!(text.contains("6 chars"));
    }
}
