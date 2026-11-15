//! Recurring events and time zones.
//!
//! Supported `RRULE` parts: `FREQ` (`DAILY`, `WEEKLY`, `MONTHLY`, `YEARLY`),
//! `INTERVAL`, `COUNT`, `UNTIL`, `BYDAY` (with an ordinal for monthly and
//! yearly rules: `1MO`, `-1FR`), `BYMONTHDAY` and `BYMONTH`. Others
//! (`BYSETPOS`, `BYWEEKNO`, `BYHOUR`...) are ignored, which can show extra
//! occurrences; sub-daily frequencies are not expanded (the first occurrence
//! is shown). Occurrences are computed in the event's own wall-clock time, so
//! a 09:00 meeting in `Europe/Berlin` stays at 09:00 there across daylight
//! saving changes. `EXDATE`, `RDATE` and `RECURRENCE-ID` overrides are
//! applied by [`expand`].

use std::collections::HashSet;

use chrono::{
    DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc,
    Weekday,
};
use chrono_tz::Tz;

use super::ics::{parse_when_text, Event, When, Zone};

/// Periods (days, weeks, months, years) one rule is followed for at most.
const MAX_PERIODS: i64 = 50_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RRule {
    pub freq: Freq,
    pub interval: u32,
    pub count: Option<u32>,
    pub until: Option<When>,
    /// `(ordinal, day)`: `1MO` is `(Some(1), Mon)`, `FR` is `(None, Fri)`.
    pub by_day: Vec<(Option<i32>, Weekday)>,
    pub by_month_day: Vec<i32>,
    pub by_month: Vec<u32>,
}

/// An `RRULE` value, or `None` for one that is not followed (sub-daily or
/// malformed).
pub fn parse_rrule(value: &str) -> Option<RRule> {
    let mut freq = None;
    let mut rule = RRule {
        freq: Freq::Daily,
        interval: 1,
        count: None,
        until: None,
        by_day: Vec::new(),
        by_month_day: Vec::new(),
        by_month: Vec::new(),
    };
    for part in value.trim().split(';') {
        let Some((key, val)) = part.split_once('=') else {
            continue;
        };
        let val = val.trim();
        match key.trim().to_ascii_uppercase().as_str() {
            "FREQ" => {
                freq = Some(match val.to_ascii_uppercase().as_str() {
                    "DAILY" => Freq::Daily,
                    "WEEKLY" => Freq::Weekly,
                    "MONTHLY" => Freq::Monthly,
                    "YEARLY" => Freq::Yearly,
                    _ => return None,
                });
            }
            "INTERVAL" => rule.interval = val.parse::<u32>().ok().filter(|&n| n > 0)?,
            "COUNT" => rule.count = Some(val.parse::<u32>().ok()?.max(1)),
            "UNTIL" => rule.until = Some(parse_when_text(val, None, None)?),
            "BYDAY" => {
                for day in val.split(',') {
                    rule.by_day.push(parse_by_day(day)?);
                }
            }
            "BYMONTHDAY" => {
                for day in val.split(',') {
                    let day: i32 = day.trim().parse().ok()?;
                    if day == 0 || !(-31..=31).contains(&day) {
                        return None;
                    }
                    rule.by_month_day.push(day);
                }
            }
            "BYMONTH" => {
                for month in val.split(',') {
                    let month: u32 = month.trim().parse().ok()?;
                    if !(1..=12).contains(&month) {
                        return None;
                    }
                    rule.by_month.push(month);
                }
            }
            _ => {}
        }
    }
    rule.freq = freq?;
    Some(rule)
}

fn parse_by_day(text: &str) -> Option<(Option<i32>, Weekday)> {
    let text = text.trim().to_ascii_uppercase();
    if text.len() < 2 {
        return None;
    }
    let (ordinal, day) = text.split_at(text.len() - 2);
    let weekday = match day {
        "MO" => Weekday::Mon,
        "TU" => Weekday::Tue,
        "WE" => Weekday::Wed,
        "TH" => Weekday::Thu,
        "FR" => Weekday::Fri,
        "SA" => Weekday::Sat,
        "SU" => Weekday::Sun,
        _ => return None,
    };
    let ordinal = if ordinal.is_empty() {
        None
    } else {
        let n: i32 = ordinal.parse().ok()?;
        if n == 0 || !(-53..=53).contains(&n) {
            return None;
        }
        Some(n)
    };
    Some((ordinal, weekday))
}

/// The instant of a wall-clock time in `zone`. A time skipped by a daylight
/// saving change is moved an hour on; a repeated one is the first of the two.
pub fn zoned_to_utc<Z: TimeZone>(zone: &Z, time: NaiveDateTime) -> DateTime<Utc> {
    zone.from_local_datetime(&time)
        .earliest()
        .or_else(|| {
            zone.from_local_datetime(&(time + Duration::hours(1)))
                .earliest()
        })
        .map_or_else(|| time.and_utc(), |t| t.with_timezone(&Utc))
}

/// The time zone of the person looking at the agenda.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalZone {
    /// This computer's.
    System,
    /// A fixed one (tests).
    Fixed(Tz),
}

impl LocalZone {
    /// The instant of a local wall-clock time.
    pub fn to_utc(&self, time: NaiveDateTime) -> DateTime<Utc> {
        match self {
            Self::System => zoned_to_utc(&Local, time),
            Self::Fixed(tz) => zoned_to_utc(tz, time),
        }
    }

    /// The local wall-clock time of an instant.
    pub fn local(&self, time: DateTime<Utc>) -> NaiveDateTime {
        match self {
            Self::System => time.with_timezone(&Local).naive_local(),
            Self::Fixed(tz) => time.with_timezone(tz).naive_local(),
        }
    }

    /// Local midnight at the start of `date`.
    pub fn midnight(&self, date: NaiveDate) -> DateTime<Utc> {
        self.to_utc(date.and_time(NaiveTime::MIN))
    }
}

/// The instant of a time given in `zone`.
pub fn to_utc(time: NaiveDateTime, zone: Zone, local: &LocalZone) -> DateTime<Utc> {
    match zone {
        Zone::Utc => time.and_utc(),
        Zone::Floating => local.to_utc(time),
        Zone::Named(tz) => zoned_to_utc(&tz, time),
    }
}

/// `when` as a wall-clock time in `zone`.
fn in_zone(when: When, zone: Zone, local: &LocalZone) -> NaiveDateTime {
    if when.is_date() || when.zone() == zone {
        return when.naive();
    }
    let instant = to_utc(when.naive(), when.zone(), local);
    match zone {
        Zone::Utc => instant.naive_utc(),
        Zone::Floating => local.local(instant),
        Zone::Named(tz) => instant.with_timezone(&tz).naive_local(),
    }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let next = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    };
    next.and_then(|n| n.pred_opt()).map_or(28, |d| d.day())
}

/// The days of `year`-`month` a monthly (or yearly) rule picks.
fn month_days(year: i32, month: u32, rule: &RRule, start_day: u32) -> Vec<u32> {
    let len = days_in_month(year, month);
    let weekday = |day: u32| NaiveDate::from_ymd_opt(year, month, day).map(|d| d.weekday());
    let mut days: Vec<u32> = Vec::new();
    if !rule.by_month_day.is_empty() {
        for &d in &rule.by_month_day {
            let day = if d > 0 { d } else { len as i32 + 1 + d };
            if (1..=len as i32).contains(&day) {
                days.push(day as u32);
            }
        }
        if !rule.by_day.is_empty() {
            days.retain(|&d| rule.by_day.iter().any(|(_, wd)| weekday(d) == Some(*wd)));
        }
    } else if !rule.by_day.is_empty() {
        for &(ordinal, wd) in &rule.by_day {
            let all: Vec<u32> = (1..=len).filter(|&d| weekday(d) == Some(wd)).collect();
            match ordinal {
                None => days.extend(&all),
                Some(n) if n > 0 => days.extend(all.get(n as usize - 1)),
                Some(n) => {
                    let back = n.unsigned_abs() as usize;
                    if back <= all.len() {
                        days.push(all[all.len() - back]);
                    }
                }
            }
        }
    } else if start_day <= len {
        days.push(start_day);
    }
    days.sort_unstable();
    days.dedup();
    days
}

/// The wall-clock starts of a rule's occurrences, from `start` (always the
/// first) up to and including `last` and `until`.
pub fn occurrences(
    start: NaiveDateTime,
    rule: &RRule,
    until: Option<NaiveDateTime>,
    last: NaiveDateTime,
) -> Vec<NaiveDateTime> {
    let end = until.map_or(last, |u| u.min(last));
    if start > end {
        return Vec::new();
    }
    let mut out = vec![start];
    let mut emitted: u32 = 1;
    if rule.count == Some(1) {
        return out;
    }
    let time = start.time();
    let date = start.date();
    let interval = i64::from(rule.interval);
    let month_ok = |d: NaiveDate| rule.by_month.is_empty() || rule.by_month.contains(&d.month());
    let monday = date - Duration::days(i64::from(date.weekday().num_days_from_monday()));

    for period in 0..MAX_PERIODS {
        let step = period * interval;
        // The first day of the period, and the days it contributes.
        let (first, days): (NaiveDate, Vec<NaiveDate>) = match rule.freq {
            Freq::Daily => {
                let Some(day) = date.checked_add_signed(Duration::days(step)) else {
                    break;
                };
                let by_day_ok = rule.by_day.is_empty()
                    || rule.by_day.iter().any(|(_, wd)| *wd == day.weekday());
                let month_day_ok = rule.by_month_day.is_empty()
                    || month_days(day.year(), day.month(), rule, day.day()).contains(&day.day());
                let keep = month_ok(day) && by_day_ok && month_day_ok;
                (day, if keep { vec![day] } else { Vec::new() })
            }
            Freq::Weekly => {
                let Some(week) = monday.checked_add_signed(Duration::weeks(step)) else {
                    break;
                };
                let mut weekdays: Vec<Weekday> = if rule.by_day.is_empty() {
                    vec![date.weekday()]
                } else {
                    rule.by_day.iter().map(|(_, wd)| *wd).collect()
                };
                weekdays.sort_by_key(Weekday::num_days_from_monday);
                weekdays.dedup();
                let days = weekdays
                    .iter()
                    .map(|wd| week + Duration::days(i64::from(wd.num_days_from_monday())))
                    .filter(|d| month_ok(*d))
                    .collect();
                (week, days)
            }
            Freq::Monthly => {
                let months = i64::from(date.month0()) + step;
                let year = date.year() as i64 + months.div_euclid(12);
                let month = (months.rem_euclid(12) + 1) as u32;
                let Some(first) = i32::try_from(year)
                    .ok()
                    .and_then(|y| NaiveDate::from_ymd_opt(y, month, 1))
                else {
                    break;
                };
                let days = if month_ok(first) {
                    month_days(first.year(), month, rule, date.day())
                        .into_iter()
                        .filter_map(|d| NaiveDate::from_ymd_opt(first.year(), month, d))
                        .collect()
                } else {
                    Vec::new()
                };
                (first, days)
            }
            Freq::Yearly => {
                let Some(year) = i32::try_from(date.year() as i64 + step).ok() else {
                    break;
                };
                let Some(first) = NaiveDate::from_ymd_opt(year, 1, 1) else {
                    break;
                };
                let months = if rule.by_month.is_empty() {
                    vec![date.month()]
                } else {
                    rule.by_month.clone()
                };
                let mut days: Vec<NaiveDate> = Vec::new();
                for month in months {
                    let picked = if rule.by_day.is_empty() && rule.by_month_day.is_empty() {
                        // The same date each year (29 February only in leap years).
                        NaiveDate::from_ymd_opt(year, month, date.day())
                            .into_iter()
                            .collect::<Vec<_>>()
                    } else {
                        month_days(year, month, rule, date.day())
                            .into_iter()
                            .filter_map(|d| NaiveDate::from_ymd_opt(year, month, d))
                            .collect()
                    };
                    days.extend(picked);
                }
                days.sort_unstable();
                (first, days)
            }
        };
        if first.and_time(NaiveTime::MIN) > end {
            break;
        }
        for day in days {
            let candidate = day.and_time(time);
            if candidate <= start {
                continue;
            }
            if candidate > end {
                return out;
            }
            out.push(candidate);
            emitted += 1;
            if rule.count.is_some_and(|c| emitted >= c) {
                return out;
            }
        }
    }
    out
}

/// One occurrence on the agenda.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    /// Index into the events it came from.
    pub event: usize,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub all_day: bool,
    /// Local dates it covers (inclusive).
    pub first_day: NaiveDate,
    pub last_day: NaiveDate,
}

/// Every occurrence of `events` that overlaps `from..to`, sorted by start,
/// at most `max`.
pub fn expand(
    events: &[Event],
    local: &LocalZone,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    max: usize,
) -> Vec<Instance> {
    let instant = |when: When| to_utc(when.naive(), when.zone(), local).timestamp();
    // Occurrences replaced by an event of their own.
    let overridden: HashSet<(&str, i64)> = events
        .iter()
        .filter_map(|e| Some((e.uid.as_deref()?, instant(e.recurrence_id?))))
        .collect();

    let mut instances = Vec::new();
    for (index, event) in events.iter().enumerate() {
        if event.cancelled && event.recurrence_id.is_none() {
            continue;
        }
        let zone = event.start.zone();
        let starts: Vec<NaiveDateTime> = match (&event.rrule, event.recurrence_id) {
            (Some(rule), None) => {
                let until = rule.until.map(|u| match u {
                    When::Date(d) => d
                        .and_hms_opt(23, 59, 59)
                        .unwrap_or(d.and_time(NaiveTime::MIN)),
                    other => in_zone(other, zone, local),
                });
                let last = match zone {
                    Zone::Utc => to.naive_utc(),
                    Zone::Floating => local.local(to),
                    Zone::Named(tz) => to.with_timezone(&tz).naive_local(),
                } + Duration::days(1);
                let mut starts = occurrences(event.start.naive(), rule, until, last);
                starts.extend(event.rdates.iter().map(|r| in_zone(*r, zone, local)));
                starts
            }
            _ => {
                let mut starts = vec![event.start.naive()];
                if event.recurrence_id.is_none() {
                    starts.extend(event.rdates.iter().map(|r| in_zone(*r, zone, local)));
                }
                starts
            }
        };
        // A cancelled override only removes its occurrence.
        if event.cancelled {
            continue;
        }
        let excluded: HashSet<i64> = event.exdates.iter().map(|w| instant(*w)).collect();
        let mut seen = HashSet::new();
        for naive in starts {
            let when = if event.start.is_date() {
                When::Date(naive.date())
            } else {
                When::DateTime(naive, zone)
            };
            let key = instant(when);
            if !seen.insert(key) || excluded.contains(&key) {
                continue;
            }
            // A repeating event's occurrence that has an override of its own.
            let replaced = event.recurrence_id.is_none()
                && (event.rrule.is_some() || naive != event.start.naive())
                && event
                    .uid
                    .as_deref()
                    .is_some_and(|uid| overridden.contains(&(uid, key)));
            if replaced {
                continue;
            }
            let instance = occurrence(index, event, naive, local);
            if instance.start < to && (instance.end > from || instance.start >= from) {
                instances.push(instance);
            }
        }
    }
    instances.sort_by(|a, b| a.start.cmp(&b.start).then(a.event.cmp(&b.event)));
    instances.truncate(max);
    instances
}

fn occurrence(index: usize, event: &Event, naive: NaiveDateTime, local: &LocalZone) -> Instance {
    if event.start.is_date() {
        let first = naive.date();
        let days = event.duration.num_days().max(1);
        let end_date = first + Duration::days(days);
        Instance {
            event: index,
            start: local.midnight(first),
            end: local.midnight(end_date),
            all_day: true,
            first_day: first,
            last_day: end_date.pred_opt().unwrap_or(first),
        }
    } else {
        let start = to_utc(naive, event.start.zone(), local);
        let end = start + event.duration;
        let first_day = local.local(start).date();
        let last_day = if end > start {
            local.local(end - Duration::seconds(1)).date()
        } else {
            first_day
        };
        Instance {
            event: index,
            start,
            end,
            all_day: false,
            first_day,
            last_day: last_day.max(first_day),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::ics::parse_events;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn at(y: i32, m: u32, d: u32, h: u32) -> NaiveDateTime {
        day(y, m, d).and_hms_opt(h, 0, 0).unwrap()
    }

    fn dates(times: &[NaiveDateTime]) -> Vec<String> {
        times
            .iter()
            .map(|t| t.format("%a %Y-%m-%d").to_string())
            .collect()
    }

    fn rule(text: &str) -> RRule {
        parse_rrule(text).unwrap()
    }

    #[test]
    fn parses_the_supported_parts() {
        let r = rule("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,-1FR,+2TU;COUNT=5;WKST=SU");
        assert_eq!(r.freq, Freq::Weekly);
        assert_eq!(r.interval, 2);
        assert_eq!(r.count, Some(5));
        assert_eq!(
            r.by_day,
            [
                (None, Weekday::Mon),
                (Some(-1), Weekday::Fri),
                (Some(2), Weekday::Tue)
            ]
        );
        let until = rule("FREQ=DAILY;UNTIL=20261231T235959Z").until;
        assert_eq!(
            until,
            Some(When::DateTime(
                at(2026, 12, 31, 23) + Duration::seconds(3599),
                Zone::Utc
            ))
        );
        assert!(parse_rrule("FREQ=HOURLY").is_none());
        assert!(parse_rrule("INTERVAL=2").is_none());
        assert!(parse_rrule("FREQ=DAILY;INTERVAL=0").is_none());
        assert!(parse_rrule("FREQ=WEEKLY;BYDAY=XX").is_none());
        assert!(parse_rrule("FREQ=MONTHLY;BYMONTHDAY=32").is_none());
    }

    #[test]
    fn daily_with_count_until_and_interval() {
        let start = at(2026, 11, 1, 9);
        let far = at(2030, 1, 1, 0);
        assert_eq!(
            dates(&occurrences(start, &rule("FREQ=DAILY;COUNT=3"), None, far)),
            ["Sun 2026-11-01", "Mon 2026-11-02", "Tue 2026-11-03"]
        );
        assert_eq!(
            occurrences(
                start,
                &rule("FREQ=DAILY;INTERVAL=10"),
                Some(at(2026, 11, 21, 9)),
                far
            )
            .len(),
            3
        );
        // Weekdays only.
        assert_eq!(
            dates(&occurrences(
                start,
                &rule("FREQ=DAILY;BYDAY=MO,TU,WE,TH,FR;COUNT=3"),
                None,
                far
            )),
            ["Sun 2026-11-01", "Mon 2026-11-02", "Tue 2026-11-03"]
        );
        // Nothing after `last`.
        assert_eq!(
            occurrences(start, &rule("FREQ=DAILY"), None, at(2026, 11, 5, 0)).len(),
            4
        );
        assert!(occurrences(start, &rule("FREQ=DAILY"), None, at(2026, 10, 1, 0)).is_empty());
    }

    #[test]
    fn weekly_by_day() {
        // A Monday start, on Mondays and Thursdays, every other week.
        let start = at(2026, 11, 2, 9);
        let far = at(2030, 1, 1, 0);
        assert_eq!(
            dates(&occurrences(
                start,
                &rule("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,TH;COUNT=5"),
                None,
                far
            )),
            [
                "Mon 2026-11-02",
                "Thu 2026-11-05",
                "Mon 2026-11-16",
                "Thu 2026-11-19",
                "Mon 2026-11-30"
            ]
        );
        // Without BYDAY: the start's weekday.
        assert_eq!(
            dates(&occurrences(start, &rule("FREQ=WEEKLY;COUNT=2"), None, far)),
            ["Mon 2026-11-02", "Mon 2026-11-09"]
        );
        // A start on Wednesday, BYDAY=MO: Monday of the first week is before the
        // start and skipped.
        let wed = at(2026, 11, 4, 9);
        assert_eq!(
            dates(&occurrences(
                wed,
                &rule("FREQ=WEEKLY;BYDAY=MO;UNTIL=20261117"),
                Some(at(2026, 11, 17, 23)),
                far
            )),
            ["Wed 2026-11-04", "Mon 2026-11-09", "Mon 2026-11-16"]
        );
    }

    #[test]
    fn monthly_and_yearly() {
        let far = at(2030, 1, 1, 0);
        // The 31st: months without one are skipped.
        assert_eq!(
            dates(&occurrences(
                at(2026, 1, 31, 9),
                &rule("FREQ=MONTHLY;COUNT=3"),
                None,
                far
            )),
            ["Sat 2026-01-31", "Tue 2026-03-31", "Sun 2026-05-31"]
        );
        // The last Friday of the month.
        assert_eq!(
            dates(&occurrences(
                at(2026, 11, 27, 9),
                &rule("FREQ=MONTHLY;BYDAY=-1FR;COUNT=3"),
                None,
                far
            )),
            ["Fri 2026-11-27", "Fri 2026-12-25", "Fri 2027-01-29"]
        );
        // The first Monday.
        assert_eq!(
            dates(&occurrences(
                at(2026, 11, 2, 9),
                &rule("FREQ=MONTHLY;BYDAY=1MO;COUNT=2"),
                None,
                far
            )),
            ["Mon 2026-11-02", "Mon 2026-12-07"]
        );
        assert_eq!(
            dates(&occurrences(
                at(2026, 11, 2, 9),
                &rule("FREQ=MONTHLY;BYMONTHDAY=-1;COUNT=2"),
                None,
                far
            )),
            ["Mon 2026-11-02", "Mon 2026-11-30"]
        );
        // Birthdays, and 29 February only in leap years.
        assert_eq!(
            dates(&occurrences(
                at(2024, 2, 29, 0),
                &rule("FREQ=YEARLY;COUNT=2"),
                None,
                far
            )),
            ["Thu 2024-02-29", "Tue 2028-02-29"]
        );
        // The last Sunday of March.
        assert_eq!(
            dates(&occurrences(
                at(2026, 3, 29, 1),
                &rule("FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU;COUNT=2"),
                None,
                far
            )),
            ["Sun 2026-03-29", "Sun 2027-03-28"]
        );
    }

    #[test]
    fn zoned_times_keep_their_wall_clock_across_daylight_saving() {
        let berlin = chrono_tz::Europe::Berlin;
        // 09:00 in Berlin is 08:00 UTC in winter and 07:00 UTC in summer.
        assert_eq!(
            zoned_to_utc(&berlin, at(2026, 1, 5, 9)),
            at(2026, 1, 5, 8).and_utc()
        );
        assert_eq!(
            zoned_to_utc(&berlin, at(2026, 7, 6, 9)),
            at(2026, 7, 6, 7).and_utc()
        );
        // 02:30 on the spring-forward night does not exist; it moves on.
        let gap = day(2026, 3, 29).and_hms_opt(2, 30, 0).unwrap();
        assert_eq!(
            zoned_to_utc(&berlin, gap),
            day(2026, 3, 29).and_hms_opt(1, 30, 0).unwrap().and_utc()
        );
    }

    const LOCAL: LocalZone = LocalZone::Fixed(chrono_tz::UTC);

    fn window(from: NaiveDateTime, to: NaiveDateTime) -> (DateTime<Utc>, DateTime<Utc>) {
        (from.and_utc(), to.and_utc())
    }

    #[test]
    fn expand_applies_exdates_overrides_and_cancellations() {
        let events = parse_events(
            "BEGIN:VEVENT\nUID:standup\nSUMMARY:Stand-up\nDTSTART;TZID=Europe/Berlin:20261102T090000\n\
             DURATION:PT15M\nRRULE:FREQ=WEEKLY;BYDAY=MO,WE;COUNT=6\n\
             EXDATE;TZID=Europe/Berlin:20261104T090000\nEND:VEVENT\n\
             BEGIN:VEVENT\nUID:standup\nRECURRENCE-ID;TZID=Europe/Berlin:20261109T090000\n\
             SUMMARY:Stand-up (moved)\nDTSTART;TZID=Europe/Berlin:20261109T110000\nDURATION:PT15M\nEND:VEVENT\n\
             BEGIN:VEVENT\nUID:standup\nRECURRENCE-ID;TZID=Europe/Berlin:20261111T090000\n\
             STATUS:CANCELLED\nDTSTART;TZID=Europe/Berlin:20261111T090000\nEND:VEVENT\n",
        );
        let (from, to) = window(at(2026, 11, 1, 0), at(2026, 12, 31, 0));
        let instances = expand(&events, &LOCAL, from, to, 100);
        let shown: Vec<String> = instances
            .iter()
            .map(|i| {
                format!(
                    "{} {}",
                    events[i.event].summary,
                    i.start.format("%m-%d %H:%M")
                )
            })
            .collect();
        assert_eq!(
            shown,
            [
                "Stand-up 11-02 08:00",
                "Stand-up (moved) 11-09 10:00",
                "Stand-up 11-16 08:00",
                "Stand-up 11-18 08:00",
            ]
        );
        assert_eq!(instances[0].end - instances[0].start, Duration::minutes(15));
    }

    #[test]
    fn expand_keeps_what_overlaps_the_window() {
        let events = parse_events(
            "BEGIN:VEVENT\nSUMMARY:Trip\nDTSTART;VALUE=DATE:20261110\nDTEND;VALUE=DATE:20261113\nEND:VEVENT\n\
             BEGIN:VEVENT\nSUMMARY:Old\nDTSTART:20200101T100000Z\nEND:VEVENT\n\
             BEGIN:VEVENT\nSUMMARY:Later\nDTSTART:20270101T100000Z\nEND:VEVENT\n\
             BEGIN:VEVENT\nSUMMARY:Daily since 2020\nDTSTART:20200101T120000Z\nDURATION:PT1H\nRRULE:FREQ=DAILY\nEND:VEVENT\n\
             BEGIN:VEVENT\nSUMMARY:Extra\nDTSTART:20261111T080000Z\nRDATE:20261112T080000Z\nEND:VEVENT\n",
        );
        let (from, to) = window(at(2026, 11, 11, 0), at(2026, 11, 13, 0));
        let instances = expand(&events, &LOCAL, from, to, 100);
        let shown: Vec<String> = instances
            .iter()
            .map(|i| format!("{} {}", events[i.event].summary, i.start.format("%m-%d")))
            .collect();
        assert_eq!(
            shown,
            [
                "Trip 11-10",
                "Extra 11-11",
                "Daily since 2020 11-11",
                "Extra 11-12",
                "Daily since 2020 11-12",
            ]
        );
        let trip = &instances[0];
        assert!(trip.all_day);
        assert_eq!(
            (trip.first_day, trip.last_day),
            (day(2026, 11, 10), day(2026, 11, 12))
        );
        // The limit is respected.
        assert_eq!(expand(&events, &LOCAL, from, to, 2).len(), 2);
    }

    #[test]
    fn floating_times_follow_the_local_zone() {
        let events =
            parse_events("BEGIN:VEVENT\nSUMMARY:Lunch\nDTSTART:20261111T120000\nEND:VEVENT\n");
        let tokyo = LocalZone::Fixed(chrono_tz::Asia::Tokyo);
        let (from, to) = window(at(2026, 11, 10, 0), at(2026, 11, 13, 0));
        let instances = expand(&events, &tokyo, from, to, 10);
        assert_eq!(instances[0].start, at(2026, 11, 11, 3).and_utc());
        assert_eq!(instances[0].first_day, day(2026, 11, 11));
    }
}
