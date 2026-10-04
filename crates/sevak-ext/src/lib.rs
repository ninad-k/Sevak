//! The logic of `sevak-ext`, as a library so it can be tested.
//!
//! | Command | What it does |
//! |---|---|
//! | `init` | scaffolds a native extension project from `templates/rust-extension` |
//! | `validate` | checks a project folder or a `.sevakext` package the way Sevak does |
//! | `pack` | builds a deterministic `.sevakext` (or one per platform) |
//! | `entry` | prints the gallery index entry for packages |
//!
//! The checks are Sevak's own (`sevak_plugins::extensions`), so what `validate`
//! accepts is what the app installs.

mod entry;
mod init;
mod pack;
mod validate;

use std::io::Write;

pub use init::{render_template, Values};

/// The help text.
pub const USAGE: &str = "\
Usage: sevak-ext <command> [options]

Commands:
  init <dir> [--name NAME] [--keyword KEY] [--description TEXT] [--author NAME]
        Create a native extension project in <dir> (new or empty) from the
        template. NAME defaults to the folder's name.
  validate <dir|package.sevakext>
        Check a project's plugin.toml, or a whole package (checksums, paths,
        sizes, manifest for every platform), the way Sevak does on install.
  pack [<dir>] --binary PLATFORM=FILE... [--include FILE[=PATH]]... [--out DIR] [--split]
        Build <name>-<version>.sevakext from <dir>/plugin.toml (default: the
        current folder) with the given programs. PLATFORM is one of
        windows-x86_64, windows-aarch64, macos-x86_64, macos-aarch64,
        linux-x86_64, linux-aarch64. --split writes one package per platform
        (what the gallery lists). Prints each package's SHA-256.
  entry <package.sevakext>...
        Print the gallery/index.json entry for packages built from one
        manifest, with the files' SHA-256. Copy the packages to
        gallery/extensions/<id>/ and the entry into the index.

Options:
  -h, --help     Print this help
  -V, --version  Print the version";

/// Why a command did not succeed.
#[derive(Debug, PartialEq, Eq)]
pub enum Failure {
    /// The command line was wrong; the usage text follows the message.
    Usage(String),
    /// The command ran and failed.
    Message(String),
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self::Message(message)
    }
}

/// Runs `sevak-ext` with `args` (without the program name), writing to `out`.
pub fn run(args: &[String], out: &mut dyn Write) -> Result<(), Failure> {
    let Some((command, rest)) = args.split_first() else {
        return Err(Failure::Usage("no command given".to_owned()));
    };
    match command.as_str() {
        "-h" | "--help" | "help" => {
            writeln!(out, "{USAGE}").map_err(io)?;
            Ok(())
        }
        "-V" | "--version" => {
            writeln!(out, "sevak-ext {}", env!("CARGO_PKG_VERSION")).map_err(io)?;
            Ok(())
        }
        "init" => init::command(rest, out),
        "validate" => validate::command(rest, out),
        "pack" => pack::command(rest, out),
        "entry" => entry::command(rest, out),
        other => Err(Failure::Usage(format!("unknown command \"{other}\""))),
    }
}

fn io(err: std::io::Error) -> Failure {
    Failure::Message(err.to_string())
}

/// A tiny option parser: `--flag`, `--name VALUE` and `--name=VALUE`, plus
/// positional arguments. Unknown options are an error.
pub(crate) struct Args {
    pub positional: Vec<String>,
    pub values: Vec<(String, String)>,
    pub flags: Vec<String>,
}

impl Args {
    pub fn parse(
        args: &[String],
        value_options: &[&str],
        flag_options: &[&str],
    ) -> Result<Self, Failure> {
        let mut parsed = Self {
            positional: Vec::new(),
            values: Vec::new(),
            flags: Vec::new(),
        };
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            if let Some(option) = arg.strip_prefix("--") {
                let (name, inline) = match option.split_once('=') {
                    Some((name, value)) => (name, Some(value.to_owned())),
                    None => (option, None),
                };
                if flag_options.contains(&name) && inline.is_none() {
                    parsed.flags.push(name.to_owned());
                } else if value_options.contains(&name) {
                    let value = match inline {
                        Some(value) => value,
                        None => iter
                            .next()
                            .cloned()
                            .ok_or_else(|| Failure::Usage(format!("--{name} needs a value")))?,
                    };
                    parsed.values.push((name.to_owned(), value));
                } else {
                    return Err(Failure::Usage(format!("unknown option \"{arg}\"")));
                }
            } else {
                parsed.positional.push(arg.clone());
            }
        }
        Ok(parsed)
    }

    /// The last value given for `name`.
    pub fn value(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .rev()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// Every value given for `name`, in order.
    pub fn all(&self, name: &str) -> Vec<&str> {
        self.values
            .iter()
            .filter(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
            .collect()
    }

    pub fn flag(&self, name: &str) -> bool {
        self.flags.iter().any(|flag| flag == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_text(args: &[&str]) -> Result<String, Failure> {
        let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
        let mut out = Vec::new();
        run(&args, &mut out).map(|()| String::from_utf8(out).unwrap())
    }

    #[test]
    fn help_and_version() {
        assert!(run_text(&["--help"]).unwrap().contains("Usage: sevak-ext"));
        assert!(run_text(&["-V"]).unwrap().starts_with("sevak-ext "));
    }

    #[test]
    fn bad_command_lines_are_usage_errors() {
        assert!(matches!(run_text(&[]), Err(Failure::Usage(_))));
        assert!(matches!(run_text(&["frobnicate"]), Err(Failure::Usage(_))));
        assert!(matches!(
            run_text(&["pack", "--bogus"]),
            Err(Failure::Usage(_))
        ));
        assert!(matches!(run_text(&["init"]), Err(Failure::Usage(_))));
    }

    #[test]
    fn options_parse_in_both_spellings() {
        let args: Vec<String> = ["a", "--out", "x", "--split", "--binary=l=p", "b"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        let parsed = Args::parse(&args, &["out", "binary"], &["split"]).unwrap();
        assert_eq!(parsed.positional, ["a", "b"]);
        assert_eq!(parsed.value("out"), Some("x"));
        assert_eq!(parsed.all("binary"), ["l=p"]);
        assert!(parsed.flag("split"));
        let missing: Vec<String> = vec!["--out".to_owned()];
        assert!(Args::parse(&missing, &["out"], &[]).is_err());
    }
}
