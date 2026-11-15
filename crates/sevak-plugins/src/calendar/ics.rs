//! A small iCalendar (RFC 5545) reader: just what an agenda needs.
//!
//! Lines are unfolded, content lines split into name, parameters and value,
//! and each `VEVENT` (at any depth below `VCALENDAR`, without the `VALARM`s
//! inside it) becomes an [`Event`]. Times keep their `TZID`: IANA names
//! (`Europe/Berlin`, also behind a vendor prefix such as
//! `/mozilla.org/20050126_1/Europe/Berlin`) and the common Windows names
//! (`W. Europe Standard Time`, as Outlook writes them) go through `chrono-tz`;
//! an unknown zone is read as local time. `VTIMEZONE` definitions are not
//! needed for that and are skipped.
//!
//! Anything malformed is skipped, never an error: one broken event does not
//! hide the rest of the calendar.

use std::str::FromStr;

use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime};
use chrono_tz::Tz;

use super::recur::{parse_rrule, RRule};

/// Events kept from one file.
const MAX_EVENTS_PER_FILE: usize = 20_000;
/// Longest text kept from a description.
const MAX_TEXT_CHARS: usize = 2_000;

/// One content line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Property {
    /// Upper case.
    pub name: String,
    /// Names upper case; values without their quotes.
    pub params: Vec<(String, String)>,
    pub value: String,
}

impl Property {
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// How a time is meant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Utc,
    /// No zone: the same wall-clock time wherever you are.
    Floating,
    Named(Tz),
}

/// A `DATE` or `DATE-TIME` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    Date(NaiveDate),
    DateTime(NaiveDateTime, Zone),
}

impl When {
    pub fn is_date(&self) -> bool {
        matches!(self, Self::Date(_))
    }

    /// The wall-clock time (midnight for a date).
    pub fn naive(&self) -> NaiveDateTime {
        match *self {
            Self::Date(date) => date.and_time(NaiveTime::MIN),
            Self::DateTime(time, _) => time,
        }
    }

    pub fn zone(&self) -> Zone {
        match *self {
            Self::Date(_) => Zone::Floating,
            Self::DateTime(_, zone) => zone,
        }
    }
}

/// A `VEVENT`, with what the agenda shows.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub uid: Option<String>,
    pub summary: String,
    pub location: Option<String>,
    pub description: Option<String>,
    pub url: Option<String>,
    pub start: When,
    /// Length: `DTEND - DTSTART`, `DURATION`, or one day for an all-day
    /// event and nothing for a timed one without either.
    pub duration: Duration,
    pub rrule: Option<RRule>,
    pub exdates: Vec<When>,
    pub rdates: Vec<When>,
    pub recurrence_id: Option<When>,
    pub cancelled: bool,
}

/// Joins folded lines (a line starting with a space or tab continues the one
/// before it).
pub fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        match line.strip_prefix([' ', '\t']) {
            Some(rest) if !lines.is_empty() => {
                if let Some(last) = lines.last_mut() {
                    last.push_str(rest);
                }
            }
            _ => {
                if !line.is_empty() {
                    lines.push(line.to_owned());
                }
            }
        }
    }
    lines
}

/// `NAME;PARAM=a,"b:c":value`.
pub fn parse_property(line: &str) -> Option<Property> {
    // The value starts at the first `:` outside a quoted parameter value.
    let mut quoted = false;
    let mut colon = None;
    for (i, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ':' if !quoted => {
                colon = Some(i);
                break;
            }
            _ => {}
        }
    }
    let colon = colon?;
    let (head, value) = (&line[..colon], &line[colon + 1..]);
    let mut parts = split_unquoted(head, ';').into_iter();
    let name = parts.next()?.trim().to_ascii_uppercase();
    if name.is_empty() {
        return None;
    }
    let params = parts
        .filter_map(|part| {
            let (key, value) = part.split_once('=')?;
            Some((
                key.trim().to_ascii_uppercase(),
                value.trim().trim_matches('"').to_owned(),
            ))
        })
        .collect();
    Some(Property {
        name,
        params,
        value: value.to_owned(),
    })
}

fn split_unquoted(text: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut quoted = false;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        if c == '"' {
            quoted = !quoted;
        } else if c == separator && !quoted {
            parts.push(&text[start..i]);
            start = i + c.len_utf8();
        }
    }
    parts.push(&text[start..]);
    parts
}

/// A `TEXT` value: `\n` is a line break, `\,` `\;` `\\` are literal.
pub fn unescape_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => out.push('\n'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// The zone a `TZID` names, or [`Zone::Floating`] when it is not known.
pub fn zone_for_tzid(tzid: &str) -> Zone {
    let tzid = tzid.trim();
    if let Ok(tz) = Tz::from_str(tzid) {
        return Zone::Named(tz);
    }
    // `/mozilla.org/20050126_1/America/New_York`, `/softwarestudio.org/.../Europe/Paris`:
    // the IANA name is at the end.
    let segments: Vec<&str> = tzid.split('/').filter(|s| !s.is_empty()).collect();
    for take in [3, 2, 1] {
        if segments.len() >= take {
            let tail = segments[segments.len() - take..].join("/");
            if let Ok(tz) = Tz::from_str(&tail) {
                return Zone::Named(tz);
            }
        }
    }
    windows_zone(tzid).map_or(Zone::Floating, Zone::Named)
}

/// The IANA zone for the Windows zone names Outlook and Exchange write most.
fn windows_zone(name: &str) -> Option<Tz> {
    use chrono_tz::*;
    let tz = match name {
        "UTC" | "Coordinated Universal Time" => UTC,
        "GMT Standard Time" => Europe::London,
        "Greenwich Standard Time" => Atlantic::Reykjavik,
        "W. Europe Standard Time" => Europe::Berlin,
        "Romance Standard Time" => Europe::Paris,
        "Central Europe Standard Time" => Europe::Budapest,
        "Central European Standard Time" => Europe::Warsaw,
        "E. Europe Standard Time" => Europe::Chisinau,
        "FLE Standard Time" => Europe::Kiev,
        "GTB Standard Time" => Europe::Bucharest,
        "Russian Standard Time" => Europe::Moscow,
        "Turkey Standard Time" => Europe::Istanbul,
        "Israel Standard Time" => Asia::Jerusalem,
        "Arabian Standard Time" => Asia::Dubai,
        "Arab Standard Time" => Asia::Riyadh,
        "India Standard Time" => Asia::Kolkata,
        "China Standard Time" => Asia::Shanghai,
        "Tokyo Standard Time" => Asia::Tokyo,
        "Korea Standard Time" => Asia::Seoul,
        "Singapore Standard Time" => Asia::Singapore,
        "AUS Eastern Standard Time" => Australia::Sydney,
        "E. Australia Standard Time" => Australia::Brisbane,
        "New Zealand Standard Time" => Pacific::Auckland,
        "Eastern Standard Time" => America::New_York,
        "Central Standard Time" => America::Chicago,
        "Mountain Standard Time" => America::Denver,
        "US Mountain Standard Time" => America::Phoenix,
        "Pacific Standard Time" => America::Los_Angeles,
        "Alaskan Standard Time" => America::Anchorage,
        "Hawaiian Standard Time" => Pacific::Honolulu,
        "Atlantic Standard Time" => America::Halifax,
        "E. South America Standard Time" => America::Sao_Paulo,
        "South Africa Standard Time" => Africa::Johannesburg,
        _ => return None,
    };
    Some(tz)
}

/// A `DTSTART`-like property's value (the first one, for a list).
pub fn parse_when(property: &Property) -> Option<When> {
    let first = property.value.split(',').next()?;
    parse_when_text(first, property.param("VALUE"), property.param("TZID"))
}

/// Every value of an `EXDATE` / `RDATE` list.
pub fn parse_when_list(property: &Property) -> Vec<When> {
    property
        .value
        .split(',')
        .filter_map(|v| parse_when_text(v, property.param("VALUE"), property.param("TZID")))
        .collect()
}

pub(super) fn parse_when_text(
    text: &str,
    value_type: Option<&str>,
    tzid: Option<&str>,
) -> Option<When> {
    let text = text.trim();
    let is_date = value_type.is_some_and(|v| v.eq_ignore_ascii_case("DATE")) || text.len() == 8;
    if is_date {
        return NaiveDate::parse_from_str(text.get(..8)?, "%Y%m%d")
            .ok()
            .map(When::Date);
    }
    let (body, utc) = match text.strip_suffix(['Z', 'z']) {
        Some(body) => (body, true),
        None => (text, false),
    };
    let time = NaiveDateTime::parse_from_str(body, "%Y%m%dT%H%M%S").ok()?;
    let zone = if utc {
        Zone::Utc
    } else {
        tzid.map_or(Zone::Floating, zone_for_tzid)
    };
    Some(When::DateTime(time, zone))
}

/// An RFC 5545 duration: `P1D`, `PT1H30M`, `P2W`, `-PT15M`.
pub fn parse_duration(text: &str) -> Option<Duration> {
    let text = text.trim();
    let (negative, text) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let mut rest = text.strip_prefix(['P', 'p'])?;
    let mut total = Duration::zero();
    let mut in_time = false;
    let mut any = false;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix(['T', 't']) {
            in_time = true;
            rest = after;
            continue;
        }
        let digits = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        if digits == 0 {
            return None;
        }
        let number: i64 = rest[..digits].parse().ok()?;
        let unit = rest[digits..].chars().next()?.to_ascii_uppercase();
        rest = &rest[digits + 1..];
        total += match (unit, in_time) {
            ('W', false) => Duration::try_weeks(number)?,
            ('D', false) => Duration::try_days(number)?,
            ('H', true) => Duration::try_hours(number)?,
            ('M', true) => Duration::try_minutes(number)?,
            ('S', true) => Duration::try_seconds(number)?,
            _ => return None,
        };
        any = true;
    }
    any.then(|| if negative { -total } else { total })
}

/// The events in an `.ics` text.
pub fn parse_events(text: &str) -> Vec<Event> {
    let mut events = Vec::new();
    // Component names, innermost last.
    let mut stack: Vec<String> = Vec::new();
    let mut props: Vec<Property> = Vec::new();
    for line in unfold(text) {
        let Some(property) = parse_property(&line) else {
            continue;
        };
        match property.name.as_str() {
            "BEGIN" => {
                let component = property.value.trim().to_ascii_uppercase();
                if component == "VEVENT" {
                    props.clear();
                }
                stack.push(component);
            }
            "END" => {
                let component = property.value.trim().to_ascii_uppercase();
                if let Some(pos) = stack.iter().rposition(|c| *c == component) {
                    stack.truncate(pos);
                }
                if component == "VEVENT" {
                    if let Some(event) = build_event(&props) {
                        if events.len() < MAX_EVENTS_PER_FILE {
                            events.push(event);
                        }
                    }
                    props.clear();
                }
            }
            // Only the event's own lines, not those of an alarm inside it.
            _ if stack.last().is_some_and(|c| c == "VEVENT") => props.push(property),
            _ => {}
        }
    }
    events
}

fn text_of(props: &[Property], name: &str) -> Option<String> {
    let property = props.iter().find(|p| p.name == name)?;
    let text: String = unescape_text(&property.value)
        .chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .take(MAX_TEXT_CHARS)
        .collect();
    let text = text.trim().to_owned();
    (!text.is_empty()).then_some(text)
}

/// The instant of a UTC or zoned time (a floating one depends on where you are).
fn fixed_instant(when: When) -> Option<chrono::DateTime<chrono::Utc>> {
    match when {
        When::DateTime(time, Zone::Utc) => Some(time.and_utc()),
        When::DateTime(time, Zone::Named(tz)) => Some(super::recur::zoned_to_utc(&tz, time)),
        _ => None,
    }
}

fn build_event(props: &[Property]) -> Option<Event> {
    let start = parse_when(props.iter().find(|p| p.name == "DTSTART")?)?;
    let end = props
        .iter()
        .find(|p| p.name == "DTEND")
        .and_then(parse_when);
    let duration = match end {
        Some(end) if end.is_date() == start.is_date() => {
            let length = end.naive() - start.naive();
            // An end in another zone than the start: compare real instants.
            match (fixed_instant(start), fixed_instant(end)) {
                (Some(a), Some(b)) if start.zone() != end.zone() => b - a,
                _ => length,
            }
        }
        _ => props
            .iter()
            .find(|p| p.name == "DURATION")
            .and_then(|p| parse_duration(&p.value))
            .unwrap_or_else(|| {
                if start.is_date() {
                    Duration::days(1)
                } else {
                    Duration::zero()
                }
            }),
    };
    let duration = duration.max(Duration::zero());
    Some(Event {
        uid: text_of(props, "UID"),
        summary: text_of(props, "SUMMARY").unwrap_or_default(),
        location: text_of(props, "LOCATION"),
        description: text_of(props, "DESCRIPTION"),
        url: text_of(props, "URL"),
        start,
        duration,
        rrule: props
            .iter()
            .find(|p| p.name == "RRULE")
            .and_then(|p| parse_rrule(&p.value)),
        exdates: props
            .iter()
            .filter(|p| p.name == "EXDATE")
            .flat_map(parse_when_list)
            .collect(),
        rdates: props
            .iter()
            .filter(|p| p.name == "RDATE")
            .flat_map(parse_when_list)
            .collect(),
        recurrence_id: props
            .iter()
            .find(|p| p.name == "RECURRENCE-ID")
            .and_then(parse_when),
        cancelled: props
            .iter()
            .any(|p| p.name == "STATUS" && p.value.trim().eq_ignore_ascii_case("CANCELLED")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime {
        date(y, m, d).and_hms_opt(h, min, 0).unwrap()
    }

    #[test]
    fn unfolds_and_splits_content_lines() {
        // One leading space or tab is the fold; anything after it is text.
        let lines = unfold("SUMMARY:Long\r\n  title\r\n\t goes on\r\nX:1\n\n");
        assert_eq!(lines, ["SUMMARY:Long title goes on", "X:1"]);
        let p = parse_property("DTSTART;TZID=\"Europe/Berlin\";X-A=1,2:20261120T093000").unwrap();
        assert_eq!(p.name, "DTSTART");
        assert_eq!(p.param("TZID"), Some("Europe/Berlin"));
        assert_eq!(p.param("X-A"), Some("1,2"));
        assert_eq!(p.value, "20261120T093000");
        let quoted = parse_property("ATTENDEE;CN=\"Doe: John\":mailto:j@x").unwrap();
        assert_eq!(quoted.param("CN"), Some("Doe: John"));
        assert_eq!(quoted.value, "mailto:j@x");
        assert!(parse_property("no colon").is_none());
        assert!(parse_property(":value").is_none());
    }

    #[test]
    fn text_escapes() {
        assert_eq!(unescape_text(r"a\, b\; c\\d\nnext\N"), "a, b; c\\d\nnext\n");
    }

    #[test]
    fn dates_times_and_zones() {
        let parse = |line: &str| parse_when(&parse_property(line).unwrap()).unwrap();
        assert_eq!(
            parse("DTSTART;VALUE=DATE:20261120"),
            When::Date(date(2026, 11, 20))
        );
        assert_eq!(parse("DTSTART:20261120"), When::Date(date(2026, 11, 20)));
        assert_eq!(
            parse("DTSTART:20261120T093000Z"),
            When::DateTime(at(2026, 11, 20, 9, 30), Zone::Utc)
        );
        assert_eq!(
            parse("DTSTART:20261120T093000"),
            When::DateTime(at(2026, 11, 20, 9, 30), Zone::Floating)
        );
        assert_eq!(
            parse("DTSTART;TZID=Europe/Berlin:20261120T093000"),
            When::DateTime(
                at(2026, 11, 20, 9, 30),
                Zone::Named(chrono_tz::Europe::Berlin)
            )
        );
        assert_eq!(
            zone_for_tzid("/mozilla.org/20050126_1/America/New_York"),
            Zone::Named(chrono_tz::America::New_York)
        );
        assert_eq!(
            zone_for_tzid("W. Europe Standard Time"),
            Zone::Named(chrono_tz::Europe::Berlin)
        );
        assert_eq!(
            zone_for_tzid("GMT Standard Time"),
            Zone::Named(chrono_tz::Europe::London)
        );
        assert_eq!(zone_for_tzid("Mars/Olympus_Mons"), Zone::Floating);
        let exdates = parse_when_list(
            &parse_property("EXDATE;TZID=Europe/Berlin:20261127T093000,20261204T093000").unwrap(),
        );
        assert_eq!(exdates.len(), 2);
        assert!(parse_property("DTSTART:nonsense")
            .and_then(|p| parse_when(&p))
            .is_none());
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("PT1H30M"), Some(Duration::minutes(90)));
        assert_eq!(parse_duration("P1D"), Some(Duration::days(1)));
        assert_eq!(parse_duration("P2W"), Some(Duration::weeks(2)));
        assert_eq!(parse_duration("P1DT12H"), Some(Duration::hours(36)));
        assert_eq!(parse_duration("-PT15M"), Some(Duration::minutes(-15)));
        assert_eq!(parse_duration("PT"), None);
        assert_eq!(parse_duration("P1H"), None);
        assert_eq!(parse_duration("1H"), None);
    }

    #[test]
    fn events_with_ends_durations_and_alarms() {
        let events = parse_events(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\n\
             BEGIN:VTIMEZONE\r\nTZID:Europe/Berlin\r\nBEGIN:STANDARD\r\nDTSTART:19701025T030000\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\n\
             BEGIN:VEVENT\r\nUID:one@x\r\nSUMMARY:Stand-up\\, daily\r\nDTSTART;TZID=Europe/Berlin:20261120T093000\r\n\
             DTEND;TZID=Europe/Berlin:20261120T094500\r\nLOCATION:Room 1\r\n\
             BEGIN:VALARM\r\nACTION:DISPLAY\r\nSUMMARY:Not the event\r\nTRIGGER:-PT5M\r\nEND:VALARM\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nSUMMARY:Holiday\r\nDTSTART;VALUE=DATE:20261224\r\nDTEND;VALUE=DATE:20261227\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nSUMMARY:Call\r\nDTSTART:20261121T100000Z\r\nDURATION:PT30M\r\nSTATUS:CANCELLED\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nSUMMARY:Day\r\nDTSTART;VALUE=DATE:20261130\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nSUMMARY:No start\r\nEND:VEVENT\r\n\
             END:VCALENDAR\r\n",
        );
        let summaries: Vec<_> = events.iter().map(|e| e.summary.as_str()).collect();
        assert_eq!(summaries, ["Stand-up, daily", "Holiday", "Call", "Day"]);
        assert_eq!(events[0].duration, Duration::minutes(15));
        assert_eq!(events[0].location.as_deref(), Some("Room 1"));
        assert_eq!(events[0].uid.as_deref(), Some("one@x"));
        assert_eq!(events[1].duration, Duration::days(3));
        assert!(events[2].cancelled);
        assert_eq!(events[2].duration, Duration::minutes(30));
        assert_eq!(events[3].duration, Duration::days(1));
    }

    #[test]
    fn an_end_in_another_zone_is_compared_as_instants() {
        let events = parse_events(
            "BEGIN:VEVENT\nSUMMARY:Flight\nDTSTART;TZID=Europe/London:20261120T100000\n\
             DTEND;TZID=Europe/Berlin:20261120T130000\nEND:VEVENT\n",
        );
        assert_eq!(events[0].duration, Duration::hours(2));
    }
}
