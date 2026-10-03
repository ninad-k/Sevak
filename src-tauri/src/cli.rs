//! Hand-rolled command-line parsing. The surface is tiny, so no clap.

pub const USAGE: &str = "\
Usage: sevak [OPTION]

Options:
  (none)                 Start Sevak and show the search bar
      --toggle           Show the search bar, or hide it if it is visible
                         (bind this to a key on Wayland)
      --background       Start without showing the window
      --quit             Quit the running instance
      --setup-hotkey [KEY]
                         Bind KEY (default: the hotkey from config.toml) to
                         `sevak --toggle` in GNOME. Needed on Wayland.
  -h, --help             Print this help
  -V, --version          Print the version";

/// What a launch of Sevak should do once the app is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Launch {
    #[default]
    Show,
    Toggle,
    Background,
    Quit,
}

/// What the process was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invocation {
    Run(Launch),
    SetupHotkey(Option<String>),
    Help,
    Version,
}

/// Parses the arguments after the executable name.
pub fn parse<I, S>(args: I) -> Result<Invocation, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut args = args.into_iter();
    let Some(first) = args.next() else {
        return Ok(Invocation::Run(Launch::Show));
    };

    let invocation = match first.as_ref() {
        "--toggle" => Invocation::Run(Launch::Toggle),
        "--background" => Invocation::Run(Launch::Background),
        "--quit" => Invocation::Run(Launch::Quit),
        "-h" | "--help" => Invocation::Help,
        "-V" | "--version" => Invocation::Version,
        "--setup-hotkey" => {
            let key = args.next().map(|key| key.as_ref().to_owned());
            if let Some(key) = &key {
                if key.starts_with('-') {
                    return Err(format!("--setup-hotkey: expected a key, found \"{key}\""));
                }
            }
            Invocation::SetupHotkey(key)
        }
        other => return Err(format!("unknown option \"{other}\"")),
    };

    match args.next() {
        None => Ok(invocation),
        Some(extra) => Err(format!("unexpected argument \"{}\"", extra.as_ref())),
    }
}

/// Parses the argv a second instance forwarded through single-instance.
/// `argv[0]` is the executable. Anything unusable is treated as a plain launch.
pub fn parse_remote(argv: &[String]) -> Launch {
    match parse(argv.iter().skip(1)) {
        Ok(Invocation::Run(launch)) => launch,
        Ok(other) => {
            tracing::warn!(?other, "forwarded command is not a launch request; showing");
            Launch::Show
        }
        Err(err) => {
            tracing::warn!(%err, ?argv, "could not parse forwarded arguments; showing");
            Launch::Show
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_strs(args: &[&str]) -> Result<Invocation, String> {
        parse(args.iter().copied())
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
        assert_eq!(parse_strs(&["--quit"]), Ok(Invocation::Run(Launch::Quit)));
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
    }

    #[test]
    fn remote_skips_the_executable() {
        assert_eq!(parse_remote(&argv(&["sevak"])), Launch::Show);
        assert_eq!(parse_remote(&argv(&["sevak", "--toggle"])), Launch::Toggle);
        assert_eq!(parse_remote(&argv(&["sevak", "--quit"])), Launch::Quit);
        assert_eq!(
            parse_remote(&argv(&["sevak", "--background"])),
            Launch::Background
        );
    }

    #[test]
    fn remote_falls_back_to_show() {
        assert_eq!(parse_remote(&argv(&["sevak", "--bogus"])), Launch::Show);
        assert_eq!(parse_remote(&argv(&["sevak", "--help"])), Launch::Show);
        assert_eq!(parse_remote(&[]), Launch::Show);
    }
}
