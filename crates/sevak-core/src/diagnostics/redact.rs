//! Redaction: the one place that decides what must not leave the computer in a
//! diagnostics report.
//!
//! Two strengths, both pure functions of the text and the [`Identity`]:
//!
//! - [`Redactor::text`] for report fields that are free text (an error message,
//!   a path): the home folder, user name and computer name are replaced, and
//!   anything that looks like a secret is dropped.
//! - [`Redactor::log_line`] for log lines, which may have picked up text the
//!   user typed, copied or selected (Sevak's own logging avoids that, so this
//!   is defence in depth): everything `text` does, plus quoted text goes and
//!   paths outside Sevak's own folders and the system's collapse to `<path>`.
//!
//! What the filters recognise, in the order they run:
//!
//! 1. URLs with a query string, a fragment, credentials, or the `file:` scheme
//!    become `<redacted>`; plain `https://host/path` URLs stay (the update URL
//!    is useful in a bug report).
//! 2. The user's home folder becomes `~` (any slash style, escaped backslashes,
//!    the `\\?\` prefix, any case), other users' profile folders
//!    (`D:\Users\x`, `/home/x`, `/Users/x`) become `...\<user>`, and the user
//!    name and computer name become `<user>` and `<host>`.
//! 3. (log lines) Quoted text becomes `"<redacted>"`, except short identifiers
//!    (`"apps:firefox.desktop"`) and paths inside Sevak's own folders.
//! 4. (log lines) Absolute paths outside Sevak's own folders and the system's
//!    become `<path>`.
//! 5. Words: e-mail addresses, IPv4/IPv6 addresses, MAC addresses, API tokens
//!    (`ghp_`, `github_pat_`, `AKIA`, `xox`, `sk-`, `AIza`, JWTs), the value
//!    after `password=`/`token:`/`Bearer`, runs of 16 or more hex digits and
//!    other long random-looking strings all become `<redacted>`.
//!
//! The filters err on the side of removing too much: a version string such as
//! `1.2.3.4` looks like an address and goes too. They are idempotent.

use std::net::{Ipv4Addr, Ipv6Addr};

pub const REDACTED: &str = "<redacted>";
pub const USER: &str = "<user>";
pub const HOST: &str = "<host>";
pub const PATH: &str = "<path>";

/// A log line is cut to this many characters.
const MAX_LINE_CHARS: usize = 600;
/// A quoted span this short and made of identifier characters survives.
const MAX_QUOTED_IDENTIFIER: usize = 40;
/// Shortest user or computer name that is replaced where it stands alone:
/// replacing `al` would mangle ordinary words.
const MIN_NAME_CHARS: usize = 3;

/// What identifies this computer's user.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Identity {
    /// The user's home (profile) folder, as the OS spells it.
    pub home_dirs: Vec<String>,
    pub user_names: Vec<String>,
    pub host_names: Vec<String>,
}

/// Redacts text for a diagnostics report. See the module docs.
#[derive(Debug, Clone, Default)]
pub struct Redactor {
    homes: Vec<HomeMatcher>,
    users: Vec<Vec<char>>,
    hosts: Vec<Vec<char>>,
}

impl Redactor {
    pub fn new(identity: &Identity) -> Self {
        let mut homes: Vec<HomeMatcher> = identity
            .home_dirs
            .iter()
            .filter_map(|home| HomeMatcher::new(home))
            .collect();
        // The longest first, so `C:\Users\me\Documents` wins over `C:\Users\me`.
        homes.sort_by_key(|home| std::cmp::Reverse(home.segments.len()));
        let names = |names: &[String]| -> Vec<Vec<char>> {
            let mut out: Vec<Vec<char>> = Vec::new();
            for name in names {
                let name: Vec<char> = name.trim().chars().collect();
                if name.len() >= MIN_NAME_CHARS && !out.iter().any(|n| same_chars(n, &name)) {
                    out.push(name);
                }
            }
            out
        };
        Self {
            homes,
            users: names(&identity.user_names),
            hosts: names(&identity.host_names),
        }
    }

    /// A redactor that knows nobody: only the secret filters apply.
    pub fn anonymous() -> Self {
        Self::default()
    }

    /// Redacts a free-text report field (an error message, a path).
    pub fn text(&self, text: &str) -> String {
        let text = strip_controls(text, true);
        let text = redact_urls(&text);
        let text = self.redact_identity(&text);
        scrub_words(&text)
    }

    /// Redacts a path: the home folder becomes `~`, and a path outside Sevak's
    /// own folders and the system's collapses to `<path>` (a folder somebody
    /// chose can name a client or a project).
    pub fn path(&self, path: &str) -> String {
        let path = strip_controls(path, false);
        let path = self.redact_identity(&path);
        scrub_words(&collapse_paths(&path))
    }

    /// Redacts one log line, the strict way (see the module docs).
    pub fn log_line(&self, line: &str) -> String {
        let line = strip_controls(line, false);
        let line = redact_urls(&line);
        let line = self.redact_identity(&line);
        let line = drop_quoted(&line);
        let line = collapse_paths(&line);
        let line = scrub_words(&line);
        cap_line(&line)
    }

    fn redact_identity(&self, text: &str) -> String {
        let mut text = self.replace_homes(text);
        text = replace_foreign_profiles(&text);
        for name in &self.users {
            text = replace_word(&text, name, USER);
        }
        for name in &self.hosts {
            text = replace_word(&text, name, HOST);
        }
        text
    }

    fn replace_homes(&self, text: &str) -> String {
        if self.homes.is_empty() {
            return text.to_owned();
        }
        let chars: Vec<char> = text.chars().collect();
        let mut out = String::with_capacity(text.len());
        let mut i = 0;
        while i < chars.len() {
            if let Some(end) = self.homes.iter().find_map(|home| home.match_at(&chars, i)) {
                // `\\?\C:\Users\me` is one path: drop the verbatim prefix too.
                if out.ends_with(r"\\?\") {
                    out.truncate(out.len() - 4);
                }
                out.push('~');
                i = end;
            } else {
                out.push(chars[i]);
                i += 1;
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Character classes
// ---------------------------------------------------------------------------

fn is_sep(c: char) -> bool {
    c == '\\' || c == '/'
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-'
}

fn same_char(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

fn same_chars(a: &[char], b: &[char]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same_char(*x, *y))
}

/// Whether `chars[at..]` starts with `word`, ignoring case.
fn starts_with_at(chars: &[char], at: usize, word: &[char]) -> bool {
    chars.len() >= at + word.len() && same_chars(&chars[at..at + word.len()], word)
}

fn seps_at(chars: &[char], at: usize) -> usize {
    chars[at.min(chars.len())..]
        .iter()
        .take_while(|c| is_sep(**c))
        .count()
}

/// Control characters other than tab (and newline when `keep_newlines`) go: an
/// ANSI escape or a carriage return has no place in a pasted report.
fn strip_controls(text: &str, keep_newlines: bool) -> String {
    text.chars()
        .filter_map(|c| match c {
            '\t' => Some(' '),
            '\n' | '\r' if keep_newlines => Some(c),
            '\n' | '\r' => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect()
}

fn cap_line(line: &str) -> String {
    if line.chars().count() <= MAX_LINE_CHARS {
        return line.to_owned();
    }
    let mut cut: String = line.chars().take(MAX_LINE_CHARS).collect();
    cut.push('…');
    cut
}

// ---------------------------------------------------------------------------
// The home folder
// ---------------------------------------------------------------------------

/// A home folder, matched whatever slashes the text uses.
#[derive(Debug, Clone)]
struct HomeMatcher {
    /// A path that starts with a separator (`/home/me`, `\\server\share`).
    leading_sep: bool,
    segments: Vec<Vec<char>>,
}

impl HomeMatcher {
    fn new(home: &str) -> Option<Self> {
        let home = home.trim();
        let home = home.strip_prefix(r"\\?\").unwrap_or(home);
        let segments: Vec<Vec<char>> = home
            .split(is_sep)
            .filter(|segment| !segment.is_empty())
            .map(|segment| segment.chars().collect())
            .collect();
        // `C:\` alone, or nothing, would match far too much.
        let drive_only = segments.len() == 1 && segments[0].last() == Some(&':');
        if segments.is_empty() || drive_only {
            return None;
        }
        Some(Self {
            leading_sep: home.starts_with(is_sep),
            segments,
        })
    }

    /// The end of the home folder if it starts at `chars[at]`.
    fn match_at(&self, chars: &[char], at: usize) -> Option<usize> {
        let mut pos = at;
        // Not in the middle of a longer name (`xC:\Users\me`, `/foo/home/me`).
        if at > 0 {
            let prev = chars[at - 1];
            if is_name_char(prev) || (!self.leading_sep && prev == '.') {
                return None;
            }
        }
        if self.leading_sep {
            let seps = seps_at(chars, pos);
            if seps == 0 {
                return None;
            }
            pos += seps;
        }
        for (k, segment) in self.segments.iter().enumerate() {
            if k > 0 {
                let seps = seps_at(chars, pos);
                if seps == 0 {
                    return None;
                }
                pos += seps;
            }
            if !starts_with_at(chars, pos, segment) {
                return None;
            }
            pos += segment.len();
        }
        // Not a longer name (`C:\Users\meow`, `C:\Users\me.old`).
        match chars.get(pos) {
            Some(c) if is_name_char(*c) => None,
            Some('.') if chars.get(pos + 1).is_some_and(|c| c.is_alphanumeric()) => None,
            _ => Some(pos),
        }
    }
}

/// Profile folders of other users, or the same user's on another drive:
/// `D:\Users\x`, `C:\Documents and Settings\x`, `/home/x`, `/Users/x`.
fn replace_foreign_profiles(text: &str) -> String {
    const KEYWORDS: [&str; 3] = ["users", "home", "documents and settings"];
    const SHARED: [&str; 6] = [
        "public",
        "default",
        "default user",
        "all users",
        "shared",
        "~",
    ];
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    'scan: while i < chars.len() {
        if is_sep(chars[i]) {
            for keyword in KEYWORDS {
                let word: Vec<char> = keyword.chars().collect();
                let after = i + 1 + word.len();
                if !starts_with_at(&chars, i + 1, &word) || chars.get(after).is_none() {
                    continue;
                }
                let seps = seps_at(&chars, after);
                if seps == 0 {
                    continue;
                }
                // The folder must be the first one of its path: after a drive
                // letter, or the start of a path.
                let drive_before = i >= 2 && chars[i - 1] == ':' && chars[i - 2].is_alphabetic();
                let path_start = i == 0
                    || !(is_name_char(chars[i - 1]) || is_sep(chars[i - 1]) || chars[i - 1] == '.');
                let allowed = drive_before || (path_start && keyword != "documents and settings");
                if !allowed {
                    continue;
                }
                let start = after + seps;
                let end = profile_name_end(&chars, start);
                let name: String = chars[start..end].iter().collect();
                if name.is_empty() || SHARED.contains(&name.to_lowercase().as_str()) {
                    continue;
                }
                out.extend(&chars[i..start]);
                out.push_str(USER);
                i = end;
                continue 'scan;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Where the user-name folder that starts at `start` ends. Windows names may
/// hold spaces (`John Smith`), but only a folder name followed by a separator
/// is taken as such; otherwise the name stops at the first space.
fn profile_name_end(chars: &[char], start: usize) -> usize {
    let stops =
        |c: char| is_sep(c) || matches!(c, '"' | '\'' | '`' | '<' | '>' | '|' | '?' | '*' | ':');
    let mut end = start;
    while end < chars.len() && !stops(chars[end]) {
        end += 1;
    }
    let name = &chars[start..end];
    if name.iter().any(|c| c.is_whitespace()) && !chars.get(end).is_some_and(|c| is_sep(*c)) {
        end = start
            + name
                .iter()
                .position(|c| c.is_whitespace())
                .unwrap_or(name.len());
    }
    // A sentence's full stop is not part of the name.
    while end > start && matches!(chars[end - 1], '.' | ' ') {
        end -= 1;
    }
    end
}

/// Replaces `word` where it stands alone, ignoring case.
fn replace_word(text: &str, word: &[char], with: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let standalone = starts_with_at(&chars, i, word)
            && (i == 0 || !chars[i - 1].is_alphanumeric())
            && chars
                .get(i + word.len())
                .is_none_or(|c| !c.is_alphanumeric());
        if standalone {
            out.push_str(with);
            i += word.len();
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// URLs
// ---------------------------------------------------------------------------

fn is_url_stop(c: char) -> bool {
    c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '<' | '>' | ')' | ']' | '}')
}

/// Replaces URLs that carry a query, a fragment, credentials, or point at a
/// local file.
fn redact_urls(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let is_scheme_end =
            chars[i] == ':' && chars.get(i + 1) == Some(&'/') && chars.get(i + 2) == Some(&'/');
        if is_scheme_end {
            // The scheme is already in `out`: take it back.
            let scheme_len = out
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
                .count();
            let scheme: String = out
                .chars()
                .rev()
                .take(scheme_len)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            if scheme.len() >= 2 && scheme.starts_with(|c: char| c.is_ascii_alphabetic()) {
                let mut end = i + 3;
                while end < chars.len() && !is_url_stop(chars[end]) {
                    end += 1;
                }
                while end > i + 3 && matches!(chars[end - 1], '.' | ',' | ';' | ':' | '!' | '?') {
                    end -= 1;
                }
                let rest: String = chars[i + 3..end].iter().collect();
                let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
                let drop = scheme.eq_ignore_ascii_case("file")
                    || rest.contains(['?', '#'])
                    || authority.contains('@');
                if drop {
                    let keep = out.len() - scheme.len();
                    out.truncate(keep);
                    out.push_str(REDACTED);
                    i = end;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// Quoted text (log lines)
// ---------------------------------------------------------------------------

/// A quoted span that is safe to keep: a short identifier such as a plugin id
/// or result id, or a path inside Sevak's own folders.
fn is_safe_quoted(content: &str) -> bool {
    if content.is_empty() || content == REDACTED || content == PATH {
        return true;
    }
    let identifier = content.chars().count() <= MAX_QUOTED_IDENTIFIER
        && content
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '@' | '+' | '-'));
    identifier || is_sevak_home_path(content)
}

/// `~\AppData\Roaming\sevak\config.toml`, `~/.config/sevak/...`: below the
/// home folder and inside a folder named `sevak`.
fn is_sevak_home_path(path: &str) -> bool {
    let Some(rest) = path.strip_prefix('~') else {
        return false;
    };
    rest.starts_with(is_sep)
        && !rest.contains(['"', '\n'])
        && path_components(rest).any(|part| part.eq_ignore_ascii_case("sevak"))
}

fn path_components(path: &str) -> impl Iterator<Item = &str> {
    path.split(is_sep).filter(|part| !part.is_empty())
}

/// A quote with white space (or nothing) on both sides opens and closes
/// nothing: pairing it with a real quote would swallow or expose the wrong text.
fn is_stray(chars: &[char], at: usize) -> bool {
    (at == 0 || chars[at - 1].is_whitespace())
        && chars.get(at + 1).is_none_or(|c| c.is_whitespace())
}

/// Replaces the text between quotes by `<redacted>`, unless it is safe.
fn drop_quoted(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let quote = chars[i];
        let opens = match quote {
            '"' | '`' => true,
            // An apostrophe opens a quote only where a word could not be.
            '\'' => i == 0 || !chars[i - 1].is_alphanumeric(),
            _ => false,
        };
        if !opens || is_stray(&chars, i) {
            out.push(quote);
            i += 1;
            continue;
        }
        // Find the closing quote, skipping escapes (`\"` in Debug output).
        let mut j = i + 1;
        let mut closed = false;
        while j < chars.len() {
            if chars[j] == '\\' && quote != '\'' {
                j += 2;
                continue;
            }
            if chars[j] == quote
                && (quote != '\'' || !chars.get(j + 1).is_some_and(|c| c.is_alphanumeric()))
                && !is_stray(&chars, j)
            {
                closed = true;
                break;
            }
            j += 1;
        }
        let j = j.min(chars.len());
        let content: String = chars[i + 1..j].iter().collect();
        out.push(quote);
        if is_safe_quoted(&content) {
            out.push_str(&content);
        } else {
            out.push_str(REDACTED);
        }
        if closed {
            out.push(quote);
            i = j + 1;
        } else {
            // An unterminated quote: everything after it is the quoted text.
            i = chars.len();
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Paths (log lines)
// ---------------------------------------------------------------------------

/// First folders of the system's own locations; paths into them are kept.
const SYSTEM_DRIVE_DIRS: [&str; 4] = [
    "windows",
    "program files",
    "program files (x86)",
    "programdata",
];
const SYSTEM_UNIX_DIRS: [&str; 17] = [
    "usr",
    "opt",
    "etc",
    "bin",
    "sbin",
    "lib",
    "lib64",
    "var",
    "proc",
    "sys",
    "run",
    "dev",
    "snap",
    "nix",
    "applications",
    "system",
    "library",
];

enum PathStart {
    Tilde,
    Drive,
    Unix,
}

fn path_start(chars: &[char], i: usize) -> Option<PathStart> {
    let boundary = i == 0
        || chars[i - 1].is_whitespace()
        || matches!(chars[i - 1], '(' | '=' | '[' | ',' | ';' | '"' | '\'' | '`');
    if !boundary {
        return None;
    }
    let next = chars.get(i + 1).copied();
    match chars[i] {
        '~' if next.is_some_and(is_sep) => Some(PathStart::Tilde),
        c if c.is_ascii_alphabetic()
            && next == Some(':')
            && chars.get(i + 2).is_some_and(|c| is_sep(*c)) =>
        {
            Some(PathStart::Drive)
        }
        '/' if next.is_some_and(|c| c.is_alphanumeric() || c == '.') => Some(PathStart::Unix),
        _ => None,
    }
}

/// Where the path that starts at `start` ends: at whitespace, a quote or a
/// closing bracket, but a later word that holds a separator continues it (a
/// folder name with a space in it).
fn path_end(chars: &[char], start: usize) -> usize {
    let stops = |c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '"' | '\'' | '`' | '<' | '>' | ')' | ']' | '}' | ',' | ';' | '|'
            )
    };
    let mut end = start;
    loop {
        while end < chars.len() {
            // `/home/<user>/x`: a placeholder is part of the path.
            let placeholder = placeholder_len(chars, end);
            if placeholder > 0 {
                end += placeholder;
            } else if stops(chars[end]) {
                break;
            } else {
                end += 1;
            }
        }
        // Continue over spaces into a word that has a separator.
        let mut next = end;
        while next < chars.len() && chars[next] == ' ' {
            next += 1;
        }
        let mut word_end = next;
        while word_end < chars.len() && !stops(chars[word_end]) && !chars[word_end].is_whitespace()
        {
            word_end += 1;
        }
        let continues = next > end
            && word_end > next
            && path_start(chars, next).is_none()
            && chars[next..word_end].iter().any(|c| is_sep(*c))
            && !chars[next..word_end].contains(&'=');
        if continues {
            end = word_end;
        } else {
            break;
        }
    }
    while end > start && matches!(chars[end - 1], '.' | ':' | '!' | '?') {
        end -= 1;
    }
    end
}

/// The length of the placeholder (`<user>`, `<host>`, `<path>`, `<redacted>`)
/// that starts at `chars[at]`, or 0.
fn placeholder_len(chars: &[char], at: usize) -> usize {
    [USER, HOST, PATH, REDACTED]
        .iter()
        .find(|placeholder| {
            let placeholder: Vec<char> = placeholder.chars().collect();
            chars.len() >= at + placeholder.len()
                && chars[at..at + placeholder.len()] == placeholder[..]
        })
        .map_or(0, |placeholder| placeholder.chars().count())
}

fn keep_path(kind: &PathStart, path: &str) -> bool {
    let parts: Vec<&str> = path_components(path).collect();
    match kind {
        PathStart::Tilde => is_sevak_home_path(path),
        PathStart::Drive => parts
            .get(1)
            .is_some_and(|first| SYSTEM_DRIVE_DIRS.contains(&first.to_lowercase().as_str())),
        PathStart::Unix => {
            // `/word` alone is not a path (a fraction, an option).
            parts.len() < 2 || SYSTEM_UNIX_DIRS.contains(&parts[0].to_lowercase().as_str())
        }
    }
}

/// Replaces absolute paths that are not Sevak's own or the system's.
fn collapse_paths(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let Some(kind) = path_start(&chars, i) else {
            out.push(chars[i]);
            i += 1;
            continue;
        };
        let end = path_end(&chars, i);
        let path: String = chars[i..end].iter().collect();
        if keep_path(&kind, &path) {
            out.push_str(&path);
        } else {
            out.push_str(PATH);
        }
        i = end;
    }
    out
}

// ---------------------------------------------------------------------------
// Words
// ---------------------------------------------------------------------------

fn is_word_delimiter(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '"' | '\''
                | '`'
                | '<'
                | '>'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | ','
                | ';'
                | '='
                | '|'
        )
}

/// Words after which the next word is a secret (`password=...`, `token: ...`).
const SECRET_KEYS: [&str; 14] = [
    "password",
    "passwd",
    "pwd",
    "passphrase",
    "secret",
    "token",
    "apikey",
    "api_key",
    "api-key",
    "authorization",
    "auth",
    "cookie",
    "credentials",
    "credential",
];

/// Token prefixes: `(prefix, shortest whole token)`.
const SECRET_PREFIXES: [(&str, usize); 16] = [
    ("ghp_", 20),
    ("gho_", 20),
    ("ghu_", 20),
    ("ghs_", 20),
    ("ghr_", 20),
    ("github_pat_", 20),
    ("glpat-", 16),
    ("xoxb-", 12),
    ("xoxp-", 12),
    ("xoxa-", 12),
    ("xoxs-", 12),
    ("sk-", 20),
    ("pk_live_", 16),
    ("sk_live_", 16),
    ("AIza", 30),
    ("npm_", 24),
];

fn scrub_words(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    // The previous word named a secret: the next word is it.
    let mut secret_next = false;
    while i < chars.len() {
        if is_word_delimiter(chars[i]) {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && !is_word_delimiter(chars[i]) {
            i += 1;
        }
        let word: String = chars[start..i].iter().collect();
        if secret_next {
            // `Authorization: Bearer abc`: the scheme and then the secret.
            secret_next = matches!(word.to_lowercase().as_str(), "bearer" | "basic");
            // Already redacted: `password=<redacted>` stays as it is.
            out.push_str(if word == "redacted" { &word } else { REDACTED });
            continue;
        }
        let key = word.trim_end_matches([':', '.']).to_lowercase();
        let delimiters_after: String = chars[i..]
            .iter()
            .take_while(|c| is_word_delimiter(**c))
            .collect();
        let assigned = word.ends_with(':') || delimiters_after.contains(['=', ':']);
        if (SECRET_KEYS.contains(&key.as_str()) && assigned)
            || (matches!(key.as_str(), "bearer" | "basic") && delimiters_after.trim() == "")
        {
            secret_next = true;
            out.push_str(&word);
            continue;
        }
        out.push_str(&scrub_word(&word));
    }
    out
}

/// Redacts one delimiter-free word, or parts of it.
fn scrub_word(word: &str) -> String {
    let core = word.trim_end_matches(['.', ':', '!', '?']);
    let tail = &word[core.len()..];
    if core.is_empty() {
        return word.to_owned();
    }
    if is_email(core) || is_address(core) || is_mac(core) || is_jwt(core) || has_secret_prefix(core)
    {
        return format!("{REDACTED}{tail}");
    }
    // A long path-like word is checked a part at a time (`https://host/<token>`).
    let mut out = String::with_capacity(word.len());
    let mut part = String::new();
    let flush = |part: &mut String, out: &mut String| {
        if !part.is_empty() {
            if is_secret_part(part) {
                out.push_str(REDACTED);
            } else {
                out.push_str(part);
            }
            part.clear();
        }
    };
    for c in core.chars() {
        if matches!(c, '/' | '\\' | ':' | '?' | '&' | '#') {
            flush(&mut part, &mut out);
            out.push(c);
        } else {
            part.push(c);
        }
    }
    flush(&mut part, &mut out);
    out.push_str(tail);
    out
}

fn is_email(word: &str) -> bool {
    let Some((local, domain)) = word.rsplit_once('@') else {
        return false;
    };
    let tld = domain.rsplit('.').next().unwrap_or("");
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && tld.len() >= 2
        && tld.chars().all(char::is_alphabetic)
}

/// An IPv4 or IPv6 address, with an optional port, zone or prefix length.
fn is_address(word: &str) -> bool {
    let bare = word.split(['%', '/']).next().unwrap_or(word);
    if bare.parse::<Ipv6Addr>().is_ok() || bare.parse::<Ipv4Addr>().is_ok() {
        return true;
    }
    match bare.rsplit_once(':') {
        Some((host, port)) => {
            !port.is_empty()
                && port.chars().all(|c| c.is_ascii_digit())
                && host.parse::<Ipv4Addr>().is_ok()
        }
        None => false,
    }
}

fn is_mac(word: &str) -> bool {
    let groups: Vec<&str> = word.split([':', '-']).collect();
    groups.len() == 6
        && groups
            .iter()
            .all(|g| g.len() == 2 && g.chars().all(|c| c.is_ascii_hexdigit()))
}

/// `eyJ...` header, then two more base64url segments.
fn is_jwt(word: &str) -> bool {
    let segments: Vec<&str> = word.split('.').collect();
    let base64url = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '='))
    };
    segments.len() == 3
        && word.starts_with("eyJ")
        && segments.iter().all(|s| base64url(s))
        && segments.iter().map(|s| s.len()).sum::<usize>() >= 24
}

fn has_secret_prefix(word: &str) -> bool {
    if SECRET_PREFIXES
        .iter()
        .any(|(prefix, min)| word.starts_with(prefix) && word.len() >= *min)
    {
        return true;
    }
    // AWS access key ids.
    (word.starts_with("AKIA") || word.starts_with("ASIA"))
        && word.len() >= 20
        && word
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// A part of a word that looks like random data: a run of 16 or more hex
/// digits (hashes, ids), or 32 or more characters of base64 that mix cases or
/// digits.
fn is_secret_part(part: &str) -> bool {
    if has_secret_prefix(part) || is_address(part) {
        return true;
    }
    let mut run = 0;
    for c in part.chars() {
        if c.is_ascii_hexdigit() {
            run += 1;
            if run >= 16 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    // A UUID is an identifier, not random data worth hiding.
    let groups: Vec<&str> = part.split('-').collect();
    let uuid = groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(group, len)| group.len() == len && group.chars().all(|c| c.is_ascii_hexdigit()));
    let base64 = !uuid
        && part.chars().count() >= 32
        && part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+' | '='));
    base64
        && (part.chars().any(|c| c.is_ascii_digit())
            || (part.chars().any(|c| c.is_ascii_uppercase())
                && part.chars().any(|c| c.is_ascii_lowercase())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn redactor() -> Redactor {
        Redactor::new(&Identity {
            home_dirs: vec![r"C:\Users\Ninad".to_owned()],
            user_names: vec!["Ninad".to_owned()],
            host_names: vec!["DESKTOP-7QK2LM".to_owned()],
        })
    }

    fn unix_redactor() -> Redactor {
        Redactor::new(&Identity {
            home_dirs: vec!["/home/ninad".to_owned()],
            user_names: vec!["ninad".to_owned()],
            host_names: vec!["thinkpad".to_owned()],
        })
    }

    #[test]
    fn the_home_folder_becomes_a_tilde_in_every_spelling() {
        let r = redactor();
        for (input, expected) in [
            (
                r"C:\Users\Ninad\AppData\Roaming\sevak",
                r"~\AppData\Roaming\sevak",
            ),
            ("C:/Users/Ninad/Documents/a.txt", "~/Documents/a.txt"),
            (r"c:\users\NINAD\x", r"~\x"),
            (r"C:\\Users\\Ninad\\x", r"~\\x"),
            (r"\\?\C:\Users\Ninad\x", r"~\x"),
            (r"open C:\Users\Ninad", "open ~"),
            (r#"path="C:\\Users\\Ninad\\a b""#, r#"path="~\\a b""#),
        ] {
            assert_eq!(r.text(input), expected, "{input}");
        }
        let u = unix_redactor();
        assert_eq!(u.text("/home/ninad/.config/sevak"), "~/.config/sevak");
        assert_eq!(u.text("HOME=/home/ninad"), "HOME=~");
    }

    #[test]
    fn a_longer_name_that_starts_like_the_home_folder_is_left_alone() {
        let r = redactor();
        assert!(!r.text(r"C:\Users\NinadKulkarni\x").contains('~'));
        assert!(!r.text(r"C:\Users\Ninad.old\x").contains('~'));
        assert!(!r.text(r"xC:\Users\Ninad\x").contains('~'));
        assert_eq!(r.text(r"in C:\Users\Ninad."), "in ~.");
        let u = unix_redactor();
        assert!(!u.text("/foo/home/ninad/x").contains('~'));
        assert!(!u.text("/home/ninadx/x").contains('~'));
    }

    #[test]
    fn other_users_and_other_drives_become_user() {
        let r = redactor();
        assert_eq!(
            r.text(r"D:\Users\Alice\Documents"),
            r"D:\Users\<user>\Documents"
        );
        assert_eq!(
            r.text(r"C:\Users\John Smith\AppData"),
            r"C:\Users\<user>\AppData"
        );
        assert_eq!(
            r.text(r"C:\Documents and Settings\bob\x"),
            r"C:\Documents and Settings\<user>\x"
        );
        assert_eq!(r.text("/home/bob/x"), "/home/<user>/x");
        assert_eq!(r.text("/Users/bob/Library"), "/Users/<user>/Library");
        // Shared profile folders say nothing about a person.
        assert_eq!(
            r.text(r"C:\Users\Public\Desktop"),
            r"C:\Users\Public\Desktop"
        );
        assert_eq!(r.text(r"C:\Users\Default\x"), r"C:\Users\Default\x");
        // Own folder on another drive.
        assert_eq!(r.text(r"D:\Users\Ninad\x"), r"D:\Users\<user>\x");
        // Not a profile folder.
        assert_eq!(r.text("/srv/home/data/x"), "/srv/home/data/x");
    }

    #[test]
    fn the_user_and_computer_names_are_replaced_as_words() {
        let r = redactor();
        assert_eq!(
            r.text("hello ninad, from desktop-7qk2lm"),
            "hello <user>, from <host>"
        );
        assert_eq!(r.text("Ninad's laptop"), "<user>'s laptop");
        assert_eq!(r.text("ninadkulkarni"), "ninadkulkarni");
        // Too short to replace safely.
        let short = Redactor::new(&Identity {
            user_names: vec!["al".to_owned()],
            ..Identity::default()
        });
        assert_eq!(short.text("al and altitude"), "al and altitude");
    }

    #[test]
    fn urls_with_queries_credentials_or_files_are_dropped() {
        let r = Redactor::anonymous();
        assert_eq!(
            r.text("GET https://www.google.com/search?q=my+secret+thing failed"),
            "GET <redacted> failed"
        );
        assert_eq!(r.text("see https://a.test/p#frag."), "see <redacted>.");
        assert_eq!(r.text("https://user:pw@host.test/x"), "<redacted>");
        assert_eq!(r.text("file:///C:/Users/x/a.pdf"), "<redacted>");
        assert_eq!(
            r.text(r#"url="https://a.test/x?token=1""#),
            r#"url="<redacted>""#
        );
        // Plain URLs are kept: the update endpoint helps a bug report.
        assert_eq!(
            r.text("https://github.com/ninad-k/Sevak/releases/latest/download/latest.json"),
            "https://github.com/ninad-k/Sevak/releases/latest/download/latest.json"
        );
        assert_eq!(r.text("mailto:a"), "mailto:a");
    }

    #[test]
    fn emails_addresses_and_macs_are_dropped() {
        let r = Redactor::anonymous();
        for (input, expected) in [
            ("mail me@example.com now", "mail <redacted> now"),
            ("<me.x+tag@sub.example.co.uk>", "<<redacted>>"),
            ("peer 192.168.1.20 up", "peer <redacted> up"),
            ("peer 192.168.1.20:8080 up", "peer <redacted> up"),
            ("addr [fe80::1ff:fe23:4567:890a] up", "addr [<redacted>] up"),
            ("addr 2001:db8::8a2e:370:7334.", "addr <redacted>."),
            ("addr fe80::1%eth0 up", "addr <redacted> up"),
            ("mac aa:bb:cc:dd:ee:ff up", "mac <redacted> up"),
            ("http://10.0.0.5/x", "http://<redacted>/x"),
        ] {
            assert_eq!(r.text(input), expected, "{input}");
        }
        // Not addresses or emails.
        for keep in [
            "version 0.1.0 build 10.0.26300",
            "sevak::search::engine",
            "12:34:56",
            "user@localhost",
            "a @ b",
            "2026-10-04T12:34:56.123456Z",
        ] {
            assert_eq!(r.text(keep), keep, "{keep}");
        }
    }

    #[test]
    fn tokens_and_keys_are_dropped() {
        let r = Redactor::anonymous();
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dBjftJeZ4CVPmB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        for secret in [
            "ghp_16C7e42F292c6912E7710c838347Ae178B4a",
            "github_pat_11ABCDEFG0123456789_abcdefghijklmnopqrstuvwxyz",
            "AKIAIOSFODNN7EXAMPLE",
            "xoxb-123456789012-abcdefghijkl",
            "sk-abcdefghijklmnopqrstuvwxyz0123456789",
            "AIzaSyA-abcdefghijklmnopqrstuvwxyz01234",
            jwt,
            "d41d8cd98f00b204e9800998ecf8427e",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "dGhpcyBpcyBhIHNlY3JldCB0b2tlbiB2YWx1ZQ",
        ] {
            let out = r.text(&format!("saw {secret} here"));
            assert_eq!(out, "saw <redacted> here", "{secret}");
        }
        // Inside a URL path and after a key.
        assert_eq!(
            r.text("https://a.test/ghp_16C7e42F292c6912E7710c838347Ae178B4a/x"),
            "https://a.test/<redacted>/x"
        );
        assert_eq!(r.text("password=hunter2 ok"), "password=<redacted> ok");
        assert_eq!(r.text("Password: hunter2 ok"), "Password: <redacted> ok");
        assert_eq!(
            r.text("Authorization: Bearer abc123"),
            "Authorization: <redacted> <redacted>"
        );
        assert_eq!(r.text("token = abc"), "token = <redacted>");
        // The word alone, or a hex run that is too short, is ordinary text.
        assert_eq!(r.text("the token count is 3"), "the token count is 3");
        assert_eq!(
            r.text("id 550e8400-e29b-41d4-a716-446655440000"),
            "id 550e8400-e29b-41d4-a716-446655440000"
        );
        assert_eq!(r.text("0123456789abcde"), "0123456789abcde");
        assert_eq!(r.text("0123456789abcdef"), "<redacted>");
        assert_eq!(
            r.text("a-very-long-descriptive-plugin-name-that-is-fine"),
            "a-very-long-descriptive-plugin-name-that-is-fine"
        );
    }

    #[test]
    fn log_lines_lose_quoted_text_but_keep_identifiers() {
        let r = redactor();
        assert_eq!(
            r.log_line(r#"2026-10-04T12:00:00Z  INFO sevak::search: executed plugin="apps" id="apps:firefox.desktop""#),
            r#"2026-10-04T12:00:00Z  INFO sevak::search: executed plugin="apps" id="apps:firefox.desktop""#
        );
        assert_eq!(
            r.log_line(r#"WARN could not paste text="my bank password is 1234" into app"#),
            r#"WARN could not paste text="<redacted>" into app"#
        );
        assert_eq!(
            r.log_line(r#"INFO loaded=["apps", "web:g", "my secret note"]"#),
            r#"INFO loaded=["apps", "web:g", "<redacted>"]"#
        );
        assert_eq!(r.log_line("query='hello world' x"), "query='<redacted>' x");
        assert_eq!(
            r.log_line("it's fine, don't worry"),
            "it's fine, don't worry"
        );
        // Escaped quotes inside Debug output do not end the span early.
        assert_eq!(
            r.log_line(r#"text="say \"hi there\" ok" done"#),
            r#"text="<redacted>" done"#
        );
        // Unterminated: the rest of the line is the quoted text.
        assert_eq!(
            r.log_line(r#"INFO clip="secret thing"#),
            r#"INFO clip="<redacted>"#
        );
        // A long identifier-looking value is still not kept.
        let long = "a".repeat(60);
        assert_eq!(r.log_line(&format!("k=\"{long}\"")), r#"k="<redacted>""#);
    }

    #[test]
    fn log_lines_keep_sevak_s_own_paths_and_collapse_the_rest() {
        let r = redactor();
        assert_eq!(
            r.log_line(r#"INFO loaded config path="C:\\Users\\Ninad\\AppData\\Roaming\\sevak\\config.toml""#),
            r#"INFO loaded config path="~\\AppData\\Roaming\\sevak\\config.toml""#
        );
        assert_eq!(
            r.log_line(
                r"WARN cannot read C:\Users\Ninad\Documents\Clients\Acme Corp\plan.pdf: denied"
            ),
            "WARN cannot read <path>: denied"
        );
        assert_eq!(
            r.log_line(r"WARN cannot read D:\Clients\Acme\plan.pdf: denied"),
            "WARN cannot read <path>: denied"
        );
        assert_eq!(
            r.log_line(r"WARN cannot read C:\Windows\System32\x.dll: denied"),
            r"WARN cannot read C:\Windows\System32\x.dll: denied"
        );
        assert_eq!(
            r.log_line(r#"INFO opening path="D:\\Clients\\x.pdf""#),
            r#"INFO opening path="<redacted>""#
        );
        let u = unix_redactor();
        assert_eq!(
            u.log_line("WARN cannot read /home/ninad/Taxes/2025.pdf: denied"),
            "WARN cannot read <path>: denied"
        );
        assert_eq!(
            u.log_line("INFO config /home/ninad/.config/sevak/config.toml"),
            "INFO config ~/.config/sevak/config.toml"
        );
        assert_eq!(
            u.log_line("WARN missing /usr/lib/x.so"),
            "WARN missing /usr/lib/x.so"
        );
        assert_eq!(
            u.log_line("WARN missing /srv/data/x.so"),
            "WARN missing <path>"
        );
        // A lone slash word is not a path.
        assert_eq!(u.log_line("and/or /dev"), "and/or /dev");
    }

    #[test]
    fn a_path_field_keeps_sevak_s_own_folders_only() {
        let r = redactor();
        assert_eq!(
            r.path(r"C:\Users\Ninad\AppData\Roaming\sevak"),
            r"~\AppData\Roaming\sevak"
        );
        assert_eq!(
            r.path(r"C:\Program Files\Sevak\sevak.exe"),
            r"C:\Program Files\Sevak\sevak.exe"
        );
        // A folder somebody chose can name a client.
        assert_eq!(r.path(r"D:\Clients\Acme\sevak"), "<path>");
        assert_eq!(r.path(r"C:\Users\Ninad\Documents\Acme"), "<path>");
        let u = unix_redactor();
        assert_eq!(u.path("/home/ninad/.config/sevak"), "~/.config/sevak");
        assert_eq!(u.path("/usr/bin/sevak"), "/usr/bin/sevak");
        assert_eq!(u.path("/srv/acme/sevak"), "<path>");
    }

    #[test]
    fn a_path_ends_where_the_next_field_starts() {
        let u = unix_redactor();
        assert_eq!(
            u.log_line("INFO read path=/home/ninad/a.txt elapsed_ms=3"),
            "INFO read path=<path> elapsed_ms=3"
        );
    }

    #[test]
    fn log_lines_are_capped_and_stripped_of_control_characters() {
        let r = Redactor::anonymous();
        let long = format!("INFO {}", "word ".repeat(300));
        let out = r.log_line(&long);
        assert!(out.chars().count() <= MAX_LINE_CHARS + 1);
        assert!(out.ends_with('…'));
        assert_eq!(r.log_line("a\u{1b}[31mred\u{1b}[0m\r\tb"), "a[31mred[0m  b");
    }

    #[test]
    fn adversarial_log_lines() {
        let r = redactor();
        let cases = [
            // Clipboard text with everything at once.
            (
                r#"DEBUG clip text="pw: ghp_16C7e42F292c6912E7710c838347Ae178B4a mail me@x.io 10.1.2.3""#,
                r#"DEBUG clip text="<redacted>""#,
            ),
            // The same, unquoted: the word filters still catch it.
            (
                "DEBUG clip pw ghp_16C7e42F292c6912E7710c838347Ae178B4a mail me@x.io 10.1.2.3",
                "DEBUG clip pw <redacted> mail <redacted> <redacted>",
            ),
            // Quotes of the other kinds.
            ("INFO text=`two words`", "INFO text=`<redacted>`"),
            // A query in a Debug struct.
            (
                r#"INFO Query { text: "calc 2+2 secret", n: 3 }"#,
                r#"INFO Query { text: "<redacted>", n: 3 }"#,
            ),
            // Unicode text.
            ("INFO text=\"日本語のテキスト\"", "INFO text=\"<redacted>\""),
            // The user's name in a sentence and in a path.
            (
                r"INFO user Ninad opened C:\Users\Ninad\Desktop\x.txt",
                "INFO user <user> opened <path>",
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(r.log_line(input), expected, "{input}");
        }
    }

    #[test]
    fn nothing_sensitive_survives_a_gauntlet() {
        let r = redactor();
        let sensitive = [
            "Ninad",
            "DESKTOP-7QK2LM",
            "ghp_16C7e42F292c6912E7710c838347Ae178B4a",
            "AKIAIOSFODNN7EXAMPLE",
            "me@example.com",
            "192.168.10.77",
            "d41d8cd98f00b204e9800998ecf8427e",
            "hunter2",
            "Acme",
        ];
        let lines = [
            r#"INFO a="Acme Corp hunter2" b=Ninad c=DESKTOP-7QK2LM"#,
            r"INFO D:\Acme\Ninad\x.txt AKIAIOSFODNN7EXAMPLE",
            "INFO password=hunter2 mail=me@example.com ip=192.168.10.77",
            "INFO ghp_16C7e42F292c6912E7710c838347Ae178B4a d41d8cd98f00b204e9800998ecf8427e",
            r"INFO C:\Users\Ninad\Acme\hunter2.txt",
        ];
        for line in lines {
            let out = r.log_line(line);
            for needle in sensitive {
                assert!(
                    !out.to_lowercase().contains(&needle.to_lowercase()),
                    "{needle} survived in {out:?} (from {line:?})"
                );
            }
        }
    }

    #[test]
    fn redaction_is_idempotent() {
        let r = redactor();
        let lines = [
            r#"INFO a="x y" C:\Users\Ninad\Docs me@a.io 10.0.0.1 https://a.test/?q=1"#,
            "WARN password=hunter2 /home/bob/x ghp_16C7e42F292c6912E7710c838347Ae178B4a",
            "plain text with nothing to hide",
        ];
        for line in lines {
            let once = r.log_line(line);
            assert_eq!(r.log_line(&once), once, "{line}");
            let once = r.text(line);
            assert_eq!(r.text(&once), once, "{line}");
        }
    }

    #[test]
    fn multi_line_text_keeps_its_lines() {
        let r = redactor();
        assert_eq!(
            r.text("first C:\\Users\\Ninad\\a\nsecond me@x.io"),
            "first ~\\a\nsecond <redacted>"
        );
    }

    /// Lines made of random pieces, some of them secrets: nothing panics (the
    /// filters slice text by characters), the result is stable, and no secret
    /// survives in a log line.
    #[test]
    fn random_mixtures_are_safe_stable_and_clean() {
        let secrets: [(&str, &str); 14] = [
            ("password=hunter2", "hunter2"),
            (r"C:\Users\Ninad\Acme Corp\plan.pdf", "Acme"),
            (r#"text="my private note""#, "private note"),
            ("'another private note'", "another private"),
            ("https://x.test/?q=secretquery", "secretquery"),
            ("ghp_16C7e42F292c6912E7710c838347Ae178B4a", "16C7e42F"),
            ("user@mail.test", "user@mail"),
            ("10.1.2.3", "10.1.2.3"),
            ("fe80::1ff:fe23:4567:890a", "fe80"),
            ("AKIAIOSFODNN7EXAMPLE", "IOSFODNN7"),
            ("d41d8cd98f00b204e9800998ecf8427e", "d41d8cd9"),
            ("/home/bob/diary.txt", "diary"),
            ("DESKTOP-7QK2LM", "7QK2LM"),
            ("Authorization: Bearer abcdef123456", "abcdef123456"),
        ];
        let plain = [
            "INFO",
            "WARN",
            "sevak::search:",
            "loaded",
            r#"plugin="apps""#,
            "日本語",
            "→",
            "…",
            "'",
            "\"",
            "`",
            "\\",
            "/",
            "~",
            "<",
            ">",
            "=",
            ":",
            "(",
            "))",
            "[x]",
            "a\u{301}",
            "😀",
            "C:",
            r"~\AppData\Roaming\sevak\config.toml",
            "2026-10-04T12:00:00.000000Z",
            "key=value",
        ];
        let r = redactor();
        let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = move |bound: usize| {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((seed >> 33) as usize) % bound
        };
        for _ in 0..3000 {
            let mut line = String::new();
            let mut chosen: Vec<&str> = Vec::new();
            for _ in 0..(2 + next(6)) {
                if next(3) == 0 {
                    let (text, needle) = secrets[next(secrets.len())];
                    line.push_str(text);
                    chosen.push(needle);
                } else {
                    line.push_str(plain[next(plain.len())]);
                }
                line.push(' ');
            }
            let once = r.log_line(&line);
            assert_eq!(r.log_line(&once), once, "not stable: {line:?}");
            let text = r.text(&line);
            assert_eq!(r.text(&text), text, "text not stable: {line:?}");
            for needle in chosen {
                assert!(
                    !once.to_lowercase().contains(&needle.to_lowercase()),
                    "{needle:?} survived in {once:?} (from {line:?})"
                );
            }
        }
    }

    #[test]
    fn matchers_ignore_degenerate_homes() {
        let r = Redactor::new(&Identity {
            home_dirs: vec![String::new(), r"C:\".to_owned(), "/".to_owned()],
            ..Identity::default()
        });
        assert_eq!(
            r.text(r"C:\Windows and /usr/lib"),
            r"C:\Windows and /usr/lib"
        );
    }
}
