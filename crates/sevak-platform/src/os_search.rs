//! Whole-disk file search through the operating system's own index.
//!
//! Sevak's `files` plugin keeps a small in-memory index of a few folders. This
//! module asks the OS index instead, which already covers the whole disk and
//! (where the OS reads them) the text inside files:
//!
//! | OS | Names | Contents |
//! |---|---|---|
//! | Windows | "Everything" (`es.exe`) when it is installed and running, else Windows Search (`SystemIndex`, over ADO) | Windows Search |
//! | macOS | Spotlight (`mdfind`) | Spotlight |
//! | Linux | `plocate` / `locate` | Tracker 3 (`tracker3 search`), else Baloo (`baloosearch`) |
//!
//! Every query goes to a program or service on this computer; nothing is sent
//! anywhere. A search is slow compared to a keystroke (tens of milliseconds to
//! seconds), so [`search`] blocks until it is done or its timeout passes and the
//! caller runs it on a background thread; the files plugin does, and delivers
//! the answer late when it has to.
//!
//! The query builders and output parsers ([`windows_search_sql`],
//! [`spotlight_query`], [`locate_args`], [`parse_file_uris`], ...) are pure
//! functions compiled on every OS, so their unit tests run everywhere; only the
//! dispatcher at the bottom is cfg-gated. Search words never reach a shell:
//! processes get an argument list, and the one SQL statement escapes its text.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use thiserror::Error;

/// Most words of a query that reach the index; the rest are ignored.
pub const MAX_WORDS: usize = 8;
/// Longest single word, in characters.
const MAX_WORD_CHARS: usize = 64;

/// What to search for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsSearchKind {
    /// File and folder names.
    Names,
    /// The text inside files.
    Content,
}

/// One search of the OS index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsSearchRequest {
    /// What the user typed; split into words by [`search_words`].
    pub text: String,
    pub kind: OsSearchKind,
    /// At most this many hits are returned.
    pub limit: usize,
    /// Give up (and kill the helper process) after this long.
    pub timeout: Duration,
}

/// A file or folder the OS index found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsHit {
    pub path: PathBuf,
    pub is_dir: bool,
}

#[derive(Debug, Error)]
pub enum OsSearchError {
    /// No index to ask here (service stopped, tool not installed). The text is
    /// shown to the user, so it says what to do.
    #[error("{0}")]
    Unavailable(String),
    #[error("the search took too long")]
    TimedOut,
    #[error("{0}")]
    Failed(String),
}

/// Searches the OS index for `request`.
///
/// Slow and blocking: call it from a background thread. Hits come back in the
/// index's own order, without paths that no longer exist.
pub fn search(request: &OsSearchRequest) -> Result<Vec<OsHit>, OsSearchError> {
    let words = search_words(&request.text);
    if words.is_empty() || request.limit == 0 {
        return Ok(Vec::new());
    }
    #[cfg(windows)]
    {
        crate::windows::os_search::search(request, &words)
    }
    #[cfg(target_os = "macos")]
    {
        mac_search(request, &words)
    }
    #[cfg(target_os = "linux")]
    {
        linux_search(request, &words)
    }
}

/// The words of `text` that go to the index: split at whitespace, without
/// quotes, control characters or a lone `*`; words with no letter or digit
/// (`-`, `&&`) are dropped because index word-breakers ignore them and some
/// reject a query made only of them. At most [`MAX_WORDS`], each truncated.
pub fn search_words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|word| {
            word.chars()
                .filter(|c| !c.is_control() && *c != '"' && *c != '*')
                .take(MAX_WORD_CHARS)
                .collect::<String>()
        })
        .filter(|word| word.chars().any(char::is_alphanumeric))
        .take(MAX_WORDS)
        .collect()
}

// ---- Windows Search ------------------------------------------------------

/// The SQL for Windows Search's `SystemIndex`: every word is a prefix match
/// (`"word*"`), all of them required.
///
/// Names use `CONTAINS(System.FileName, ...)`: word-prefix, so `rep` finds
/// `annual_report.docx`. (`LIKE '%rep%'` finds any substring, but takes seconds
/// on a large index and treats `_` as a wildcard.) Contents use
/// `System.Search.Contents`, best match first. The words sit inside a quoted
/// phrase, where query operators are plain text; the only characters that need
/// escaping are the SQL string's own `'`.
pub fn windows_search_sql(words: &[String], kind: OsSearchKind, limit: usize) -> String {
    let phrases = words
        .iter()
        .map(|word| format!("\"{}*\"", word.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" AND ")
        .replace('\'', "''");
    let (column, order) = match kind {
        OsSearchKind::Names => ("System.FileName", ""),
        OsSearchKind::Content => (
            "System.Search.Contents",
            " ORDER BY System.Search.Rank DESC",
        ),
    };
    format!(
        "SELECT TOP {limit} System.ItemPathDisplay, System.ItemType FROM SystemIndex \
         WHERE SCOPE='file:' AND CONTAINS({column}, '{phrases}'){order}"
    )
}

/// Parses `GetString` output of [`windows_search_sql`]: one row per line,
/// `path<TAB>item type`. A folder's item type is `Directory`.
pub fn parse_windows_search_rows(text: &str) -> Vec<OsHit> {
    text.lines()
        .filter_map(|line| {
            let (path, item_type) = line.split_once('\t').unwrap_or((line, ""));
            let path = path.trim();
            looks_absolute(path).then(|| OsHit {
                path: PathBuf::from(path),
                is_dir: item_type.trim().eq_ignore_ascii_case("Directory"),
            })
        })
        .collect()
}

// ---- Everything (voidtools) ----------------------------------------------

/// Arguments for `es.exe`, the command-line client of Everything. Each word is
/// wrapped in quotes so one that starts with `-` (`-export-txt` writes a file)
/// is a search word and never an option; Everything reads `"word"` as the
/// word itself. Names only, one full path per line.
pub fn everything_args(words: &[String], limit: usize) -> Vec<String> {
    let mut args = vec!["-n".to_owned(), limit.to_string()];
    args.extend(words.iter().map(|word| format!("\"{word}\"")));
    args
}

// ---- macOS: Spotlight ----------------------------------------------------

/// The Spotlight query for `mdfind`: names match the display name anywhere
/// (`"*word*"`, case and diacritics ignored), contents match word prefixes of
/// the extracted text. All words are required. Quotes and backslashes are
/// escaped, and `*` is removed by [`search_words`], so the text cannot change
/// the shape of the query.
pub fn spotlight_query(words: &[String], kind: OsSearchKind) -> String {
    words
        .iter()
        .map(|word| {
            let word = word.replace('\\', "\\\\").replace('"', "\\\"");
            match kind {
                OsSearchKind::Names => format!("kMDItemDisplayName == \"*{word}*\"cd"),
                OsSearchKind::Content => format!("kMDItemTextContent == \"{word}*\"cd"),
            }
        })
        .collect::<Vec<_>>()
        .join(" && ")
}

// ---- Linux: locate, Tracker, Baloo ---------------------------------------

/// `locate` arguments for names: case-insensitive, base name only, every word
/// required, at most `limit` rows. Works for `plocate`, `mlocate` and GNU
/// `locate`.
pub fn locate_args(words: &[String], limit: usize) -> Vec<String> {
    let mut args: Vec<String> = ["-i", "-b", "-A", "-l"].map(str::to_owned).into();
    args.push(limit.to_string());
    args.push("--".to_owned());
    args.extend(words.iter().map(|word| locate_pattern(word)));
    args
}

/// A `locate` pattern for a literal word. Without glob characters a pattern is
/// a plain substring match; with them `locate` treats it as a glob that must
/// match the whole name, so the characters are escaped and the word is wrapped
/// in `*...*`.
pub fn locate_pattern(word: &str) -> String {
    if !word.contains(['*', '?', '[', ']', '\\']) {
        return word.to_owned();
    }
    let mut pattern = String::from("*");
    for c in word.chars() {
        if matches!(c, '*' | '?' | '[' | ']' | '\\') {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern.push('*');
    pattern
}

/// `tracker3 search` arguments for file contents.
pub fn tracker_args(words: &[String], limit: usize) -> Vec<String> {
    let mut args = vec![
        "search".to_owned(),
        "--files".to_owned(),
        format!("--limit={limit}"),
        "--".to_owned(),
    ];
    args.extend(words.iter().cloned());
    args
}

/// `baloosearch` arguments (a plain query searches names and contents).
pub fn baloo_args(words: &[String], limit: usize) -> Vec<String> {
    let mut args = vec!["-l".to_owned(), limit.to_string(), "--".to_owned()];
    args.extend(words.iter().cloned());
    args
}

// ---- Parsing --------------------------------------------------------------

/// Whether `text` is an absolute path in Unix (`/x`), Windows drive (`C:\x`,
/// `C:/x`) or UNC (`\\server\share`) form. Used on tool output, so it does not
/// depend on the OS Sevak runs on.
pub fn looks_absolute(text: &str) -> bool {
    let bytes = text.as_bytes();
    match bytes {
        [b'/', ..] | [b'\\', b'\\', ..] => true,
        [drive, b':', b'\\' | b'/', ..] => drive.is_ascii_alphabetic(),
        _ => false,
    }
}

/// One absolute path per line (`locate`, `mdfind`, `es.exe`, `baloosearch`);
/// anything else (headers, timing lines) is skipped. `\r` is trimmed.
pub fn parse_path_lines<S: AsRef<str>>(lines: &[S]) -> Vec<PathBuf> {
    lines
        .iter()
        .map(|line| line.as_ref().trim_end_matches(['\r', '\n']))
        .filter(|line| looks_absolute(line))
        .map(PathBuf::from)
        .collect()
}

/// The `file://` URIs in `lines`, as paths (`tracker3 search` prints them with
/// decoration around, and percent-encodes spaces). A URI ends at whitespace,
/// quotes or angle brackets; anything that is not a local file is skipped.
pub fn parse_file_uris<S: AsRef<str>>(lines: &[S]) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for line in lines {
        let line = line.as_ref();
        let mut rest = line;
        while let Some(start) = rest.find("file://") {
            let uri = &rest[start + "file://".len()..];
            let end = uri
                .find(|c: char| {
                    c.is_whitespace() || c.is_control() || matches!(c, '"' | '\'' | '<' | '>')
                })
                .unwrap_or(uri.len());
            let (encoded, tail) = uri.split_at(end);
            if encoded.starts_with('/') {
                paths.push(PathBuf::from(percent_decode(encoded)));
            }
            rest = tail;
        }
    }
    paths
}

/// Decodes `%XX` escapes; an invalid escape stays as typed, and bytes that are
/// not UTF-8 are replaced.
pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((high * 16 + low) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Turns found paths into hits: unreadable and vanished ones (an index can lag
/// behind the disk) are dropped, folders are told from files, and at most
/// `limit` remain.
pub fn hits_from_paths(paths: Vec<PathBuf>, limit: usize) -> Vec<OsHit> {
    paths
        .into_iter()
        .filter_map(|path| {
            let is_dir = std::fs::metadata(&path).ok()?.is_dir();
            Some(OsHit { path, is_dir })
        })
        .take(limit)
        .collect()
}

// ---- Running helper programs ---------------------------------------------

/// What a helper program printed before it finished, was stopped or timed out.
#[derive(Debug)]
pub struct Output {
    pub lines: Vec<String>,
    /// The exit code; `None` when the process was stopped (enough lines, or
    /// the timeout) or ended without a code.
    pub code: Option<i32>,
    pub timed_out: bool,
}

/// Runs `program` and collects up to `max_lines` lines of its output, within
/// `timeout`. The process is killed when it times out or has printed enough
/// (`mdfind` cannot limit its own output). stdin and stderr are closed, and on
/// Windows no console window appears.
pub fn run_lines(
    program: &Path,
    args: &[String],
    timeout: Duration,
    max_lines: usize,
) -> Result<Output, OsSearchError> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    crate::process::configure_helper_command(&mut command);
    let mut child = command.spawn().map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            OsSearchError::Unavailable(format!("{} is not installed", program.display()))
        } else {
            OsSearchError::Failed(format!("could not start {}: {err}", program.display()))
        }
    })?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(OsSearchError::Failed("no output pipe".to_owned()));
    };

    let (sender, receiver) = mpsc::channel::<String>();
    // Ends by itself when the process is killed (the pipe closes) or Sevak drops
    // the receiver.
    thread::spawn(move || {
        for line in BufReader::new(stdout).split(b'\n') {
            let Ok(bytes) = line else { break };
            let line = String::from_utf8_lossy(&bytes)
                .trim_end_matches('\r')
                .to_owned();
            if sender.send(line).is_err() {
                break;
            }
        }
    });

    let deadline = Instant::now() + timeout;
    let mut lines = Vec::new();
    let mut stop = false;
    let mut timed_out = false;
    while lines.len() < max_lines {
        let left = deadline.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(left) {
            Ok(line) => lines.push(line),
            Err(RecvTimeoutError::Timeout) => {
                timed_out = true;
                stop = true;
                break;
            }
            // End of output.
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    stop |= lines.len() >= max_lines;

    let code = if stop {
        let _ = child.kill();
        let _ = child.wait();
        None
    } else {
        wait_until(&mut child, deadline)
    };
    Ok(Output {
        lines,
        code,
        timed_out,
    })
}

/// The exit code of `child` once it ends; kills it if it is still running at
/// `deadline`.
fn wait_until(child: &mut std::process::Child, deadline: Instant) -> Option<i32> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.code(),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// The first of `names` that is an executable on `PATH` (or, on Unix, in the
/// usual system folders, which a GUI session's `PATH` can lack).
pub fn first_program(names: &[&str]) -> Option<PathBuf> {
    names.iter().find_map(|name| {
        crate::process::find_in_path(name).or_else(|| {
            ["/usr/bin", "/usr/local/bin", "/opt/homebrew/bin", "/bin"]
                .iter()
                .map(|dir| Path::new(dir).join(name))
                .find(|path| cfg!(unix) && path.is_file())
        })
    })
}

/// Outcome of a helper that may have been cut short.
pub fn timed_out_or(timed_out: bool, paths: Vec<PathBuf>) -> Result<Vec<PathBuf>, OsSearchError> {
    if timed_out && paths.is_empty() {
        Err(OsSearchError::TimedOut)
    } else {
        Ok(paths)
    }
}

// ---- Dispatch --------------------------------------------------------------

/// Spotlight through `mdfind`. It prints one path per line and has no limit
/// option, so [`run_lines`] stops it after enough lines.
#[cfg(target_os = "macos")]
fn mac_search(request: &OsSearchRequest, words: &[String]) -> Result<Vec<OsHit>, OsSearchError> {
    let program = first_program(&["mdfind"])
        .ok_or_else(|| OsSearchError::Unavailable("Spotlight (mdfind) was not found".to_owned()))?;
    let args = vec![spotlight_query(words, request.kind)];
    // Spotlight's order is not by relevance, and the caller drops noise
    // (app bundle contents, system folders), so read more than `limit`.
    let output = run_lines(&program, &args, request.timeout, request.limit * 4)?;
    if output.code.is_some_and(|code| code != 0) && output.lines.is_empty() {
        return Err(OsSearchError::Failed(format!(
            "mdfind failed (exit code {})",
            output.code.unwrap_or_default()
        )));
    }
    let paths = parse_path_lines(&output.lines);
    let paths = timed_out_or(output.timed_out, paths)?;
    Ok(hits_from_paths(paths, request.limit * 4))
}

/// Names through `plocate`/`locate`; contents through Tracker 3, else Baloo.
#[cfg(target_os = "linux")]
fn linux_search(request: &OsSearchRequest, words: &[String]) -> Result<Vec<OsHit>, OsSearchError> {
    // The index returns files in database order, not by relevance, and the
    // caller drops hidden and generated paths, so ask for more than `limit`.
    let wanted = request.limit * 4;
    match request.kind {
        OsSearchKind::Names => {
            let program = first_program(&["plocate", "locate"]).ok_or_else(|| {
                OsSearchError::Unavailable(
                    "install plocate (or mlocate) and run updatedb to search by name".to_owned(),
                )
            })?;
            let output = run_lines(
                &program,
                &locate_args(words, wanted),
                request.timeout,
                wanted,
            )?;
            // `locate` exits with 1 when nothing matched.
            if output.code.is_some_and(|code| code > 1) && output.lines.is_empty() {
                return Err(OsSearchError::Failed(format!(
                    "{} failed (exit code {})",
                    program.display(),
                    output.code.unwrap_or_default()
                )));
            }
            let paths = parse_path_lines(&output.lines);
            let paths = timed_out_or(output.timed_out, paths)?;
            Ok(hits_from_paths(paths, wanted))
        }
        OsSearchKind::Content => {
            let mut last_error = OsSearchError::Unavailable(
                "install Tracker 3 (tracker3) or Baloo to search inside files".to_owned(),
            );
            if let Some(program) = first_program(&["tracker3"]) {
                let output = run_lines(
                    &program,
                    &tracker_args(words, wanted),
                    request.timeout,
                    wanted * 4,
                )?;
                let paths = parse_file_uris(&output.lines);
                if output.code.is_some_and(|code| code != 0) && paths.is_empty() {
                    last_error = OsSearchError::Failed(format!(
                        "tracker3 failed (exit code {})",
                        output.code.unwrap_or_default()
                    ));
                } else {
                    let paths = timed_out_or(output.timed_out, paths)?;
                    return Ok(hits_from_paths(paths, wanted));
                }
            }
            if let Some(program) = first_program(&["baloosearch", "baloosearch6"]) {
                let output = run_lines(
                    &program,
                    &baloo_args(words, wanted),
                    request.timeout,
                    wanted,
                )?;
                let paths = parse_path_lines(&output.lines);
                if output.code.is_some_and(|code| code != 0) && paths.is_empty() {
                    last_error = OsSearchError::Failed(format!(
                        "baloosearch failed (exit code {})",
                        output.code.unwrap_or_default()
                    ));
                } else {
                    let paths = timed_out_or(output.timed_out, paths)?;
                    return Ok(hits_from_paths(paths, wanted));
                }
            }
            Err(last_error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<String> {
        search_words(text)
    }

    #[test]
    fn words_are_split_cleaned_and_capped() {
        assert_eq!(words("  annual   report "), ["annual", "report"]);
        assert_eq!(words("say \"hello\" wor*ld"), ["say", "hello", "world"]);
        // Words with nothing a word-breaker would keep are dropped.
        assert_eq!(words("a - && b * \"\""), ["a", "b"]);
        assert_eq!(words("tab\there\u{7}bell"), ["tab", "herebell"]);
        assert!(words("   ").is_empty());
        let many = (0..20)
            .map(|i| format!("w{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(words(&many).len(), MAX_WORDS);
        assert_eq!(words(&"x".repeat(500))[0].chars().count(), MAX_WORD_CHARS);
    }

    #[test]
    fn windows_sql_matches_word_prefixes_and_requires_every_word() {
        assert_eq!(
            windows_search_sql(&words("annual report"), OsSearchKind::Names, 100),
            "SELECT TOP 100 System.ItemPathDisplay, System.ItemType FROM SystemIndex \
             WHERE SCOPE='file:' AND CONTAINS(System.FileName, '\"annual*\" AND \"report*\"')"
        );
        assert_eq!(
            windows_search_sql(&words("invoice"), OsSearchKind::Content, 50),
            "SELECT TOP 50 System.ItemPathDisplay, System.ItemType FROM SystemIndex \
             WHERE SCOPE='file:' AND CONTAINS(System.Search.Contents, '\"invoice*\"') \
             ORDER BY System.Search.Rank DESC"
        );
    }

    #[test]
    fn windows_sql_escapes_the_string_literal() {
        let sql = windows_search_sql(&words("bob's x') OR 1=1 --"), OsSearchKind::Names, 10);
        // Every quote of the text is doubled, so it cannot end the literal; the
        // double quotes of the phrases are ours.
        assert!(sql.contains("\"bob''s*\""), "{sql}");
        assert!(sql.contains("\"x'')*\""), "{sql}");
        assert!(sql.ends_with("')"), "{sql}");
        assert_eq!(sql.matches("CONTAINS(").count(), 1);
        // A hand-built word list with double quotes cannot close a phrase.
        let sql = windows_search_sql(&["a\"b".to_owned()], OsSearchKind::Names, 10);
        assert!(sql.contains("'\"ab*\"'"), "{sql}");
    }

    #[test]
    fn parses_windows_search_rows() {
        let text = "C:\\Users\\me\\Documents\\Report.docx\t.docx\r\n\
                    C:\\Users\\me\\Documents\\Reports\tDirectory\r\n\
                    D:/data/x.txt\r\n\
                    \\\\server\\share\\y.txt\t.txt\n\
                    relative\\no.txt\t.txt\n\
                    \n";
        let hits = parse_windows_search_rows(text);
        assert_eq!(
            hits,
            [
                OsHit {
                    path: PathBuf::from("C:\\Users\\me\\Documents\\Report.docx"),
                    is_dir: false
                },
                OsHit {
                    path: PathBuf::from("C:\\Users\\me\\Documents\\Reports"),
                    is_dir: true
                },
                OsHit {
                    path: PathBuf::from("D:/data/x.txt"),
                    is_dir: false
                },
                OsHit {
                    path: PathBuf::from("\\\\server\\share\\y.txt"),
                    is_dir: false
                },
            ]
        );
    }

    #[test]
    fn everything_words_cannot_become_options() {
        assert_eq!(
            everything_args(&words("-export-txt out"), 30),
            ["-n", "30", "\"-export-txt\"", "\"out\""]
        );
    }

    #[test]
    fn spotlight_queries() {
        assert_eq!(
            spotlight_query(&words("annual report"), OsSearchKind::Names),
            "kMDItemDisplayName == \"*annual*\"cd && kMDItemDisplayName == \"*report*\"cd"
        );
        assert_eq!(
            spotlight_query(&words("budget 2026"), OsSearchKind::Content),
            "kMDItemTextContent == \"budget*\"cd && kMDItemTextContent == \"2026*\"cd"
        );
    }

    #[test]
    fn spotlight_queries_escape_quotes_and_backslashes() {
        let query = spotlight_query(&["a\"b\\c".to_owned()], OsSearchKind::Names);
        assert_eq!(query, "kMDItemDisplayName == \"*a\\\"b\\\\c*\"cd");
        // Hand-built words with a trailing backslash cannot escape the closing quote.
        let query = spotlight_query(&["x\\".to_owned()], OsSearchKind::Content);
        assert_eq!(query, "kMDItemTextContent == \"x\\\\*\"cd");
    }

    #[test]
    fn locate_arguments() {
        assert_eq!(
            locate_args(&words("annual report"), 200),
            ["-i", "-b", "-A", "-l", "200", "--", "annual", "report"]
        );
        // A word that looks like an option stays after `--`.
        assert!(locate_args(&words("-r x"), 5)
            .iter()
            .position(|a| a == "--")
            .is_some_and(|i| i < 7));
    }

    #[test]
    fn locate_patterns_are_literal() {
        assert_eq!(locate_pattern("report"), "report");
        assert_eq!(locate_pattern("a.b"), "a.b");
        assert_eq!(locate_pattern("a[1]"), "*a\\[1\\]*");
        assert_eq!(locate_pattern("what?"), "*what\\?*");
        assert_eq!(locate_pattern("c\\d"), "*c\\\\d*");
    }

    #[test]
    fn tracker_and_baloo_arguments() {
        assert_eq!(
            tracker_args(&words("budget plan"), 40),
            ["search", "--files", "--limit=40", "--", "budget", "plan"]
        );
        assert_eq!(
            baloo_args(&words("budget"), 40),
            ["-l", "40", "--", "budget"]
        );
    }

    #[test]
    fn absolute_path_forms() {
        for yes in ["/usr/bin", "C:\\x", "c:/x", "\\\\srv\\share"] {
            assert!(looks_absolute(yes), "{yes}");
        }
        for no in ["", "x", "./x", "C:", "C:x", "1:\\x", "Elapsed: 5 ms", "\\x"] {
            assert!(!looks_absolute(no), "{no}");
        }
    }

    #[test]
    fn path_lines_skip_everything_else() {
        let lines = [
            "/home/me/a.txt",
            "Elapsed: 12 ms",
            "",
            "/home/me/b c.txt\r",
            "Searching...",
        ];
        assert_eq!(
            parse_path_lines(&lines),
            [
                PathBuf::from("/home/me/a.txt"),
                PathBuf::from("/home/me/b c.txt")
            ]
        );
    }

    #[test]
    fn file_uris_are_found_in_decorated_output_and_decoded() {
        let lines = [
            "Results:",
            "  file:///home/me/My%20Report.txt",
            "  file:///home/me/caf%C3%A9.txt (text/plain)",
            "  \x1b[1mfile:///home/me/bold.txt\x1b[0m",
            "  file:///a.txt file:///b.txt",
            "  https://example.com/x",
            "  file://remote/share/x",
            "  file:///bad%zzescape%2",
        ];
        let paths = parse_file_uris(&lines);
        assert_eq!(
            paths,
            [
                PathBuf::from("/home/me/My Report.txt"),
                PathBuf::from("/home/me/café.txt"),
                PathBuf::from("/home/me/bold.txt"),
                PathBuf::from("/a.txt"),
                PathBuf::from("/b.txt"),
                PathBuf::from("/bad%zzescape%2"),
            ]
        );
    }

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("%2F%2f"), "//");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%"), "%");
        assert_eq!(percent_decode("%4"), "%4");
        assert_eq!(percent_decode("%41%4"), "A%4");
        assert_eq!(percent_decode("%FF"), "\u{FFFD}");
    }

    #[test]
    fn hits_drop_vanished_paths_and_tell_folders_apart() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, b"x").unwrap();
        let hits = hits_from_paths(
            vec![dir.path().join("gone.txt"), file.clone(), dir.path().into()],
            10,
        );
        assert_eq!(
            hits,
            [
                OsHit {
                    path: file,
                    is_dir: false
                },
                OsHit {
                    path: dir.path().into(),
                    is_dir: true
                }
            ]
        );
        assert_eq!(hits_from_paths(vec![dir.path().into()], 0), []);
    }

    #[test]
    fn empty_queries_return_nothing_without_asking_the_os() {
        let request = OsSearchRequest {
            text: " - * ".to_owned(),
            kind: OsSearchKind::Names,
            limit: 10,
            timeout: Duration::from_secs(1),
        };
        assert!(search(&request).unwrap().is_empty());
    }

    #[cfg(unix)]
    mod processes {
        use super::*;

        fn sh(script: &str) -> (PathBuf, Vec<String>) {
            (
                PathBuf::from("/bin/sh"),
                vec!["-c".to_owned(), script.to_owned()],
            )
        }

        #[test]
        fn collects_lines_and_the_exit_code() {
            let (program, args) = sh("echo one; echo two; exit 3");
            let output = run_lines(&program, &args, Duration::from_secs(5), 10).unwrap();
            assert_eq!(output.lines, ["one", "two"]);
            assert_eq!(output.code, Some(3));
            assert!(!output.timed_out);
        }

        #[test]
        fn stops_the_process_after_enough_lines() {
            let (program, args) = sh("while true; do echo x; done");
            let started = Instant::now();
            let output = run_lines(&program, &args, Duration::from_secs(10), 5).unwrap();
            assert_eq!(output.lines.len(), 5);
            assert_eq!(output.code, None);
            assert!(started.elapsed() < Duration::from_secs(5));
        }

        #[test]
        fn kills_a_process_that_outlives_the_timeout() {
            // The timeout leaves a loaded machine plenty of time to start the
            // shell and print "early", and the sleep far outlasts it.
            let (program, args) = sh("echo early; sleep 120");
            let started = Instant::now();
            let output = run_lines(&program, &args, Duration::from_secs(5), 10).unwrap();
            assert!(output.timed_out);
            assert_eq!(output.lines, ["early"]);
            assert!(started.elapsed() < Duration::from_secs(60));
            // Nothing found before the timeout is an error; something found is kept.
            assert!(matches!(
                timed_out_or(output.timed_out, Vec::new()),
                Err(OsSearchError::TimedOut)
            ));
        }

        #[test]
        fn a_missing_program_is_unavailable() {
            let err = run_lines(
                Path::new("/definitely/not/installed"),
                &[],
                Duration::from_secs(1),
                1,
            )
            .unwrap_err();
            assert!(matches!(err, OsSearchError::Unavailable(_)), "{err:?}");
        }
    }

    /// The same behaviour as `processes`, with Windows PowerShell as the helper.
    #[cfg(windows)]
    mod windows_processes {
        use super::*;

        fn powershell(script: &str) -> (PathBuf, Vec<String>) {
            (
                PathBuf::from("powershell"),
                ["-NoProfile", "-NonInteractive", "-Command", script]
                    .map(str::to_owned)
                    .into(),
            )
        }

        #[test]
        fn collects_lines_and_the_exit_code() {
            let (program, args) = powershell("'one'; 'two'; exit 3");
            let output = run_lines(&program, &args, Duration::from_secs(30), 10).unwrap();
            assert_eq!(output.lines, ["one", "two"]);
            assert_eq!(output.code, Some(3));
            assert!(!output.timed_out);
        }

        #[test]
        fn stops_the_process_after_enough_lines() {
            let (program, args) = powershell("while ($true) { 'x' }");
            let output = run_lines(&program, &args, Duration::from_secs(30), 5).unwrap();
            assert_eq!(output.lines.len(), 5);
            assert_eq!(output.code, None);
        }

        #[test]
        fn kills_a_process_that_outlives_the_timeout() {
            // PowerShell can take seconds to start on a loaded machine: the
            // timeout leaves room for that, and the sleep far outlasts it.
            let (program, args) = powershell("'early'; Start-Sleep 300");
            let started = Instant::now();
            let output = run_lines(&program, &args, Duration::from_secs(20), 10).unwrap();
            assert!(output.timed_out);
            assert_eq!(output.lines, ["early"]);
            assert!(started.elapsed() < Duration::from_secs(120));
        }
    }
}
