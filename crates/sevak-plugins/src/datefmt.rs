//! Date and time placeholders for workflow templates.
//!
//! `{date}`, `{time}` and `{datetime}` use the same defaults as snippets;
//! `{isodate}`, `{isotime}` and `{isodatetime}` are fixed ISO 8601 shapes; a
//! `:format` suffix on `date`, `time` or `datetime` is a strftime format
//! (`{date:%d %B %Y}`). A bad format is not a placeholder: it stays as written.
//!
//! The clock is [`now`]; tests pin it with [`set_fixed_now`].

use std::cell::Cell;
use std::fmt::Write as _;

use chrono::format::StrftimeItems;
use chrono::{DateTime, FixedOffset, Local};

const DATE: &str = "%Y-%m-%d";
const TIME: &str = "%H:%M";
const DATETIME: &str = "%Y-%m-%d %H:%M";
const ISO_TIME: &str = "%H:%M:%S";
const ISO_DATETIME: &str = "%Y-%m-%dT%H:%M:%S";

thread_local! {
    static FIXED: Cell<Option<DateTime<FixedOffset>>> = const { Cell::new(None) };
}

/// The current local time, or the time pinned on this thread by
/// [`set_fixed_now`].
pub fn now() -> DateTime<FixedOffset> {
    FIXED
        .with(Cell::get)
        .unwrap_or_else(|| Local::now().fixed_offset())
}

/// Pins [`now`] on the calling thread (`None` releases it). For tests.
pub fn set_fixed_now(time: Option<DateTime<FixedOffset>>) {
    FIXED.with(|fixed| fixed.set(time));
}

/// The strftime format the placeholder written `{inner}` asks for, or `None`
/// when `inner` is not a date placeholder.
fn format_of(inner: &str) -> Option<&str> {
    let inner = inner.trim();
    let (name, format) = match inner.split_once(':') {
        Some((name, format)) => (name.trim(), Some(format)),
        None => (inner, None),
    };
    let default = match name {
        "date" => DATE,
        "time" => TIME,
        "datetime" => DATETIME,
        "isodate" if format.is_none() => DATE,
        "isotime" if format.is_none() => ISO_TIME,
        "isodatetime" if format.is_none() => ISO_DATETIME,
        _ => return None,
    };
    Some(format.filter(|f| !f.is_empty()).unwrap_or(default))
}

/// The text of the placeholder written `{inner}` at time `at`, or `None` when
/// `inner` is not a date placeholder (or its format is invalid).
pub fn placeholder(inner: &str, at: &DateTime<FixedOffset>) -> Option<String> {
    format_time(at, format_of(inner)?)
}

/// [`placeholder`] at [`now`]; the clock is read only for a date placeholder.
pub fn placeholder_now(inner: &str) -> Option<String> {
    format_time(&now(), format_of(inner)?)
}

/// strftime formatting that reports a bad format instead of panicking
/// (`DateTime::format(..).to_string()` panics on one).
pub fn format_time(at: &DateTime<FixedOffset>, format: &str) -> Option<String> {
    let mut formatted = String::new();
    write!(
        formatted,
        "{}",
        at.format_with_items(StrftimeItems::new(format))
    )
    .ok()?;
    Some(formatted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at() -> DateTime<FixedOffset> {
        FixedOffset::east_opt(5 * 3600 + 1800)
            .unwrap()
            .with_ymd_and_hms(2026, 10, 3, 14, 5, 9)
            .unwrap()
    }

    #[test]
    fn named_placeholders() {
        let at = at();
        assert_eq!(placeholder("date", &at).unwrap(), "2026-10-03");
        assert_eq!(placeholder("time", &at).unwrap(), "14:05");
        assert_eq!(placeholder("datetime", &at).unwrap(), "2026-10-03 14:05");
        assert_eq!(placeholder("isodate", &at).unwrap(), "2026-10-03");
        assert_eq!(placeholder("isotime", &at).unwrap(), "14:05:09");
        assert_eq!(
            placeholder("isodatetime", &at).unwrap(),
            "2026-10-03T14:05:09"
        );
    }

    #[test]
    fn formats_and_rejections() {
        let at = at();
        assert_eq!(
            placeholder("date:%d %B %Y", &at).unwrap(),
            "03 October 2026"
        );
        assert_eq!(placeholder("time:%I:%M %p", &at).unwrap(), "02:05 PM");
        assert_eq!(placeholder("date:", &at).unwrap(), "2026-10-03");
        assert_eq!(placeholder("date:%Q", &at), None);
        assert_eq!(placeholder("isodate:%Y", &at), None);
        assert_eq!(placeholder("query", &at), None);
        assert_eq!(placeholder("dates", &at), None);
    }

    #[test]
    fn the_clock_can_be_pinned() {
        set_fixed_now(Some(at()));
        assert_eq!(now(), at());
        set_fixed_now(None);
        assert_ne!(now(), at());
    }
}
