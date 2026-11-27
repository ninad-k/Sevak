//! Calendar: `cal` shows today's and upcoming events from local `.ics` files.
//!
//! | Input | Shows |
//! |---|---|
//! | `cal` | what is on now and in the next `[calendar] days` days |
//! | `cal today`, `cal tomorrow`, `cal yesterday` | that day |
//! | `cal fri`, `cal friday` | the next Friday (today if it is Friday) |
//! | `cal 2026-12-24` | that date |
//! | `cal week` | the next seven days |
//! | `cal <text>`, `cal fri <text>` | events whose title or place matches, in the next year or on that day |
//!
//! | Key | Action |
//! |---|---|
//! | Enter | copy the event's details (title, time, place, link, notes) |
//! | `Ctrl+Enter` | open the `.ics` file it came from |
//! | `Alt+Enter` | show that file in the file manager |
//! | `Shift+Enter` | open the event's link (`http(s)` only), if it has one |
//!
//! # Where the events come from
//!
//! Only the files and folders in `[calendar] files` (a folder contributes the
//! `.ics` files up to three levels below it, as vdirsyncer and other sync
//! tools store them). There is no account, no subscription and no network
//! access. See [`ics`] for what is understood and [`recur`] for repeating
//! events.
//!
//! # Privacy
//!
//! The files are read on a background thread by [`Plugin::refresh`] and the
//! events kept in memory only; the log has counts. Choosing an event is kept
//! out of the usage statistics and the search history
//! ([`Plugin::tracks_usage`]).

pub mod ics;
pub mod recur;

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc, Weekday};
use sevak_core::bounded_read::read_capped;
use sevak_core::config::CalendarConfig;
use sevak_core::model::score;
use sevak_core::{Action, FuzzyQuery, IconSource, Modifier, Plugin, PluginResult, ResultItem};
use sevak_platform::PlatformProvider;

use crate::actions::execute_action;
use crate::files::{expand_home, home_dir};
use crate::local_source::confined;
use ics::{parse_events, Event};
pub use recur::LocalZone;
use recur::{expand, Instance};

const ID: &str = "calendar";
/// Rows returned for one query.
const MAX_ROWS: usize = 30;
/// An `.ics` file larger than this is not read.
const MAX_ICS_BYTES: u64 = 32 * 1024 * 1024;
/// Files read from every configured path together.
const MAX_FILES: usize = 5_000;
/// Folder levels searched for `.ics` files.
const FOLDER_DEPTH: usize = 3;
/// How far ahead occurrences are worked out (and searched by text).
const HORIZON_DAYS: i64 = 400;
/// How far ahead `cal <text>` looks.
const SEARCH_DAYS: i64 = 366;
/// Occurrences kept.
const MAX_INSTANCES: usize = 50_000;
const PAYLOAD_NOTHING: &str = "nothing";
const SETUP_SNIPPET: &str = "[calendar]\nfiles = [\"~/calendar.ics\"]";

/// The current time; replaced in tests.
pub type Clock = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>;

/// What a row shows of an event.
struct Entry {
    summary: String,
    location: Option<String>,
    description: Option<String>,
    url: Option<String>,
    file: PathBuf,
}

#[derive(Default)]
struct Snapshot {
    entries: Vec<Entry>,
    instances: Vec<Instance>,
    files: usize,
    loaded: bool,
}

/// What part of the agenda a query asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Span {
    /// From now, `[calendar] days` ahead.
    Upcoming,
    Day(NaiveDate),
    /// From now, seven days ahead.
    Week,
}

/// The day words at the start of `input`, and the rest as search text.
pub fn parse_span(input: &str, today: NaiveDate) -> (Span, String) {
    let input = input.trim();
    let (first, rest) = input.split_once(char::is_whitespace).unwrap_or((input, ""));
    let lower = first.to_lowercase();
    let span = match lower.as_str() {
        "today" | "tod" | "now" => Some(Span::Day(today)),
        "tomorrow" | "tmr" | "tmrw" | "tom" => today.succ_opt().map(Span::Day),
        "yesterday" => today.pred_opt().map(Span::Day),
        "week" => Some(Span::Week),
        word => weekday(word)
            .map(|wd| {
                let ahead =
                    (7 + wd.num_days_from_monday() - today.weekday().num_days_from_monday()) % 7;
                Span::Day(today + Duration::days(i64::from(ahead)))
            })
            .or_else(|| {
                NaiveDate::parse_from_str(word, "%Y-%m-%d")
                    .ok()
                    .map(Span::Day)
            }),
    };
    match span {
        Some(span) => (span, rest.trim().to_owned()),
        None => (Span::Upcoming, input.to_owned()),
    }
}

fn weekday(word: &str) -> Option<Weekday> {
    Some(match word {
        "mon" | "monday" => Weekday::Mon,
        "tue" | "tues" | "tuesday" => Weekday::Tue,
        "wed" | "weds" | "wednesday" => Weekday::Wed,
        "thu" | "thur" | "thurs" | "thursday" => Weekday::Thu,
        "fri" | "friday" => Weekday::Fri,
        "sat" | "saturday" => Weekday::Sat,
        "sun" | "sunday" => Weekday::Sun,
        _ => return None,
    })
}

/// The `.ics` files the configured paths name: a file as given, or the
/// `.ics` files inside a folder whose real paths stay inside it.
fn calendar_files(paths: &[String], home: Option<&Path>) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = Vec::new();
    for raw in paths {
        let path = expand_home(raw.trim(), home);
        if path.is_dir() {
            let mut found: Vec<PathBuf> = walkdir::WalkDir::new(&path)
                .max_depth(FOLDER_DEPTH)
                .follow_links(false)
                .into_iter()
                .filter_map(Result::ok)
                .filter(|entry| !entry.file_type().is_dir())
                .filter(|entry| {
                    entry
                        .path()
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("ics"))
                })
                .filter_map(|entry| confined(&path, entry.path()))
                .filter(|real| real.is_file())
                .collect();
            found.sort();
            files.extend(found);
        } else if path.is_file() {
            files.push(path);
        }
        if files.len() >= MAX_FILES {
            break;
        }
    }
    let mut seen = std::collections::HashSet::new();
    files.retain(|f| seen.insert(f.canonicalize().unwrap_or_else(|_| f.clone())));
    files.truncate(MAX_FILES);
    files
}

/// The `cal` plugin.
pub struct CalendarPlugin {
    config: CalendarConfig,
    platform: Arc<dyn PlatformProvider>,
    zone: LocalZone,
    clock: Clock,
    home: Option<PathBuf>,
    snapshot: RwLock<Arc<Snapshot>>,
}

impl CalendarPlugin {
    pub fn new(config: &CalendarConfig, platform: Arc<dyn PlatformProvider>) -> Self {
        Self::with_clock(
            config,
            platform,
            LocalZone::System,
            Arc::new(Utc::now),
            home_dir(),
        )
    }

    /// With a fixed time zone and clock (tests).
    pub fn with_clock(
        config: &CalendarConfig,
        platform: Arc<dyn PlatformProvider>,
        zone: LocalZone,
        clock: Clock,
        home: Option<PathBuf>,
    ) -> Self {
        Self {
            config: config.clone(),
            platform,
            zone,
            clock,
            home,
            snapshot: RwLock::new(Arc::new(Snapshot::default())),
        }
    }

    fn snapshot(&self) -> Arc<Snapshot> {
        self.snapshot
            .read()
            .map(|s| Arc::clone(&s))
            .unwrap_or_default()
    }

    /// Reads every file and works out the occurrences around now. Slow;
    /// background thread.
    fn load(&self) -> Snapshot {
        let files = calendar_files(&self.config.files, self.home.as_deref());
        let mut events: Vec<Event> = Vec::new();
        let mut entries: Vec<Entry> = Vec::new();
        for file in &files {
            let bytes = match read_capped(file, MAX_ICS_BYTES) {
                Ok(bytes) => bytes,
                Err(err) => {
                    tracing::debug!(%err, "skipped a calendar file");
                    continue;
                }
            };
            for event in parse_events(&String::from_utf8_lossy(&bytes)) {
                entries.push(Entry {
                    summary: event.summary.clone(),
                    location: event.location.clone(),
                    description: event.description.clone(),
                    url: event.url.clone(),
                    file: file.clone(),
                });
                events.push(event);
            }
        }
        let now = (self.clock)();
        let instances = expand(
            &events,
            &self.zone,
            now - Duration::days(1),
            now + Duration::days(HORIZON_DAYS),
            MAX_INSTANCES,
        );
        tracing::info!(
            files = files.len(),
            events = events.len(),
            occurrences = instances.len(),
            "loaded calendars"
        );
        Snapshot {
            entries,
            instances,
            files: files.len(),
            loaded: true,
        }
    }

    fn status(&self, key: &str, title: &str, subtitle: &str, action: Action) -> ResultItem {
        ResultItem::new(ID, key, title, action)
            .with_subtitle(subtitle)
            .with_icon(IconSource::builtin("calendar"))
            .with_score(score::KEYWORD)
    }

    fn nothing() -> Action {
        Action::Custom {
            payload: PAYLOAD_NOTHING.to_owned(),
        }
    }

    /// `Today`, `Tomorrow`, `Fri 20 Nov` (with the year when it is not this one).
    fn day_label(&self, date: NaiveDate, today: NaiveDate) -> String {
        if date == today {
            "Today".to_owned()
        } else if Some(date) == today.succ_opt() {
            "Tomorrow".to_owned()
        } else if Some(date) == today.pred_opt() {
            "Yesterday".to_owned()
        } else if date.year() == today.year() {
            date.format("%a %-d %b").to_string()
        } else {
            date.format("%a %-d %b %Y").to_string()
        }
    }

    fn when_text(&self, instance: &Instance, today: NaiveDate) -> String {
        let first = self.day_label(instance.first_day, today);
        if instance.all_day {
            return if instance.first_day == instance.last_day {
                format!("{first}, all day")
            } else {
                format!(
                    "{first} – {}, all day",
                    self.day_label(instance.last_day, today)
                )
            };
        }
        let start = self.zone.local(instance.start).format("%H:%M");
        let end = self.zone.local(instance.end).format("%H:%M");
        if instance.end <= instance.start {
            format!("{first} {start}")
        } else if instance.first_day == instance.last_day {
            format!("{first} {start}–{end}")
        } else {
            format!(
                "{first} {start} – {} {end}",
                self.day_label(instance.last_day, today)
            )
        }
    }

    fn row(&self, instance: &Instance, entry: &Entry, today: NaiveDate) -> ResultItem {
        let title = if entry.summary.is_empty() {
            "(no title)".to_owned()
        } else {
            entry.summary.clone()
        };
        let when = self.when_text(instance, today);
        let file_name = entry
            .file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut subtitle = vec![when.clone()];
        subtitle.extend(entry.location.iter().map(|l| l.replace('\n', " ")));
        subtitle.push(file_name);

        let mut details = vec![title.clone(), when.clone()];
        details.extend(entry.location.clone());
        details.extend(entry.url.clone());
        if let Some(description) = &entry.description {
            details.push(String::new());
            details.push(description.clone());
        }

        let mut item = ResultItem::new(
            ID,
            format!("{}@{}", instance.event, instance.start.timestamp()),
            &title,
            Action::CopyText {
                text: details.join("\n"),
            },
        )
        .with_subtitle(subtitle.join(" · "))
        .with_icon(IconSource::builtin("calendar"))
        .with_secondary(
            "Open calendar file",
            Some(Modifier::Ctrl),
            Action::OpenPath {
                path: entry.file.clone(),
            },
        )
        .with_secondary(
            "Show calendar file in folder",
            Some(Modifier::Alt),
            Action::RevealPath {
                path: entry.file.clone(),
            },
        )
        .with_large_text(format!("{title}\n{when}"));
        let link = entry
            .url
            .clone()
            .or_else(|| entry.location.clone())
            .filter(|u| {
                let lower = u.to_ascii_lowercase();
                (lower.starts_with("https://") || lower.starts_with("http://"))
                    && !u.contains(char::is_whitespace)
            });
        if let Some(url) = link {
            item = item.with_secondary("Open link", Some(Modifier::Shift), Action::OpenUrl { url });
        }
        item
    }
}

impl Plugin for CalendarPlugin {
    fn id(&self) -> &str {
        ID
    }

    fn name(&self) -> &str {
        "Calendar"
    }

    fn description(&self) -> &str {
        "Type `cal` for today's and upcoming events from local .ics files (`cal tomorrow`, `cal fri`). No accounts, no network."
    }

    fn keyword(&self) -> Option<&str> {
        Some(&self.config.keyword)
    }

    fn global(&self) -> bool {
        false
    }

    fn query(&self, input: &str) -> Vec<ResultItem> {
        if self.config.files.is_empty() {
            return vec![self.status(
                "setup",
                "No calendar files yet",
                "Enter copies the setting to add to config.toml: list your .ics files or folders",
                Action::CopyText {
                    text: SETUP_SNIPPET.to_owned(),
                },
            )];
        }
        let snapshot = self.snapshot();
        if !snapshot.loaded {
            return vec![self.status(
                "loading",
                "Reading calendars…",
                "Try again in a moment",
                Self::nothing(),
            )];
        }
        if snapshot.files == 0 {
            return vec![self.status(
                "none",
                "No .ics files found",
                "Check the paths in [calendar] files, then choose Reload index",
                Self::nothing(),
            )];
        }

        let now = (self.clock)();
        let today = self.zone.local(now).date();
        let (span, text) = parse_span(input, today);
        let days = i64::from(self.config.days);
        let (from, to, empty) = match span {
            Span::Upcoming if !text.is_empty() => (
                now,
                now + Duration::days(SEARCH_DAYS),
                "No upcoming events match".to_owned(),
            ),
            Span::Upcoming => (
                now,
                self.zone.midnight(today + Duration::days(days)),
                if days == 1 {
                    "Nothing more today".to_owned()
                } else {
                    format!("Nothing in the next {days} days")
                },
            ),
            Span::Week => (
                now,
                self.zone.midnight(today + Duration::days(7)),
                "Nothing in the next 7 days".to_owned(),
            ),
            Span::Day(date) => (
                self.zone.midnight(date),
                self.zone.midnight(date + Duration::days(1)),
                format!("Nothing on {}", date.format("%A %-d %B %Y")),
            ),
        };

        let mut query = (!text.is_empty()).then(|| FuzzyQuery::new(&text));
        let rows: Vec<ResultItem> = snapshot
            .instances
            .iter()
            .filter(|i| i.start < to && (i.end > from || i.start >= from))
            .filter(|i| {
                let Some(query) = query.as_mut() else {
                    return true;
                };
                let entry = &snapshot.entries[i.event];
                query.score(&entry.summary).is_some()
                    || entry
                        .location
                        .as_deref()
                        .is_some_and(|l| query.score(l).is_some())
            })
            .take(MAX_ROWS)
            .enumerate()
            .map(|(rank, instance)| {
                self.row(instance, &snapshot.entries[instance.event], today)
                    .with_score(score::KEYWORD - rank as f64)
            })
            .collect();
        if rows.is_empty() {
            return vec![self.status("empty", &empty, "Enter does nothing", Self::nothing())];
        }
        rows
    }

    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        match &item.action {
            Action::Custom { payload } if payload == PAYLOAD_NOTHING => Ok(()),
            action => execute_action(self.platform.as_ref(), action),
        }
    }

    /// Event titles and times stay out of `usage.json` and the search history.
    fn tracks_usage(&self) -> bool {
        false
    }

    fn refresh(&self) -> PluginResult<()> {
        if self.config.files.is_empty() {
            return Ok(());
        }
        let snapshot = self.load();
        if let Ok(mut slot) = self.snapshot.write() {
            *slot = Arc::new(snapshot);
        }
        Ok(())
    }

    fn index_size(&self) -> Option<usize> {
        Some(self.snapshot().entries.len())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use chrono::NaiveDateTime;

    use super::*;
    use crate::test_util::MockPlatform;

    /// Sunday 15 November 2026, 10:00 in Berlin (09:00 UTC).
    fn now() -> DateTime<Utc> {
        NaiveDateTime::parse_from_str("2026-11-15 09:00", "%Y-%m-%d %H:%M")
            .unwrap()
            .and_utc()
    }

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    const WORK: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\n\
BEGIN:VEVENT\r\nUID:standup@work\r\nSUMMARY:Stand-up\r\nLOCATION:https://meet.example/abc\r\n\
DTSTART;TZID=Europe/Berlin:20261102T093000\r\nDTEND;TZID=Europe/Berlin:20261102T094500\r\n\
RRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:review@work\r\nSUMMARY:Design review\r\nLOCATION:Room 4\r\n\
DESCRIPTION:Bring the\\nmock-ups\r\nURL:https://wiki.example/review\r\n\
DTSTART:20261120T130000Z\r\nDURATION:PT1H\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nSUMMARY:Earlier today\r\nDTSTART;TZID=Europe/Berlin:20261115T080000\r\nDURATION:PT30M\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nSUMMARY:Later today\r\nDTSTART;TZID=Europe/Berlin:20261115T180000\r\nDURATION:PT2H\r\nEND:VEVENT\r\n\
END:VCALENDAR\r\n";

    const HOME: &str = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nSUMMARY:Holiday\nDTSTART;VALUE=DATE:20261124\n\
DTEND;VALUE=DATE:20261127\nEND:VEVENT\nBEGIN:VEVENT\nSUMMARY:Birthday\nDTSTART;VALUE=DATE:20001116\n\
RRULE:FREQ=YEARLY\nEND:VEVENT\nEND:VCALENDAR\n";

    struct Fixture {
        dir: tempfile::TempDir,
        plugin: CalendarPlugin,
        platform: Arc<MockPlatform>,
    }

    fn fixture_with(files: impl FnOnce(&Path) -> Vec<String>, days: u32) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let platform = MockPlatform::empty();
        let config = CalendarConfig {
            files: files(dir.path()),
            days,
            ..CalendarConfig::default()
        };
        let plugin = CalendarPlugin::with_clock(
            &config,
            platform.clone(),
            LocalZone::Fixed(chrono_tz::Europe::Berlin),
            Arc::new(now),
            Some(dir.path().to_path_buf()),
        );
        plugin.refresh().unwrap();
        Fixture {
            dir,
            plugin,
            platform,
        }
    }

    /// `work.ics` as a file (with `~`) and a folder holding `home.ics`.
    fn fixture() -> Fixture {
        fixture_with(
            |dir| {
                fs::write(dir.join("work.ics"), WORK).unwrap();
                let folder = dir.join("calendars").join("family");
                fs::create_dir_all(&folder).unwrap();
                fs::write(folder.join("home.ics"), HOME).unwrap();
                fs::write(folder.join("notes.txt"), HOME).unwrap();
                vec![
                    "~/work.ics".into(),
                    dir.join("calendars").display().to_string(),
                ]
            },
            7,
        )
    }

    fn titles(rows: &[ResultItem]) -> Vec<String> {
        rows.iter()
            .map(|r| {
                format!(
                    "{} | {}",
                    r.title,
                    r.subtitle.split(" · ").next().unwrap_or("")
                )
            })
            .collect()
    }

    #[test]
    fn metadata() {
        let f = fixture();
        assert_eq!(f.plugin.id(), "calendar");
        assert_eq!(f.plugin.keyword(), Some("cal"));
        assert!(!f.plugin.global());
        assert!(!f.plugin.tracks_usage());
        assert_eq!(f.plugin.index_size(), Some(6));
    }

    #[test]
    fn day_words_and_dates() {
        let sunday = day(2026, 11, 15);
        assert_eq!(parse_span("", sunday), (Span::Upcoming, String::new()));
        assert_eq!(
            parse_span("tomorrow", sunday),
            (Span::Day(day(2026, 11, 16)), String::new())
        );
        assert_eq!(
            parse_span("Fri review", sunday),
            (Span::Day(day(2026, 11, 20)), "review".into())
        );
        assert_eq!(
            parse_span("sun", sunday),
            (Span::Day(sunday), String::new())
        );
        assert_eq!(
            parse_span("2026-12-24", sunday).0,
            Span::Day(day(2026, 12, 24))
        );
        assert_eq!(parse_span("week", sunday).0, Span::Week);
        assert_eq!(
            parse_span("standup", sunday),
            (Span::Upcoming, "standup".into())
        );
        assert_eq!(
            parse_span("yesterday", sunday).0,
            Span::Day(day(2026, 11, 14))
        );
    }

    #[test]
    fn the_agenda_shows_what_is_left_today_and_the_next_days() {
        let f = fixture();
        let rows = f.plugin.query("");
        let shown = titles(&rows);
        // "Earlier today" (08:00-08:30) is over; the birthday is all day tomorrow.
        assert_eq!(
            shown[..4],
            [
                "Later today | Today 18:00–20:00",
                "Birthday | Tomorrow, all day",
                "Stand-up | Tomorrow 09:30–09:45",
                "Stand-up | Tue 17 Nov 09:30–09:45",
            ]
        );
        assert!(shown.iter().all(|s| !s.starts_with("Earlier today")));
        assert!(shown
            .iter()
            .any(|s| s == "Design review | Fri 20 Nov 14:00–15:00"));
        // Seven days: up to and including Saturday 21 November.
        assert!(shown.iter().all(|s| !s.contains("Holiday")));
        assert!(rows.windows(2).all(|w| w[0].score > w[1].score));
    }

    #[test]
    fn a_day_a_weekday_and_a_search() {
        let f = fixture();
        assert_eq!(
            titles(&f.plugin.query("today")),
            [
                "Earlier today | Today 08:00–08:30",
                "Later today | Today 18:00–20:00"
            ]
        );
        assert_eq!(
            titles(&f.plugin.query("fri")),
            [
                "Stand-up | Fri 20 Nov 09:30–09:45",
                "Design review | Fri 20 Nov 14:00–15:00"
            ]
        );
        assert_eq!(
            titles(&f.plugin.query("wed")),
            ["Stand-up | Wed 18 Nov 09:30–09:45",]
        );
        assert_eq!(
            titles(&f.plugin.query("2026-11-25")),
            [
                "Holiday | Tue 24 Nov – Thu 26 Nov, all day",
                "Stand-up | Wed 25 Nov 09:30–09:45"
            ]
        );
        assert_eq!(
            titles(&f.plugin.query("review")),
            ["Design review | Fri 20 Nov 14:00–15:00"]
        );
        assert_eq!(titles(&f.plugin.query("fri stand")).len(), 1);
        let none = f.plugin.query("2026-11-28");
        assert_eq!(none.len(), 1);
        assert_eq!(none[0].title, "Nothing on Saturday 28 November 2026");
        f.plugin.execute(&none[0]).unwrap();
        assert_eq!(
            f.plugin.query("zzzzqq")[0].title,
            "No upcoming events match"
        );
    }

    #[test]
    fn enter_copies_the_details_and_the_panel_opens_the_file_or_link() {
        let f = fixture();
        let review = f.plugin.query("review").remove(0);
        f.plugin.execute(&review).unwrap();
        assert_eq!(
            *f.platform.clipboard.lock().unwrap(),
            ["Design review\nFri 20 Nov 14:00–15:00\nRoom 4\nhttps://wiki.example/review\n\nBring the\nmock-ups"]
        );
        assert!(review.subtitle.ends_with("Room 4 · work.ics"));
        for modifier in [Modifier::Ctrl, Modifier::Alt, Modifier::Shift] {
            let index = review
                .secondary
                .iter()
                .position(|s| s.modifier == Some(modifier))
                .unwrap();
            f.plugin
                .execute(&review.secondary_as_primary(index).unwrap())
                .unwrap();
        }
        let work = f.dir.path().join("work.ics");
        assert_eq!(
            *f.platform.opened_paths.lock().unwrap(),
            std::slice::from_ref(&work)
        );
        assert_eq!(*f.platform.revealed.lock().unwrap(), [work]);
        assert_eq!(
            *f.platform.opened_urls.lock().unwrap(),
            ["https://wiki.example/review"]
        );
        // A meeting link in LOCATION is offered too.
        let standup = f.plugin.query("stand").remove(0);
        assert!(standup.secondary.iter().any(|s| s.action
            == Action::OpenUrl {
                url: "https://meet.example/abc".into()
            }));
    }

    #[test]
    fn without_files_it_says_how_to_add_them() {
        let f = fixture_with(|_| Vec::new(), 7);
        let rows = f.plugin.query("");
        assert_eq!(rows[0].title, "No calendar files yet");
        assert_eq!(
            rows[0].action,
            Action::CopyText {
                text: SETUP_SNIPPET.to_owned()
            }
        );
        let missing = fixture_with(|dir| vec![dir.join("nope.ics").display().to_string()], 7);
        assert_eq!(missing.plugin.query("")[0].title, "No .ics files found");
    }

    #[test]
    fn one_day_ahead_and_nothing_left() {
        let f = fixture_with(
            |dir| {
                fs::write(
                    dir.join("a.ics"),
                    "BEGIN:VEVENT\nSUMMARY:Done\nDTSTART:20261115T070000Z\nEND:VEVENT\n",
                )
                .unwrap();
                vec![dir.join("a.ics").display().to_string()]
            },
            1,
        );
        assert_eq!(f.plugin.query("")[0].title, "Nothing more today");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_out_of_a_calendar_folder_is_not_followed() {
        let f = fixture_with(
            |dir| {
                let folder = dir.join("cals");
                fs::create_dir(&folder).unwrap();
                fs::write(dir.join("outside.ics"), WORK).unwrap();
                std::os::unix::fs::symlink(dir.join("outside.ics"), folder.join("link.ics"))
                    .unwrap();
                vec![folder.display().to_string()]
            },
            7,
        );
        assert_eq!(f.plugin.query("")[0].title, "No .ics files found");
    }
}
