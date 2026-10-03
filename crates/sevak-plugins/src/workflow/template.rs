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
//! links are URL-encoded unless the placeholder says `|raw`; everything else
//! gets the text as it is. There is no shell anywhere: script arguments are
//! passed as separate arguments, so they need no quoting. Command lines for a
//! terminal are the exception, and `|sh` / `|ps` quote for POSIX shells and
//! PowerShell.

use std::collections::BTreeMap;

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
}

/// The values placeholders refer to.
#[derive(Debug, Clone, Copy)]
pub struct Scope<'a> {
    pub query: &'a str,
    pub vars: &'a BTreeMap<String, String>,
}

/// `template` with its placeholders filled in.
pub fn expand(template: &str, scope: &Scope<'_>, target: Target) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}').and_then(|close| {
            let inner = &after[..close];
            placeholder(inner, scope, target).map(|text| (close, text))
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
/// is not one.
fn placeholder(inner: &str, scope: &Scope<'_>, target: Target) -> Option<String> {
    let mut parts = inner.split('|');
    let source = parts.next()?.trim();
    let filters: Vec<&str> = parts.map(str::trim).collect();
    let value = if source == "query" {
        scope.query
    } else if let Some(name) = source.strip_prefix("var:") {
        let name = name.trim();
        if !super::validate::valid_variable_name(name) {
            return None;
        }
        scope.vars.get(name).map_or("", String::as_str)
    } else {
        return None;
    };
    if filters.iter().any(|filter| !is_filter(filter)) {
        return None;
    }
    let mut text = value.to_owned();
    for filter in &filters {
        text = apply(filter, &text);
    }
    let encoded_already = filters.iter().any(|f| matches!(*f, "url" | "raw"));
    if target == Target::Url && !encoded_already {
        text = percent_encode(&text);
    }
    Some(text)
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

    #[test]
    fn output_is_capped_on_a_character_boundary() {
        let big = "é".repeat(MAX_EXPANDED_BYTES);
        let expanded = plain("{query}{query}", &big, &[]);
        assert!(expanded.len() <= MAX_EXPANDED_BYTES);
        assert!(expanded.chars().all(|c| c == 'é'));
    }
}
