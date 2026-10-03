//! `{query}` and `{var:name}`: the placeholders in a node's text.
//!
//! ```text
//! {query}            the argument that reached the node
//! {var:site}         the variable `site` (empty when it is not set)
//! {query|upper}      filters, left to right: url, raw, upper, lower, trim,
//!                    json, sh, ps
//! ```
//!
//! Anything else in braces is left alone, so JSON and shell snippets need no
//! escaping. A `{` that does not start a placeholder is just a `{`.
//!
//! What a placeholder becomes depends on where the text goes ([`Target`]):
//! links are URL-encoded unless the placeholder says `|raw`, and a terminal
//! command line gets every placeholder **quoted for its shell** unless it says
//! `|raw` (or already quotes with `|sh` / `|ps`); everything else gets the text
//! as it is. There is no other shell anywhere: script arguments are passed as
//! separate arguments, so they need no quoting.

use std::collections::BTreeMap;

use sevak_platform::ShellQuoting;

use crate::selection::transform::title_case;
use crate::web_search::percent_encode;

/// The longest text a template may expand to.
pub const MAX_EXPANDED_BYTES: usize = 4 * 1024 * 1024;

/// What the expanded text is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Used as it is.
    Plain,
    /// A link: placeholders are percent-encoded unless they say `raw` (or
    /// `url`, which already does it).
    Url,
    /// A command line for this shell: each placeholder becomes one quoted,
    /// literal word unless it says `raw` (or quotes itself with `sh` / `ps`).
    /// Use [`expand_command`], which reports a value the shell cannot take.
    Command(ShellQuoting),
}

/// The values placeholders refer to.
#[derive(Debug, Clone, Copy)]
pub struct Scope<'a> {
    pub query: &'a str,
    pub vars: &'a BTreeMap<String, String>,
}

/// `template` with its placeholders filled in. For [`Target::Command`] prefer
/// [`expand_command`]: a value that cannot be quoted for `cmd` becomes empty
/// here.
pub fn expand(template: &str, scope: &Scope<'_>, target: Target) -> String {
    expand_with(template, scope, target, &mut None)
}

/// A terminal command line: `template` with each placeholder quoted as one
/// literal word for the shell (see [`Target::Command`]). An error names a
/// value that the shell cannot take literally (`cmd` has no way to quote `"`,
/// `%`, `!` or a line break); nothing should run then.
pub fn expand_command(
    template: &str,
    scope: &Scope<'_>,
    quoting: ShellQuoting,
) -> Result<String, String> {
    let mut failure = None;
    let text = expand_with(template, scope, Target::Command(quoting), &mut failure);
    match failure {
        Some(message) => Err(message),
        None => Ok(text),
    }
}

/// [`expand`], recording in `failure` the first placeholder that could not be
/// quoted.
fn expand_with(
    template: &str,
    scope: &Scope<'_>,
    target: Target,
    failure: &mut Option<String>,
) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}').and_then(|close| {
            let inner = &after[..close];
            placeholder(inner, scope, target, failure).map(|text| (close, text))
        }) {
            Some((close, text)) => {
                out.push_str(&text);
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
        if out.len() > MAX_EXPANDED_BYTES {
            truncate_to(&mut out, MAX_EXPANDED_BYTES);
            return out;
        }
    }
    out.push_str(rest);
    if out.len() > MAX_EXPANDED_BYTES {
        truncate_to(&mut out, MAX_EXPANDED_BYTES);
    }
    out
}

fn truncate_to(text: &mut String, max: usize) {
    let mut end = max.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
}

/// The text for the placeholder `inner` (without braces), or `None` when it
/// is not one. A value that cannot be quoted for the target's shell becomes
/// empty and is reported in `failure`.
fn placeholder(
    inner: &str,
    scope: &Scope<'_>,
    target: Target,
    failure: &mut Option<String>,
) -> Option<String> {
    let mut parts = inner.split('|');
    let source = parts.next()?.trim();
    let filters: Vec<&str> = parts.map(str::trim).collect();
    let value = if source == "query" {
        scope.query
    } else {
        let name = source.strip_prefix("var:")?.trim();
        if !super::validate::valid_variable_name(name) {
            return None;
        }
        scope.vars.get(name).map_or("", String::as_str)
    };
    if filters.iter().any(|filter| !is_filter(filter)) {
        return None;
    }
    let mut text = value.to_owned();
    for filter in &filters {
        text = apply(filter, &text);
    }
    match target {
        Target::Plain => {}
        Target::Url => {
            if !filters.iter().any(|f| matches!(*f, "url" | "raw")) {
                text = percent_encode(&text);
            }
        }
        Target::Command(quoting) => {
            if !filters.iter().any(|f| matches!(*f, "raw" | "sh" | "ps")) {
                text = match quote_for(quoting, &text) {
                    Ok(quoted) => quoted,
                    Err(reason) => {
                        failure.get_or_insert_with(|| {
                            format!(
                                "{{{}}} {reason}; add |raw to insert it as it is",
                                inner.trim()
                            )
                        });
                        String::new()
                    }
                };
            }
        }
    }
    Some(text)
}

/// `text` as one literal word for a shell that quotes like `quoting`.
fn quote_for(quoting: ShellQuoting, text: &str) -> Result<String, &'static str> {
    match quoting {
        ShellQuoting::Posix => Ok(quote_posix(text)),
        ShellQuoting::PowerShell => Ok(quote_powershell(text)),
        ShellQuoting::Cmd => quote_cmd(text),
    }
}

/// `text` in double quotes for `cmd.exe`, where `& | < > ^ ( )` are literal.
/// `cmd` has no escape inside quotes for `"`, for `%` (variables are expanded
/// even there) or for `!` (delayed expansion), and a line break ends the
/// command, so text with any of them is refused.
fn quote_cmd(text: &str) -> Result<String, &'static str> {
    if text
        .chars()
        .any(|c| matches!(c, '"' | '%' | '!' | '\r' | '\n' | '\0'))
    {
        return Err("contains \", %, ! or a line break, which cmd cannot take literally");
    }
    Ok(format!("\"{text}\""))
}

fn is_filter(name: &str) -> bool {
    matches!(
        name,
        "url" | "raw" | "upper" | "lower" | "title" | "trim" | "json" | "sh" | "ps"
    )
}

fn apply(filter: &str, text: &str) -> String {
    match filter {
        "url" => percent_encode(text),
        "upper" => text.to_uppercase(),
        "lower" => text.to_lowercase(),
        "title" => title_case(text),
        "trim" => text.trim().to_owned(),
        "json" => json_escape(text),
        "sh" => quote_posix(text),
        "ps" => quote_powershell(text),
        // `raw` only opts out of the default encoding.
        _ => text.to_owned(),
    }
}

/// The contents of a JSON string literal for `text` (no surrounding quotes).
fn json_escape(text: &str) -> String {
    let quoted = serde_json::to_string(text).unwrap_or_default();
    quoted
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or_default()
        .to_owned()
}

/// `text` as one POSIX shell word: single quotes, with `'` as `'\''`.
pub fn quote_posix(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// `text` as one PowerShell string: single quotes, with `'` doubled (and the
/// typographic quotes PowerShell also treats as quotes doubled too).
pub fn quote_powershell(text: &str) -> String {
    let mut out = String::from("'");
    for ch in text.chars() {
        out.push(ch);
        if matches!(ch, '\'' | '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}') {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    fn plain(template: &str, query: &str, pairs: &[(&str, &str)]) -> String {
        expand(
            template,
            &Scope {
                query,
                vars: &vars(pairs),
            },
            Target::Plain,
        )
    }

    fn url(template: &str, query: &str, pairs: &[(&str, &str)]) -> String {
        expand(
            template,
            &Scope {
                query,
                vars: &vars(pairs),
            },
            Target::Url,
        )
    }

    #[test]
    fn query_and_variables_are_filled_in() {
        assert_eq!(plain("hello {query}!", "Ada", &[]), "hello Ada!");
        assert_eq!(
            plain("{var:a}-{var:b}", "", &[("a", "1"), ("b", "2")]),
            "1-2"
        );
        assert_eq!(plain("[{var:missing}]", "", &[]), "[]");
        assert_eq!(plain("{query}{query}", "ab", &[]), "abab");
        assert_eq!(plain("no placeholders", "x", &[]), "no placeholders");
        assert_eq!(plain("", "x", &[]), "");
    }

    #[test]
    fn other_braces_are_left_alone() {
        assert_eq!(plain(r#"{"a": 1}"#, "x", &[]), r#"{"a": 1}"#);
        assert_eq!(
            plain("{unknown} {var:} {query", "x", &[]),
            "{unknown} {var:} {query"
        );
        assert_eq!(plain("${HOME} {{query}}", "x", &[]), "${HOME} {x}");
        assert_eq!(plain("{ query }", "x", &[]), "x");
        // A bad filter makes it not a placeholder at all.
        assert_eq!(plain("{query|shout}", "x", &[]), "{query|shout}");
        assert_eq!(plain("{var:bad name}", "x", &[]), "{var:bad name}");
    }

    #[test]
    fn filters_apply_left_to_right() {
        assert_eq!(plain("{query|upper}", "ab", &[]), "AB");
        assert_eq!(plain("{query|trim|upper}", "  ab ", &[]), "AB");
        assert_eq!(plain("{query|lower}", "AB", &[]), "ab");
        assert_eq!(
            plain("{query|title}", "hello big world", &[]),
            "Hello Big World"
        );
        assert_eq!(plain("{query|url}", "a b&c", &[]), "a%20b%26c");
        assert_eq!(plain("{var:v|upper}", "", &[("v", "x")]), "X");
        assert_eq!(
            plain("{query|json}", "say \"hi\"\n", &[]),
            "say \\\"hi\\\"\\n"
        );
    }

    #[test]
    fn links_encode_placeholders_unless_raw() {
        assert_eq!(
            url("https://x.example/?q={query}", "a b&c=d/é", &[]),
            "https://x.example/?q=a%20b%26c%3Dd%2F%C3%A9"
        );
        // The literal part of the link is untouched.
        assert_eq!(
            url("https://x.example/a b", "", &[]),
            "https://x.example/a b"
        );
        assert_eq!(
            url(
                "{var:site}/s?q={query}",
                "x y",
                &[("site", "https://h.example")]
            ),
            "https%3A%2F%2Fh.example/s?q=x%20y",
            "a variable holding a whole base URL must be marked raw"
        );
        assert_eq!(
            url(
                "{var:site|raw}/s?q={query}",
                "x y",
                &[("site", "https://h.example")]
            ),
            "https://h.example/s?q=x%20y"
        );
        // `url` is not applied twice.
        assert_eq!(url("{query|url}", "a b", &[]), "a%20b");
        assert_eq!(url("{query|raw}", "a b", &[]), "a b");
    }

    #[test]
    fn quoting_for_shells() {
        assert_eq!(quote_posix("it's"), "'it'\\''s'");
        assert_eq!(quote_posix("; rm -rf ~"), "'; rm -rf ~'");
        assert_eq!(quote_powershell("it's"), "'it''s'");
        assert_eq!(quote_powershell("\u{2019}"), "'\u{2019}\u{2019}'");
        assert_eq!(plain("echo {query|sh}", "a'b", &[]), "echo 'a'\\''b'");
        assert_eq!(plain("echo {query|ps}", "a'b", &[]), "echo 'a''b'");
    }

    fn command(
        template: &str,
        query: &str,
        pairs: &[(&str, &str)],
        quoting: ShellQuoting,
    ) -> Result<String, String> {
        expand_command(
            template,
            &Scope {
                query,
                vars: &vars(pairs),
            },
            quoting,
        )
    }

    #[test]
    fn command_lines_quote_every_placeholder_for_their_shell() {
        let evil = "x; rm -rf ~ $(whoami) `id` & del *";
        assert_eq!(
            command("grep {query} notes.txt", evil, &[], ShellQuoting::Posix).unwrap(),
            format!("grep '{evil}' notes.txt")
        );
        assert_eq!(
            command("echo {query}", "it's", &[], ShellQuoting::Posix).unwrap(),
            "echo 'it'\\''s'"
        );
        assert_eq!(
            command(
                "Select-String {query} a.txt",
                "a'b; Remove-Item *",
                &[],
                ShellQuoting::PowerShell
            )
            .unwrap(),
            "Select-String 'a''b; Remove-Item *' a.txt"
        );
        assert_eq!(
            command(
                "findstr {query} a.txt",
                "a & del * | x > y",
                &[],
                ShellQuoting::Cmd
            )
            .unwrap(),
            "findstr \"a & del * | x > y\" a.txt"
        );
        // Variables are quoted too, after their filters.
        assert_eq!(
            command(
                "cd {var:dir|trim}",
                "",
                &[("dir", " my dir ")],
                ShellQuoting::Posix
            )
            .unwrap(),
            "cd 'my dir'"
        );
        // An empty value is still one (empty) word.
        assert_eq!(
            command("ls {query}", "", &[], ShellQuoting::Posix).unwrap(),
            "ls ''"
        );
    }

    #[test]
    fn command_lines_keep_explicit_quoting_and_raw() {
        // `sh` / `ps` already quote: not quoted twice, whatever the shell.
        assert_eq!(
            command("echo {query|sh}", "a b", &[], ShellQuoting::PowerShell).unwrap(),
            "echo 'a b'"
        );
        assert_eq!(
            command("echo {query|ps}", "a'b", &[], ShellQuoting::Posix).unwrap(),
            "echo 'a''b'"
        );
        // `raw` opts out: the text becomes part of the command line.
        assert_eq!(
            command(
                "git {query|raw}",
                "status --short",
                &[],
                ShellQuoting::Posix
            )
            .unwrap(),
            "git status --short"
        );
        assert_eq!(
            command(
                "{var:cmd|raw} {query}",
                "a b",
                &[("cmd", "ls -l")],
                ShellQuoting::Posix
            )
            .unwrap(),
            "ls -l 'a b'"
        );
        // Text that is not a placeholder is never touched.
        assert_eq!(
            command("echo ${HOME} {other}", "x", &[], ShellQuoting::Posix).unwrap(),
            "echo ${HOME} {other}"
        );
    }

    #[test]
    fn cmd_refuses_what_it_cannot_quote() {
        for value in ["50%", "say \"hi\"", "wow!", "two\nlines"] {
            let err = command("echo {query}", value, &[], ShellQuoting::Cmd).unwrap_err();
            assert!(
                err.contains("{query}") && err.contains("|raw"),
                "{value:?}: {err}"
            );
        }
        // With `raw` it is the author's choice.
        assert_eq!(
            command("echo {query|raw}", "50%", &[], ShellQuoting::Cmd).unwrap(),
            "echo 50%"
        );
        // Plain text and links are unaffected by the shell rules.
        assert_eq!(plain("{query}", "50%", &[]), "50%");
    }

    #[test]
    fn output_is_capped_on_a_character_boundary() {
        let big = "é".repeat(MAX_EXPANDED_BYTES);
        let expanded = plain("{query}{query}", &big, &[]);
        assert!(expanded.len() <= MAX_EXPANDED_BYTES);
        assert!(expanded.chars().all(|c| c == 'é'));
    }
}
