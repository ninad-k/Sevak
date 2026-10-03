//! Parser for freedesktop `.desktop` files (Desktop Entry Specification).
//!
//! Pure string processing with no OS calls, so it is compiled and unit-tested
//! on every platform; only the Linux provider uses it at runtime.
//!
//! The pieces:
//!
//! - [`parse`] turns file contents into a [`DesktopEntry`], resolving localized
//!   keys for a [`Locale`] and splitting `Exec` into an argv.
//! - [`parse_exec`] implements the `Exec` quoting and field-code rules.
//! - [`is_visible`] applies `Type`, `NoDisplay`, `Hidden`, `OnlyShowIn` and
//!   `NotShowIn`.
//! - [`desktop_file_id`] derives the desktop-file ID from a path.

use std::collections::HashMap;
use std::env;
use std::path::{Component, Path};

use thiserror::Error;

/// The group every application entry lives in.
const MAIN_GROUP: &str = "Desktop Entry";

/// Field codes dropped from `Exec` (`%i %c %k` are stripped, not expanded).
/// The first six are the current codes, the rest are deprecated.
const FIELD_CODES: &str = "fFuUickdDnNvm";

/// Why a file could not be parsed as a desktop entry.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ParseError {
    #[error("no [Desktop Entry] group")]
    MissingDesktopEntryGroup,
}

/// A locale for localized keys such as `Name[de_DE]`.
///
/// `Locale::default()` means "no locale" (the `C`/`POSIX` locale): only
/// unlocalized values are used.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Locale {
    lang: String,
    country: Option<String>,
    modifier: Option<String>,
}

impl Locale {
    /// Parses `lang_COUNTRY.ENCODING@MODIFIER` (every part but `lang` optional).
    /// `C`, `POSIX` and empty strings yield the default (no) locale.
    pub fn parse(value: &str) -> Self {
        let value = value.trim();
        let (rest, modifier) = match value.split_once('@') {
            Some((rest, modifier)) => (rest, Some(modifier)),
            None => (value, None),
        };
        let rest = rest.split_once('.').map_or(rest, |(rest, _encoding)| rest);
        let (lang, country) = match rest.split_once('_') {
            Some((lang, country)) => (lang, Some(country)),
            None => (rest, None),
        };
        if lang.is_empty() || lang == "C" || lang == "POSIX" {
            return Self::default();
        }
        Self {
            lang: lang.to_owned(),
            country: country.filter(|c| !c.is_empty()).map(str::to_owned),
            modifier: modifier.filter(|m| !m.is_empty()).map(str::to_owned),
        }
    }

    /// Picks the message locale from the first non-empty of `LC_ALL`,
    /// `LC_MESSAGES` and `LANG`, like gettext does.
    pub fn from_env() -> Self {
        Self::from_vars(
            env::var("LC_ALL").ok().as_deref(),
            env::var("LC_MESSAGES").ok().as_deref(),
            env::var("LANG").ok().as_deref(),
        )
    }

    /// The pure part of [`Locale::from_env`].
    pub fn from_vars(lc_all: Option<&str>, lc_messages: Option<&str>, lang: Option<&str>) -> Self {
        [lc_all, lc_messages, lang]
            .into_iter()
            .flatten()
            .find(|value| !value.is_empty())
            .map(Self::parse)
            .unwrap_or_default()
    }

    /// The key suffixes to try, most specific first, per the spec's matching
    /// order: `lang_COUNTRY@MODIFIER`, `lang_COUNTRY`, `lang@MODIFIER`, `lang`.
    fn candidates(&self) -> Vec<String> {
        if self.lang.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::with_capacity(4);
        if let (Some(country), Some(modifier)) = (&self.country, &self.modifier) {
            out.push(format!("{}_{country}@{modifier}", self.lang));
        }
        if let Some(country) = &self.country {
            out.push(format!("{}_{country}", self.lang));
        }
        if let Some(modifier) = &self.modifier {
            out.push(format!("{}@{modifier}", self.lang));
        }
        out.push(self.lang.clone());
        out
    }
}

/// The parts of a `[Desktop Entry]` group Sevak uses, with escapes resolved and
/// localized keys already matched against the [`Locale`] given to [`parse`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DesktopEntry {
    /// `Type` (`Application`, `Link`, `Directory`), if present.
    pub entry_type: Option<String>,
    pub name: Option<String>,
    pub generic_name: Option<String>,
    pub comment: Option<String>,
    pub keywords: Vec<String>,
    /// The raw `Icon` value: a theme icon name or an absolute path.
    pub icon: Option<String>,
    /// `Exec` split into argv with field codes removed. Empty when `Exec` is
    /// missing or malformed (see [`DesktopEntry::exec_error`]).
    pub exec: Vec<String>,
    /// Why `Exec` could not be split, if it could not.
    pub exec_error: Option<String>,
    pub try_exec: Option<String>,
    /// `Path`: the working directory.
    pub path: Option<String>,
    pub terminal: bool,
    pub no_display: bool,
    pub hidden: bool,
    pub only_show_in: Vec<String>,
    pub not_show_in: Vec<String>,
}

/// Parses the `[Desktop Entry]` group of `.desktop` file contents.
///
/// Other groups (`[Desktop Action ...]`), comments and unknown keys are
/// ignored. A malformed `Exec` does not fail the parse; it leaves
/// [`DesktopEntry::exec`] empty and sets [`DesktopEntry::exec_error`], so a
/// `Hidden=true` override with a broken `Exec` still hides its system twin.
pub fn parse(content: &str, locale: &Locale) -> Result<DesktopEntry, ParseError> {
    let group = parse_groups(content)
        .into_iter()
        .find(|group| group.name == MAIN_GROUP)
        .ok_or(ParseError::MissingDesktopEntryGroup)?;
    let candidates = locale.candidates();

    let localized = |key: &str| -> Option<String> {
        let raw = candidates
            .iter()
            .find_map(|suffix| group.get(&format!("{key}[{suffix}]")))
            .or_else(|| group.get(key))?;
        let value = unescape(raw);
        (!value.trim().is_empty()).then_some(value)
    };
    let plain = |key: &str| -> Option<String> {
        let value = unescape(group.get(key)?);
        (!value.trim().is_empty()).then_some(value)
    };
    let boolean = |key: &str| matches!(group.get(key), Some("true" | "1"));
    let list = |key: &str| group.get(key).map(split_list).unwrap_or_default();

    let keywords = {
        let raw = candidates
            .iter()
            .find_map(|suffix| group.get(&format!("Keywords[{suffix}]")))
            .or_else(|| group.get("Keywords"));
        raw.map(split_list).unwrap_or_default()
    };

    let (exec, exec_error) = match group.get("Exec") {
        None => (Vec::new(), None),
        Some(raw) => match parse_exec(raw) {
            Ok(argv) => (argv, None),
            Err(err) => (Vec::new(), Some(err.to_string())),
        },
    };

    Ok(DesktopEntry {
        entry_type: plain("Type"),
        name: localized("Name"),
        generic_name: localized("GenericName"),
        comment: localized("Comment"),
        keywords,
        icon: localized("Icon"),
        exec,
        exec_error,
        try_exec: plain("TryExec"),
        path: plain("Path"),
        terminal: boolean("Terminal"),
        no_display: boolean("NoDisplay"),
        hidden: boolean("Hidden"),
        only_show_in: list("OnlyShowIn"),
        not_show_in: list("NotShowIn"),
    })
}

/// Whether an application should appear in a launcher running under
/// `current_desktops` (the `XDG_CURRENT_DESKTOP` components).
///
/// `Type` must be `Application`; `NoDisplay` and `Hidden` hide the entry;
/// `OnlyShowIn`, when present, must share a (case-insensitive) name with the
/// current desktops, and `NotShowIn` must not.
pub fn is_visible(entry: &DesktopEntry, current_desktops: &[String]) -> bool {
    if entry.entry_type.as_deref() != Some("Application") || entry.no_display || entry.hidden {
        return false;
    }
    let intersects = |names: &[String]| {
        names.iter().any(|name| {
            current_desktops
                .iter()
                .any(|desktop| desktop.eq_ignore_ascii_case(name))
        })
    };
    if !entry.only_show_in.is_empty() && !intersects(&entry.only_show_in) {
        return false;
    }
    !intersects(&entry.not_show_in)
}

/// The desktop-file ID of `file_path` inside `applications_dir`: the relative
/// path with `/` replaced by `-` (`kde4/foo.desktop` becomes
/// `kde4-foo.desktop`). `None` if the file is outside the directory, is not a
/// `.desktop` file, or its path is not valid UTF-8.
pub fn desktop_file_id(applications_dir: &Path, file_path: &Path) -> Option<String> {
    let relative = file_path.strip_prefix(applications_dir).ok()?;
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str()?),
            _ => return None,
        }
    }
    let id = parts.join("-");
    (id.len() > ".desktop".len() && id.ends_with(".desktop")).then_some(id)
}

/// Why an `Exec` value could not be split.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ExecError {
    #[error("unterminated quote in Exec")]
    UnterminatedQuote,
}

/// Splits a raw `Exec` value (as written in the file) into argv with field
/// codes removed.
///
/// General value escapes (`\\`, `\s`, ...) are resolved first, then the
/// argument is split with shell-like quoting, which is what GLib does: double
/// quotes (where `\"`, `` \` ``, `\$` and `\\` are escapes), single quotes, and
/// backslash-escaped characters outside quotes (so the common
/// `/opt/My\ App/run` mistake still works).
///
/// Field codes `%f %F %u %U %i %c %k` and the deprecated `%d %D %n %N %v %m`
/// are removed, even inside a longer argument (`--file=%f` becomes `--file=`);
/// an argument that becomes empty this way is dropped. `%%` becomes `%`.
pub fn parse_exec(raw: &str) -> Result<Vec<String>, ExecError> {
    let unescaped = unescape(raw);
    let args = split_quoted(&unescaped)?;
    Ok(args
        .into_iter()
        .filter_map(|arg| {
            let (arg, removed_code) = strip_field_codes(&arg);
            if removed_code && arg.is_empty() {
                None
            } else {
                Some(arg)
            }
        })
        .collect())
}

fn split_quoted(input: &str) -> Result<Vec<String>, ExecError> {
    let mut args = Vec::new();
    let mut current = String::new();
    // True once the current argument has started, even if still empty (`""`).
    let mut started = false;
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' | '\n' | '\r' => {
                if started {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            '\\' => {
                started = true;
                // A trailing backslash is kept literally.
                current.push(chars.next().unwrap_or('\\'));
            }
            '"' => {
                started = true;
                loop {
                    match chars.next().ok_or(ExecError::UnterminatedQuote)? {
                        '"' => break,
                        '\\' => match chars.peek() {
                            Some(&escaped @ ('"' | '`' | '$' | '\\')) => {
                                current.push(escaped);
                                chars.next();
                            }
                            _ => current.push('\\'),
                        },
                        other => current.push(other),
                    }
                }
            }
            '\'' => {
                started = true;
                loop {
                    match chars.next().ok_or(ExecError::UnterminatedQuote)? {
                        '\'' => break,
                        other => current.push(other),
                    }
                }
            }
            other => {
                started = true;
                current.push(other);
            }
        }
    }
    if started {
        args.push(current);
    }
    Ok(args)
}

/// Removes field codes from one argument. The flag says whether any code was
/// removed (an argument that was empty to begin with must be kept).
fn strip_field_codes(arg: &str) -> (String, bool) {
    if !arg.contains('%') {
        return (arg.to_owned(), false);
    }
    let mut out = String::with_capacity(arg.len());
    let mut removed = false;
    let mut chars = arg.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('%') => {
                out.push('%');
                chars.next();
            }
            Some(&code) if FIELD_CODES.contains(code) => {
                removed = true;
                chars.next();
            }
            // Unknown code or trailing `%`: keep literally (`date +%Y`).
            _ => out.push('%'),
        }
    }
    (out, removed)
}

/// Resolves the spec's value escapes: `\s` space, `\n`, `\t`, `\r`, `\\`, and
/// `\;` (meaningful inside lists). Unknown escapes are kept as written.
fn unescape(raw: &str) -> String {
    if !raw.contains('\\') {
        return raw.to_owned();
    }
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(';') => out.push(';'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Splits a string-list value on unescaped `;` and unescapes each element.
/// Empty elements (such as the one after a trailing `;`) are dropped.
fn split_list(raw: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                current.push('\\');
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            ';' => items.push(std::mem::take(&mut current)),
            other => current.push(other),
        }
    }
    items.push(current);
    items
        .iter()
        .map(|item| unescape(item).trim().to_owned())
        .filter(|item| !item.is_empty())
        .collect()
}

/// One `[group]` of an INI-style file with its `key=value` pairs. Shared with
/// the icon-theme `index.theme` parser, which uses the same syntax.
#[derive(Debug, Default)]
pub(crate) struct Group {
    pub(crate) name: String,
    entries: HashMap<String, String>,
}

impl Group {
    /// The raw (still escaped) value of `key`. If a key repeats, the first wins.
    pub(crate) fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }
}

/// Splits INI-style content into groups. Comments (`#`), blank lines and lines
/// before the first group header are ignored; whitespace around `=` is too.
pub(crate) fn parse_groups(content: &str) -> Vec<Group> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let mut groups: Vec<Group> = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            groups.push(Group {
                name: name.to_owned(),
                entries: HashMap::new(),
            });
        } else if let (Some(group), Some((key, value))) = (groups.last_mut(), line.split_once('='))
        {
            group
                .entries
                .entry(key.trim_end().to_owned())
                .or_insert_with(|| value.trim_start().to_owned());
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIREFOX: &str = "\
[Desktop Entry]
Version=1.0
Name=Firefox Web Browser
Name[de]=Firefox Webbrowser
Name[fr]=Navigateur Web Firefox
Comment=Browse the World Wide Web
Comment[de]=Im Internet surfen
GenericName=Web Browser
Keywords=Internet;WWW;Browser;Web;Explorer;
Exec=firefox %u
Icon=firefox
Terminal=false
X-MultipleArgs=false
Type=Application
MimeType=text/html;text/xml;application/xhtml+xml;x-scheme-handler/http;
Categories=GNOME;GTK;Network;WebBrowser;
StartupNotify=true
Actions=new-window;new-private-window;

[Desktop Action new-window]
Name=Open a New Window
Exec=firefox --new-window %u

[Desktop Action new-private-window]
Name=Open a New Private Window
Exec=firefox --private-window %u
";

    const CALCULATOR: &str = "\
[Desktop Entry]
Name=Calculator
Comment=Perform arithmetic, scientific or financial calculations
Keywords=calculation;arithmetic;scientific;financial;
Exec=/usr/bin/flatpak run --branch=stable --arch=x86_64 --command=gnome-calculator org.gnome.Calculator
Icon=org.gnome.Calculator
Terminal=false
Type=Application
Categories=GNOME;GTK;Utility;Calculator;
StartupNotify=true
DBusActivatable=true
X-Flatpak=org.gnome.Calculator
";

    const SNAP_CODE: &str = "\
[Desktop Entry]
Name=Visual Studio Code
Comment=Code Editing. Redefined.
GenericName=Text Editor
Exec=/snap/bin/code --force-user-env %F
Icon=/snap/code/150/meta/gui/vscode.png
Type=Application
StartupNotify=false
Categories=Utility;TextEditor;Development;IDE;
Keywords=vscode;
Actions=new-empty-window;
";

    fn en() -> Locale {
        Locale::default()
    }

    fn desktops(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| (*n).to_owned()).collect()
    }

    #[test]
    fn parses_firefox() {
        let entry = parse(FIREFOX, &en()).unwrap();
        assert_eq!(entry.entry_type.as_deref(), Some("Application"));
        assert_eq!(entry.name.as_deref(), Some("Firefox Web Browser"));
        assert_eq!(entry.generic_name.as_deref(), Some("Web Browser"));
        assert_eq!(entry.comment.as_deref(), Some("Browse the World Wide Web"));
        assert_eq!(
            entry.keywords,
            ["Internet", "WWW", "Browser", "Web", "Explorer"]
        );
        assert_eq!(entry.icon.as_deref(), Some("firefox"));
        assert_eq!(entry.exec, ["firefox"]);
        assert!(entry.exec_error.is_none());
        assert!(!entry.terminal && !entry.no_display && !entry.hidden);
        assert!(is_visible(&entry, &desktops(&["ubuntu", "GNOME"])));
    }

    #[test]
    fn ignores_action_groups() {
        let entry = parse(FIREFOX, &en()).unwrap();
        // The first action's Name/Exec must not leak into the main entry.
        assert_ne!(entry.name.as_deref(), Some("Open a New Window"));
        assert_eq!(entry.exec, ["firefox"]);
    }

    #[test]
    fn parses_flatpak_export() {
        let entry = parse(CALCULATOR, &en()).unwrap();
        assert_eq!(entry.name.as_deref(), Some("Calculator"));
        assert_eq!(
            entry.exec,
            [
                "/usr/bin/flatpak",
                "run",
                "--branch=stable",
                "--arch=x86_64",
                "--command=gnome-calculator",
                "org.gnome.Calculator",
            ]
        );
        assert_eq!(entry.icon.as_deref(), Some("org.gnome.Calculator"));
    }

    #[test]
    fn parses_snap_entry() {
        let entry = parse(SNAP_CODE, &en()).unwrap();
        assert_eq!(entry.exec, ["/snap/bin/code", "--force-user-env"]);
        assert_eq!(
            entry.icon.as_deref(),
            Some("/snap/code/150/meta/gui/vscode.png")
        );
        assert_eq!(entry.generic_name.as_deref(), Some("Text Editor"));
    }

    #[test]
    fn missing_group_is_an_error() {
        assert_eq!(
            parse("[Other]\nName=x\n", &en()),
            Err(ParseError::MissingDesktopEntryGroup)
        );
        assert_eq!(parse("", &en()), Err(ParseError::MissingDesktopEntryGroup));
    }

    #[test]
    fn handles_comments_bom_crlf_and_spaces_around_equals() {
        let content = "\u{feff}# comment\r\n\r\n[Desktop Entry]\r\n# another\r\nName = Spacey\r\nType=Application\r\nExec =  spacey --x\r\n";
        let entry = parse(content, &en()).unwrap();
        assert_eq!(entry.name.as_deref(), Some("Spacey"));
        assert_eq!(entry.exec, ["spacey", "--x"]);
    }

    #[test]
    fn first_duplicate_key_wins() {
        let entry = parse("[Desktop Entry]\nName=One\nName=Two\n", &en()).unwrap();
        assert_eq!(entry.name.as_deref(), Some("One"));
    }

    // --- locale ---

    #[test]
    fn locale_parsing() {
        let l = Locale::parse("de_DE.UTF-8@euro");
        assert_eq!(
            l.candidates(),
            ["de_DE@euro", "de_DE", "de@euro", "de"].map(String::from)
        );
        assert_eq!(
            Locale::parse("pt_BR.UTF-8").candidates(),
            ["pt_BR", "pt"].map(String::from)
        );
        assert_eq!(Locale::parse("sr@latin").candidates(), ["sr@latin", "sr"]);
        assert_eq!(Locale::parse("fr").candidates(), ["fr"]);
        assert!(Locale::parse("C").candidates().is_empty());
        assert!(Locale::parse("C.UTF-8").candidates().is_empty());
        assert!(Locale::parse("POSIX").candidates().is_empty());
        assert!(Locale::parse("").candidates().is_empty());
    }

    #[test]
    fn locale_from_vars_precedence() {
        let l = Locale::from_vars(Some(""), Some("fr_FR.UTF-8"), Some("de_DE"));
        assert_eq!(l.candidates(), ["fr_FR", "fr"]);
        let l = Locale::from_vars(Some("es_ES"), Some("fr_FR"), Some("de_DE"));
        assert_eq!(l.candidates(), ["es_ES", "es"]);
        let l = Locale::from_vars(None, None, Some("de_DE.UTF-8"));
        assert_eq!(l.candidates(), ["de_DE", "de"]);
        assert!(Locale::from_vars(None, None, None).candidates().is_empty());
    }

    #[test]
    fn localized_names_follow_the_spec_order() {
        let content = "\
[Desktop Entry]
Type=Application
Name=Plain
Name[de]=Lang only
Name[de@euro]=Lang modifier
Name[de_DE]=Lang country
Name[de_DE@euro]=Everything
Name[fr]=Francais
Comment=Plain comment
Comment[de]=Deutscher Kommentar
";
        let parse_with = |locale: &str| parse(content, &Locale::parse(locale)).unwrap();
        let name = |locale: &str| parse_with(locale).name.unwrap();

        assert_eq!(name("de_DE.UTF-8@euro"), "Everything");
        assert_eq!(name("de_DE.UTF-8"), "Lang country");
        assert_eq!(name("de_AT@euro"), "Lang modifier");
        assert_eq!(name("de_AT"), "Lang only");
        assert_eq!(name("de"), "Lang only");
        assert_eq!(name("fr_CA.UTF-8"), "Francais");
        assert_eq!(name("ja_JP.UTF-8"), "Plain");
        assert_eq!(name("C"), "Plain");
        // Keys without a matching translation fall back to the plain value.
        assert_eq!(parse_with("fr_FR").comment.unwrap(), "Plain comment");
        assert_eq!(parse_with("de_DE").comment.unwrap(), "Deutscher Kommentar");
    }

    #[test]
    fn localized_keywords() {
        let content = "[Desktop Entry]\nKeywords=web;browser;\nKeywords[de]=Internet;Webbrowser;\n";
        let de = parse(content, &Locale::parse("de_DE")).unwrap();
        assert_eq!(de.keywords, ["Internet", "Webbrowser"]);
        let other = parse(content, &Locale::parse("it")).unwrap();
        assert_eq!(other.keywords, ["web", "browser"]);
    }

    #[test]
    fn unlocalized_lookup_ignores_localized_keys() {
        let content = "[Desktop Entry]\nName[de]=Nur Deutsch\n";
        assert_eq!(parse(content, &en()).unwrap().name, None);
    }

    // --- escapes and lists ---

    #[test]
    fn value_escapes() {
        let content = "[Desktop Entry]\nName=a\\sb\\\\c\\td\nComment=line1\\nline2\n";
        let entry = parse(content, &en()).unwrap();
        assert_eq!(entry.name.as_deref(), Some("a b\\c\td"));
        assert_eq!(entry.comment.as_deref(), Some("line1\nline2"));
    }

    #[test]
    fn list_splitting_honours_escaped_semicolons() {
        assert_eq!(split_list("a;b;c;"), ["a", "b", "c"]);
        assert_eq!(split_list("a\\;b;c"), ["a;b", "c"]);
        assert_eq!(split_list("one"), ["one"]);
        assert_eq!(split_list(";;"), Vec::<String>::new());
        assert_eq!(split_list("a\\\\;b"), ["a\\", "b"]);
    }

    #[test]
    fn booleans() {
        let entry = parse(
            "[Desktop Entry]\nTerminal=true\nNoDisplay=true\nHidden=false\n",
            &en(),
        )
        .unwrap();
        assert!(entry.terminal && entry.no_display && !entry.hidden);
    }

    // --- Exec ---

    #[test]
    fn exec_field_codes_are_removed() {
        assert_eq!(parse_exec("firefox %u").unwrap(), ["firefox"]);
        assert_eq!(parse_exec("gedit %U %F").unwrap(), ["gedit"]);
        assert_eq!(
            parse_exec("app --icon %i --name %c --desktop %k %f").unwrap(),
            ["app", "--icon", "--name", "--desktop"]
        );
        assert_eq!(
            parse_exec("old %d %D %n %N %v %m end").unwrap(),
            ["old", "end"]
        );
    }

    #[test]
    fn exec_field_code_inside_an_argument() {
        assert_eq!(
            parse_exec("viewer --file=%f --url=%u -x").unwrap(),
            ["viewer", "--file=", "--url=", "-x"]
        );
        // "%f" alone becomes empty and is dropped; "x%f" keeps the "x".
        assert_eq!(parse_exec("app x%f %f").unwrap(), ["app", "x"]);
    }

    #[test]
    fn exec_percent_escape_and_unknown_codes() {
        assert_eq!(
            parse_exec("app 100%% sure").unwrap(),
            ["app", "100%", "sure"]
        );
        assert_eq!(
            parse_exec("sh -c 'date +%Y'").unwrap(),
            ["sh", "-c", "date +%Y"]
        );
        assert_eq!(parse_exec("app 50%").unwrap(), ["app", "50%"]);
    }

    #[test]
    fn exec_double_quotes() {
        assert_eq!(
            parse_exec(r#""/opt/My App/bin/my app" --flag %f"#).unwrap(),
            ["/opt/My App/bin/my app", "--flag"]
        );
    }

    #[test]
    fn exec_escaped_quotes_inside_quotes() {
        // In the file: \\" -> (general escape) \" -> (quoting rule) "
        assert_eq!(
            parse_exec(r#"app --title "He said \\"hi\\"" %F"#).unwrap(),
            ["app", "--title", r#"He said "hi""#]
        );
        // Backtick, dollar and backslash escapes. The file needs `\\\\` for one
        // backslash: general escapes first, then the quoting rule.
        assert_eq!(
            parse_exec(r#"app "\\`x\\` \\$HOME C:\\\\dir""#).unwrap(),
            ["app", r"`x` $HOME C:\dir"]
        );
    }

    #[test]
    fn exec_unknown_escape_in_quotes_keeps_backslash() {
        assert_eq!(parse_exec(r#"app "a\\nb""#).unwrap(), ["app", r"a\nb"]);
    }

    #[test]
    fn exec_escaped_spaces_outside_quotes() {
        assert_eq!(
            parse_exec(r"/opt/My\ App/run --x").unwrap(),
            ["/opt/My App/run", "--x"]
        );
        // `\s` is resolved to a real space *before* splitting, so it separates
        // arguments unless quoted (same as GLib).
        assert_eq!(parse_exec(r"a\sb").unwrap(), ["a", "b"]);
        assert_eq!(parse_exec(r#""a\sb""#).unwrap(), ["a b"]);
    }

    #[test]
    fn exec_single_quotes_like_glib() {
        assert_eq!(
            parse_exec("sh -c 'cd ~ && exec foo %U'").unwrap(),
            ["sh", "-c", "cd ~ && exec foo "]
        );
    }

    #[test]
    fn exec_empty_quoted_argument_is_kept() {
        assert_eq!(
            parse_exec(r#"app --title "" %f"#).unwrap(),
            ["app", "--title", ""]
        );
    }

    #[test]
    fn exec_whitespace_variations() {
        assert_eq!(parse_exec("  app   a\tb  ").unwrap(), ["app", "a", "b"]);
        assert!(parse_exec("").unwrap().is_empty());
        assert!(parse_exec("   ").unwrap().is_empty());
        assert!(parse_exec("%U").unwrap().is_empty());
    }

    #[test]
    fn exec_unterminated_quote_is_an_error() {
        assert_eq!(
            parse_exec("app \"unterminated"),
            Err(ExecError::UnterminatedQuote)
        );
        assert_eq!(parse_exec("app 'oops"), Err(ExecError::UnterminatedQuote));
    }

    #[test]
    fn malformed_exec_does_not_fail_the_parse() {
        let entry = parse(
            "[Desktop Entry]\nType=Application\nName=Bad\nHidden=true\nExec=app \"x\n",
            &en(),
        )
        .unwrap();
        assert!(entry.exec.is_empty());
        assert!(entry.exec_error.is_some());
        assert!(entry.hidden);
    }

    #[test]
    fn exec_env_wrapper_is_plain_argv() {
        assert_eq!(
            parse_exec("env BAMF_DESKTOP_FILE_HINT=/x.desktop /usr/bin/app %U").unwrap(),
            ["env", "BAMF_DESKTOP_FILE_HINT=/x.desktop", "/usr/bin/app"]
        );
    }

    // --- visibility ---

    #[test]
    fn nodisplay_and_hidden_are_invisible() {
        let gnome = desktops(&["GNOME"]);
        let mut entry = parse(FIREFOX, &en()).unwrap();
        assert!(is_visible(&entry, &gnome));
        entry.no_display = true;
        assert!(!is_visible(&entry, &gnome));
        entry.no_display = false;
        entry.hidden = true;
        assert!(!is_visible(&entry, &gnome));
    }

    #[test]
    fn only_applications_are_visible() {
        let gnome = desktops(&["GNOME"]);
        for ty in ["Link", "Directory"] {
            let entry = parse(
                &format!("[Desktop Entry]\nType={ty}\nName=x\nExec=x\n"),
                &en(),
            )
            .unwrap();
            assert!(!is_visible(&entry, &gnome), "{ty}");
        }
        let missing = parse("[Desktop Entry]\nName=x\nExec=x\n", &en()).unwrap();
        assert!(!is_visible(&missing, &gnome));
    }

    #[test]
    fn only_show_in_kde() {
        let entry = parse(
            "[Desktop Entry]\nType=Application\nName=Dolphin\nExec=dolphin\nOnlyShowIn=KDE;\n",
            &en(),
        )
        .unwrap();
        assert_eq!(entry.only_show_in, ["KDE"]);
        assert!(!is_visible(&entry, &desktops(&["ubuntu", "GNOME"])));
        assert!(is_visible(&entry, &desktops(&["KDE"])));
        assert!(is_visible(&entry, &desktops(&["kde"])));
        assert!(!is_visible(&entry, &[]));
    }

    #[test]
    fn only_show_in_several_desktops() {
        let entry = parse(
            "[Desktop Entry]\nType=Application\nName=Tweaks\nExec=t\nOnlyShowIn=GNOME;Unity;\n",
            &en(),
        )
        .unwrap();
        assert!(is_visible(&entry, &desktops(&["ubuntu", "GNOME"])));
        assert!(!is_visible(&entry, &desktops(&["XFCE"])));
    }

    #[test]
    fn not_show_in() {
        let entry = parse(
            "[Desktop Entry]\nType=Application\nName=X\nExec=x\nNotShowIn=GNOME;KDE;\n",
            &en(),
        )
        .unwrap();
        assert!(!is_visible(&entry, &desktops(&["ubuntu", "GNOME"])));
        assert!(is_visible(&entry, &desktops(&["XFCE"])));
        assert!(is_visible(&entry, &[]));
    }

    // --- desktop file ids ---

    #[test]
    fn desktop_file_ids() {
        let dir = Path::new("/usr/share/applications");
        let id = |p: &str| desktop_file_id(dir, Path::new(p));
        assert_eq!(
            id("/usr/share/applications/firefox.desktop").as_deref(),
            Some("firefox.desktop")
        );
        assert_eq!(
            id("/usr/share/applications/kde4/foo.desktop").as_deref(),
            Some("kde4-foo.desktop")
        );
        assert_eq!(
            id("/usr/share/applications/a/b/c.desktop").as_deref(),
            Some("a-b-c.desktop")
        );
        assert_eq!(id("/usr/share/applications/readme.txt"), None);
        assert_eq!(id("/usr/share/applications/.desktop"), None);
        assert_eq!(id("/elsewhere/foo.desktop"), None);
        assert_eq!(id("/usr/share/applications"), None);
    }
}
