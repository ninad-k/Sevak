//! Query filters for file search: `f report ext:pdf size:>1mb modified:7d`.
//!
//! [`parse`] splits what was typed into the name text (fuzzy-matched as
//! before), quoted phrases that must appear in the name, excluded words, and
//! filters on the type, size, dates, location and kind of an entry. It is a pure
//! function: the current time and the home folder are passed in, so the tests
//! pin both. [`FileQuery::matches`] then decides, without touching the disk,
//! whether one entry passes.
//!
//! | Filter | Meaning |
//! |---|---|
//! | `ext:pdf`, `ext:jpg,png` | the name ends in one of these extensions |
//! | `type:image` (`video`, `audio`, `doc`, `code`, `archive`) | an extension of that group ([`TYPE_GROUPS`]) |
//! | `size:>10mb`, `size:<=500kb`, `size:1mb..2gb`, `size:empty` | file size (1 kb = 1024 bytes) |
//! | `modified:today`, `yesterday`, `7d`, `>30d`, `2026-01-01`, `>=2026-01`, `2025..2026-06` | last change |
//! | `created:` | the same, for the creation time |
//! | `in:~/Projects`, `folder:C:\Work`, `in:src` | inside that folder (a bare name: any folder of that name) |
//! | `is:folder`, `is:file` | folders or files only |
//! | `"two words"` | the name contains the phrase |
//! | `-ext:tmp`, `-draft`, `-"old copy"` | negation |
//!
//! Only these keys are filters. Anything else with a colon (`12:30`,
//! `http://x`) stays search text, so ordinary searches are not affected. A
//! known key with a value that makes no sense is a [`FilterError`] with a
//! message saying what would work, never a silent no-op.

use std::fmt;
use std::path::{Component, Path, PathBuf};

use chrono::{DateTime, Months, NaiveDate, NaiveDateTime, TimeDelta, TimeZone};

/// Extension groups for `type:`. Lower case, without the dot.
pub const TYPE_GROUPS: &[(&str, &[&str])] = &[
    (
        "image",
        &[
            "jpg", "jpeg", "png", "gif", "bmp", "webp", "tif", "tiff", "svg", "heic", "heif",
            "ico", "avif", "raw", "cr2", "nef", "arw", "dng", "psd",
        ],
    ),
    (
        "video",
        &[
            "mp4", "mkv", "mov", "avi", "wmv", "webm", "m4v", "flv", "mpg", "mpeg", "3gp",
        ],
    ),
    (
        "audio",
        &[
            "mp3", "wav", "flac", "aac", "ogg", "oga", "m4a", "wma", "opus", "aiff", "mid", "midi",
        ],
    ),
    (
        "doc",
        &[
            "pdf", "doc", "docx", "odt", "rtf", "txt", "md", "xls", "xlsx", "ods", "csv", "ppt",
            "pptx", "odp", "pages", "numbers", "key", "epub", "tex",
        ],
    ),
    (
        "code",
        &[
            "rs", "py", "js", "mjs", "ts", "jsx", "tsx", "java", "c", "h", "cpp", "hpp", "cc",
            "cs", "go", "rb", "php", "swift", "kt", "kts", "scala", "sh", "bash", "zsh", "ps1",
            "bat", "cmd", "lua", "pl", "r", "sql", "html", "htm", "css", "scss", "json", "toml",
            "yaml", "yml", "xml", "svelte", "vue", "dart",
        ],
    ),
    (
        "archive",
        &[
            "zip", "rar", "7z", "tar", "gz", "tgz", "bz2", "xz", "zst", "iso", "dmg", "cab",
        ],
    ),
];

/// Other spellings accepted by `type:`.
const TYPE_ALIASES: &[(&str, &str)] = &[
    ("images", "image"),
    ("picture", "image"),
    ("pictures", "image"),
    ("photo", "image"),
    ("photos", "image"),
    ("videos", "video"),
    ("movie", "video"),
    ("movies", "video"),
    ("music", "audio"),
    ("sound", "audio"),
    ("document", "doc"),
    ("documents", "doc"),
    ("docs", "doc"),
    ("source", "code"),
    ("archives", "archive"),
    ("compressed", "archive"),
];

const KEYS: &[&str] = &[
    "ext", "type", "size", "modified", "created", "in", "folder", "is",
];

/// What was typed, split into name text and conditions.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileQuery {
    /// The words to fuzzy-match against names, phrases included, joined by
    /// single spaces. Empty when only filters were typed.
    pub text: String,
    /// Quoted phrases (lower case) the name must contain.
    pub phrases: Vec<String>,
    /// Words and phrases (lower case) the name must not contain.
    pub excluded: Vec<String>,
    pub filters: Vec<Filter>,
    /// Every condition as typed, in order, for the result subtitle.
    pub labels: Vec<String>,
}

/// One `key:value` condition, possibly negated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    pub negated: bool,
    pub test: Test,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Test {
    /// Lower-case extensions without the leading dot (`pdf`, `tar.gz`).
    Ext(Vec<String>),
    Size(SizeRange),
    Modified(TimeRange),
    Created(TimeRange),
    Location(Scope),
    Kind(Kind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Folder,
    File,
}

/// Inclusive byte bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SizeRange {
    pub min: u64,
    pub max: u64,
}

impl SizeRange {
    pub fn contains(&self, size: u64) -> bool {
        self.min <= size && size <= self.max
    }
}

/// Unix seconds, `from` inclusive and `until` exclusive; `None` is unbounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeRange {
    pub from: Option<i64>,
    pub until: Option<i64>,
}

impl TimeRange {
    pub fn contains(&self, time: i64) -> bool {
        self.from.is_none_or(|from| time >= from) && self.until.is_none_or(|until| time < until)
    }
}

/// Where an entry must be for `in:` / `folder:`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// Somewhere below this folder (an absolute or `~` path).
    Below(PathBuf),
    /// Inside a folder with this name, or this run of folder names
    /// (`src/components`), anywhere; ASCII case is ignored.
    Named(Vec<String>),
}

/// A filter that cannot be used, and what would work instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterError {
    /// The offending part of the query, as typed.
    pub token: String,
    pub message: String,
}

impl fmt::Display for FilterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.token, self.message)
    }
}

/// What [`FileQuery::matches`] looks at. Sizes and times are `None` when they
/// are unknown, which fails any filter that needs them.
#[derive(Debug, Clone, Copy)]
pub struct Candidate<'a> {
    pub name: &'a str,
    pub path: &'a Path,
    pub is_dir: bool,
    pub size: Option<u64>,
    pub modified: Option<i64>,
    pub created: Option<i64>,
}

impl FileQuery {
    /// Nothing but name text: the search works exactly as without filters.
    pub fn is_plain(&self) -> bool {
        self.filters.is_empty() && self.phrases.is_empty() && self.excluded.is_empty()
    }

    /// Whether the query can list entries without name text (`f ext:pdf`):
    /// excluded words alone would list everything else, so they cannot.
    pub fn can_list_without_text(&self) -> bool {
        !self.filters.is_empty() || !self.phrases.is_empty()
    }

    /// Whether a filter needs the size or a time of an entry.
    pub fn needs_metadata(&self) -> bool {
        self.filters
            .iter()
            .any(|f| matches!(f.test, Test::Size(_) | Test::Modified(_) | Test::Created(_)))
    }

    fn has(&self, wanted: fn(&Test) -> bool) -> bool {
        self.filters.iter().any(|f| wanted(&f.test))
    }

    /// The result subtitle should show the size (a size filter is active).
    pub fn shows_size(&self) -> bool {
        self.has(|t| matches!(t, Test::Size(_)))
    }

    pub fn shows_modified(&self) -> bool {
        self.has(|t| matches!(t, Test::Modified(_)))
    }

    pub fn shows_created(&self) -> bool {
        self.has(|t| matches!(t, Test::Created(_)))
    }

    /// The conditions as typed, for the result subtitle.
    pub fn summary(&self) -> String {
        self.labels.join(" ")
    }

    /// The words to send to an OS index. For name searches a single wanted
    /// extension is added as a word (`report.pdf` contains the word `pdf` for
    /// every backend), which narrows the index's answer before the exact
    /// filter is applied; contents searches get the text only.
    pub fn index_words(&self, names: bool) -> String {
        let mut words = self.text.clone();
        if names {
            let wanted: Vec<&Vec<String>> = self
                .filters
                .iter()
                .filter_map(|f| match (&f.test, f.negated) {
                    (Test::Ext(exts), false) => Some(exts),
                    _ => None,
                })
                .collect();
            if let [exts] = wanted.as_slice() {
                if let [ext] = exts.as_slice() {
                    if !ext.contains('.') && ext.chars().any(char::is_alphanumeric) {
                        if !words.is_empty() {
                            words.push(' ');
                        }
                        words.push_str(ext);
                    }
                }
            }
        }
        words
    }

    /// The filters alone (no phrases or excluded words): cheap, no allocation.
    /// `fold_case` compares folder names ignoring ASCII case (Windows, macOS).
    pub fn passes_filters(&self, entry: &Candidate<'_>, fold_case: bool) -> bool {
        self.filters
            .iter()
            .all(|filter| filter.test.matches(entry, fold_case) != filter.negated)
    }

    /// Phrases and excluded words, against the lower-cased name.
    pub fn passes_words(&self, name: &str) -> bool {
        if self.phrases.is_empty() && self.excluded.is_empty() {
            return true;
        }
        let lower = name.to_lowercase();
        self.phrases.iter().all(|p| lower.contains(p.as_str()))
            && !self.excluded.iter().any(|x| lower.contains(x.as_str()))
    }

    pub fn matches(&self, entry: &Candidate<'_>, fold_case: bool) -> bool {
        self.passes_filters(entry, fold_case) && self.passes_words(entry.name)
    }
}

impl Test {
    fn matches(&self, entry: &Candidate<'_>, fold_case: bool) -> bool {
        match self {
            Test::Ext(exts) => !entry.is_dir && exts.iter().any(|ext| has_ext(entry.name, ext)),
            Test::Size(range) => !entry.is_dir && entry.size.is_some_and(|s| range.contains(s)),
            Test::Modified(range) => entry.modified.is_some_and(|t| range.contains(t)),
            Test::Created(range) => entry.created.is_some_and(|t| range.contains(t)),
            Test::Location(scope) => scope.contains(entry.path, fold_case),
            Test::Kind(Kind::Folder) => entry.is_dir,
            Test::Kind(Kind::File) => !entry.is_dir,
        }
    }
}

/// `name` ends in `.ext` (ASCII case ignored), with something before the dot.
pub fn has_ext(name: &str, ext: &str) -> bool {
    let (name, ext) = (name.as_bytes(), ext.as_bytes());
    if name.len() < ext.len() + 2 {
        return false;
    }
    let tail = &name[name.len() - ext.len() - 1..];
    tail[0] == b'.' && tail[1..].eq_ignore_ascii_case(ext)
}

fn same_component(a: Component<'_>, b: Component<'_>, fold_case: bool) -> bool {
    if fold_case {
        a.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&b.as_os_str().to_string_lossy())
    } else {
        a.as_os_str() == b.as_os_str()
    }
}

impl Scope {
    /// Whether `path` is strictly inside the scope (the folder itself is not).
    pub fn contains(&self, path: &Path, fold_case: bool) -> bool {
        match self {
            Scope::Below(root) => {
                let mut inside = path.components();
                for part in root.components() {
                    match inside.next() {
                        Some(c) if same_component(c, part, fold_case) => {}
                        _ => return false,
                    }
                }
                inside.next().is_some()
            }
            Scope::Named(names) => {
                let Some(parent) = path.parent() else {
                    return false;
                };
                let same = |c: Component<'_>, name: &str| {
                    c.as_os_str().to_string_lossy().eq_ignore_ascii_case(name)
                };
                // One name (the usual case): a single pass over the folders.
                if let [name] = names.as_slice() {
                    return parent.components().any(|c| same(c, name));
                }
                let count = parent.components().count();
                (0..count).any(|start| {
                    let mut run = parent.components().skip(start);
                    names
                        .iter()
                        .all(|name| run.next().is_some_and(|c| same(c, name)))
                })
            }
        }
    }
}

/// Splits `input` into name text and conditions. `now` (in the user's time
/// zone) anchors `today`, `7d` and dates; `home` expands `in:~/...`.
pub fn parse<Tz: TimeZone>(
    input: &str,
    now: &DateTime<Tz>,
    home: Option<&Path>,
) -> Result<FileQuery, FilterError> {
    let mut query = FileQuery::default();
    let mut words: Vec<String> = Vec::new();

    for raw in tokens(input) {
        let (negated, body) = match raw.strip_prefix('-') {
            Some(rest)
                if rest
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphanumeric() || c == '"') =>
            {
                (true, rest)
            }
            _ => (false, raw.as_str()),
        };

        if let Some(quoted) = body.strip_prefix('"') {
            let phrase = quoted.replace('"', "");
            let phrase = phrase.trim();
            if phrase.is_empty() {
                continue;
            }
            if negated {
                query.excluded.push(phrase.to_lowercase());
            } else {
                query.phrases.push(phrase.to_lowercase());
                words.push(phrase.to_owned());
            }
            query.labels.push(raw.clone());
            continue;
        }

        let filter_key = body
            .split_once(':')
            .map(|(key, value)| (key.to_ascii_lowercase(), value))
            .filter(|(key, _)| KEYS.contains(&key.as_str()));
        let Some((key, value)) = filter_key else {
            if negated {
                query.excluded.push(body.replace('"', "").to_lowercase());
                query.labels.push(raw.clone());
            } else {
                words.push(raw.clone());
            }
            continue;
        };

        let value = value.replace('"', "");
        let value = value.trim();
        let error = |message: String| FilterError {
            token: raw.clone(),
            message,
        };
        if value.is_empty() {
            return Err(error(format!(
                "{key}: needs a value, for example {}",
                example(&key)
            )));
        }
        let test = match key.as_str() {
            "ext" => Test::Ext(parse_exts(value).map_err(error)?),
            "type" => parse_type(value).map_err(error)?,
            "size" => Test::Size(parse_size(value).map_err(error)?),
            "modified" => Test::Modified(parse_time(value, now).map_err(error)?),
            "created" => Test::Created(parse_time(value, now).map_err(error)?),
            "in" | "folder" => Test::Location(parse_scope(value, home)),
            "is" => Test::Kind(parse_kind(value).map_err(error)?),
            _ => unreachable!("every key in KEYS is handled"),
        };
        query.filters.push(Filter { negated, test });
        query.labels.push(raw.clone());
    }
    query.text = words.join(" ");
    Ok(query)
}

fn example(key: &str) -> &'static str {
    match key {
        "ext" => "ext:pdf",
        "type" => "type:image",
        "size" => "size:>10mb",
        "modified" => "modified:7d",
        "created" => "created:today",
        "in" => "in:~/Projects",
        "folder" => "folder:~/Projects",
        _ => "is:folder",
    }
}

/// Whitespace-separated tokens; a double-quoted run keeps its spaces (`"two
/// words"`, `in:"~/My Projects"`). An unclosed quote runs to the end.
fn tokens(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in input.chars() {
        if c == '"' {
            quoted = !quoted;
            current.push(c);
        } else if c.is_whitespace() && !quoted {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn list(value: &str) -> impl Iterator<Item = &str> {
    value
        .split([',', '|'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn parse_exts(value: &str) -> Result<Vec<String>, String> {
    let mut exts = Vec::new();
    for ext in list(value) {
        let ext = ext.trim_start_matches('.').to_lowercase();
        let valid = !ext.is_empty()
            && ext
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-' | '+' | '~'));
        if !valid {
            return Err(format!(
                "“{ext}” is not an extension; write it without wildcards, like ext:pdf or ext:jpg,png"
            ));
        }
        exts.push(ext);
    }
    if exts.is_empty() {
        return Err("ext: needs an extension, like ext:pdf or ext:jpg,png".to_owned());
    }
    Ok(exts)
}

fn type_names() -> String {
    TYPE_GROUPS
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(", ")
}

fn parse_type(value: &str) -> Result<Test, String> {
    let mut exts: Vec<String> = Vec::new();
    let mut kind = None;
    for name in list(value) {
        let name = name.to_lowercase();
        let name = TYPE_ALIASES
            .iter()
            .find(|(alias, _)| *alias == name)
            .map_or(name.as_str(), |(_, group)| group);
        if let Ok(k) = parse_kind(name) {
            kind = Some(k);
            continue;
        }
        let Some((_, group)) = TYPE_GROUPS.iter().find(|(group, _)| *group == name) else {
            return Err(format!(
                "there is no file type “{name}”; use one of {}, or ext: for one extension",
                type_names()
            ));
        };
        exts.extend(group.iter().map(|e| (*e).to_owned()));
    }
    match (kind, exts.is_empty()) {
        (Some(k), true) => Ok(Test::Kind(k)),
        (None, false) => Ok(Test::Ext(exts)),
        (Some(_), false) => Err("type: cannot mix folder or file with file types".to_owned()),
        (None, true) => Err(format!("type: needs one of {}", type_names())),
    }
}

fn parse_kind(value: &str) -> Result<Kind, String> {
    match value.to_lowercase().as_str() {
        "folder" | "folders" | "dir" | "dirs" | "directory" => Ok(Kind::Folder),
        "file" | "files" => Ok(Kind::File),
        other => Err(format!("“{other}” is not a kind; use is:folder or is:file")),
    }
}

fn parse_scope(value: &str, home: Option<&Path>) -> Scope {
    let home_relative = value == "~" || value.starts_with("~/") || value.starts_with("~\\");
    let bytes = value.as_bytes();
    let drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    let rooted = value.starts_with('/') || value.starts_with('\\');
    if (home_relative && home.is_some()) || drive || rooted {
        return Scope::Below(crate::files::expand_home(value, home));
    }
    Scope::Named(
        value
            .split(['/', '\\'])
            .filter(|part| !part.is_empty() && *part != "~")
            .map(str::to_owned)
            .collect(),
    )
}

/// Splits a leading comparison off a value.
fn comparison(value: &str) -> (&str, &str) {
    for op in [">=", "<=", ">", "<", "="] {
        if let Some(rest) = value.strip_prefix(op) {
            return (op, rest.trim());
        }
    }
    ("", value)
}

const KB: f64 = 1024.0;

/// `10mb`, `1.5g`, `300` (bytes), `2 KiB`.
fn parse_bytes(text: &str) -> Option<u64> {
    let text = text.trim();
    let split = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let number: f64 = number.parse().ok()?;
    let factor = match unit.trim() {
        "" | "b" => 1.0,
        "k" | "kb" | "kib" => KB,
        "m" | "mb" | "mib" => KB * KB,
        "g" | "gb" | "gib" => KB * KB * KB,
        "t" | "tb" | "tib" => KB * KB * KB * KB,
        _ => return None,
    };
    let bytes = (number * factor).round();
    (bytes.is_finite() && bytes >= 0.0 && bytes < u64::MAX as f64).then_some(bytes as u64)
}

const SIZE_HELP: &str = "use size:>10mb, size:<500kb, size:>=1gb, size:1mb..20mb or size:empty";

fn parse_size(value: &str) -> Result<SizeRange, String> {
    let value = value.to_lowercase();
    if value == "empty" {
        return Ok(SizeRange { min: 0, max: 0 });
    }
    let bad = || format!("“{value}” is not a size; {SIZE_HELP}");
    if let Some((low, high)) = value.split_once("..") {
        let (min, max) = (
            parse_bytes(low).ok_or_else(bad)?,
            parse_bytes(high).ok_or_else(bad)?,
        );
        if min > max {
            return Err(format!("the range {value} ends before it starts"));
        }
        return Ok(SizeRange { min, max });
    }
    let (op, rest) = comparison(&value);
    let n = parse_bytes(rest).ok_or_else(bad)?;
    match op {
        ">" => Ok(SizeRange {
            min: n.saturating_add(1),
            max: u64::MAX,
        }),
        ">=" => Ok(SizeRange {
            min: n,
            max: u64::MAX,
        }),
        "<" if n == 0 => Err("nothing is smaller than 0 bytes; try size:empty".to_owned()),
        "<" => Ok(SizeRange { min: 0, max: n - 1 }),
        "<=" => Ok(SizeRange { min: 0, max: n }),
        "=" => Ok(SizeRange { min: n, max: n }),
        _ => Err(format!(
            "say whether larger or smaller: size:>{rest} or size:<{rest}"
        )),
    }
}

const TIME_HELP: &str = "use today, yesterday, an age such as 7d, 12h, 2w or 1y (>30d: older), \
     or a date such as 2026-01-01, 2026-01 or 2026 (with >, >=, <, <= or a range 2026-01..2026-03)";

/// The first second of local midnight on `date`, or the first valid local
/// time after it when midnight falls in a daylight-saving gap.
fn day_start<Tz: TimeZone>(tz: &Tz, date: NaiveDate) -> i64 {
    let midnight: NaiveDateTime = date.and_hms_opt(0, 0, 0).unwrap_or_default();
    (0..=3)
        .find_map(|hours| {
            tz.from_local_datetime(&(midnight + TimeDelta::hours(hours)))
                .earliest()
                .map(|t| t.timestamp())
        })
        .unwrap_or_else(|| midnight.and_utc().timestamp())
}

/// A calendar span `[start, end)` named by `text`: `today`, `yesterday`, a
/// day, a month or a year.
fn span<Tz: TimeZone>(text: &str, now: &DateTime<Tz>) -> Option<(i64, i64)> {
    let tz = now.timezone();
    let (first, next) = match text {
        "today" => {
            let today = now.date_naive();
            (today, today.succ_opt()?)
        }
        "yesterday" => {
            let today = now.date_naive();
            (today.pred_opt()?, today)
        }
        _ => {
            let parts: Vec<&str> = text.split('-').collect();
            let number = |s: &str, len: usize| {
                (s.len() == len && s.bytes().all(|b| b.is_ascii_digit()))
                    .then(|| s.parse::<u32>().ok())
                    .flatten()
            };
            match parts.as_slice() {
                [y] => {
                    let first = NaiveDate::from_ymd_opt(number(y, 4)? as i32, 1, 1)?;
                    (first, first.checked_add_months(Months::new(12))?)
                }
                [y, m] => {
                    let first = NaiveDate::from_ymd_opt(number(y, 4)? as i32, number(m, 2)?, 1)?;
                    (first, first.checked_add_months(Months::new(1))?)
                }
                [y, m, d] => {
                    let first = NaiveDate::from_ymd_opt(
                        number(y, 4)? as i32,
                        number(m, 2)?,
                        number(d, 2)?,
                    )?;
                    (first, first.succ_opt()?)
                }
                _ => return None,
            }
        }
    };
    Some((day_start(&tz, first), day_start(&tz, next)))
}

/// An age such as `7d`: seconds.
fn age(text: &str) -> Option<i64> {
    let split = text.find(|c: char| !c.is_ascii_digit())?;
    let (number, unit) = text.split_at(split);
    let n: i64 = number.parse().ok().filter(|n| *n > 0)?;
    let unit = match unit {
        "h" => 3600,
        "d" => 86_400,
        "w" => 7 * 86_400,
        "y" => 365 * 86_400,
        _ => return None,
    };
    n.checked_mul(unit)
}

fn parse_time<Tz: TimeZone>(value: &str, now: &DateTime<Tz>) -> Result<TimeRange, String> {
    let value = value.to_lowercase();
    let bad = || format!("“{value}” is not a time; {TIME_HELP}");
    if let Some((low, high)) = value.split_once("..") {
        let (from, _) = span(low.trim(), now).ok_or_else(bad)?;
        let (_, until) = span(high.trim(), now).ok_or_else(bad)?;
        if from >= until {
            return Err(format!("the range {value} ends before it starts"));
        }
        return Ok(TimeRange {
            from: Some(from),
            until: Some(until),
        });
    }
    let (op, rest) = comparison(&value);
    if let Some(seconds) = age(rest) {
        // An age: `7d` and `<7d` are newer than 7 days, `>7d` older.
        let edge = now.timestamp().saturating_sub(seconds);
        return match op {
            "" | "<" | "<=" => Ok(TimeRange {
                from: Some(edge),
                until: None,
            }),
            ">" | ">=" => Ok(TimeRange {
                from: None,
                until: Some(edge),
            }),
            _ => Err(format!(
                "an age is a range, not a moment; use {rest} (newer) or >{rest} (older)"
            )),
        };
    }
    let (start, end) = span(rest, now).ok_or_else(bad)?;
    let (from, until) = match op {
        "" | "=" => (Some(start), Some(end)),
        ">" => (Some(end), None),
        ">=" => (Some(start), None),
        "<" => (None, Some(start)),
        _ => (None, Some(end)), // "<="
    };
    Ok(TimeRange { from, until })
}

/// `1.2 MB`, `340 KB`, `12 bytes`.
pub fn human_size(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} bytes");
    }
    let units = ["KB", "MB", "GB", "TB"];
    let mut value = bytes as f64 / KB;
    let mut unit = 0;
    while value >= 1024.0 && unit < units.len() - 1 {
        value /= KB;
        unit += 1;
    }
    if value < 10.0 {
        format!("{value:.1} {}", units[unit])
    } else {
        format!("{value:.0} {}", units[unit])
    }
}

#[cfg(test)]
mod tests {
    use chrono::{FixedOffset, TimeZone};

    use super::*;

    /// 2026-11-15 14:30 at UTC+02:00.
    fn now() -> DateTime<FixedOffset> {
        FixedOffset::east_opt(2 * 3600)
            .unwrap()
            .with_ymd_and_hms(2026, 11, 15, 14, 30, 0)
            .unwrap()
    }

    fn at(y: i32, m: u32, d: u32) -> i64 {
        FixedOffset::east_opt(2 * 3600)
            .unwrap()
            .with_ymd_and_hms(y, m, d, 0, 0, 0)
            .unwrap()
            .timestamp()
    }

    fn home() -> PathBuf {
        PathBuf::from(if cfg!(windows) {
            r"C:\Users\me"
        } else {
            "/home/me"
        })
    }

    fn ok(input: &str) -> FileQuery {
        parse(input, &now(), Some(&home())).unwrap_or_else(|e| panic!("{input:?}: {e}"))
    }

    fn err(input: &str) -> FilterError {
        parse(input, &now(), Some(&home())).expect_err(input)
    }

    fn only_test(input: &str) -> (bool, Test) {
        let query = ok(input);
        assert_eq!(query.filters.len(), 1, "{input:?}: {query:?}");
        let filter = query.filters.into_iter().next().unwrap();
        (filter.negated, filter.test)
    }

    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    const DAY: i64 = 86_400;

    #[test]
    fn text_phrases_and_exclusions() {
        // (input, text, phrases, excluded, labels)
        type Case<'a> = (
            &'a str,
            &'a str,
            &'a [&'a str],
            &'a [&'a str],
            &'a [&'a str],
        );
        let cases: &[Case] = &[
            ("report", "report", &[], &[], &[]),
            ("annual   report", "annual report", &[], &[], &[]),
            (
                "\"Annual Report\" 2026",
                "Annual Report 2026",
                &["annual report"],
                &[],
                &["\"Annual Report\""],
            ),
            ("budget -draft", "budget", &[], &["draft"], &["-draft"]),
            (
                "budget -\"old copy\"",
                "budget",
                &[],
                &["old copy"],
                &["-\"old copy\""],
            ),
            (
                "\"unclosed phrase",
                "unclosed phrase",
                &["unclosed phrase"],
                &[],
                &["\"unclosed phrase"],
            ),
            ("a - b", "a - b", &[], &[], &[]),
            ("x -- y", "x -- y", &[], &[], &[]),
            ("\"\"", "", &[], &[], &[]),
            // Unknown keys stay text.
            (
                "12:30 http://x c:temp",
                "12:30 http://x c:temp",
                &[],
                &[],
                &[],
            ),
        ];
        for (input, text, phrases, excluded, labels) in cases {
            let q = ok(input);
            assert_eq!(q.text, *text, "{input:?}");
            assert_eq!(q.phrases, *phrases, "{input:?}");
            assert_eq!(q.excluded, *excluded, "{input:?}");
            assert_eq!(q.labels, *labels, "{input:?}");
            assert!(q.filters.is_empty(), "{input:?}");
        }
        assert!(ok("plain words here").is_plain());
        assert!(!ok("x -y").is_plain());
        assert!(!ok("-y").can_list_without_text());
        assert!(ok("\"y\"").can_list_without_text());
    }

    #[test]
    fn extensions_and_types() {
        let cases: &[(&str, bool, &[&str])] = &[
            ("ext:pdf", false, &["pdf"]),
            ("EXT:PDF", false, &["pdf"]),
            ("ext:.pdf", false, &["pdf"]),
            ("ext:jpg,png", false, &["jpg", "png"]),
            ("ext:jpg|.PNG", false, &["jpg", "png"]),
            ("ext:tar.gz", false, &["tar.gz"]),
            ("-ext:tmp", true, &["tmp"]),
            (
                "type:archive",
                false,
                &[
                    "zip", "rar", "7z", "tar", "gz", "tgz", "bz2", "xz", "zst", "iso", "dmg", "cab",
                ],
            ),
        ];
        for (input, negated, exts) in cases {
            assert_eq!(
                only_test(input),
                (
                    *negated,
                    Test::Ext(exts.iter().map(|e| (*e).to_owned()).collect())
                ),
                "{input:?}"
            );
        }
        for group in [
            "image", "video", "audio", "doc", "code", "archive", "Photos", "music", "docs",
        ] {
            let (_, test) = only_test(&format!("type:{group}"));
            assert!(matches!(test, Test::Ext(ref e) if !e.is_empty()), "{group}");
        }
        let (_, Test::Ext(both)) = only_test("type:image,video") else {
            panic!("type list")
        };
        assert!(both.contains(&"png".to_owned()) && both.contains(&"mkv".to_owned()));
        assert_eq!(only_test("type:folder"), (false, Test::Kind(Kind::Folder)));
    }

    #[test]
    fn sizes() {
        let cases: &[(&str, u64, u64)] = &[
            ("size:>10mb", 10 * MB + 1, u64::MAX),
            ("size:>=10MB", 10 * MB, u64::MAX),
            ("size:<500kb", 0, 500 * KB - 1),
            ("size:<=500k", 0, 500 * KB),
            ("size:=300", 300, 300),
            ("size:>1.5m", (1.5 * MB as f64) as u64 + 1, u64::MAX),
            ("size:>2gib", 2 * 1024 * MB + 1, u64::MAX),
            ("size:1mb..20mb", MB, 20 * MB),
            ("size:empty", 0, 0),
            ("size:>0", 1, u64::MAX),
            ("size:>12b", 13, u64::MAX),
        ];
        for (input, min, max) in cases {
            assert_eq!(
                only_test(input),
                (
                    false,
                    Test::Size(SizeRange {
                        min: *min,
                        max: *max
                    })
                ),
                "{input:?}"
            );
        }
        assert!(only_test("-size:>1mb").0);
    }

    #[test]
    fn times() {
        let now = now().timestamp();
        // (input, from, until)
        let cases: &[(&str, Option<i64>, Option<i64>)] = &[
            (
                "modified:today",
                Some(at(2026, 11, 15)),
                Some(at(2026, 11, 16)),
            ),
            (
                "modified:yesterday",
                Some(at(2026, 11, 14)),
                Some(at(2026, 11, 15)),
            ),
            ("modified:7d", Some(now - 7 * DAY), None),
            ("modified:<7d", Some(now - 7 * DAY), None),
            ("modified:>30d", None, Some(now - 30 * DAY)),
            ("modified:12h", Some(now - 12 * 3600), None),
            ("modified:2w", Some(now - 14 * DAY), None),
            ("modified:1y", Some(now - 365 * DAY), None),
            (
                "modified:2026-01-01",
                Some(at(2026, 1, 1)),
                Some(at(2026, 1, 2)),
            ),
            ("modified:>2026-01-01", Some(at(2026, 1, 2)), None),
            ("modified:>=2026-01-01", Some(at(2026, 1, 1)), None),
            ("modified:<2026-01-01", None, Some(at(2026, 1, 1))),
            ("modified:<=2026-01-01", None, Some(at(2026, 1, 2))),
            (
                "modified:2026-02",
                Some(at(2026, 2, 1)),
                Some(at(2026, 3, 1)),
            ),
            ("modified:2025", Some(at(2025, 1, 1)), Some(at(2026, 1, 1))),
            (
                "modified:2025-12..2026-01",
                Some(at(2025, 12, 1)),
                Some(at(2026, 2, 1)),
            ),
            ("modified:>today", Some(at(2026, 11, 16)), None),
            (
                "created:today",
                Some(at(2026, 11, 15)),
                Some(at(2026, 11, 16)),
            ),
        ];
        for (input, from, until) in cases {
            let (_, test) = only_test(input);
            let range = TimeRange {
                from: *from,
                until: *until,
            };
            let expected = if input.starts_with("created") {
                Test::Created(range)
            } else {
                Test::Modified(range)
            };
            assert_eq!(test, expected, "{input:?}");
        }
    }

    #[test]
    fn locations_and_kinds() {
        let below = |p: PathBuf| (false, Test::Location(Scope::Below(p)));
        let named = |n: &[&str]| {
            (
                false,
                Test::Location(Scope::Named(n.iter().map(|s| (*s).to_owned()).collect())),
            )
        };
        assert_eq!(only_test("in:~/Projects"), below(home().join("Projects")));
        assert_eq!(only_test("folder:~"), below(home()));
        assert_eq!(
            only_test("in:\"~/My Projects\""),
            below(home().join("My Projects"))
        );
        assert_eq!(only_test("in:/srv/data"), below(PathBuf::from("/srv/data")));
        assert_eq!(only_test(r"in:C:\Work"), below(PathBuf::from(r"C:\Work")));
        assert_eq!(only_test("in:src"), named(&["src"]));
        assert_eq!(
            only_test("in:src/components/"),
            named(&["src", "components"])
        );
        assert_eq!(only_test("is:folder"), (false, Test::Kind(Kind::Folder)));
        assert_eq!(only_test("is:dir"), (false, Test::Kind(Kind::Folder)));
        assert_eq!(only_test("IS:File"), (false, Test::Kind(Kind::File)));
        assert_eq!(only_test("-is:folder"), (true, Test::Kind(Kind::Folder)));
        // Without a home folder `~` cannot be expanded: it names a folder.
        let q = parse("in:~/Projects", &now(), None).unwrap();
        assert_eq!(
            q.filters[0].test,
            Test::Location(Scope::Named(vec!["Projects".into()]))
        );
    }

    #[test]
    fn combined_filters_keep_text_and_labels_in_order() {
        let q = ok("report ext:pdf -in:archive size:>1mb \"q3\" modified:7d");
        assert_eq!(q.text, "report q3");
        assert_eq!(q.filters.len(), 4);
        assert_eq!(
            q.summary(),
            "ext:pdf -in:archive size:>1mb \"q3\" modified:7d"
        );
        assert!(q.needs_metadata() && q.shows_size() && q.shows_modified());
        assert!(!q.shows_created());
        assert!(!ok("ext:pdf in:x is:file").needs_metadata());
    }

    #[test]
    fn invalid_filters_explain_themselves() {
        // (input, words the message must contain)
        let cases: &[(&str, &str)] = &[
            ("ext:", "needs a value, for example ext:pdf"),
            ("type:", "type:image"),
            ("size:", "size:>10mb"),
            ("ext:*.pdf", "without wildcards"),
            ("ext:,", "needs an extension"),
            (
                "type:spreadsheet",
                "image, video, audio, doc, code, archive",
            ),
            ("type:image,folder", "cannot mix"),
            ("size:big", "not a size"),
            ("size:10mb", "size:>10mb or size:<10mb"),
            ("size:<0", "size:empty"),
            ("size:10zb", "not a size"),
            ("size:20mb..1mb", "ends before it starts"),
            ("modified:soon", "not a time"),
            ("modified:2026-13-01", "not a time"),
            ("modified:2026-1-1", "not a time"),
            ("modified:0d", "not a time"),
            ("modified:7x", "not a time"),
            ("modified:=7d", "an age is a range"),
            ("modified:2026-03..2026-01", "ends before it starts"),
            ("created:", "created:today"),
            ("is:link", "is:folder or is:file"),
            ("in:\"\"", "in:~/Projects"),
        ];
        for (input, needle) in cases {
            let e = err(input);
            assert!(
                e.message.contains(needle),
                "{input:?}: {:?} lacks {needle:?}",
                e.message
            );
            assert_eq!(e.token, *input);
        }
        // The first bad filter is reported, with the rest of the query ignored.
        assert_eq!(err("report size:huge ext:pdf").token, "size:huge");
    }

    fn file<'a>(name: &'a str, path: &'a Path) -> Candidate<'a> {
        Candidate {
            name,
            path,
            is_dir: false,
            size: Some(2 * MB),
            modified: Some(now().timestamp() - DAY),
            created: None,
        }
    }

    #[test]
    fn matching_entries() {
        let path = home().join("Projects").join("Q3 Report.PDF");
        let pdf = file("Q3 Report.PDF", &path);
        let folder = Candidate {
            is_dir: true,
            size: Some(0),
            ..pdf
        };
        // (query, matches the file, matches it as a folder)
        let cases: &[(&str, bool, bool)] = &[
            ("ext:pdf", true, false),
            ("ext:doc,pdf", true, false),
            ("-ext:pdf", false, true),
            ("ext:df", false, false),
            ("type:doc", true, false),
            ("type:image", false, false),
            ("size:>1mb", true, false),
            ("size:<1mb", false, false),
            ("-size:<1mb", true, true),
            ("modified:7d", true, true),
            ("modified:today", false, false),
            ("modified:yesterday", true, true),
            ("created:7d", false, false), // unknown creation time
            ("in:~/Projects", true, true),
            (
                "in:~/projects",
                cfg!(any(windows, target_os = "macos")),
                cfg!(any(windows, target_os = "macos")),
            ),
            ("in:~/Projects/Q3 Report.PDF", false, false),
            ("in:~/Other", false, false),
            ("in:projects", true, true),
            ("in:me/projects", true, true),
            ("in:me/other", false, false),
            ("is:file", true, false),
            ("is:folder", false, true),
            ("\"q3 rep\"", true, true),
            ("\"q3 x\"", false, false),
            ("-report", false, false),
            ("-draft", true, true),
            ("ext:pdf size:>1mb in:projects modified:7d", true, false),
            ("ext:pdf size:>10mb", false, false),
        ];
        let fold = cfg!(any(windows, target_os = "macos"));
        for (input, as_file, as_folder) in cases {
            let q = ok(input);
            assert_eq!(q.matches(&pdf, fold), *as_file, "{input:?} (file)");
            assert_eq!(q.matches(&folder, fold), *as_folder, "{input:?} (folder)");
        }
    }

    #[test]
    fn extension_matching_needs_a_name_before_the_dot() {
        assert!(has_ext("a.pdf", "pdf"));
        assert!(has_ext("A.PDF", "pdf"));
        assert!(has_ext("x.tar.gz", "tar.gz"));
        assert!(has_ext("x.tar.gz", "gz"));
        assert!(!has_ext(".pdf", "pdf"));
        assert!(!has_ext("pdf", "pdf"));
        assert!(!has_ext("apdf", "pdf"));
        assert!(!has_ext("é.pdf", "df"));
    }

    #[test]
    fn index_words_push_down_one_extension_for_names() {
        assert_eq!(ok("report ext:pdf").index_words(true), "report pdf");
        assert_eq!(ok("report ext:pdf").index_words(false), "report");
        assert_eq!(ok("ext:pdf").index_words(true), "pdf");
        assert_eq!(ok("report ext:jpg,png").index_words(true), "report");
        assert_eq!(ok("report type:image").index_words(true), "report");
        assert_eq!(ok("report -ext:pdf").index_words(true), "report");
        assert_eq!(ok("report ext:tar.gz").index_words(true), "report");
        assert_eq!(ok("report ext:pdf ext:doc").index_words(true), "report");
        assert_eq!(ok("size:>1mb").index_words(true), "");
    }

    #[test]
    fn human_sizes() {
        let cases: &[(u64, &str)] = &[
            (0, "0 bytes"),
            (1023, "1023 bytes"),
            (1024, "1.0 KB"),
            (1536, "1.5 KB"),
            (500 * KB, "500 KB"),
            (12 * MB + MB / 4, "12 MB"),
            (3 * 1024 * MB, "3.0 GB"),
            (5 * 1024 * 1024 * MB, "5.0 TB"),
        ];
        for (bytes, text) in cases {
            assert_eq!(human_size(*bytes), *text, "{bytes}");
        }
    }

    #[test]
    fn a_day_starts_at_local_midnight() {
        let date = NaiveDate::from_ymd_opt(2026, 3, 29).unwrap();
        let tz = FixedOffset::east_opt(2 * 3600).unwrap();
        assert_eq!(day_start(&tz, date), at(2026, 3, 29));
        let utc = chrono::Utc;
        let date = NaiveDate::from_ymd_opt(2026, 3, 29).unwrap();
        assert_eq!(
            day_start(&utc, date),
            utc.with_ymd_and_hms(2026, 3, 29, 0, 0, 0)
                .unwrap()
                .timestamp()
        );
    }
}
