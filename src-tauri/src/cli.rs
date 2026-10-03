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
      --background       Start without showing the window
      --settings         Open the settings window
      --quit             Quit the running instance
      --setup-hotkey [KEY]
                         Bind KEY (default: the hotkey from config.toml) to
                         `sevak --toggle` in GNOME, and the [[hotkey]] entries
                         to `--query` / `--run`. Needed on Wayland.
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
        }
    }
}

/// What the process was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invocation {
    Run(Launch),
    SetupHotkey(Option<String>),
    Help,
    Version,
}

/// A parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub invocation: Invocation,
    /// `--config PATH`, as given.
    pub config: Option<PathBuf>,
}

/// Options that take a value, as `--name VALUE` or `--name=VALUE`.
const VALUE_OPTIONS: [&str; 3] = ["--config", "--query", "--run"];

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
                "--config" => {
                    if value.is_empty() {
                        return Err("--config: expected a path".to_owned());
                    }
                    if config.replace(PathBuf::from(value)).is_some() {
                        return Err("--config was given twice".to_owned());
                    }
                }
                "--query" => set(&mut invocation, Invocation::Run(Launch::Query(value)))?,
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
            "-h" | "--help" => set(&mut invocation, Invocation::Help)?,
            "-V" | "--version" => set(&mut invocation, Invocation::Version)?,
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
            tracing::warn!(%err, ?argv, "could not parse forwarded arguments; showing");
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
