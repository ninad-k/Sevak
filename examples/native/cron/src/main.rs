//! Explain a cron expression and list when it runs next.
//!
//! Type `cron` and a five-field expression (`cron */15 9-17 * * 1-5`). Sevak
//! says what it means in words and lists the next five run times. Times are
//! UTC unless the expression ends with an offset such as `+05:30`. Everything
//! is computed here: no dependencies beyond the SDK, no clock but the system's.

use std::fmt::Write as _;
use std::time::{SystemTime, UNIX_EPOCH};

use sevak_extension_sdk::{run, Action, Icon, Item, Query};

/// How many upcoming runs are listed.
const RUNS: usize = 5;
/// How far ahead to look before deciding an expression never runs (leap days
/// make 8 years the shortest span that always contains one: 1900 to 2100).
const MAX_DAYS: i64 = 366 * 8;
const DAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

fn main() {
    run(|query: &Query| Ok(answer(query.text(), now())));
}

/// Seconds since 1970-01-01 UTC; 0 if the clock is before it.
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

fn answer(input: &str, now: i64) -> Vec<Item> {
    if input.trim().is_empty() {
        return usage();
    }
    match Parsed::parse(input) {
        Err(why) => vec![Item::new(format!("Not a cron expression: {why}"))
            .key("problem")
            .subtitle("Five fields: minute hour day-of-month month day-of-week, e.g. 0 9 * * 1-5")
            .icon(Icon::builtin("warning"))],
        Ok(parsed) => rows(&parsed, now),
    }
}

fn usage() -> Vec<Item> {
    vec![
        Item::new("Type a cron expression")
            .key("hint")
            .subtitle("minute hour day-of-month month day-of-week, e.g. cron */15 9-17 * * 1-5")
            .icon(Icon::builtin("plugin")),
        Item::new("Times are UTC")
            .key("hint-zone")
            .subtitle("End the expression with an offset for another zone: cron 0 9 * * * +05:30"),
        Item::new("Shortcuts work too")
            .key("hint-macros")
            .subtitle("@hourly @daily @weekly @monthly @yearly (and @midnight, @annually)"),
    ]
}

fn rows(parsed: &Parsed, now: i64) -> Vec<Item> {
    let schedule = &parsed.schedule;
    let zone = zone_label(parsed.offset);
    let mut rows = vec![Item::new(schedule.describe())
        .key("meaning")
        .subtitle(format!("{} · {zone}", schedule.normalized()))
        .icon(Icon::builtin("plugin"))
        .action(Action::copy_text(schedule.normalized()))];

    // Work in the zone's wall clock; the first run is in a later minute than now.
    let local_now = now + parsed.offset;
    let first_minute = local_now.div_euclid(60) + 1;
    let runs = schedule.next_runs(first_minute, RUNS);
    if runs.is_empty() {
        rows.push(
            Item::new("Never runs: no date matches")
                .key("never")
                .subtitle("For example day 30 of February; check the day and month fields")
                .icon(Icon::builtin("warning")),
        );
    }
    for (n, minute) in runs.iter().enumerate() {
        let local = minute * 60;
        let wait = u64::try_from(local - local_now).unwrap_or(0);
        rows.push(
            Item::new(format!("{} {zone}", wall_clock(local)))
                .key(format!("next-{}", n + 1))
                .subtitle(format!("Run {} · in {}", n + 1, human(wait)))
                .action(Action::copy_text(iso(local, parsed.offset))),
        );
    }
    rows
}

// ---- the expression ------------------------------------------------------

/// An expression and the zone it is read in.
#[derive(Debug, PartialEq, Eq)]
struct Parsed {
    schedule: Schedule,
    /// Seconds east of UTC.
    offset: i64,
}

impl Parsed {
    fn parse(input: &str) -> Result<Self, String> {
        let words: Vec<&str> = input.split_whitespace().collect();
        let (fields, rest): (Vec<String>, &[&str]) = match words.first() {
            Some(first) if first.starts_with('@') => (
                macro_fields(first)?.split(' ').map(str::to_owned).collect(),
                &words[1..],
            ),
            _ if words.len() < 5 => {
                return Err(format!(
                    "found {} field(s), expected 5 (minute hour day month weekday)",
                    words.len()
                ))
            }
            _ => (
                words[..5].iter().map(|s| (*s).to_owned()).collect(),
                &words[5..],
            ),
        };
        let offset = match rest {
            [] => 0,
            [zone] => parse_offset(zone).ok_or_else(|| {
                format!(
                    "{zone:?} after the five fields is not a zone like +05:30 or utc \
                     (6-field expressions with seconds are not supported)"
                )
            })?,
            [_, ..] => return Err("there is extra text after the zone".to_owned()),
        };
        Ok(Self {
            schedule: Schedule::new(&fields)?,
            offset,
        })
    }
}

fn macro_fields(name: &str) -> Result<&'static str, String> {
    Ok(match name.to_ascii_lowercase().as_str() {
        "@yearly" | "@annually" => "0 0 1 1 *",
        "@monthly" => "0 0 1 * *",
        "@weekly" => "0 0 * * 0",
        "@daily" | "@midnight" => "0 0 * * *",
        "@hourly" => "0 * * * *",
        "@reboot" => return Err("@reboot runs at startup, not on a schedule".to_owned()),
        other => return Err(format!("unknown shortcut {other}")),
    })
}

/// `utc`, `z`, `+05:30`, `-8`, `+0530`: seconds east of UTC.
fn parse_offset(text: &str) -> Option<i64> {
    let lower = text.to_ascii_lowercase();
    if matches!(lower.as_str(), "utc" | "z" | "gmt") {
        return Some(0);
    }
    let (sign, digits) = match lower.chars().next()? {
        '+' => (1, &lower[1..]),
        '-' => (-1, &lower[1..]),
        _ => return None,
    };
    let (hours, minutes) = match digits.split_once(':') {
        Some((h, m)) => (h, m),
        None if digits.len() == 4 => digits.split_at(2),
        None => (digits, "0"),
    };
    let hours: i64 = hours.parse().ok().filter(|h| (0..=14).contains(h))?;
    let minutes: i64 = minutes.parse().ok().filter(|m| (0..60).contains(m))?;
    Some(sign * (hours * 3600 + minutes * 60))
}

/// One field: which values it allows, and how it was written.
#[derive(Debug, PartialEq, Eq)]
struct Field {
    /// Bit `v` is set when value `v` matches.
    set: u64,
    /// As written (lower case), for the summary.
    text: String,
    /// Written with a leading `*` or `?`: Vixie cron's rule for whether day of
    /// month and day of week are both required or either one is enough.
    star: bool,
    /// `*/n` with nothing else: "every n".
    step: Option<u32>,
    lowest: u32,
    highest: u32,
}

impl Field {
    fn parse(
        text: &str,
        what: &str,
        (lowest, highest): (u32, u32),
        names: &[&str],
        name_base: u32,
    ) -> Result<Self, String> {
        let lower = text.to_ascii_lowercase();
        let value = |word: &str| -> Result<u32, String> {
            if let Ok(n) = word.parse::<u32>() {
                return Ok(n);
            }
            names
                .iter()
                .position(|name| {
                    word.eq_ignore_ascii_case(name) || word.eq_ignore_ascii_case(&name[..3])
                })
                .map(|i| i as u32 + name_base)
                .ok_or_else(|| format!("{what}: {word:?} is not a number or name"))
        };
        let mut set = 0_u64;
        let mut step_only = None;
        for item in lower.split(',') {
            let (range, step) = match item.split_once('/') {
                Some((range, step)) => {
                    let step: u32 =
                        step.parse().ok().filter(|s| *s >= 1).ok_or_else(|| {
                            format!("{what}: the step in {item:?} must be 1 or more")
                        })?;
                    (range, Some(step))
                }
                None => (item, None),
            };
            let (from, to) = if range == "*" || range == "?" {
                (lowest, highest)
            } else if let Some((a, b)) = range.split_once('-') {
                (value(a)?, value(b)?)
            } else {
                let from = value(range)?;
                // "5/10" means from 5 to the end, every 10.
                (from, if step.is_some() { highest } else { from })
            };
            // Sunday may be written 7; it is 0 internally.
            let clamp_high = if what == "weekday" { 7 } else { highest };
            if from < lowest || from > clamp_high || to < lowest || to > clamp_high {
                return Err(format!(
                    "{what}: {item:?} is outside {lowest}-{}",
                    clamp_high
                ));
            }
            if from > to {
                return Err(format!("{what}: the range {item:?} runs backwards"));
            }
            let mut v = from;
            while v <= to {
                set |= 1 << (if what == "weekday" && v == 7 { 0 } else { v });
                v += step.unwrap_or(1);
            }
            if item == lower && step.is_some() && range == "*" {
                step_only = step;
            }
        }
        Ok(Self {
            set,
            star: lower.starts_with('*') || lower.starts_with('?'),
            step: step_only.filter(|s| *s > 1),
            text: lower,
            lowest,
            highest,
        })
    }

    fn values(&self) -> Vec<u32> {
        (self.lowest..=self.highest)
            .filter(|v| self.set & (1 << v) != 0)
            .collect()
    }

    fn has(&self, value: u32) -> bool {
        self.set & (1 << value) != 0
    }

    fn is_all(&self) -> bool {
        self.values().len() as u32 == self.highest - self.lowest + 1
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Schedule {
    minute: Field,
    hour: Field,
    day: Field,
    month: Field,
    weekday: Field,
}

impl Schedule {
    fn new(fields: &[String]) -> Result<Self, String> {
        let [minute, hour, day, month, weekday] = fields else {
            return Err(format!("expected 5 fields, found {}", fields.len()));
        };
        let month_names: Vec<&str> = MONTHS.to_vec();
        let day_names: Vec<&str> = DAYS.to_vec();
        Ok(Self {
            minute: Field::parse(minute, "minute", (0, 59), &[], 0)?,
            hour: Field::parse(hour, "hour", (0, 23), &[], 0)?,
            day: Field::parse(day, "day of month", (1, 31), &[], 0)?,
            month: Field::parse(month, "month", (1, 12), &month_names, 1)?,
            weekday: Field::parse(weekday, "weekday", (0, 6), &day_names, 0)?,
        })
    }

    fn normalized(&self) -> String {
        [
            &self.minute,
            &self.hour,
            &self.day,
            &self.month,
            &self.weekday,
        ]
        .map(|f| f.text.as_str())
        .join(" ")
    }

    fn day_matches(&self, day: i64) -> bool {
        let (_, month, date) = civil_from_days(day);
        let weekday = (day + 4).rem_euclid(7) as u32; // 1970-01-01 was a Thursday
        if !self.month.has(month as u32) {
            return false;
        }
        let by_date = self.day.has(date as u32);
        let by_weekday = self.weekday.has(weekday);
        // Vixie cron: when both day fields are restricted, either one matches.
        if self.day.star || self.weekday.star {
            by_date && by_weekday
        } else {
            by_date || by_weekday
        }
    }

    /// The next `count` runs at or after `first_minute` (minutes since the
    /// epoch on the wall clock), as minutes since the epoch.
    fn next_runs(&self, first_minute: i64, count: usize) -> Vec<i64> {
        let (hours, minutes) = (self.hour.values(), self.minute.values());
        let first_day = first_minute.div_euclid(1440);
        let mut runs = Vec::new();
        for day in first_day..first_day + MAX_DAYS {
            if !self.day_matches(day) {
                continue;
            }
            for hour in &hours {
                for minute in &minutes {
                    let at = day * 1440 + i64::from(*hour) * 60 + i64::from(*minute);
                    if at >= first_minute {
                        runs.push(at);
                        if runs.len() == count {
                            return runs;
                        }
                    }
                }
            }
        }
        runs
    }

    // ---- in words ----------------------------------------------------------

    fn describe(&self) -> String {
        let (time, explicit) = self.time_phrase();
        let mut parts = vec![time];
        let by_date = !self.day.is_all();
        let by_weekday = !self.weekday.is_all();
        let date = format!("on day {} of the month", list(&self.day.values()));
        let weekday = format!("on {}", names(&self.weekday.values(), &DAYS, 0));
        match (by_date, by_weekday) {
            (false, false) if explicit && self.month.is_all() => {
                parts.push("every day".to_owned());
            }
            (false, false) => {}
            (true, false) => parts.push(date),
            (false, true) => parts.push(weekday),
            (true, true) if self.day.star || self.weekday.star => {
                parts.push(date);
                parts.push(weekday);
            }
            (true, true) => parts.push(format!("{date} or {weekday}")),
        }
        if !self.month.is_all() {
            parts.push(format!("in {}", names(&self.month.values(), &MONTHS, 1)));
        }
        parts.join(", ")
    }

    /// The times in words, and whether they are explicit clock times (which
    /// read better with "every day" after them).
    fn time_phrase(&self) -> (String, bool) {
        let (minutes, hours) = (self.minute.values(), self.hour.values());
        // A short, explicit list of times reads best as the times themselves.
        if !self.minute.star && !self.hour.star && minutes.len() * hours.len() <= 6 {
            let times: Vec<String> = hours
                .iter()
                .flat_map(|h| minutes.iter().map(move |m| format!("{h:02}:{m:02}")))
                .collect();
            return (format!("At {}", join_and(&times)), true);
        }
        let minute_part = if self.minute.is_all() {
            "every minute".to_owned()
        } else if let Some(n) = self.minute.step {
            format!("every {n} minutes")
        } else if minutes.len() == 1 {
            format!("at minute {}", minutes[0])
        } else {
            format!("at minutes {}", list(&minutes))
        };
        let hour_part = if self.hour.is_all() {
            if self.minute.is_all() || self.minute.step.is_some() {
                String::new()
            } else {
                " of every hour".to_owned()
            }
        } else if let Some(n) = self.hour.step {
            format!(", every {n} hours")
        } else if hours.len() > 1 && is_run(&hours) {
            format!(
                ", between {:02}:00 and {:02}:59",
                hours[0],
                hours[hours.len() - 1]
            )
        } else if hours.len() == 1 {
            format!(", during hour {}", hours[0])
        } else {
            format!(", during hours {}", list(&hours))
        };
        (capitalize(&format!("{minute_part}{hour_part}")), false)
    }
}

fn is_run(values: &[u32]) -> bool {
    values.windows(2).all(|w| w[1] == w[0] + 1)
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

fn join_and(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [only] => only.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// `0, 15 and 30`, with runs of three or more as `9-17`.
fn list(values: &[u32]) -> String {
    let mut parts = Vec::new();
    let mut i = 0;
    while i < values.len() {
        let mut j = i;
        while j + 1 < values.len() && values[j + 1] == values[j] + 1 {
            j += 1;
        }
        if j - i >= 2 {
            parts.push(format!("{}-{}", values[i], values[j]));
            i = j + 1;
        } else {
            parts.push(values[i].to_string());
            i += 1;
        }
    }
    join_and(&parts)
}

/// `Monday through Friday`, `January and July`: names, runs of three or more
/// as `from through to`. `base` is the value of the first name.
fn names(values: &[u32], all: &[&str], base: u32) -> String {
    let name = |v: u32| all[(v - base) as usize].to_owned();
    let mut parts = Vec::new();
    let mut i = 0;
    while i < values.len() {
        let mut j = i;
        while j + 1 < values.len() && values[j + 1] == values[j] + 1 {
            j += 1;
        }
        if j - i >= 2 {
            parts.push(format!("{} through {}", name(values[i]), name(values[j])));
            i = j + 1;
        } else {
            parts.push(name(values[i]));
            i += 1;
        }
    }
    join_and(&parts)
}

// ---- dates ---------------------------------------------------------------

fn zone_label(offset: i64) -> String {
    if offset == 0 {
        return "UTC".to_owned();
    }
    let sign = if offset < 0 { '-' } else { '+' };
    let magnitude = offset.abs();
    format!(
        "UTC{sign}{:02}:{:02}",
        magnitude / 3600,
        magnitude % 3600 / 60
    )
}

/// `Mon 2026-10-05 09:00` for wall-clock seconds.
fn wall_clock(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rest = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let weekday = DAYS[(days + 4).rem_euclid(7) as usize];
    format!(
        "{} {year:04}-{month:02}-{day:02} {:02}:{:02}",
        &weekday[..3],
        rest / 3600,
        rest % 3600 / 60
    )
}

/// `2026-10-05T09:00:00+05:30` (or `Z`) for wall-clock seconds.
fn iso(secs: i64, offset: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rest = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let mut text = format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    );
    if offset == 0 {
        text.push('Z');
    } else {
        let sign = if offset < 0 { '-' } else { '+' };
        let magnitude = offset.abs();
        let _ = write!(
            text,
            "{sign}{:02}:{:02}",
            magnitude / 3600,
            magnitude % 3600 / 60
        );
    }
    text
}

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian.
/// (Howard Hinnant's `civil_from_days`.)
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

fn human(secs: u64) -> String {
    fn plural(n: u64, unit: &str) -> String {
        format!("{n} {unit}{}", if n == 1 { "" } else { "s" })
    }
    let (minutes, hours, days) = (secs / 60, secs / 3600, secs / 86_400);
    match secs {
        0..=59 => plural(secs, "second"),
        60..=3599 => plural(minutes, "minute"),
        3600..=86_399 => match minutes % 60 {
            0 => plural(hours, "hour"),
            m => format!("{} {}", plural(hours, "hour"), plural(m, "minute")),
        },
        _ => match hours % 24 {
            0 => plural(days, "day"),
            h => format!("{} {}", plural(days, "day"), plural(h, "hour")),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-04 12:00:00 UTC, a Sunday.
    const NOW: i64 = 1_791_115_200;

    fn dump(rows: &[Item]) -> String {
        rows.iter()
            .map(|row| format!("{row:?}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn describe(expr: &str) -> String {
        Parsed::parse(expr).unwrap().schedule.describe()
    }

    fn next(expr: &str, count: usize) -> Vec<String> {
        let parsed = Parsed::parse(expr).unwrap();
        let first = (NOW + parsed.offset).div_euclid(60) + 1;
        parsed
            .schedule
            .next_runs(first, count)
            .iter()
            .map(|m| wall_clock(m * 60))
            .collect()
    }

    #[test]
    fn the_clock_constant_is_what_the_comments_say() {
        assert_eq!(wall_clock(NOW), "Sun 2026-10-04 12:00");
    }

    #[test]
    fn expressions_are_explained() {
        assert_eq!(describe("* * * * *"), "Every minute");
        assert_eq!(describe("*/5 * * * *"), "Every 5 minutes");
        assert_eq!(
            describe("0 9 * * 1-5"),
            "At 09:00, on Monday through Friday"
        );
        assert_eq!(describe("30 2 1 * *"), "At 02:30, on day 1 of the month");
        assert_eq!(
            describe("0 0 1 1 *"),
            "At 00:00, on day 1 of the month, in January"
        );
        assert_eq!(describe("0 0 * * *"), "At 00:00, every day");
        assert_eq!(describe("@hourly"), "At minute 0 of every hour");
        assert_eq!(describe("0 */2 * * *"), "At minute 0, every 2 hours");
        assert_eq!(
            describe("15,45 8-17 * * *"),
            "At minutes 15 and 45, between 08:00 and 17:59"
        );
        assert_eq!(
            describe("0 9,17 * * mon,wed"),
            "At 09:00 and 17:00, on Monday and Wednesday"
        );
        assert_eq!(
            describe("0 0 * jan-mar *"),
            "At 00:00, in January through March"
        );
        assert_eq!(
            describe("0 0 13 * fri"),
            "At 00:00, on day 13 of the month or on Friday"
        );
        assert_eq!(
            describe("*/15 9-17 * * 1-5"),
            "Every 15 minutes, between 09:00 and 17:59, on Monday through Friday"
        );
    }

    #[test]
    fn next_runs_are_found() {
        assert_eq!(
            next("*/20 * * * *", 3),
            [
                "Sun 2026-10-04 12:20",
                "Sun 2026-10-04 12:40",
                "Sun 2026-10-04 13:00"
            ]
        );
        assert_eq!(
            next("0 9 * * 1-5", 3),
            [
                "Mon 2026-10-05 09:00",
                "Tue 2026-10-06 09:00",
                "Wed 2026-10-07 09:00"
            ]
        );
        // Not "now": a run due this very minute is not upcoming.
        assert_eq!(next("0 12 * * *", 1), ["Mon 2026-10-05 12:00"]);
        // Day of month OR weekday when both are given.
        assert_eq!(
            next("0 0 5 * mon", 3),
            [
                "Mon 2026-10-05 00:00",
                "Mon 2026-10-12 00:00",
                "Mon 2026-10-19 00:00"
            ]
        );
        // Sunday as 0 and as 7.
        assert_eq!(next("0 0 * * 7", 1), next("0 0 * * 0", 1));
        assert_eq!(next("0 0 * * 0", 1), ["Sun 2026-10-11 00:00"]);
        // A leap day is found, four years out.
        assert_eq!(next("0 0 29 2 *", 1), ["Tue 2028-02-29 00:00"]);
        // A date that does not exist never runs.
        assert!(next("0 0 30 2 *", 1).is_empty());
    }

    #[test]
    fn an_offset_moves_the_clock() {
        assert_eq!(next("0 9 * * * +05:30", 1), ["Mon 2026-10-05 09:00"]);
        // 12:00 UTC is 17:30 in +05:30, so 17:45 is today.
        assert_eq!(next("45 17 * * * +05:30", 1), ["Sun 2026-10-04 17:45"]);
        let text = dump(&answer("45 17 * * * +05:30", NOW));
        assert!(text.contains("UTC+05:30"), "{text}");
        assert!(text.contains("2026-10-04T17:45:00+05:30"), "{text}");
        assert_eq!(parse_offset("-8"), Some(-8 * 3600));
        assert_eq!(parse_offset("+0530"), Some(19_800));
        assert_eq!(parse_offset("utc"), Some(0));
        assert_eq!(parse_offset("+15:00"), None);
        assert_eq!(parse_offset("nonsense"), None);
    }

    #[test]
    fn rows_have_the_meaning_and_five_runs() {
        let rows = answer("0 9 * * 1-5", NOW);
        assert_eq!(rows.len(), 6);
        let text = dump(&rows);
        assert!(
            text.contains("At 09:00, on Monday through Friday"),
            "{text}"
        );
        assert!(text.contains("Mon 2026-10-05 09:00 UTC"), "{text}");
        assert!(text.contains("in 21 hours"), "{text}");
        assert!(text.contains("2026-10-05T09:00:00Z"), "{text}");
        assert!(rows.iter().all(|row| row.problems().is_empty()));
    }

    #[test]
    fn names_steps_and_macros_parse() {
        assert_eq!(
            Parsed::parse("@daily").unwrap().schedule.normalized(),
            "0 0 * * *"
        );
        assert_eq!(
            Parsed::parse("@WEEKLY").unwrap().schedule.normalized(),
            "0 0 * * 0"
        );
        assert!(Parsed::parse("5/20 * * * *")
            .unwrap()
            .schedule
            .minute
            .has(45));
        assert!(Parsed::parse("0 0 * DEC-JAN *").is_err()); // backwards
        assert!(Parsed::parse("0 0 * * sun")
            .unwrap()
            .schedule
            .weekday
            .has(0));
        assert!(Parsed::parse("0 0 1,15 * *").unwrap().schedule.day.has(15));
    }

    #[test]
    fn bad_expressions_get_a_reason() {
        for (expr, reason) in [
            ("* * * *", "found 4 field"),
            ("61 * * * *", "is outside 0-59"),
            ("* 24 * * *", "hour"),
            ("* * 0 * *", "day of month"),
            ("* * * 13 *", "month"),
            ("* * * * 8", "weekday"),
            ("*/0 * * * *", "step"),
            ("* * * * abc", "not a number or name"),
            ("30-10 * * * *", "backwards"),
            ("0 0 * * * * *", "extra text"),
            ("0 0 0 * * *", "6-field"),
            ("@reboot", "@reboot"),
            ("@sometimes", "unknown shortcut"),
        ] {
            let text = dump(&answer(expr, NOW));
            assert!(text.contains("Not a cron expression"), "{expr}: {text}");
            assert!(text.contains(reason), "{expr}: {text}");
        }
    }

    #[test]
    fn an_empty_query_shows_usage() {
        let rows = answer("   ", NOW);
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|row| row.problems().is_empty()));
    }

    #[test]
    fn never_running_is_said_not_looped() {
        let text = dump(&answer("0 0 31 2 *", NOW));
        assert!(text.contains("Never runs"), "{text}");
    }
}
