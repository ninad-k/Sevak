//! World clock and time-zone converter, offline: `time`.
//!
//! | You type | You get |
//! |---|---|
//! | `time` | the time here and in each `[clock] cities` entry |
//! | `time tokyo`, `time in london`, `time new` | the current time in the places that match |
//! | `time 3pm` | 3 PM here, in each configured city |
//! | `time 3pm pst`, `time 9:30 tokyo` | that time there, here and in the configured cities |
//! | `time 3pm pst in ist`, `time 14:30 berlin to new york` | the time converted |
//! | `time tokyo to london` | the time there now, and the difference |
//!
//! Enter copies the highlighted answer.
//!
//! # Places
//!
//! A place is a city of the IANA time-zone database (`Tokyo`, `Kolkata`,
//! `Buenos Aires`), a full IANA name (`Europe/Berlin`), or one of the names in
//! [`ALIASES`]: cities that have no zone of their own (`San Francisco`,
//! `Mumbai`), countries with one time zone (`Japan`), and the common
//! abbreviations (`PST`, `IST`, `CET`). An abbreviation stands for its region,
//! daylight saving time included: `3pm pst` in July is 3 PM Pacific *daylight*
//! time, which the answer shows as `PDT`. `CST` is US Central time and `IST`
//! India.
//!
//! # Time zones
//!
//! The rules come from the IANA database compiled into the binary by the
//! `chrono-tz` crate, so nothing is read at run time and installers need no
//! extra files. A time like `3pm` is taken on today's date in its zone (today
//! there, which may differ from today here). A time the clocks skip that day
//! (02:30 when they jump from 02:00 to 03:00) is moved forward by the jump; a
//! time that happens twice (when the clocks go back) is the first one. Both are
//! pointed out in the answer.
//!
//! This computer's own zone is asked of the OS (`iana-time-zone`) once, and
//! again on every [`Plugin::refresh`], so travelling with a laptop is noticed.

use std::collections::HashSet;
use std::sync::{Arc, OnceLock, RwLock};

use chrono::{DateTime, Duration, LocalResult, NaiveDate, NaiveTime, Offset, TimeZone, Utc};
use chrono_tz::{OffsetName, Tz, TZ_VARIANTS};
use sevak_core::config::ClockConfig;
use sevak_core::model::score;
use sevak_core::{Action, IconSource, Plugin, PluginResult, ResultItem};
use sevak_platform::PlatformProvider;

use crate::actions::execute_action;

pub const KEYWORD: &str = "time";
const ID: &str = "clock";
const PAYLOAD_NOTHING: &str = "nothing";
/// Places listed for a partial name (`time san`).
const MAX_MATCHES: usize = 8;
/// Longer input is not a time or a place.
const MAX_INPUT_CHARS: usize = 120;
/// What the local row is called in titles (`15:00 local time = ...`).
const LOCAL_NAME: &str = "local time";

/// Names that are not (only) a city of the IANA database: `(name, zone,
/// display name)`. Earlier entries win over later ones and over the database's
/// own city names, so `cst` is US Central time and not one of the zones the
/// database names after that abbreviation.
pub const ALIASES: &[(&str, &str, &str)] = &[
    // Abbreviations; each stands for its region, daylight saving time included.
    ("utc", "UTC", "UTC"),
    ("gmt", "Etc/GMT", "GMT"),
    ("z", "UTC", "UTC"),
    ("zulu", "UTC", "UTC"),
    ("pst", "America/Los_Angeles", "Pacific Time"),
    ("pdt", "America/Los_Angeles", "Pacific Time"),
    ("pt", "America/Los_Angeles", "Pacific Time"),
    ("pacific", "America/Los_Angeles", "Pacific Time"),
    ("mst", "America/Denver", "Mountain Time"),
    ("mdt", "America/Denver", "Mountain Time"),
    ("mt", "America/Denver", "Mountain Time"),
    ("mountain", "America/Denver", "Mountain Time"),
    ("cst", "America/Chicago", "Central Time"),
    ("cdt", "America/Chicago", "Central Time"),
    ("ct", "America/Chicago", "Central Time"),
    ("central", "America/Chicago", "Central Time"),
    ("est", "America/New_York", "Eastern Time"),
    ("edt", "America/New_York", "Eastern Time"),
    ("et", "America/New_York", "Eastern Time"),
    ("eastern", "America/New_York", "Eastern Time"),
    ("akst", "America/Anchorage", "Alaska Time"),
    ("akdt", "America/Anchorage", "Alaska Time"),
    ("hst", "Pacific/Honolulu", "Hawaii"),
    ("bst", "Europe/London", "UK"),
    ("wet", "Europe/Lisbon", "Western European Time"),
    ("west", "Europe/Lisbon", "Western European Time"),
    ("cet", "Europe/Paris", "Central European Time"),
    ("cest", "Europe/Paris", "Central European Time"),
    ("eet", "Europe/Athens", "Eastern European Time"),
    ("eest", "Europe/Athens", "Eastern European Time"),
    ("msk", "Europe/Moscow", "Moscow"),
    ("ist", "Asia/Kolkata", "India"),
    ("pkt", "Asia/Karachi", "Pakistan"),
    ("npt", "Asia/Kathmandu", "Nepal"),
    ("ict", "Asia/Bangkok", "Indochina Time"),
    ("wib", "Asia/Jakarta", "Jakarta"),
    ("sgt", "Asia/Singapore", "Singapore"),
    ("hkt", "Asia/Hong_Kong", "Hong Kong"),
    ("pht", "Asia/Manila", "Philippines"),
    ("jst", "Asia/Tokyo", "Japan"),
    ("kst", "Asia/Seoul", "Korea"),
    ("awst", "Australia/Perth", "Perth"),
    ("acst", "Australia/Adelaide", "Adelaide"),
    ("acdt", "Australia/Adelaide", "Adelaide"),
    ("aest", "Australia/Sydney", "Sydney"),
    ("aedt", "Australia/Sydney", "Sydney"),
    ("nzst", "Pacific/Auckland", "New Zealand"),
    ("nzdt", "Pacific/Auckland", "New Zealand"),
    ("gst", "Asia/Dubai", "Gulf"),
    ("sast", "Africa/Johannesburg", "South Africa"),
    ("wat", "Africa/Lagos", "West Africa"),
    ("eat", "Africa/Nairobi", "East Africa"),
    ("brt", "America/Sao_Paulo", "Brasília"),
    ("art", "America/Argentina/Buenos_Aires", "Argentina"),
    // North America.
    ("san francisco", "America/Los_Angeles", "San Francisco"),
    ("sf", "America/Los_Angeles", "San Francisco"),
    ("seattle", "America/Los_Angeles", "Seattle"),
    ("san diego", "America/Los_Angeles", "San Diego"),
    ("san jose", "America/Los_Angeles", "San Jose"),
    ("palo alto", "America/Los_Angeles", "Palo Alto"),
    ("las vegas", "America/Los_Angeles", "Las Vegas"),
    ("la", "America/Los_Angeles", "Los Angeles"),
    ("salt lake city", "America/Denver", "Salt Lake City"),
    ("dallas", "America/Chicago", "Dallas"),
    ("houston", "America/Chicago", "Houston"),
    ("austin", "America/Chicago", "Austin"),
    ("minneapolis", "America/Chicago", "Minneapolis"),
    ("new orleans", "America/Chicago", "New Orleans"),
    ("new york city", "America/New_York", "New York"),
    ("nyc", "America/New_York", "New York"),
    ("ny", "America/New_York", "New York"),
    ("boston", "America/New_York", "Boston"),
    ("washington", "America/New_York", "Washington"),
    ("washington dc", "America/New_York", "Washington"),
    ("dc", "America/New_York", "Washington"),
    ("philadelphia", "America/New_York", "Philadelphia"),
    ("atlanta", "America/New_York", "Atlanta"),
    ("miami", "America/New_York", "Miami"),
    ("montreal", "America/Toronto", "Montreal"),
    ("ottawa", "America/Toronto", "Ottawa"),
    ("calgary", "America/Edmonton", "Calgary"),
    // Europe.
    ("manchester", "Europe/London", "Manchester"),
    ("edinburgh", "Europe/London", "Edinburgh"),
    ("birmingham", "Europe/London", "Birmingham"),
    ("munich", "Europe/Berlin", "Munich"),
    ("frankfurt", "Europe/Berlin", "Frankfurt"),
    ("hamburg", "Europe/Berlin", "Hamburg"),
    ("cologne", "Europe/Berlin", "Cologne"),
    ("milan", "Europe/Rome", "Milan"),
    ("florence", "Europe/Rome", "Florence"),
    ("venice", "Europe/Rome", "Venice"),
    ("barcelona", "Europe/Madrid", "Barcelona"),
    ("geneva", "Europe/Zurich", "Geneva"),
    ("lyon", "Europe/Paris", "Lyon"),
    ("rotterdam", "Europe/Amsterdam", "Rotterdam"),
    ("the hague", "Europe/Amsterdam", "The Hague"),
    ("krakow", "Europe/Warsaw", "Kraków"),
    ("st petersburg", "Europe/Moscow", "St Petersburg"),
    ("saint petersburg", "Europe/Moscow", "St Petersburg"),
    ("porto", "Europe/Lisbon", "Porto"),
    ("antwerp", "Europe/Brussels", "Antwerp"),
    // Asia and Oceania.
    ("mumbai", "Asia/Kolkata", "Mumbai"),
    ("bombay", "Asia/Kolkata", "Mumbai"),
    ("delhi", "Asia/Kolkata", "Delhi"),
    ("new delhi", "Asia/Kolkata", "New Delhi"),
    ("bangalore", "Asia/Kolkata", "Bengaluru"),
    ("bengaluru", "Asia/Kolkata", "Bengaluru"),
    ("chennai", "Asia/Kolkata", "Chennai"),
    ("hyderabad", "Asia/Kolkata", "Hyderabad"),
    ("pune", "Asia/Kolkata", "Pune"),
    ("beijing", "Asia/Shanghai", "Beijing"),
    ("shenzhen", "Asia/Shanghai", "Shenzhen"),
    ("guangzhou", "Asia/Shanghai", "Guangzhou"),
    ("osaka", "Asia/Tokyo", "Osaka"),
    ("kyoto", "Asia/Tokyo", "Kyoto"),
    ("busan", "Asia/Seoul", "Busan"),
    ("abu dhabi", "Asia/Dubai", "Abu Dhabi"),
    ("tel aviv", "Asia/Jerusalem", "Tel Aviv"),
    ("hanoi", "Asia/Ho_Chi_Minh", "Hanoi"),
    ("saigon", "Asia/Ho_Chi_Minh", "Ho Chi Minh City"),
    ("islamabad", "Asia/Karachi", "Islamabad"),
    ("lahore", "Asia/Karachi", "Lahore"),
    ("bali", "Asia/Makassar", "Bali"),
    ("canberra", "Australia/Sydney", "Canberra"),
    ("wellington", "Pacific/Auckland", "Wellington"),
    // Africa and South America.
    ("cape town", "Africa/Johannesburg", "Cape Town"),
    ("rio", "America/Sao_Paulo", "Rio de Janeiro"),
    ("rio de janeiro", "America/Sao_Paulo", "Rio de Janeiro"),
    ("brasilia", "America/Sao_Paulo", "Brasília"),
    // Countries with one time zone (or one most people mean).
    ("uk", "Europe/London", "UK"),
    ("england", "Europe/London", "England"),
    ("britain", "Europe/London", "UK"),
    ("united kingdom", "Europe/London", "UK"),
    ("ireland", "Europe/Dublin", "Ireland"),
    ("germany", "Europe/Berlin", "Germany"),
    ("france", "Europe/Paris", "France"),
    ("italy", "Europe/Rome", "Italy"),
    ("spain", "Europe/Madrid", "Spain"),
    ("portugal", "Europe/Lisbon", "Portugal"),
    ("netherlands", "Europe/Amsterdam", "Netherlands"),
    ("belgium", "Europe/Brussels", "Belgium"),
    ("switzerland", "Europe/Zurich", "Switzerland"),
    ("austria", "Europe/Vienna", "Austria"),
    ("sweden", "Europe/Stockholm", "Sweden"),
    ("norway", "Europe/Oslo", "Norway"),
    ("denmark", "Europe/Copenhagen", "Denmark"),
    ("finland", "Europe/Helsinki", "Finland"),
    ("poland", "Europe/Warsaw", "Poland"),
    ("czechia", "Europe/Prague", "Czechia"),
    ("hungary", "Europe/Budapest", "Hungary"),
    ("romania", "Europe/Bucharest", "Romania"),
    ("greece", "Europe/Athens", "Greece"),
    ("ukraine", "Europe/Kyiv", "Ukraine"),
    ("turkey", "Europe/Istanbul", "Turkey"),
    ("israel", "Asia/Jerusalem", "Israel"),
    ("uae", "Asia/Dubai", "UAE"),
    ("saudi arabia", "Asia/Riyadh", "Saudi Arabia"),
    ("egypt", "Africa/Cairo", "Egypt"),
    ("nigeria", "Africa/Lagos", "Nigeria"),
    ("kenya", "Africa/Nairobi", "Kenya"),
    ("south africa", "Africa/Johannesburg", "South Africa"),
    ("india", "Asia/Kolkata", "India"),
    ("pakistan", "Asia/Karachi", "Pakistan"),
    ("bangladesh", "Asia/Dhaka", "Bangladesh"),
    ("nepal", "Asia/Kathmandu", "Nepal"),
    ("sri lanka", "Asia/Colombo", "Sri Lanka"),
    ("china", "Asia/Shanghai", "China"),
    ("taiwan", "Asia/Taipei", "Taiwan"),
    ("japan", "Asia/Tokyo", "Japan"),
    ("korea", "Asia/Seoul", "Korea"),
    ("south korea", "Asia/Seoul", "South Korea"),
    ("thailand", "Asia/Bangkok", "Thailand"),
    ("vietnam", "Asia/Ho_Chi_Minh", "Vietnam"),
    ("philippines", "Asia/Manila", "Philippines"),
    ("malaysia", "Asia/Kuala_Lumpur", "Malaysia"),
    ("new zealand", "Pacific/Auckland", "New Zealand"),
    ("argentina", "America/Argentina/Buenos_Aires", "Argentina"),
    ("colombia", "America/Bogota", "Colombia"),
    ("peru", "America/Lima", "Peru"),
    ("chile", "America/Santiago", "Chile"),
];

/// IANA areas whose last name part is a city worth matching on its own.
const CITY_AREAS: &[&str] = &[
    "Africa",
    "America",
    "Antarctica",
    "Asia",
    "Atlantic",
    "Australia",
    "Europe",
    "Indian",
    "Pacific",
];

/// A place the user can name, and its time zone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub name: String,
    pub tz: Tz,
}

struct Entry {
    /// [`normalize`]d name.
    key: String,
    place: Place,
    /// From [`ALIASES`]: well-known names, offered first for a partial name.
    curated: bool,
}

/// Every name a place can be found by, built once (about a thousand entries).
fn index() -> &'static [Entry] {
    static INDEX: OnceLock<Vec<Entry>> = OnceLock::new();
    INDEX.get_or_init(build_index)
}

fn build_index() -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut add = |key: String, name: &str, tz: Tz, curated: bool| {
        if !key.is_empty() && seen.insert(key.clone()) {
            entries.push(Entry {
                key,
                curated,
                place: Place {
                    name: name.to_owned(),
                    tz,
                },
            });
        }
    };
    for (alias, zone, name) in ALIASES {
        if let Ok(tz) = zone.parse::<Tz>() {
            add(normalize(alias), name, tz, true);
        }
    }
    // Cities first, then full names, so `tokyo` is found before `asia/tokyo`
    // when a prefix matches both.
    for tz in TZ_VARIANTS {
        let id = tz.name();
        let Some((area, _)) = id.split_once('/') else {
            continue;
        };
        if CITY_AREAS.contains(&area) {
            let city = city_name(id);
            add(normalize(&city), &city, tz, false);
        }
    }
    for tz in TZ_VARIANTS {
        let id = tz.name();
        // `Etc/GMT+5` is UTC-5 (POSIX signs); offering it would mislead.
        if id.contains('/') && !id.starts_with("Etc/") {
            add(normalize(id), &city_name(id), tz, false);
        }
    }
    entries
}

/// `America/Argentina/Buenos_Aires` -> `Buenos Aires`.
fn city_name(id: &str) -> String {
    id.rsplit('/').next().unwrap_or(id).replace('_', " ")
}

/// Lowercase, accents folded, `_` as a space, runs of spaces as one, and
/// surrounding punctuation dropped: how names are compared.
fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        let c = match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ø' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            '_' | '\t' => ' ',
            other => other,
        };
        if c == ' ' && (out.is_empty() || out.ends_with(' ')) {
            continue;
        }
        out.push(c);
    }
    out.trim_matches(|c: char| c == ' ' || c == '?' || c == ',' || c == '.' || c == '!')
        .to_owned()
}

/// The place `name` means exactly, if any.
pub fn find_place(name: &str) -> Option<Place> {
    let key = normalize(name);
    index()
        .iter()
        .find(|entry| entry.key == key)
        .map(|entry| entry.place.clone())
}

/// Places for a name being typed: the exact match alone, or else those whose
/// name starts with it (well-known names first, then the shortest), then those
/// with a word that does.
pub fn match_places(name: &str) -> Vec<Place> {
    let key = normalize(name);
    if key.is_empty() {
        return Vec::new();
    }
    if let Some(place) = find_place(&key) {
        return vec![place];
    }
    let mut prefix: Vec<&Entry> = index().iter().filter(|e| e.key.starts_with(&key)).collect();
    prefix
        .sort_by(|a, b| (!a.curated, a.key.len(), &a.key).cmp(&(!b.curated, b.key.len(), &b.key)));
    let word = format!(" {key}");
    let words = index()
        .iter()
        .filter(|e| !e.key.starts_with(&key) && e.key.contains(&word));
    let mut places: Vec<Place> = Vec::new();
    for entry in prefix.into_iter().chain(words) {
        if !places.contains(&entry.place) {
            places.push(entry.place.clone());
            if places.len() == MAX_MATCHES {
                break;
            }
        }
    }
    places
}

/// The best place for a name in a conversion: the exact one, or else the
/// first that [`match_places`] offers.
fn best_place(name: &str) -> Option<Place> {
    match_places(name).into_iter().next()
}

/// A time of day in a query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    Now,
    At(NaiveTime),
}

/// What the text after the keyword asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// `time`: here and the configured cities.
    List,
    /// `time tokyo`: the places matching a name.
    Show(String),
    /// `time 3pm pst in ist`. An empty `from` is here; an empty `to` is here
    /// and the configured cities.
    Convert {
        when: When,
        from: String,
        to: String,
    },
}

const SEPARATORS: &[&str] = &["in", "to", "into", ">", "="];

impl Request {
    pub fn parse(input: &str) -> Self {
        let text: String = input.chars().take(MAX_INPUT_CHARS).collect();
        let text = text
            .replace("->", " to ")
            .replace("=>", " to ")
            .replace('→', " to ");
        let text = normalize(&text);
        if text.is_empty() {
            return Self::List;
        }
        if let Some((when, rest)) = parse_time(&text) {
            let (from, to) = split_places(rest);
            return match (when, from.is_empty(), to.is_empty()) {
                (When::Now, true, true) => Self::List,
                (When::Now, true, false) => Self::Show(to),
                _ => Self::Convert { when, from, to },
            };
        }
        let (from, to) = split_places(&text);
        if from.is_empty() {
            // `in london`, or a bare separator while typing.
            return if to.is_empty() {
                Self::Show(text)
            } else {
                Self::Show(to)
            };
        }
        if to.is_empty() {
            Self::Show(from)
        } else {
            Self::Convert {
                when: When::Now,
                from,
                to,
            }
        }
    }
}

/// `pst in ist` -> (`pst`, `ist`); `in ist` -> (``, `ist`); `pst` -> (`pst`, ``).
fn split_places(text: &str) -> (String, String) {
    let words: Vec<&str> = text.split_whitespace().collect();
    match words.iter().position(|word| SEPARATORS.contains(word)) {
        Some(at) => (words[..at].join(" "), words[at + 1..].join(" ")),
        None => (words.join(" "), String::new()),
    }
}

/// A time of day at the start of `text` (normalized) and what follows it:
/// `now`, `noon`, `midnight`, `15`, `15:30`, `1530`, `3pm`, `3 pm`, `3:30p.m.`,
/// `15h`, `9.45`.
fn parse_time(text: &str) -> Option<(When, &str)> {
    let (first, rest) = split_word(text);
    match first {
        "now" => return Some((When::Now, rest)),
        "noon" | "midday" => return Some((When::At(NaiveTime::from_hms_opt(12, 0, 0)?), rest)),
        "midnight" => return Some((When::At(NaiveTime::MIN), rest)),
        _ => {}
    }
    let digits_end = first
        .find(|c: char| !(c.is_ascii_digit() || c == ':' || c == '.'))
        .unwrap_or(first.len());
    let (number, mut suffix) = first.split_at(digits_end);
    if number.is_empty() || !number.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let mut rest = rest;
    if suffix.is_empty() {
        let (next, after) = split_word(rest);
        if meridiem(next).is_some() {
            suffix = next;
            rest = after;
        }
    }
    let pm = match suffix {
        "" | "h" => None,
        other => Some(meridiem(other)?),
    };
    let (hour, minute) = match number.split_once([':', '.']) {
        Some((h, m)) if (1..=2).contains(&h.len()) && m.len() == 2 => {
            (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?)
        }
        Some(_) => return None,
        None if number.len() <= 2 => (number.parse().ok()?, 0),
        None if number.len() <= 4 => {
            let (h, m) = number.split_at(number.len() - 2);
            (h.parse().ok()?, m.parse().ok()?)
        }
        None => return None,
    };
    let hour = match pm {
        None if hour < 24 => hour,
        Some(pm) if (1..=12).contains(&hour) => (hour % 12) + if pm { 12 } else { 0 },
        _ => return None,
    };
    Some((When::At(NaiveTime::from_hms_opt(hour, minute, 0)?), rest))
}

/// `Some(true)` for pm, `Some(false)` for am.
fn meridiem(word: &str) -> Option<bool> {
    match word {
        "am" | "a" | "a.m" | "a.m." => Some(false),
        "pm" | "p" | "p.m" | "p.m." => Some(true),
        _ => None,
    }
}

fn split_word(text: &str) -> (&str, &str) {
    let text = text.trim_start();
    match text.split_once(' ') {
        Some((word, rest)) => (word, rest.trim_start()),
        None => (text, ""),
    }
}

/// `time` on `date` in `tz`, and a note when the clocks skip it (it is moved
/// forward by the jump) or pass it twice (the first is taken).
fn at_time(tz: Tz, date: NaiveDate, time: NaiveTime) -> (DateTime<Tz>, Option<String>) {
    let naive = date.and_time(time);
    match tz.from_local_datetime(&naive) {
        LocalResult::Single(at) => (at, None),
        LocalResult::Ambiguous(first, _) => (
            first,
            Some(format!(
                "{} happens twice that day (the clocks go back); this is the first",
                time.format("%H:%M")
            )),
        ),
        LocalResult::None => {
            // The offset in force before the jump, applied to the skipped time,
            // lands as far past the jump as the time was past its start.
            let before = tz
                .offset_from_local_datetime(&(naive - Duration::hours(6)))
                .earliest()
                .map_or(0, |offset| offset.fix().local_minus_utc());
            let at = (naive - Duration::seconds(i64::from(before)))
                .and_utc()
                .with_timezone(&tz);
            let note = format!(
                "{} is skipped that day (the clocks go forward); showing {}",
                time.format("%H:%M"),
                at.format("%H:%M")
            );
            (at, Some(note))
        }
    }
}

fn clock24(at: &DateTime<Tz>) -> String {
    at.format("%H:%M").to_string()
}

fn clock12(at: &DateTime<Tz>) -> String {
    at.format("%-I:%M %p").to_string()
}

fn day(at: &DateTime<Tz>) -> String {
    at.format("%a %-d %b").to_string()
}

/// `UTC`, `UTC+9`, `UTC+5:30`, `UTC-3:30`.
fn utc_offset(seconds: i32) -> String {
    if seconds == 0 {
        return "UTC".to_owned();
    }
    let sign = if seconds < 0 { '-' } else { '+' };
    let minutes = seconds.unsigned_abs() / 60;
    match minutes % 60 {
        0 => format!("UTC{sign}{}", minutes / 60),
        m => format!("UTC{sign}{}:{m:02}", minutes / 60),
    }
}

fn offset_seconds(at: &DateTime<Tz>) -> i32 {
    at.offset().fix().local_minus_utc()
}

/// The zone's abbreviation (`JST`, `PDT`), or its UTC offset where the
/// database has only numbers (`+04`).
fn zone_label(at: &DateTime<Tz>) -> String {
    match at.offset().abbreviation() {
        Some(abbr) if abbr.chars().all(|c| c.is_ascii_alphabetic()) => abbr.to_owned(),
        _ => utc_offset(offset_seconds(at)),
    }
}

/// `JST (UTC+9)`, or just `UTC+4` when that is all the label says.
fn zone_with_offset(at: &DateTime<Tz>) -> String {
    let label = zone_label(at);
    let offset = utc_offset(offset_seconds(at));
    if label == offset {
        label
    } else {
        format!("{label} ({offset})")
    }
}

/// How far a zone `seconds` ahead of another is: `9 h ahead`, `5 h 30 min
/// behind`, `same time`.
fn difference(seconds: i32) -> String {
    if seconds == 0 {
        return "same time".to_owned();
    }
    let minutes = seconds.unsigned_abs() / 60;
    let amount = match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    };
    if seconds > 0 {
        format!("{amount} ahead")
    } else {
        format!("{amount} behind")
    }
}

/// ` (next day)`, ` (previous day)`, ` (+2 days)` or nothing.
fn day_shift(from: NaiveDate, to: NaiveDate) -> String {
    match (to - from).num_days() {
        0 => String::new(),
        1 => " (next day)".to_owned(),
        -1 => " (previous day)".to_owned(),
        n => format!(" ({n:+} days)"),
    }
}

/// One answer row, before it becomes a [`ResultItem`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub key: String,
    pub title: String,
    pub subtitle: String,
    /// What Enter copies; `None` for a hint.
    pub copy: Option<String>,
}

impl Row {
    fn hint(title: impl Into<String>, subtitle: impl Into<String>) -> Self {
        Self {
            key: "hint".to_owned(),
            title: title.into(),
            subtitle: subtitle.into(),
            copy: None,
        }
    }
}

/// What the plugin knows when it answers: the instant, this computer's zone
/// and the configured cities.
pub struct Context<'a> {
    pub now: DateTime<Utc>,
    pub local: Tz,
    pub cities: &'a [Place],
    /// `[clock] cities` entries that name no known place.
    pub unknown: &'a [String],
}

impl Context<'_> {
    fn here(&self) -> Place {
        Place {
            name: LOCAL_NAME.to_owned(),
            tz: self.local,
        }
    }

    /// The current time in `place`; `local` marks the row for this computer.
    fn now_row(&self, place: &Place, local: bool) -> Row {
        let at = self.now.with_timezone(&place.tz);
        let here = self.now.with_timezone(&self.local);
        let time = clock24(&at);
        let (title, last) = if local {
            (format!("{time} {LOCAL_NAME}"), place.tz.name().to_owned())
        } else {
            (
                format!("{time} in {}", place.name),
                difference(offset_seconds(&at) - offset_seconds(&here)),
            )
        };
        Row {
            key: if local {
                "local".to_owned()
            } else {
                format!("city:{}", normalize(&place.name))
            },
            subtitle: format!(
                "{} · {} · {} · {last}",
                day(&at),
                clock12(&at),
                zone_with_offset(&at)
            ),
            copy: Some(format!(
                "{time} {} ({}), {}",
                zone_label(&at),
                place.name,
                day(&at)
            )),
            title,
        }
    }

    /// `at` (in `from`'s zone) shown in `to`'s zone.
    fn convert_row(&self, at: &DateTime<Tz>, from: &Place, to: &Place, note: Option<&str>) -> Row {
        let there = at.with_timezone(&to.tz);
        let shift = day_shift(at.date_naive(), there.date_naive());
        let last = match note {
            Some(note) => note.to_owned(),
            None => format!(
                "{} {}",
                to.name,
                difference(offset_seconds(&there) - offset_seconds(at))
            ),
        };
        Row {
            key: format!("to:{}", normalize(&to.name)),
            title: format!(
                "{} {} = {} {}{shift}",
                clock24(at),
                from.name,
                clock24(&there),
                to.name
            ),
            subtitle: format!(
                "{} {} {} → {} {} {} · {last}",
                day(at),
                clock12(at),
                zone_label(at),
                day(&there),
                clock12(&there),
                zone_label(&there)
            ),
            copy: Some(format!(
                "{} {} ({}) = {} {} ({}), {}",
                clock24(at),
                zone_label(at),
                from.name,
                clock24(&there),
                zone_label(&there),
                to.name,
                day(&there)
            )),
        }
    }

    fn unknown_place(name: &str) -> Row {
        Row::hint(
            format!("No place called “{name}”"),
            "Try a city (Tokyo), an abbreviation (PST) or a zone name (Europe/Berlin)",
        )
    }

    /// The rows answering `request`.
    pub fn answer(&self, request: &Request) -> Vec<Row> {
        match request {
            Request::List => {
                let mut rows = vec![self.now_row(&self.here(), true)];
                rows.extend(self.cities.iter().map(|city| self.now_row(city, false)));
                rows.extend(self.unknown.iter().map(|name| {
                    Row::hint(
                        format!("Unknown city in [clock] cities: “{name}”"),
                        "Use a city name, an abbreviation or an IANA zone name",
                    )
                }));
                rows
            }
            Request::Show(name) => {
                let places = match_places(name);
                if places.is_empty() {
                    return vec![Self::unknown_place(name)];
                }
                places
                    .iter()
                    .map(|place| self.now_row(place, false))
                    .collect()
            }
            Request::Convert { when, from, to } => {
                let source = if from.is_empty() {
                    self.here()
                } else {
                    match best_place(from) {
                        Some(place) => place,
                        None => return vec![Self::unknown_place(from)],
                    }
                };
                let (at, note) = match when {
                    When::Now => (self.now.with_timezone(&source.tz), None),
                    When::At(time) => {
                        let today = self.now.with_timezone(&source.tz).date_naive();
                        at_time(source.tz, today, *time)
                    }
                };
                let targets: Vec<Place> = if to.is_empty() {
                    let here = (!from.is_empty()).then(|| self.here());
                    here.into_iter()
                        .chain(self.cities.iter().cloned())
                        .filter(|place| *place != source)
                        .collect()
                } else {
                    match best_place(to) {
                        Some(place) => vec![place],
                        None => return vec![Self::unknown_place(to)],
                    }
                };
                if targets.is_empty() {
                    return vec![Row::hint(
                        "Convert to which place?",
                        "For example: time 3pm pst in ist",
                    )];
                }
                targets
                    .iter()
                    .map(|target| self.convert_row(&at, &source, target, note.as_deref()))
                    .collect()
            }
        }
    }
}

/// This computer's time zone, as the OS names it.
fn system_zone() -> Option<Tz> {
    iana_time_zone::get_timezone().ok()?.parse().ok()
}

/// The world clock (`time`).
pub struct ClockPlugin {
    keyword: String,
    cities: Vec<Place>,
    unknown: Vec<String>,
    /// Asked of the OS on first use and on every refresh.
    local: RwLock<Option<Tz>>,
    platform: Arc<dyn PlatformProvider>,
}

impl ClockPlugin {
    pub fn new(config: &ClockConfig, platform: Arc<dyn PlatformProvider>) -> Self {
        let mut cities = Vec::new();
        let mut unknown = Vec::new();
        for name in &config.cities {
            match best_place(name) {
                Some(place) if !cities.contains(&place) => cities.push(place),
                Some(_) => {}
                None => unknown.push(name.clone()),
            }
        }
        let keyword = match config.keyword.trim() {
            "" => KEYWORD.to_owned(),
            keyword => keyword.to_owned(),
        };
        Self {
            keyword,
            cities,
            unknown,
            local: RwLock::new(None),
            platform,
        }
    }

    fn local_zone(&self) -> Tz {
        if let Some(tz) = *self.local.read().unwrap_or_else(|p| p.into_inner()) {
            return tz;
        }
        let tz = system_zone().unwrap_or(Tz::UTC);
        *self.local.write().unwrap_or_else(|p| p.into_inner()) = Some(tz);
        tz
    }

    /// The answer at a given instant in a given local zone (tests use fixed ones).
    pub fn query_at(&self, input: &str, now: DateTime<Utc>, local: Tz) -> Vec<ResultItem> {
        let context = Context {
            now,
            local,
            cities: &self.cities,
            unknown: &self.unknown,
        };
        context
            .answer(&Request::parse(input))
            .into_iter()
            .enumerate()
            .map(|(position, row)| {
                let action = match &row.copy {
                    Some(text) => Action::CopyText { text: text.clone() },
                    None => Action::Custom {
                        payload: PAYLOAD_NOTHING.to_owned(),
                    },
                };
                ResultItem::new(ID, row.key, row.title, action)
                    .with_subtitle(row.subtitle)
                    .with_icon(IconSource::builtin("clock"))
                    // Keep the order the rows were made in (equal scores sort by title).
                    .with_score(score::KEYWORD - position as f64)
            })
            .collect()
    }
}

impl Plugin for ClockPlugin {
    fn id(&self) -> &str {
        ID
    }

    fn name(&self) -> &str {
        "World clock"
    }

    fn description(&self) -> &str {
        "Type `time` for the time in your cities, `time tokyo` for one place, or `time 3pm pst in ist` to convert; offline. Enter copies it."
    }

    fn keyword(&self) -> Option<&str> {
        Some(&self.keyword)
    }

    fn global(&self) -> bool {
        false
    }

    fn query(&self, input: &str) -> Vec<ResultItem> {
        self.query_at(input, Utc::now(), self.local_zone())
    }

    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        match &item.action {
            Action::Custom { .. } => Ok(()),
            action => execute_action(self.platform.as_ref(), action),
        }
    }

    /// Asks the OS for its time zone again (the computer may have travelled).
    fn refresh(&self) -> PluginResult<()> {
        if let Some(tz) = system_zone() {
            *self.local.write().unwrap_or_else(|p| p.into_inner()) = Some(tz);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::MockPlatform;

    fn utc(text: &str) -> DateTime<Utc> {
        text.parse().unwrap()
    }

    fn tz(name: &str) -> Tz {
        name.parse().unwrap()
    }

    fn clock_with(cities: &[&str]) -> ClockPlugin {
        let config = ClockConfig {
            keyword: "time".to_owned(),
            cities: cities.iter().map(|c| (*c).to_owned()).collect(),
        };
        ClockPlugin::new(&config, MockPlatform::empty())
    }

    fn titles(items: &[ResultItem]) -> Vec<&str> {
        items.iter().map(|item| item.title.as_str()).collect()
    }

    fn at(time: &str) -> When {
        When::At(NaiveTime::parse_from_str(time, "%H:%M").unwrap())
    }

    #[test]
    fn every_alias_names_a_real_zone() {
        for (alias, zone, name) in ALIASES {
            assert!(zone.parse::<Tz>().is_ok(), "{alias} -> {zone}");
            assert_eq!(normalize(alias), *alias, "{alias} is not normalized");
            assert!(!name.is_empty());
        }
    }

    #[test]
    fn places_are_found_by_city_alias_abbreviation_and_zone_name() {
        let zone = |name: &str| find_place(name).map(|place| place.tz.name());
        assert_eq!(zone("Tokyo"), Some("Asia/Tokyo"));
        assert_eq!(zone("  new   york "), Some("America/New_York"));
        assert_eq!(zone("new_york"), Some("America/New_York"));
        assert_eq!(zone("Europe/Berlin"), Some("Europe/Berlin"));
        assert_eq!(zone("buenos aires"), Some("America/Argentina/Buenos_Aires"));
        assert_eq!(zone("São Paulo"), Some("America/Sao_Paulo"));
        assert_eq!(zone("San Francisco"), Some("America/Los_Angeles"));
        assert_eq!(zone("PST"), Some("America/Los_Angeles"));
        assert_eq!(zone("ist"), Some("Asia/Kolkata"));
        assert_eq!(zone("cst"), Some("America/Chicago"));
        assert_eq!(zone("utc"), Some("UTC"));
        assert_eq!(zone("india"), Some("Asia/Kolkata"));
        assert_eq!(zone("nowhere"), None);
        // POSIX-signed zones are not offered.
        assert_eq!(zone("etc/gmt+5"), None);
        assert_eq!(find_place("pst").unwrap().name, "Pacific Time");
        assert_eq!(find_place("los_angeles").unwrap().name, "Los Angeles");
    }

    #[test]
    fn partial_names_list_matches() {
        let names: Vec<String> = match_places("san").into_iter().map(|p| p.name).collect();
        assert!(names.contains(&"San Francisco".to_owned()), "{names:?}");
        assert!(names.len() <= MAX_MATCHES);
        // A word inside a name matches too.
        let names: Vec<String> = match_places("york").into_iter().map(|p| p.name).collect();
        assert!(names.contains(&"New York".to_owned()), "{names:?}");
        // An exact name is the only answer.
        assert_eq!(match_places("paris").len(), 1);
        assert!(match_places("").is_empty());
        assert!(match_places("qqqq").is_empty());
    }

    #[test]
    fn times_parse() {
        let parse = |text: &str| parse_time(text).map(|(when, rest)| (when, rest.to_owned()));
        assert_eq!(parse("3pm"), Some((at("15:00"), String::new())));
        assert_eq!(parse("3 pm pst"), Some((at("15:00"), "pst".to_owned())));
        assert_eq!(parse("3:30p.m. x"), Some((at("15:30"), "x".to_owned())));
        assert_eq!(parse("12am"), Some((at("00:00"), String::new())));
        assert_eq!(parse("12pm"), Some((at("12:00"), String::new())));
        assert_eq!(
            parse("14:30 berlin"),
            Some((at("14:30"), "berlin".to_owned()))
        );
        assert_eq!(parse("9.45"), Some((at("09:45"), String::new())));
        assert_eq!(parse("1530"), Some((at("15:30"), String::new())));
        assert_eq!(parse("15h"), Some((at("15:00"), String::new())));
        assert_eq!(parse("0"), Some((at("00:00"), String::new())));
        assert_eq!(parse("noon"), Some((at("12:00"), String::new())));
        assert_eq!(parse("midnight utc"), Some((at("00:00"), "utc".to_owned())));
        assert_eq!(parse("now"), Some((When::Now, String::new())));
        for bad in [
            "24:00", "13pm", "0am", "3:5", "12:60", "12345", "tokyo", "3x", ":30", "",
        ] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn requests_parse() {
        let convert = |when, from: &str, to: &str| Request::Convert {
            when,
            from: from.to_owned(),
            to: to.to_owned(),
        };
        assert_eq!(Request::parse(""), Request::List);
        assert_eq!(Request::parse("  "), Request::List);
        assert_eq!(Request::parse("now"), Request::List);
        assert_eq!(Request::parse("Tokyo"), Request::Show("tokyo".into()));
        assert_eq!(Request::parse("in London"), Request::Show("london".into()));
        assert_eq!(
            Request::parse("now in tokyo"),
            Request::Show("tokyo".into())
        );
        assert_eq!(Request::parse("3pm"), convert(at("15:00"), "", ""));
        assert_eq!(
            Request::parse("3pm PST in IST"),
            convert(at("15:00"), "pst", "ist")
        );
        assert_eq!(
            Request::parse("3pm in ist"),
            convert(at("15:00"), "", "ist")
        );
        assert_eq!(
            Request::parse("14:30 Berlin to New York"),
            convert(at("14:30"), "berlin", "new york")
        );
        assert_eq!(
            Request::parse("9am tokyo->london"),
            convert(at("09:00"), "tokyo", "london")
        );
        assert_eq!(
            Request::parse("tokyo to london"),
            convert(When::Now, "tokyo", "london")
        );
    }

    #[test]
    fn lists_here_and_the_configured_cities() {
        let plugin = clock_with(&["London", "New York", "Tokyo", "Atlantis"]);
        let items = plugin.query_at("", utc("2026-07-01T12:00:00Z"), tz("Europe/Berlin"));
        assert_eq!(
            titles(&items),
            [
                "14:00 local time",
                "13:00 in London",
                "08:00 in New York",
                "21:00 in Tokyo",
                "Unknown city in [clock] cities: “Atlantis”"
            ]
        );
        assert_eq!(
            items[0].subtitle,
            "Wed 1 Jul · 2:00 PM · CEST (UTC+2) · Europe/Berlin"
        );
        assert_eq!(
            items[2].subtitle,
            "Wed 1 Jul · 8:00 AM · EDT (UTC-4) · 6 h behind"
        );
        assert_eq!(
            items[3].subtitle,
            "Wed 1 Jul · 9:00 PM · JST (UTC+9) · 7 h ahead"
        );
        assert_eq!(
            items[3].action,
            Action::CopyText {
                text: "21:00 JST (Tokyo), Wed 1 Jul".into()
            }
        );
        assert!(matches!(items[4].action, Action::Custom { .. }));
        // Rows keep their order.
        assert!(items.windows(2).all(|pair| pair[0].score > pair[1].score));
        assert_eq!(items[3].id, "clock:city:tokyo");
    }

    #[test]
    fn shows_one_place_with_half_hour_offsets() {
        let plugin = clock_with(&[]);
        let items = plugin.query_at(
            "in mumbai",
            utc("2026-01-15T12:00:00Z"),
            tz("Europe/London"),
        );
        assert_eq!(titles(&items), ["17:30 in Mumbai"]);
        assert_eq!(
            items[0].subtitle,
            "Thu 15 Jan · 5:30 PM · IST (UTC+5:30) · 5 h 30 min ahead"
        );
        let items = plugin.query_at("kathmandu", utc("2026-01-15T12:00:00Z"), tz("UTC"));
        assert_eq!(titles(&items), ["17:45 in Kathmandu"]);
        let items = plugin.query_at("atlantis", utc("2026-01-15T12:00:00Z"), tz("UTC"));
        assert_eq!(titles(&items), ["No place called “atlantis”"]);
    }

    #[test]
    fn converts_with_daylight_saving_time() {
        let plugin = clock_with(&[]);
        let berlin = tz("Europe/Berlin");
        // Winter: PST is UTC-8.
        let items = plugin.query_at("3pm pst in ist", utc("2026-01-15T12:00:00Z"), berlin);
        assert_eq!(
            titles(&items),
            ["15:00 Pacific Time = 04:30 India (next day)"]
        );
        assert_eq!(
            items[0].subtitle,
            "Thu 15 Jan 3:00 PM PST → Fri 16 Jan 4:30 AM IST · India 13 h 30 min ahead"
        );
        assert_eq!(
            items[0].action,
            Action::CopyText {
                text: "15:00 PST (Pacific Time) = 04:30 IST (India), Fri 16 Jan".into()
            }
        );
        // Summer: the same words mean Pacific daylight time, UTC-7.
        let items = plugin.query_at("3pm pst in ist", utc("2026-07-01T12:00:00Z"), berlin);
        assert_eq!(
            titles(&items),
            ["15:00 Pacific Time = 03:30 India (next day)"]
        );
        assert!(
            items[0].subtitle.contains("3:00 PM PDT"),
            "{}",
            items[0].subtitle
        );

        // Between the US (8 March 2026) and EU (29 March) changes the gap is 5 h,
        // not the usual 6.
        let convert = |now: &str| {
            plugin.query_at("14:30 berlin to new york", utc(now), berlin)[0]
                .title
                .clone()
        };
        assert_eq!(
            convert("2026-01-15T12:00:00Z"),
            "14:30 Berlin = 08:30 New York"
        );
        assert_eq!(
            convert("2026-03-15T12:00:00Z"),
            "14:30 Berlin = 09:30 New York"
        );
        assert_eq!(
            convert("2026-07-01T12:00:00Z"),
            "14:30 Berlin = 08:30 New York"
        );
    }

    #[test]
    fn times_the_clocks_skip_or_repeat_are_explained() {
        let plugin = clock_with(&[]);
        // New York springs forward at 02:00 on 8 March 2026.
        let items = plugin.query_at(
            "2:30am new york to utc",
            utc("2026-03-08T15:00:00Z"),
            tz("UTC"),
        );
        assert_eq!(titles(&items), ["03:30 New York = 07:30 UTC"]);
        assert!(
            items[0].subtitle.contains("skipped"),
            "{}",
            items[0].subtitle
        );
        // ... and falls back at 02:00 on 1 November 2026: 01:30 happens twice.
        let items = plugin.query_at(
            "1:30am new york to utc",
            utc("2026-11-01T15:00:00Z"),
            tz("UTC"),
        );
        assert_eq!(titles(&items), ["01:30 New York = 05:30 UTC"]);
        assert!(items[0].subtitle.contains("twice"), "{}", items[0].subtitle);
    }

    #[test]
    fn a_time_alone_is_shown_in_the_configured_cities() {
        let plugin = clock_with(&["Tokyo", "London"]);
        let items = plugin.query_at("9am", utc("2026-01-15T03:00:00Z"), tz("America/New_York"));
        assert_eq!(
            titles(&items),
            [
                "09:00 local time = 23:00 Tokyo",
                "09:00 local time = 14:00 London"
            ]
        );
        // From another place: here first, then the cities (not the source itself).
        let items = plugin.query_at(
            "9am tokyo",
            utc("2026-01-15T03:00:00Z"),
            tz("America/New_York"),
        );
        assert_eq!(
            titles(&items),
            [
                "09:00 Tokyo = 19:00 local time (previous day)",
                "09:00 Tokyo = 00:00 London"
            ]
        );
        // No cities and no target: say what is missing.
        let items = clock_with(&[]).query_at("9am", utc("2026-01-15T03:00:00Z"), tz("UTC"));
        assert_eq!(titles(&items), ["Convert to which place?"]);
    }

    #[test]
    fn place_to_place_now_shows_the_difference() {
        let plugin = clock_with(&[]);
        let items = plugin.query_at("tokyo to london", utc("2026-01-15T12:00:00Z"), tz("UTC"));
        assert_eq!(titles(&items), ["21:00 Tokyo = 12:00 London"]);
        assert!(
            items[0].subtitle.ends_with("London 9 h behind"),
            "{}",
            items[0].subtitle
        );
        let items = plugin.query_at(
            "3pm atlantis in ist",
            utc("2026-01-15T12:00:00Z"),
            tz("UTC"),
        );
        assert_eq!(titles(&items), ["No place called “atlantis”"]);
    }

    #[test]
    fn configured_cities_are_resolved_once_and_deduplicated() {
        let plugin = clock_with(&["Tokyo", "tokyo", "Asia/Tokyo", "JST", "nowhere"]);
        // `JST` is "Japan", a different name for the same zone, so it stays.
        let names: Vec<&str> = plugin.cities.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Tokyo", "Japan"]);
        assert_eq!(plugin.unknown, ["nowhere"]);
    }

    #[test]
    fn metadata_and_execute() {
        let platform = MockPlatform::empty();
        let config = ClockConfig {
            keyword: "tz".into(),
            cities: vec![],
        };
        let plugin = ClockPlugin::new(&config, platform.clone());
        assert_eq!(plugin.id(), "clock");
        assert_eq!(plugin.keyword(), Some("tz"));
        assert!(!plugin.global());
        let item = plugin
            .query_at("tokyo", utc("2026-01-15T12:00:00Z"), tz("UTC"))
            .remove(0);
        plugin.execute(&item).unwrap();
        assert_eq!(
            *platform.clipboard.lock().unwrap(),
            ["21:00 JST (Tokyo), Thu 15 Jan"]
        );
        let hint = plugin
            .query_at("atlantis", utc("2026-01-15T12:00:00Z"), tz("UTC"))
            .remove(0);
        plugin.execute(&hint).unwrap();
        assert_eq!(platform.clipboard.lock().unwrap().len(), 1);
        // The real query works with the real clock and zone.
        assert!(!plugin.query("").is_empty());
    }

    #[test]
    fn long_input_is_cut_and_never_panics() {
        let plugin = clock_with(&["Tokyo"]);
        let now = utc("2026-01-15T12:00:00Z");
        for input in [
            "x".repeat(10_000),
            "99:99".into(),
            "3pm to".into(),
            "to".into(),
            "in".into(),
            "→".into(),
            "12:30 > > >".into(),
            "1 2 3 4".into(),
        ] {
            let _ = plugin.query_at(&input, now, tz("UTC"));
        }
    }
}
