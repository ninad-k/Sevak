//! Automation tasks: toggle dark mode, show the desktop, mute and set the
//! volume, take a screenshot, quit an app, kill a process, eject a drive, keep
//! the computer awake and more.
//!
//! A plugin with the keyword `t` that also answers plain queries by name, like
//! the system commands (`dark mode`, `mute`, `screenshot`). Besides the fixed
//! tasks it understands a few typed commands:
//!
//! | Typed | Offers |
//! |---|---|
//! | `quit [app]` | the running apps with windows; Enter asks one to quit, Shift+Enter force quits |
//! | `force quit [app]` | the same list; Enter force quits (after asking) |
//! | `kill [name]` | the running processes by name with CPU and memory; Enter ends them (after asking) |
//! | `eject [drive]` | the removable drives |
//! | `vol 30`, `vol up` | set or nudge the volume |
//! | `awake 30`, `awake 2h` | keep the computer awake for that long |
//!
//! What is offered depends on the machine (the platform provider reports which
//! tasks can work), and `[tasks] disabled` hides entries the user does not want
//! near the Enter key.
//!
//! The lists of apps, processes and drives are OS round trips, so a query only
//! reads the last answer from a [`Cache`] and starts a background refresh when
//! it is stale; the notifier makes the shell show the fresh list a moment
//! later. Typing never waits for the OS.
//!
//! Activation is two-phase like the system commands. [`Plugin::confirmation`]
//! lets the shell ask first for force quit, kill and restarting the shell
//! (unless `[tasks] confirm = false`), then [`Plugin::execute`] calls the
//! platform provider. Every result's action is an [`Action::Custom`] naming the
//! task in its [`Task::to_key`] spelling, which the platform validates again, so
//! the provider only ever receives a variant of its closed vocabulary.

use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use sevak_core::config::TasksConfig;
use sevak_core::model::score;
use sevak_core::{
    Action, FuzzyQuery, IconSource, Modifier, Plugin, PluginError, PluginResult, ResultItem,
    ResultsNotifier,
};
use sevak_platform::tasks::MAX_KEEP_AWAKE_MINUTES;
use sevak_platform::{Drive, PlatformProvider, ProcessInfo, RunningApp, Task, TaskKind};

use crate::apps::name_bonus;
use crate::live::Cache;

/// Plain-query matches shorter than this match nothing: two letters is enough
/// to mean `dark`, one would put every task above the applications.
const MIN_QUERY_CHARS: usize = 2;
/// Aliases are weaker evidence than the title.
const ALIAS_WEIGHT: f64 = 0.9;
/// Breaks ties in favour of shorter (more specific) titles, as for apps.
const LENGTH_PENALTY: f64 = 0.01;
/// How long the app, process and drive lists are trusted before the next query
/// starts a refresh.
const LIST_TTL: Duration = Duration::from_secs(3);
/// Most rows a typed command lists (the engine truncates to
/// `[search] max_results` anyway).
const MAX_LIVE_ROWS: usize = 20;

const PLUGIN_ID: &str = "tasks";
/// Prefix of the [`Action::Custom`] payload.
const PAYLOAD_PREFIX: &str = "task:";
/// Payload of the rows that only explain what to type.
const HINT_PAYLOAD: &str = "task:hint";
/// Keep-awake lengths offered by a bare `awake`, in minutes.
const AWAKE_PRESETS: [u32; 6] = [15, 30, 60, 120, 240, 480];
/// Volumes offered by a bare `vol`.
const VOLUME_PRESETS: [u8; 4] = [25, 50, 75, 100];

/// A row's static description.
struct Description {
    title: String,
    subtitle: String,
    /// Other names the task answers to (lowercase).
    aliases: &'static [&'static str],
    icon: &'static str,
}

fn describe(task: &Task) -> Description {
    let d = |title: &str, subtitle: &str, aliases: &'static [&'static str], icon: &'static str| {
        Description {
            title: title.to_owned(),
            subtitle: subtitle.to_owned(),
            aliases,
            icon,
        }
    };
    match task {
        Task::ToggleDarkMode => d(
            "Toggle dark mode",
            "Switch between the light and dark appearance",
            &[
                "dark mode",
                "light mode",
                "dark",
                "light",
                "theme",
                "night mode",
                "appearance",
            ],
            "theme",
        ),
        Task::ShowDesktop => d(
            "Show desktop",
            "Show or hide all windows",
            &["desktop", "hide windows", "peek desktop"],
            "desktop",
        ),
        Task::HideOthers => d(
            "Hide other apps",
            "Hide or minimize everything except the app in front",
            &[
                "hide others",
                "hide other windows",
                "minimize others",
                "focus mode",
            ],
            "desktop",
        ),
        Task::MinimizeAll => d(
            "Minimize all windows",
            "Minimize every open window",
            &["minimize all", "hide all windows", "minimise all"],
            "desktop",
        ),
        Task::Screenshot => d(
            "Take a screenshot",
            "Open the system screenshot tool",
            &[
                "screenshot",
                "screen capture",
                "snip",
                "snipping tool",
                "capture screen",
            ],
            "camera",
        ),
        Task::OpenDownloads => d(
            "Open Downloads folder",
            "Show your Downloads folder",
            &["downloads", "open downloads"],
            "folder",
        ),
        Task::OpenRecentFiles => d(
            "Open recent files",
            "Show the files you opened lately",
            &["recent", "recents", "recent items", "recent documents"],
            "folder",
        ),
        Task::FlushDns => d(
            "Flush DNS cache",
            "Forget cached name lookups (may ask for permission)",
            &["flush dns", "clear dns", "reset dns", "dns"],
            "wifi",
        ),
        Task::RestartShell if cfg!(windows) => d(
            "Restart Explorer",
            "Restart the taskbar and File Explorer",
            &[
                "restart explorer",
                "explorer",
                "restart taskbar",
                "restart shell",
            ],
            "restart",
        ),
        Task::RestartShell => d(
            "Restart Finder",
            "Relaunch Finder",
            &[
                "restart finder",
                "relaunch finder",
                "finder",
                "restart shell",
            ],
            "restart",
        ),
        Task::EmptyClipboard => d(
            "Empty clipboard",
            "Clear whatever you copied",
            &["clear clipboard", "wipe clipboard"],
            "copy",
        ),
        Task::Mute => d(
            "Mute volume",
            "Silence the speakers",
            &["mute", "silence"],
            "volume",
        ),
        Task::Unmute => d(
            "Unmute volume",
            "Turn the sound back on",
            &["unmute"],
            "volume",
        ),
        Task::VolumeUp => d(
            "Volume up",
            "Raise the volume by 10%",
            &["louder", "increase volume", "raise volume", "volume +"],
            "volume",
        ),
        Task::VolumeDown => d(
            "Volume down",
            "Lower the volume by 10%",
            &["quieter", "decrease volume", "lower volume", "volume -"],
            "volume",
        ),
        Task::SetVolume(percent) => d_owned(
            format!("Set volume to {percent}%"),
            "Change the output volume",
            "volume",
        ),
        Task::ToggleWifi => d(
            "Toggle Wi-Fi",
            "Turn the Wi-Fi radio on or off",
            &["wifi", "wi-fi", "wireless", "wifi on", "wifi off"],
            "wifi",
        ),
        Task::ToggleBluetooth => d(
            "Toggle Bluetooth",
            "Turn the Bluetooth radio on or off",
            &["bluetooth on", "bluetooth off", "bt"],
            "bluetooth",
        ),
        Task::KeepAwake(minutes) => Description {
            title: format!("Keep awake for {}", format_minutes(*minutes)),
            subtitle: "Stop the computer from sleeping or turning the screen off".to_owned(),
            aliases: &[
                "caffeinate",
                "keep awake",
                "stay awake",
                "prevent sleep",
                "awake",
            ],
            icon: "bolt",
        },
        Task::StopKeepAwake => d(
            "Stop keeping awake",
            "Let the computer sleep again",
            &["allow sleep", "decaffeinate", "stop caffeinate"],
            "bolt",
        ),
        Task::QuitApp(name) => d_owned(
            format!("Quit {}", display_name(name)),
            "Ask the app to quit",
            "kill",
        ),
        Task::ForceQuitApp(name) => d_owned(
            format!("Force quit {}", display_name(name)),
            "End the app at once; unsaved work is lost",
            "kill",
        ),
        Task::KillProcess(name) => d_owned(
            format!("Kill {}", display_name(name)),
            "End every process with this name",
            "kill",
        ),
        Task::Eject(id) => d_owned(format!("Eject {id}"), "Safely remove the drive", "eject"),
    }
}

fn d_owned(title: String, subtitle: &str, icon: &'static str) -> Description {
    Description {
        title,
        subtitle: subtitle.to_owned(),
        aliases: &[],
        icon,
    }
}

/// The tasks that are listed by name: every kind that needs no data, plus a
/// one-hour keep-awake (other lengths are typed: `awake 30`).
fn static_tasks() -> Vec<Task> {
    vec![
        Task::ToggleDarkMode,
        Task::ShowDesktop,
        Task::HideOthers,
        Task::MinimizeAll,
        Task::Screenshot,
        Task::OpenDownloads,
        Task::OpenRecentFiles,
        Task::FlushDns,
        Task::RestartShell,
        Task::EmptyClipboard,
        Task::Mute,
        Task::Unmute,
        Task::VolumeUp,
        Task::VolumeDown,
        Task::ToggleWifi,
        Task::ToggleBluetooth,
        Task::KeepAwake(60),
        Task::StopKeepAwake,
    ]
}

struct Entry {
    task: Task,
    title: String,
    title_lower: String,
    aliases: Vec<String>,
}

fn entry(task: Task) -> Entry {
    let d = describe(&task);
    Entry {
        task,
        title_lower: d.title.to_lowercase(),
        title: d.title,
        aliases: d.aliases.iter().map(|alias| (*alias).to_owned()).collect(),
    }
}

/// What the last refresh found.
#[derive(Default)]
struct Offer {
    supported: Vec<TaskKind>,
    entries: Vec<Entry>,
}

/// One row of the process list: every process with the same name.
#[derive(Debug, Clone, PartialEq, Default)]
struct ProcessGroup {
    /// As the OS reports it (`chrome.exe`); what [`Task::KillProcess`] takes.
    name: String,
    count: usize,
    /// Share of the machine's CPU, 0 to 100.
    cpu: f32,
    memory: u64,
}

/// Groups processes by name (case-insensitively), biggest memory use first.
fn group_processes(processes: Vec<ProcessInfo>) -> Vec<ProcessGroup> {
    let mut groups: Vec<ProcessGroup> = Vec::new();
    for process in processes {
        match groups
            .iter_mut()
            .find(|group| group.name.eq_ignore_ascii_case(&process.name))
        {
            Some(group) => {
                group.count += 1;
                group.cpu += process.cpu_percent;
                group.memory += process.memory_bytes;
            }
            None => groups.push(ProcessGroup {
                name: process.name,
                count: 1,
                cpu: process.cpu_percent,
                memory: process.memory_bytes,
            }),
        }
    }
    groups.sort_by(|a, b| {
        b.memory
            .cmp(&a.memory)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    groups
}

/// `chrome.exe` shown as `chrome`.
fn display_name(name: &str) -> &str {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".exe") {
        &name[..name.len() - 4]
    } else {
        name
    }
}

/// `1.2 GB`, `340 MB`, `12 KB`.
fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let value = bytes as f64;
    if value >= KB * KB * KB {
        format!("{:.1} GB", value / (KB * KB * KB))
    } else if value >= KB * KB {
        format!("{:.0} MB", value / (KB * KB))
    } else {
        format!("{:.0} KB", (value / KB).max(1.0))
    }
}

fn format_cpu(percent: f32) -> String {
    if percent < 0.1 {
        "0% CPU".to_owned()
    } else if percent < 10.0 {
        format!("{percent:.1}% CPU")
    } else {
        format!("{percent:.0}% CPU")
    }
}

/// `30 minutes`, `1 hour`, `1 hour 30 minutes`.
fn format_minutes(minutes: u32) -> String {
    let plural = |n: u32, unit: &str| format!("{n} {unit}{}", if n == 1 { "" } else { "s" });
    match (minutes / 60, minutes % 60) {
        (0, m) => plural(m, "minute"),
        (h, 0) => plural(h, "hour"),
        (h, m) => format!("{} {}", plural(h, "hour"), plural(m, "minute")),
    }
}

/// A typed command and what follows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Intent<'a> {
    Quit(&'a str),
    ForceQuit(&'a str),
    Kill(&'a str),
    Eject(&'a str),
    Volume(&'a str),
    Awake(&'a str),
}

/// Splits `text` at its first whitespace run.
fn split_word(text: &str) -> (&str, &str) {
    let text = text.trim();
    match text.find(char::is_whitespace) {
        Some(at) => (&text[..at], text[at..].trim_start()),
        None => (text, ""),
    }
}

/// Recognizes the typed commands by their first word (`kill chrome`,
/// `force quit slack`, `vol 30`). Partial words (`kil`) are not commands.
fn parse_intent(input: &str) -> Option<Intent<'_>> {
    let (word, rest) = split_word(input);
    match word.to_lowercase().as_str() {
        "quit" => Some(Intent::Quit(rest)),
        "force" => {
            let (next, rest) = split_word(rest);
            next.eq_ignore_ascii_case("quit")
                .then_some(Intent::ForceQuit(rest))
        }
        "forcequit" | "force-quit" | "fquit" | "fq" => Some(Intent::ForceQuit(rest)),
        "kill" | "killall" | "terminate" => Some(Intent::Kill(rest)),
        "eject" | "unmount" | "umount" => Some(Intent::Eject(rest)),
        "vol" | "volume" => Some(Intent::Volume(rest)),
        "awake" | "caffeinate" | "caffeine" | "keepawake" => Some(Intent::Awake(rest)),
        "keep" => {
            let (next, rest) = split_word(rest);
            next.eq_ignore_ascii_case("awake")
                .then_some(Intent::Awake(rest))
        }
        _ => None,
    }
}

/// What was typed after `vol`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VolumeArg {
    Nothing,
    Set(u8),
    Up,
    Down,
    Mute,
    Unmute,
    Invalid,
}

/// `30`, `30%`, `up`, `down`, `+`, `-`, `mute`, `unmute`. Anything above 100 is
/// invalid rather than clamped, so `vol 300` does not silently mean `100`.
fn parse_volume(rest: &str) -> VolumeArg {
    let rest = rest.trim();
    if rest.is_empty() {
        return VolumeArg::Nothing;
    }
    match rest.to_lowercase().as_str() {
        "up" | "+" | "louder" => return VolumeArg::Up,
        "down" | "-" | "quieter" => return VolumeArg::Down,
        "mute" | "off" => return VolumeArg::Mute,
        "unmute" | "on" => return VolumeArg::Unmute,
        _ => {}
    }
    let digits = rest.strip_suffix('%').unwrap_or(rest).trim_end();
    if digits.is_empty() || digits.len() > 3 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return VolumeArg::Invalid;
    }
    match digits.parse::<u8>() {
        Ok(percent) if percent <= 100 => VolumeArg::Set(percent),
        _ => VolumeArg::Invalid,
    }
}

/// A keep-awake length in minutes: `30`, `30m`, `45 min`, `2h`, `1.5 hours`.
/// Bare numbers are minutes. `None` outside 1 minute to a day.
fn parse_minutes(rest: &str) -> Option<u32> {
    let rest = rest.trim().to_lowercase();
    let number_end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(rest.len());
    let (number, unit) = rest.split_at(number_end);
    if number.is_empty() || number.matches('.').count() > 1 || number.len() > 8 {
        return None;
    }
    let value: f64 = number.parse().ok()?;
    let per_unit = match unit.trim() {
        "" | "m" | "min" | "mins" | "minute" | "minutes" => 1.0,
        "h" | "hr" | "hrs" | "hour" | "hours" => 60.0,
        _ => return None,
    };
    let minutes = (value * per_unit).round();
    (minutes >= 1.0 && minutes <= f64::from(MAX_KEEP_AWAKE_MINUTES)).then_some(minutes as u32)
}

/// The automation tasks this machine offers.
pub struct TasksPlugin {
    platform: Arc<dyn PlatformProvider>,
    config: TasksConfig,
    keyword: Option<String>,
    /// Rebuilt by [`Plugin::refresh`]; queries read a snapshot.
    offer: RwLock<Arc<Offer>>,
    processes: Arc<Cache<Vec<ProcessGroup>>>,
    apps: Arc<Cache<Vec<RunningApp>>>,
    drives: Arc<Cache<Vec<Drive>>>,
    notifier: OnceLock<ResultsNotifier>,
}

impl TasksPlugin {
    pub fn new(config: TasksConfig, platform: Arc<dyn PlatformProvider>) -> Self {
        let keyword = Some(config.keyword.trim().to_owned()).filter(|k| !k.is_empty());
        Self {
            platform,
            config,
            keyword,
            offer: RwLock::new(Arc::new(Offer::default())),
            processes: Cache::new(),
            apps: Cache::new(),
            drives: Cache::new(),
            notifier: OnceLock::new(),
        }
    }

    fn offer(&self) -> Arc<Offer> {
        self.offer
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Whether this kind is available here and not switched off.
    fn offers(&self, offer: &Offer, kind: TaskKind) -> bool {
        offer.supported.contains(&kind) && !self.config.is_disabled(kind.key())
    }

    fn notifier(&self) -> Option<ResultsNotifier> {
        self.notifier.get().cloned()
    }

    fn row(&self, offer: &Offer, task: &Task, score: f64) -> ResultItem {
        let d = describe(task);
        let mut item = ResultItem::new(
            PLUGIN_ID,
            task.to_key(),
            d.title,
            Action::Custom {
                payload: format!("{PAYLOAD_PREFIX}{}", task.to_key()),
            },
        )
        .with_subtitle(d.subtitle)
        .with_icon(IconSource::builtin(d.icon))
        .with_score(score);
        if let Task::QuitApp(name) = task {
            item = self.with_force_quit(offer, item, name);
        }
        item
    }

    /// Adds "Force quit" as the Shift action of a quit row.
    fn with_force_quit(&self, offer: &Offer, item: ResultItem, name: &str) -> ResultItem {
        if !self.offers(offer, TaskKind::ForceQuitApp) {
            return item;
        }
        item.with_secondary(
            "Force quit",
            Some(Modifier::Shift),
            Action::Custom {
                payload: format!(
                    "{PAYLOAD_PREFIX}{}",
                    Task::ForceQuitApp(name.to_owned()).to_key()
                ),
            },
        )
    }

    /// Rows for the fixed tasks that match `input`; with nothing typed (the
    /// keyword alone), all of them plus a hint for each typed command.
    fn named_rows(&self, offer: &Offer, input: &str) -> Vec<ResultItem> {
        let shown = |entry: &&Entry| self.offers(offer, entry.task.kind());
        if input.is_empty() {
            let mut rows: Vec<ResultItem> = offer
                .entries
                .iter()
                .filter(shown)
                .enumerate()
                .map(|(i, e)| self.row(offer, &e.task, score::KEYWORD - 100.0 - i as f64))
                .collect();
            rows.extend(self.hint_rows(offer));
            return rows;
        }
        if input.chars().count() < MIN_QUERY_CHARS {
            return Vec::new();
        }
        let mut query = FuzzyQuery::new(input);
        if query.is_empty() {
            return Vec::new();
        }
        let query_lower = input.to_lowercase();
        let mut scored: Vec<(f64, &Entry)> = Vec::new();
        for entry in offer.entries.iter().filter(shown) {
            let mut best = query.score(&entry.title).map(|s| {
                f64::from(s) + name_bonus(&entry.title_lower, &query_lower)
                    - entry.title_lower.len() as f64 * LENGTH_PENALTY
            });
            for alias in &entry.aliases {
                if let Some(s) = query.score(alias) {
                    let s = f64::from(s) * ALIAS_WEIGHT + name_bonus(alias, &query_lower);
                    best = Some(best.map_or(s, |b| b.max(s)));
                }
            }
            if let Some(best) = best {
                scored.push((best, entry));
            }
        }
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        scored
            .into_iter()
            .map(|(score, entry)| self.row(offer, &entry.task, score))
            .collect()
    }

    /// Rows that explain a typed command; Tab completes the command.
    fn hint_rows(&self, offer: &Offer) -> Vec<ResultItem> {
        let hints: [(TaskKind, &str, &str, &str, &str); 6] = [
            (
                TaskKind::QuitApp,
                "quit",
                "Quit an app",
                "Type quit and the app's name",
                "kill",
            ),
            (
                TaskKind::ForceQuitApp,
                "force quit",
                "Force quit an app",
                "Type force quit and the app's name",
                "kill",
            ),
            (
                TaskKind::KillProcess,
                "kill",
                "Kill a process",
                "Type kill and the process name",
                "kill",
            ),
            (
                TaskKind::Eject,
                "eject",
                "Eject a drive",
                "Type eject to list removable drives",
                "eject",
            ),
            (
                TaskKind::SetVolume,
                "vol",
                "Set the volume",
                "Type vol and a number, like vol 30",
                "volume",
            ),
            (
                TaskKind::KeepAwake,
                "awake",
                "Keep awake",
                "Type awake and a time, like awake 90 or awake 2h",
                "bolt",
            ),
        ];
        hints
            .iter()
            .filter(|(kind, ..)| self.offers(offer, *kind))
            .enumerate()
            .map(|(i, (_, command, title, subtitle, icon))| {
                self.hint(command, title, subtitle, icon)
                    .with_score(score::KEYWORD - 200.0 - i as f64)
            })
            .collect()
    }

    fn hint(&self, command: &str, title: &str, subtitle: &str, icon: &str) -> ResultItem {
        ResultItem::new(
            PLUGIN_ID,
            format!("hint:{}", command.replace(' ', "_")),
            title,
            Action::Custom {
                payload: HINT_PAYLOAD.to_owned(),
            },
        )
        .with_subtitle(subtitle)
        .with_icon(IconSource::builtin(icon))
        .with_autocomplete(format!("{command} "))
    }

    /// The rows of a typed command, or `None` when that kind is not offered
    /// (then the input is treated as an ordinary query).
    fn intent_rows(&self, offer: &Offer, intent: Intent<'_>) -> Option<Vec<ResultItem>> {
        match intent {
            Intent::Quit(rest) if self.offers(offer, TaskKind::QuitApp) => {
                Some(self.app_rows(offer, rest, false))
            }
            Intent::ForceQuit(rest) if self.offers(offer, TaskKind::ForceQuitApp) => {
                Some(self.app_rows(offer, rest, true))
            }
            Intent::Kill(rest) if self.offers(offer, TaskKind::KillProcess) => {
                Some(self.process_rows(offer, rest))
            }
            Intent::Eject(rest) if self.offers(offer, TaskKind::Eject) => {
                Some(self.drive_rows(offer, rest))
            }
            Intent::Volume(rest) => self.volume_rows(offer, rest),
            Intent::Awake(rest) if self.offers(offer, TaskKind::KeepAwake) => {
                Some(self.awake_rows(offer, rest))
            }
            _ => None,
        }
    }

    fn app_rows(&self, offer: &Offer, rest: &str, force: bool) -> Vec<ResultItem> {
        let platform = self.platform.clone();
        self.apps.refresh_if_stale(
            LIST_TTL,
            move || platform.list_running_apps().unwrap_or_default(),
            self.notifier(),
            PLUGIN_ID,
        );
        let apps = self.apps.get();
        ranked(&apps, rest, |app| display_name(&app.name))
            .into_iter()
            .map(|(score, app)| {
                let (task, subtitle) = if force {
                    (
                        Task::ForceQuitApp(app.name.clone()),
                        "End the app at once; unsaved work is lost",
                    )
                } else {
                    (Task::QuitApp(app.name.clone()), "Ask the app to quit")
                };
                let mut item = self.row(offer, &task, score);
                item.title = format!(
                    "{} {}",
                    if force { "Force quit" } else { "Quit" },
                    display_name(&app.name)
                );
                item.subtitle = if app.pids.len() > 1 {
                    format!("{subtitle} ({} processes)", app.pids.len())
                } else {
                    subtitle.to_owned()
                };
                item
            })
            .collect()
    }

    fn process_rows(&self, offer: &Offer, rest: &str) -> Vec<ResultItem> {
        let platform = self.platform.clone();
        self.processes.refresh_if_stale(
            LIST_TTL,
            move || {
                platform
                    .list_processes()
                    .map(group_processes)
                    .unwrap_or_default()
            },
            self.notifier(),
            PLUGIN_ID,
        );
        let groups = self.processes.get();
        ranked(&groups, rest, |group| display_name(&group.name))
            .into_iter()
            .map(|(score, group)| {
                let mut item = self.row(offer, &Task::KillProcess(group.name.clone()), score);
                item.subtitle = format!(
                    "{}{} \u{b7} {} \u{b7} {}",
                    group.count,
                    if group.count == 1 {
                        " process"
                    } else {
                        " processes"
                    },
                    format_cpu(group.cpu),
                    format_bytes(group.memory)
                );
                item
            })
            .collect()
    }

    fn drive_rows(&self, offer: &Offer, rest: &str) -> Vec<ResultItem> {
        let platform = self.platform.clone();
        self.drives.refresh_if_stale(
            LIST_TTL,
            move || platform.list_removable_drives().unwrap_or_default(),
            self.notifier(),
            PLUGIN_ID,
        );
        let drives = self.drives.get();
        ranked(&drives, rest, |drive| drive.label.as_str())
            .into_iter()
            .map(|(score, drive)| {
                let mut item = self.row(offer, &Task::Eject(drive.id.clone()), score);
                item.title = format!("Eject {}", drive.label);
                item.subtitle = format!("Safely remove {}", drive.id);
                item
            })
            .collect()
    }

    fn volume_rows(&self, offer: &Offer, rest: &str) -> Option<Vec<ResultItem>> {
        let arg = parse_volume(rest);
        let mut tasks: Vec<Task> = match arg {
            VolumeArg::Nothing => [Task::VolumeUp, Task::VolumeDown, Task::Mute, Task::Unmute]
                .into_iter()
                .chain(VOLUME_PRESETS.map(Task::SetVolume))
                .collect(),
            VolumeArg::Set(percent) => vec![Task::SetVolume(percent)],
            VolumeArg::Up => vec![Task::VolumeUp],
            VolumeArg::Down => vec![Task::VolumeDown],
            VolumeArg::Mute => vec![Task::Mute],
            VolumeArg::Unmute => vec![Task::Unmute],
            VolumeArg::Invalid => return None,
        };
        tasks.retain(|task| self.offers(offer, task.kind()));
        if tasks.is_empty() {
            return None;
        }
        Some(
            tasks
                .iter()
                .enumerate()
                .map(|(i, task)| self.row(offer, task, score::KEYWORD - i as f64))
                .collect(),
        )
    }

    fn awake_rows(&self, offer: &Offer, rest: &str) -> Vec<ResultItem> {
        let tasks: Vec<Task> = if rest.trim().is_empty() {
            AWAKE_PRESETS
                .into_iter()
                .map(Task::KeepAwake)
                .chain(
                    self.offers(offer, TaskKind::StopKeepAwake)
                        .then_some(Task::StopKeepAwake),
                )
                .collect()
        } else if rest.trim().eq_ignore_ascii_case("off")
            || rest.trim().eq_ignore_ascii_case("stop")
        {
            vec![Task::StopKeepAwake]
        } else {
            parse_minutes(rest)
                .map(Task::KeepAwake)
                .into_iter()
                .collect()
        };
        tasks
            .iter()
            .filter(|task| self.offers(offer, task.kind()))
            .enumerate()
            .map(|(i, task)| self.row(offer, task, score::KEYWORD - i as f64))
            .collect()
    }
}

/// The items whose name matches `rest`, best first, as `(score, item)`. Scores
/// start at [`score::KEYWORD`]: a typed command's rows are exact answers, kept
/// above ordinary matches and out of the usage boost. With nothing typed, the
/// list keeps its own order.
fn ranked<'a, T>(items: &'a [T], rest: &str, name: impl Fn(&T) -> &str) -> Vec<(f64, &'a T)> {
    let rest = rest.trim();
    let mut query = FuzzyQuery::new(rest);
    let mut scored: Vec<(f64, &T)> = if query.is_empty() {
        items
            .iter()
            .enumerate()
            .map(|(i, item)| (score::KEYWORD + (MAX_LIVE_ROWS * 4) as f64 - i as f64, item))
            .collect()
    } else {
        let rest_lower = rest.to_lowercase();
        items
            .iter()
            .filter_map(|item| {
                let name = name(item);
                let fuzzy = query.score(name)?;
                let bonus = name_bonus(&name.to_lowercase(), &rest_lower);
                Some((score::KEYWORD + f64::from(fuzzy) + bonus, item))
            })
            .collect()
    };
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored.truncate(MAX_LIVE_ROWS);
    scored
}

/// The question asked before a destructive task runs.
fn confirmation_text(task: &Task) -> Option<String> {
    match task {
        Task::ForceQuitApp(name) => Some(format!(
            "Force quit {}? It closes at once and unsaved work in it is lost.",
            display_name(name)
        )),
        Task::KillProcess(name) => Some(format!(
            "End every process named {}? Unsaved work in them is lost.",
            display_name(name)
        )),
        Task::RestartShell if cfg!(windows) => Some(
            "Restart Explorer? The taskbar disappears for a moment and open File Explorer windows close."
                .to_owned(),
        ),
        Task::RestartShell => {
            Some("Restart Finder? Open Finder windows close and reopen.".to_owned())
        }
        _ => None,
    }
}

impl Plugin for TasksPlugin {
    fn id(&self) -> &str {
        PLUGIN_ID
    }

    fn name(&self) -> &str {
        "Automation tasks"
    }

    fn description(&self) -> &str {
        "Dark mode, volume, screenshot, quit an app, kill a process, eject, keep awake and more; type `t` to list them."
    }

    fn keyword(&self) -> Option<&str> {
        self.keyword.as_deref()
    }

    fn global(&self) -> bool {
        self.config.global || self.keyword.is_none()
    }

    fn keyword_row(&self) -> Option<ResultItem> {
        let keyword = self.keyword.as_deref()?;
        Some(
            self.hint(
                keyword,
                "Automation tasks",
                "Press Tab to list the tasks",
                "bolt",
            )
            .with_autocomplete(format!("{keyword} ")),
        )
    }

    fn query(&self, input: &str) -> Vec<ResultItem> {
        let input = input.trim();
        let offer = self.offer();
        if let Some(rows) = parse_intent(input).and_then(|intent| self.intent_rows(&offer, intent))
        {
            return rows;
        }
        self.named_rows(&offer, input)
    }

    /// `tasks:<key>` for a task this machine offers and `[tasks] disabled` does
    /// not hide: `tasks:dark_mode`, `tasks:volume:30`, `tasks:keep_awake:45`,
    /// `tasks:quit_app:Slack`, `tasks:kill:chrome.exe`. A hotkey bound to a
    /// destructive task is still confirmed by the shell.
    fn resolve(&self, id: &str) -> Option<ResultItem> {
        let key = id.strip_prefix("tasks:")?;
        let task = Task::from_key(key)?;
        let offer = self.offer();
        self.offers(&offer, task.kind())
            .then(|| self.row(&offer, &task, 0.0))
    }

    fn confirmation(&self, item: &ResultItem) -> Option<String> {
        if !self.config.confirm {
            return None;
        }
        let Action::Custom { payload } = &item.action else {
            return None;
        };
        let task = Task::from_key(payload.strip_prefix(PAYLOAD_PREFIX)?)?;
        confirmation_text(&task)
    }

    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        let Action::Custom { payload } = &item.action else {
            return Err(PluginError::Unsupported(item.id.clone()));
        };
        let Some(key) = payload.strip_prefix(PAYLOAD_PREFIX) else {
            return Err(PluginError::Unsupported(item.id.clone()));
        };
        if payload == HINT_PAYLOAD {
            return Err(PluginError::Message(
                "Press Tab to complete the command, then type what it should act on".to_owned(),
            ));
        }
        let task = Task::from_key(key).ok_or_else(|| PluginError::Unsupported(item.id.clone()))?;
        let offer = self.offer();
        if !self.offers(&offer, task.kind()) {
            return Err(PluginError::Message(format!(
                "\u{201c}{}\u{201d} is not available on this system",
                describe(&task).title
            )));
        }
        self.platform.run_task(&task).map_err(PluginError::other)
    }

    fn refresh(&self) -> PluginResult<()> {
        let supported = self.platform.supported_tasks();
        let entries = static_tasks()
            .into_iter()
            .filter(|task| supported.contains(&task.kind()))
            .map(entry)
            .collect();
        *self
            .offer
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) =
            Arc::new(Offer { supported, entries });
        Ok(())
    }

    fn attach_notifier(&self, notifier: ResultsNotifier) {
        let _ = self.notifier.set(notifier);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::sync::Mutex;

    use super::*;
    use crate::test_util::MockPlatform;

    fn platform(kinds: &[TaskKind]) -> Arc<MockPlatform> {
        let platform = MockPlatform::empty();
        *platform.task_kinds.lock().unwrap() = kinds.to_vec();
        platform
    }

    fn everything() -> Arc<MockPlatform> {
        platform(&TaskKind::ALL)
    }

    fn plugin(config: TasksConfig, platform: &Arc<MockPlatform>) -> TasksPlugin {
        let plugin = TasksPlugin::new(config, platform.clone());
        plugin.refresh().unwrap();
        plugin
    }

    fn process(name: &str, pid: u32, cpu: f32, memory: u64) -> ProcessInfo {
        ProcessInfo {
            name: name.to_owned(),
            pid,
            cpu_percent: cpu,
            memory_bytes: memory,
        }
    }

    fn titles(plugin: &TasksPlugin, input: &str) -> Vec<String> {
        plugin.query(input).into_iter().map(|i| i.title).collect()
    }

    fn first(plugin: &TasksPlugin, input: &str) -> ResultItem {
        plugin
            .query(input)
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("no result for {input:?}"))
    }

    /// Runs the query again until the background list arrives, the way the
    /// shell re-runs it when the notifier fires.
    fn query_after_load(plugin: &TasksPlugin, input: &str) -> Vec<ResultItem> {
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let _ = plugin.notifier.set(Arc::new(move |_| {
            let _ = tx.lock().unwrap().send(());
        }));
        let first = plugin.query(input);
        if first.is_empty() {
            rx.recv_timeout(Duration::from_secs(5)).expect("a refresh");
            return plugin.query(input);
        }
        first
    }

    #[test]
    fn metadata_and_keyword_follow_the_config() {
        let plugin = TasksPlugin::new(TasksConfig::default(), everything());
        assert_eq!(plugin.id(), "tasks");
        assert_eq!(plugin.keyword(), Some("t"));
        assert!(plugin.global());
        let config = TasksConfig {
            keyword: "tk".into(),
            global: false,
            ..TasksConfig::default()
        };
        let plugin = TasksPlugin::new(config, everything());
        assert_eq!(plugin.keyword(), Some("tk"));
        assert!(!plugin.global());
        let config = TasksConfig {
            keyword: " ".into(),
            global: false,
            ..TasksConfig::default()
        };
        let plugin = TasksPlugin::new(config, everything());
        assert_eq!(plugin.keyword(), None);
        assert!(plugin.global());
        assert!(plugin.keyword_row().is_none());
    }

    #[test]
    fn keyword_row_completes_the_bare_keyword() {
        let plugin = TasksPlugin::new(TasksConfig::default(), everything());
        let row = plugin.keyword_row().unwrap();
        assert_eq!(row.autocomplete.as_deref(), Some("t "));
        assert_eq!(row.plugin_id, "tasks");
    }

    #[test]
    fn nothing_is_offered_before_the_first_refresh() {
        let plugin = TasksPlugin::new(TasksConfig::default(), everything());
        assert!(plugin.query("dark").is_empty());
        assert!(plugin.query("kill chrome").is_empty());
        assert!(plugin.query("").is_empty());
        assert!(plugin.resolve("tasks:dark_mode").is_none());
    }

    #[test]
    fn finds_tasks_by_title_and_alias() {
        let plugin = plugin(TasksConfig::default(), &everything());
        assert_eq!(first(&plugin, "dark mode").title, "Toggle dark mode");
        assert_eq!(first(&plugin, "theme").title, "Toggle dark mode");
        assert_eq!(first(&plugin, "screenshot").title, "Take a screenshot");
        assert_eq!(first(&plugin, "mute").title, "Mute volume");
        assert_eq!(first(&plugin, "unmute").title, "Unmute volume");
        assert_eq!(first(&plugin, "louder").title, "Volume up");
        assert_eq!(first(&plugin, "wifi").title, "Toggle Wi-Fi");
        assert_eq!(first(&plugin, "bluetooth on").title, "Toggle Bluetooth");
        assert_eq!(first(&plugin, "show desktop").title, "Show desktop");
        assert_eq!(first(&plugin, "downloads").title, "Open Downloads folder");
        assert_eq!(first(&plugin, "clear clipboard").title, "Empty clipboard");
        assert_eq!(first(&plugin, "stay awake").title, "Keep awake for 1 hour");
        assert_eq!(first(&plugin, "flush dns").title, "Flush DNS cache");
    }

    #[test]
    fn short_and_unrelated_queries_match_nothing() {
        let plugin = plugin(TasksConfig::default(), &everything());
        assert!(plugin.query("d").is_empty());
        assert!(plugin.query("  m ").is_empty());
        assert!(plugin.query("firefox").is_empty());
    }

    #[test]
    fn only_supported_tasks_are_offered() {
        let platform = platform(&[TaskKind::ToggleDarkMode, TaskKind::Mute]);
        let plugin = plugin(TasksConfig::default(), &platform);
        assert_eq!(first(&plugin, "dark").title, "Toggle dark mode");
        assert_eq!(first(&plugin, "mute").title, "Mute volume");
        assert!(titles(&plugin, "screenshot").is_empty());
        assert!(titles(&plugin, "wifi").is_empty());
        // Typed commands follow the same rule.
        assert!(plugin.query("kill chrome").is_empty());
        assert!(plugin.query("quit").is_empty());
        assert!(plugin.query("vol 30").is_empty());
        assert!(plugin.query("awake 30").is_empty());
        assert!(plugin.resolve("tasks:wifi").is_none());
    }

    #[test]
    fn disabled_tasks_are_hidden() {
        let config = TasksConfig {
            disabled: vec!["Mute".into(), "kill".into(), "volume".into()],
            ..TasksConfig::default()
        };
        let plugin = plugin(config, &everything());
        assert!(titles(&plugin, "mute").iter().all(|t| t != "Mute volume"));
        assert_eq!(first(&plugin, "unmute").title, "Unmute volume");
        assert!(plugin.query("kill chrome").is_empty());
        assert!(plugin.query("vol 30").is_empty());
        assert!(plugin.resolve("tasks:quit_app:Slack").is_some());
        assert!(plugin.resolve("tasks:kill:chrome.exe").is_none());
        assert!(plugin.resolve("tasks:mute").is_none());
        assert!(plugin.resolve("tasks:unmute").is_some());
    }

    #[test]
    fn the_keyword_alone_lists_every_task_and_hints_in_order() {
        let platform = everything();
        let plugin = plugin(TasksConfig::default(), &platform);
        let rows = plugin.query("");
        let list = titles(&plugin, "");
        assert_eq!(list[0], "Toggle dark mode");
        assert!(list.contains(&"Stop keeping awake".to_owned()));
        assert!(list.contains(&"Kill a process".to_owned()));
        assert!(list.contains(&"Quit an app".to_owned()));
        assert!(rows.windows(2).all(|w| w[0].score > w[1].score));
        // Hints complete the command and do nothing else.
        let kill = rows.iter().find(|r| r.title == "Kill a process").unwrap();
        assert_eq!(kill.autocomplete.as_deref(), Some("kill "));
        let error = plugin.execute(kill).unwrap_err();
        assert!(error.to_string().contains("Tab"), "{error}");
        assert!(platform.ran_tasks.lock().unwrap().is_empty());
    }

    #[test]
    fn results_are_stable_custom_actions_with_icons() {
        let plugin = plugin(TasksConfig::default(), &everything());
        let item = first(&plugin, "dark mode");
        assert_eq!(item.id, "tasks:dark_mode");
        assert_eq!(item.plugin_id, "tasks");
        assert_eq!(
            item.action,
            Action::Custom {
                payload: "task:dark_mode".into()
            }
        );
        assert_eq!(item.icon, Some(IconSource::builtin("theme")));
        assert!(item.score > 0.0);
        assert_eq!(first(&plugin, "stay awake").id, "tasks:keep_awake:60");
    }

    #[test]
    fn exact_name_ranks_above_a_loose_match() {
        let plugin = plugin(TasksConfig::default(), &everything());
        let results = plugin.query("mute");
        assert_eq!(results[0].title, "Mute volume");
        assert!(results.windows(2).all(|w| w[0].score >= w[1].score));
    }

    #[test]
    fn volume_commands() {
        let plugin = plugin(TasksConfig::default(), &everything());
        let set = first(&plugin, "vol 30");
        assert_eq!(set.id, "tasks:volume:30");
        assert_eq!(set.title, "Set volume to 30%");
        assert!(set.score >= score::KEYWORD);
        assert_eq!(first(&plugin, "volume 45%").id, "tasks:volume:45");
        assert_eq!(first(&plugin, "VOL 0").id, "tasks:volume:0");
        assert_eq!(first(&plugin, "vol up").id, "tasks:volume_up");
        assert_eq!(first(&plugin, "vol down").id, "tasks:volume_down");
        assert_eq!(first(&plugin, "vol mute").id, "tasks:mute");
        assert_eq!(first(&plugin, "vol unmute").id, "tasks:unmute");
        // A bare `vol` offers the nudges and a few presets.
        let ids: Vec<_> = plugin.query("vol").into_iter().map(|i| i.id).collect();
        assert_eq!(ids[0], "tasks:volume_up");
        assert!(ids.contains(&"tasks:volume:50".to_owned()));
        // Nonsense and out-of-range values offer nothing (and `vol` is no
        // longer a command, so the fuzzy search finds no task either).
        for bad in [
            "vol 101",
            "vol 300",
            "vol -5",
            "vol abc",
            "vol 3.5",
            "vol 1000000",
        ] {
            assert!(plugin.query(bad).is_empty(), "{bad}");
        }
    }

    #[test]
    fn volume_parsing() {
        assert_eq!(parse_volume(""), VolumeArg::Nothing);
        assert_eq!(parse_volume("30"), VolumeArg::Set(30));
        assert_eq!(parse_volume(" 30 % "), VolumeArg::Set(30));
        assert_eq!(parse_volume("100"), VolumeArg::Set(100));
        assert_eq!(parse_volume("000"), VolumeArg::Set(0));
        assert_eq!(parse_volume("101"), VolumeArg::Invalid);
        assert_eq!(parse_volume("256"), VolumeArg::Invalid);
        assert_eq!(parse_volume("-1"), VolumeArg::Invalid);
        assert_eq!(parse_volume("%"), VolumeArg::Invalid);
        assert_eq!(parse_volume("up"), VolumeArg::Up);
        assert_eq!(parse_volume("+"), VolumeArg::Up);
        assert_eq!(parse_volume("Down"), VolumeArg::Down);
        assert_eq!(parse_volume("mute"), VolumeArg::Mute);
        assert_eq!(parse_volume("unmute"), VolumeArg::Unmute);
    }

    #[test]
    fn minutes_parsing() {
        for (text, expected) in [
            ("30", Some(30)),
            ("30m", Some(30)),
            ("45 min", Some(45)),
            ("2h", Some(120)),
            ("1.5h", Some(90)),
            ("1 hour", Some(60)),
            ("2 hours", Some(120)),
            ("24h", Some(1440)),
            ("1440", Some(1440)),
            ("0", None),
            ("0.2", None),
            ("1441", None),
            ("25h", None),
            ("", None),
            ("h", None),
            ("abc", None),
            ("1.2.3", None),
            ("5 days", None),
            ("-5", None),
            ("99999999999", None),
        ] {
            assert_eq!(parse_minutes(text), expected, "{text:?}");
        }
        assert_eq!(format_minutes(1), "1 minute");
        assert_eq!(format_minutes(30), "30 minutes");
        assert_eq!(format_minutes(60), "1 hour");
        assert_eq!(format_minutes(90), "1 hour 30 minutes");
        assert_eq!(format_minutes(480), "8 hours");
    }

    #[test]
    fn awake_commands() {
        let plugin = plugin(TasksConfig::default(), &everything());
        let item = first(&plugin, "awake 90");
        assert_eq!(item.id, "tasks:keep_awake:90");
        assert_eq!(item.title, "Keep awake for 1 hour 30 minutes");
        assert_eq!(first(&plugin, "keep awake 2h").id, "tasks:keep_awake:120");
        assert_eq!(first(&plugin, "caffeinate 15").id, "tasks:keep_awake:15");
        assert_eq!(first(&plugin, "awake stop").id, "tasks:stop_keep_awake");
        let presets = plugin.query("awake");
        assert_eq!(presets.len(), AWAKE_PRESETS.len() + 1);
        assert_eq!(presets.last().unwrap().id, "tasks:stop_keep_awake");
        assert!(plugin.query("awake 0").is_empty());
        assert!(plugin.query("awake 9999").is_empty());
    }

    #[test]
    fn intent_words_are_whole_words() {
        assert_eq!(parse_intent("kill chrome"), Some(Intent::Kill("chrome")));
        assert_eq!(
            parse_intent("  KILL  chrome  x "),
            Some(Intent::Kill("chrome  x"))
        );
        assert_eq!(parse_intent("kill"), Some(Intent::Kill("")));
        assert_eq!(parse_intent("quit"), Some(Intent::Quit("")));
        assert_eq!(parse_intent("quit slack"), Some(Intent::Quit("slack")));
        assert_eq!(
            parse_intent("force quit slack"),
            Some(Intent::ForceQuit("slack"))
        );
        assert_eq!(parse_intent("Force   Quit"), Some(Intent::ForceQuit("")));
        assert_eq!(parse_intent("fq slack"), Some(Intent::ForceQuit("slack")));
        assert_eq!(parse_intent("eject usb"), Some(Intent::Eject("usb")));
        assert_eq!(parse_intent("keep awake 30"), Some(Intent::Awake("30")));
        assert_eq!(parse_intent("vol 5"), Some(Intent::Volume("5")));
        for plain in [
            "kil",
            "killer",
            "force",
            "force fed",
            "keep",
            "keep going",
            "volumes",
            "dark",
            "",
        ] {
            assert_eq!(parse_intent(plain), None, "{plain:?}");
        }
    }

    #[test]
    fn kill_lists_processes_grouped_by_name_with_cpu_and_memory() {
        let platform = everything();
        *platform.processes.lock().unwrap() = vec![
            process("chrome.exe", 10, 1.5, 300 * 1024 * 1024),
            process("chrome.exe", 11, 2.0, 500 * 1024 * 1024),
            process("Chrome.exe", 12, 0.5, 100 * 1024 * 1024),
            process("notepad.exe", 20, 0.0, 20 * 1024 * 1024),
            process("code.exe", 30, 12.0, 1500 * 1024 * 1024),
        ];
        let plugin = plugin(TasksConfig::default(), &platform);
        let rows = query_after_load(&plugin, "kill chr");
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.title, "Kill chrome");
        assert_eq!(row.id, "tasks:kill:chrome.exe");
        assert_eq!(row.subtitle, "3 processes \u{b7} 4.0% CPU \u{b7} 900 MB");
        assert!(row.score >= score::KEYWORD);
        assert_eq!(
            row.action,
            Action::Custom {
                payload: "task:kill:chrome.exe".into()
            }
        );
        // Nothing typed lists everything, biggest memory first.
        let all = titles(&plugin, "kill");
        assert_eq!(all, ["Kill code", "Kill chrome", "Kill notepad"]);
        // The list was read once, off the typing thread, and is reused.
        assert_eq!(platform.list_calls.lock().unwrap().0, 1);
    }

    #[test]
    fn process_groups_sort_by_memory_and_merge_case_variants() {
        let groups = group_processes(vec![
            process("a.exe", 1, 1.0, 10),
            process("B.exe", 2, 2.0, 30),
            process("b.exe", 3, 3.0, 30),
            process("c", 4, 0.0, 5),
        ]);
        let summary: Vec<_> = groups
            .iter()
            .map(|g| (g.name.as_str(), g.count, g.memory))
            .collect();
        assert_eq!(summary, [("B.exe", 2, 60), ("a.exe", 1, 10), ("c", 1, 5)]);
        assert_eq!(groups[0].cpu, 5.0);
        assert!(group_processes(Vec::new()).is_empty());
    }

    #[test]
    fn sizes_and_cpu_read_naturally() {
        assert_eq!(format_bytes(0), "1 KB");
        assert_eq!(format_bytes(2048), "2 KB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5 MB");
        assert_eq!(format_bytes(1536 * 1024 * 1024), "1.5 GB");
        assert_eq!(format_cpu(0.0), "0% CPU");
        assert_eq!(format_cpu(3.2), "3.2% CPU");
        assert_eq!(format_cpu(42.4), "42% CPU");
        assert_eq!(display_name("chrome.exe"), "chrome");
        assert_eq!(display_name("Slack.EXE"), "Slack");
        assert_eq!(display_name("firefox"), "firefox");
        assert_eq!(display_name(".exe"), "");
    }

    fn slack_and_chrome() -> Vec<RunningApp> {
        // In the platform's order: by name, case-insensitively.
        vec![
            RunningApp {
                name: "chrome.exe".into(),
                pids: vec![7, 8],
            },
            RunningApp {
                name: "Slack".into(),
                pids: vec![5],
            },
        ]
    }

    #[test]
    fn quit_lists_running_apps_with_a_force_quit_action() {
        let platform = everything();
        *platform.running_apps.lock().unwrap() = slack_and_chrome();
        let plugin = plugin(TasksConfig::default(), &platform);
        let rows = query_after_load(&plugin, "quit");
        assert_eq!(
            rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
            ["Quit chrome", "Quit Slack"]
        );
        let chrome = &rows[0];
        assert_eq!(chrome.id, "tasks:quit_app:chrome.exe");
        assert_eq!(chrome.subtitle, "Ask the app to quit (2 processes)");
        assert_eq!(chrome.secondary.len(), 1);
        assert_eq!(chrome.secondary[0].label, "Force quit");
        assert_eq!(chrome.secondary[0].modifier, Some(Modifier::Shift));
        assert_eq!(
            chrome.secondary[0].action,
            Action::Custom {
                payload: "task:force_quit_app:chrome.exe".into()
            }
        );
        // Quitting asks nothing; the force quit secondary does.
        assert_eq!(plugin.confirmation(chrome), None);
        let forced = chrome.secondary_as_primary(0).unwrap();
        assert!(plugin
            .confirmation(&forced)
            .unwrap()
            .starts_with("Force quit chrome?"));
        assert_eq!(titles(&plugin, "quit sla"), ["Quit Slack"]);
        assert!(titles(&plugin, "quit zzz").is_empty());

        plugin.execute(&rows[1]).unwrap();
        assert_eq!(
            *platform.ran_tasks.lock().unwrap(),
            [Task::QuitApp("Slack".into())]
        );
    }

    #[test]
    fn force_quit_rows_run_the_force_quit_task_after_asking() {
        let platform = everything();
        *platform.running_apps.lock().unwrap() = slack_and_chrome();
        let plugin = plugin(TasksConfig::default(), &platform);
        let rows = query_after_load(&plugin, "force quit sla");
        assert_eq!(rows[0].title, "Force quit Slack");
        assert!(plugin.confirmation(&rows[0]).is_some());
        plugin.execute(&rows[0]).unwrap();
        assert_eq!(
            *platform.ran_tasks.lock().unwrap(),
            [Task::ForceQuitApp("Slack".into())]
        );

        // Without force quit offered, the quit rows carry no secondary action.
        let platform = self::platform(&[TaskKind::QuitApp]);
        *platform.running_apps.lock().unwrap() = slack_and_chrome();
        let plugin = self::plugin(TasksConfig::default(), &platform);
        let rows = query_after_load(&plugin, "quit");
        assert!(rows[0].secondary.is_empty());
        assert!(plugin.query("force quit").is_empty());
    }

    #[test]
    fn eject_lists_removable_drives() {
        let platform = everything();
        *platform.drives.lock().unwrap() = vec![
            Drive {
                id: "E:".into(),
                label: "Backup (E:)".into(),
            },
            Drive {
                id: "F:".into(),
                label: "Camera (F:)".into(),
            },
        ];
        let plugin = plugin(TasksConfig::default(), &platform);
        let rows = query_after_load(&plugin, "eject");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].title, "Eject Backup (E:)");
        assert_eq!(rows[0].id, "tasks:eject:E:");
        assert_eq!(titles(&plugin, "eject cam"), ["Eject Camera (F:)"]);
        // Ejecting is safe: no question.
        assert_eq!(plugin.confirmation(&rows[0]), None);
        plugin.execute(&rows[1]).unwrap();
        assert_eq!(
            *platform.ran_tasks.lock().unwrap(),
            [Task::Eject("F:".into())]
        );
    }

    #[test]
    fn typing_never_waits_for_the_os_lists() {
        let platform = everything();
        *platform.processes.lock().unwrap() = vec![process("a.exe", 1, 0.0, 1)];
        let plugin = plugin(TasksConfig::default(), &platform);
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        plugin
            .notifier
            .set(Arc::new(move |id| {
                let _ = tx.lock().unwrap().send(id.to_owned());
            }))
            .ok();
        // Before the first load the query answers at once with nothing ...
        assert!(plugin.query("kill a").is_empty());
        // ... and the notifier fires, naming this plugin, once the list is there.
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), "tasks");
        assert_eq!(plugin.query("kill a").len(), 1);
        // Plain name queries never touch the OS lists.
        let before = *platform.list_calls.lock().unwrap();
        plugin.query("dark mode");
        plugin.query("vol 30");
        assert_eq!(*platform.list_calls.lock().unwrap(), before);
    }

    #[test]
    fn destructive_tasks_ask_first_and_the_others_do_not() {
        let plugin = plugin(TasksConfig::default(), &everything());
        let asks = |task: Task| {
            plugin
                .confirmation(&plugin.row(&plugin.offer(), &task, 1.0))
                .is_some()
        };
        assert!(asks(Task::ForceQuitApp("x".into())));
        assert!(asks(Task::KillProcess("x".into())));
        assert!(asks(Task::RestartShell));
        for task in [
            Task::ToggleDarkMode,
            Task::QuitApp("x".into()),
            Task::Eject("E:".into()),
            Task::Mute,
            Task::SetVolume(10),
            Task::KeepAwake(5),
            Task::ToggleWifi,
            Task::EmptyClipboard,
            Task::Screenshot,
        ] {
            assert!(!asks(task.clone()), "{task:?}");
        }
        let row = plugin.row(
            &plugin.offer(),
            &Task::KillProcess("chrome.exe".into()),
            1.0,
        );
        let text = plugin.confirmation(&row).unwrap();
        assert!(text.contains("chrome") && !text.contains(".exe"), "{text}");
    }

    #[test]
    fn confirmation_can_be_switched_off() {
        let config = TasksConfig {
            confirm: false,
            ..TasksConfig::default()
        };
        let plugin = plugin(config, &everything());
        let row = plugin.row(&plugin.offer(), &Task::KillProcess("x".into()), 1.0);
        assert_eq!(plugin.confirmation(&row), None);
    }

    #[test]
    fn execute_runs_the_task_through_the_platform() {
        let platform = everything();
        let plugin = plugin(TasksConfig::default(), &platform);
        plugin.execute(&first(&plugin, "dark mode")).unwrap();
        plugin.execute(&first(&plugin, "vol 30")).unwrap();
        plugin.execute(&first(&plugin, "awake 45")).unwrap();
        assert_eq!(
            *platform.ran_tasks.lock().unwrap(),
            [
                Task::ToggleDarkMode,
                Task::SetVolume(30),
                Task::KeepAwake(45)
            ]
        );
    }

    #[test]
    fn execute_rejects_foreign_malformed_and_unavailable_actions() {
        let platform = platform(&[TaskKind::Mute]);
        let plugin = plugin(TasksConfig::default(), &platform);
        let custom = |payload: &str| {
            ResultItem::new(
                "tasks",
                "x",
                "X",
                Action::Custom {
                    payload: payload.into(),
                },
            )
        };
        for payload in [
            "task:format_disk",
            "task:volume:999",
            "task:eject:/etc",
            "task:",
            "command:restart",
            "restart",
        ] {
            assert!(
                matches!(
                    plugin.execute(&custom(payload)),
                    Err(PluginError::Unsupported(_))
                ),
                "{payload}"
            );
        }
        // Valid, but this system does not offer it.
        assert!(matches!(
            plugin.execute(&custom("task:wifi")),
            Err(PluginError::Message(_))
        ));
        let copy = ResultItem::new("tasks", "y", "Y", Action::CopyText { text: "t".into() });
        assert!(matches!(
            plugin.execute(&copy),
            Err(PluginError::Unsupported(_))
        ));
        assert!(platform.ran_tasks.lock().unwrap().is_empty());
        assert_eq!(plugin.confirmation(&copy), None);
        assert_eq!(plugin.confirmation(&custom("task:nope")), None);
    }

    #[test]
    fn resolve_finds_offered_tasks_by_id() {
        let platform = platform(&[
            TaskKind::ToggleDarkMode,
            TaskKind::SetVolume,
            TaskKind::KeepAwake,
            TaskKind::KillProcess,
            TaskKind::QuitApp,
        ]);
        let plugin = plugin(TasksConfig::default(), &platform);

        let dark = plugin.resolve("tasks:dark_mode").expect("offered task");
        assert_eq!(dark.id, "tasks:dark_mode");
        assert_eq!(dark.action, first(&plugin, "dark mode").action);
        plugin.execute(&dark).unwrap();

        // Tasks with data resolve without a query.
        let volume = plugin.resolve("tasks:volume:30").unwrap();
        assert_eq!(volume.title, "Set volume to 30%");
        plugin.execute(&volume).unwrap();
        assert_eq!(
            plugin.resolve("tasks:keep_awake:45").unwrap().title,
            "Keep awake for 45 minutes"
        );
        let kill = plugin.resolve("tasks:kill:chrome.exe").unwrap();
        assert_eq!(kill.title, "Kill chrome");
        // ... and a hotkey bound to one still confirms.
        assert!(plugin.confirmation(&kill).is_some());
        assert_eq!(
            plugin.resolve("tasks:quit_app:Slack").unwrap().title,
            "Quit Slack"
        );

        assert_eq!(
            *platform.ran_tasks.lock().unwrap(),
            [Task::ToggleDarkMode, Task::SetVolume(30)]
        );

        // Not supported here, malformed, hint rows, unknown, or someone else's id.
        for id in [
            "tasks:wifi",
            "tasks:volume:101",
            "tasks:volume",
            "tasks:keep_awake:0",
            "tasks:eject:/etc",
            "tasks:hint",
            "tasks:nope",
            "tasks:",
            "system:lock",
            "apps:dark_mode",
        ] {
            assert!(plugin.resolve(id).is_none(), "{id}");
        }
    }

    #[test]
    fn every_static_task_has_a_stable_unique_id_and_a_description() {
        let plugin = plugin(TasksConfig::default(), &everything());
        let offer = plugin.offer();
        let mut ids: Vec<String> = offer
            .entries
            .iter()
            .map(|entry| plugin.row(&offer, &entry.task, 1.0).id)
            .collect();
        assert_eq!(ids.len(), static_tasks().len());
        ids.sort_unstable();
        let total = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), total);
        for entry in &offer.entries {
            assert!(!entry.title.is_empty() && !describe(&entry.task).subtitle.is_empty());
            // The id resolves back to the same task.
            let id = format!("tasks:{}", entry.task.to_key());
            assert_eq!(plugin.resolve(&id).unwrap().title, entry.title);
        }
    }
}
