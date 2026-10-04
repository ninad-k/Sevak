//! Window management: snap, resize and move the window you were using, and
//! switch between open windows.
//!
//! Two searches share this file (the way the emoji picker has two triggers):
//!
//! | Plugin id         | Keyword (config)            | What it does |
//! |-------------------|-----------------------------|--------------|
//! | `windows`         | `win` (`[window_management] keyword`) | layouts: `win left`, `win max`, `win next display`, ... |
//! | `windows:switch`  | `w` (`switcher_keyword`)    | lists open windows; Enter brings one to the front |
//!
//! # The target window
//!
//! The launcher takes focus when it opens, so a layout acts on the window that
//! was in front *before* it opened. The shell notes that window through
//! `PlatformProvider::remember_foreground_app` (the mechanism pasting already
//! uses), and [`sevak_platform::window_manager::apply_command`] reads it back.
//! Layouts therefore work from a global hotkey too: bind one with
//! `[[hotkey]] key = "Ctrl+Alt+Left"` and `run = "windows:left"`.
//!
//! # Actions
//!
//! Every result's action is an [`Action::Custom`] with a payload from a closed
//! set: `win:<layout key>` (see [`WindowCommand::key`]), `win:focus:<window
//! id>` and `win:hint`. [`WindowCommand::from_key`] and [`WindowId::new`]
//! validate what comes back, so the platform only ever receives a variant of
//! the closed vocabulary or a short identifier it made itself; nothing here can
//! make Sevak run an arbitrary command.
//!
//! # Availability
//!
//! What the system can do is the platform's to say
//! ([`PlatformProvider::window_support`]): on a Wayland session or without the
//! macOS Accessibility permission the keyword shows one row that explains why,
//! instead of failing silently when Enter is pressed.
//!
//! The list of open windows is an OS round trip, so a query only reads the last
//! answer from a cache and starts a background refresh when it is stale; the
//! notifier makes the shell show the fresh list a moment later.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sevak_core::config::WindowManagementConfig;
use sevak_core::model::score;
use sevak_core::window_layout::{Layout, WindowCommand};
use sevak_core::{
    Action, FuzzyQuery, IconSource, Plugin, PluginError, PluginResult, ResultItem, ResultsNotifier,
};
use sevak_platform::window_manager::{apply_command, shared_memory, RestoreMemory};
use sevak_platform::{PlatformProvider, WindowId, WindowSupport};

use crate::apps::name_bonus;
use crate::live::Cache;

const LAYOUTS_ID: &str = "windows";
const SWITCHER_ID: &str = "windows:switch";
/// Prefix of the [`Action::Custom`] payloads.
const PAYLOAD_PREFIX: &str = "win:";
const FOCUS_PREFIX: &str = "win:focus:";
/// Payload of the row that only explains why nothing works.
const HINT_PAYLOAD: &str = "win:hint";

/// Plain-query matches shorter than this match nothing.
const MIN_QUERY_CHARS: usize = 2;
/// Aliases are weaker evidence than the title.
const ALIAS_WEIGHT: f64 = 0.9;
/// An alias or title typed in full beats every fuzzy match.
const EXACT_BONUS: f64 = 1_000.0;
/// Breaks ties in favour of shorter titles.
const LENGTH_PENALTY: f64 = 0.01;
/// How long the window list is trusted before the next query refreshes it.
const LIST_TTL: Duration = Duration::from_secs(2);
/// How long a support answer is reused (it can change at runtime: the macOS
/// permission may be granted while Sevak runs).
const SUPPORT_TTL: Duration = Duration::from_secs(5);
/// Most windows a query lists.
const MAX_ROWS: usize = 20;

/// Everything the user reads, in one place, so it can be translated together.
mod text {
    pub const LAYOUTS_NAME: &str = "Window layouts";
    pub const SWITCHER_NAME: &str = "Window switcher";
    pub const LAYOUTS_DESCRIPTION: &str = "Snap, resize and move the window you were using: `win left`, `win max`, `win next display`, `win restore`.";
    pub const SWITCHER_DESCRIPTION: &str =
        "Type `w` and part of a window's title or app to bring that window to the front.";
    pub const LAYOUTS_KEYWORD_TITLE: &str = "Window layouts";
    pub const LAYOUTS_KEYWORD_SUBTITLE: &str = "Press Tab to list the layouts";
    pub const SWITCHER_KEYWORD_TITLE: &str = "Window switcher";
    pub const SWITCHER_KEYWORD_SUBTITLE: &str = "Press Tab, then type part of a window's title";
    pub const UNAVAILABLE_TITLE: &str = "Window management is not available";
    pub const NO_WINDOWS: &str = "No open windows found";
    pub const NO_WINDOWS_HINT: &str = "Nothing to switch to yet";
    pub const MINIMIZED: &str = "minimized";
    pub const UNTITLED: &str = "(untitled window)";
    pub const NOT_AVAILABLE_HERE: &str = "is not available on this system";
    pub const BAD_WINDOW: &str = "That window is not available any more";
}

/// A layout's static description.
struct Description {
    title: &'static str,
    subtitle: &'static str,
    /// Other names it answers to (lowercase, words separated by spaces).
    aliases: &'static [&'static str],
}

fn describe(command: WindowCommand) -> Description {
    let d = |title, subtitle, aliases| Description {
        title,
        subtitle,
        aliases,
    };
    match command {
        WindowCommand::Layout(layout) => match layout {
            Layout::LeftHalf => d(
                "Left half",
                "Fill the left half of the screen",
                &["left", "snap left", "half left", "left side"],
            ),
            Layout::RightHalf => d(
                "Right half",
                "Fill the right half of the screen",
                &["right", "snap right", "half right", "right side"],
            ),
            Layout::TopHalf => d(
                "Top half",
                "Fill the top half of the screen",
                &["top", "upper half", "half top", "up"],
            ),
            Layout::BottomHalf => d(
                "Bottom half",
                "Fill the bottom half of the screen",
                &["bottom", "lower half", "half bottom", "down"],
            ),
            Layout::TopLeft => d(
                "Top left quarter",
                "Fill the top left quarter of the screen",
                &["top left", "upper left", "tl", "quarter top left"],
            ),
            Layout::TopRight => d(
                "Top right quarter",
                "Fill the top right quarter of the screen",
                &["top right", "upper right", "tr", "quarter top right"],
            ),
            Layout::BottomLeft => d(
                "Bottom left quarter",
                "Fill the bottom left quarter of the screen",
                &["bottom left", "lower left", "bl", "quarter bottom left"],
            ),
            Layout::BottomRight => d(
                "Bottom right quarter",
                "Fill the bottom right quarter of the screen",
                &["bottom right", "lower right", "br", "quarter bottom right"],
            ),
            Layout::LeftThird => d(
                "Left third",
                "Fill the left third of the screen",
                &["first third", "third left", "left 1/3"],
            ),
            Layout::CenterThird => d(
                "Center third",
                "Fill the middle third of the screen",
                &[
                    "middle third",
                    "centre third",
                    "third center",
                    "third middle",
                ],
            ),
            Layout::RightThird => d(
                "Right third",
                "Fill the right third of the screen",
                &["last third", "third right", "right 1/3"],
            ),
            Layout::LeftTwoThirds => d(
                "Left two thirds",
                "Fill the left two thirds of the screen",
                &["first two thirds", "two thirds left", "left 2/3"],
            ),
            Layout::RightTwoThirds => d(
                "Right two thirds",
                "Fill the right two thirds of the screen",
                &["last two thirds", "two thirds right", "right 2/3"],
            ),
            Layout::Maximize => d(
                "Maximize",
                "Fill the whole screen (without the taskbar or menu bar)",
                &["max", "maximise", "full", "fill", "larger"],
            ),
            Layout::AlmostMaximize => d(
                "Almost maximize",
                "Fill most of the screen and leave a margin around the window",
                &["almost max", "almost maximise", "nearly maximize", "90%"],
            ),
            Layout::Center => d(
                "Center",
                "Keep the window's size and put it in the middle of the screen",
                &["centre", "middle", "center window"],
            ),
        },
        WindowCommand::Restore => d(
            "Restore",
            "Put the window back where it was before it was moved",
            &["undo", "back", "previous size", "original"],
        ),
        WindowCommand::NextDisplay => d(
            "Next display",
            "Move the window to the next display",
            &[
                "next monitor",
                "next screen",
                "move right display",
                "display next",
            ],
        ),
        WindowCommand::PreviousDisplay => d(
            "Previous display",
            "Move the window to the previous display",
            &[
                "previous monitor",
                "previous screen",
                "prev display",
                "display previous",
            ],
        ),
    }
}

/// Lowercases `input` and turns hyphens and underscores into spaces, so
/// `almost-max` finds "almost max".
fn normalize(input: &str) -> String {
    let spaced: String = input
        .chars()
        .map(|c| if matches!(c, '-' | '_') { ' ' } else { c })
        .collect();
    spaced
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

struct Entry {
    command: WindowCommand,
    title: &'static str,
    title_lower: String,
    aliases: &'static [&'static str],
}

fn entries() -> Vec<Entry> {
    WindowCommand::all()
        .into_iter()
        .map(|command| {
            let d = describe(command);
            Entry {
                command,
                title: d.title,
                title_lower: d.title.to_lowercase(),
                aliases: d.aliases,
            }
        })
        .collect()
}

/// One open window as the switcher keeps it between queries.
#[derive(Debug, Clone, PartialEq, Eq)]
struct WindowRow {
    id: String,
    title: String,
    app: String,
    minimized: bool,
}

/// What the layouts' search offers, or why it offers nothing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Layouts,
    Switcher,
}

/// A support answer and when it was asked for.
type SupportMemo = Option<(Instant, WindowSupport)>;

/// The window layouts (`win`) or the window switcher (`w`).
pub struct WindowManagerPlugin {
    mode: Mode,
    config: WindowManagementConfig,
    keyword: Option<String>,
    platform: Arc<dyn PlatformProvider>,
    memory: &'static RestoreMemory,
    entries: Vec<Entry>,
    support: Mutex<SupportMemo>,
    windows: Arc<Cache<Vec<WindowRow>>>,
    notifier: std::sync::OnceLock<ResultsNotifier>,
}

impl WindowManagerPlugin {
    /// Both searches for `config`. They always exist (settings lists them); a
    /// disabled configuration just makes them answer nothing and own no keyword.
    pub fn instances(
        config: &WindowManagementConfig,
        platform: Arc<dyn PlatformProvider>,
    ) -> Vec<Arc<dyn Plugin>> {
        vec![
            Arc::new(Self::new(Mode::Layouts, config, platform.clone())),
            Arc::new(Self::new(Mode::Switcher, config, platform)),
        ]
    }

    /// The layouts search only.
    pub fn layouts(config: &WindowManagementConfig, platform: Arc<dyn PlatformProvider>) -> Self {
        Self::new(Mode::Layouts, config, platform)
    }

    /// The window switcher only.
    pub fn switcher(config: &WindowManagementConfig, platform: Arc<dyn PlatformProvider>) -> Self {
        Self::new(Mode::Switcher, config, platform)
    }

    fn new(
        mode: Mode,
        config: &WindowManagementConfig,
        platform: Arc<dyn PlatformProvider>,
    ) -> Self {
        let word = match mode {
            Mode::Layouts => &config.keyword,
            Mode::Switcher => &config.switcher_keyword,
        };
        let keyword =
            Some(word.trim().to_owned()).filter(|keyword| config.enabled && !keyword.is_empty());
        Self {
            mode,
            config: config.clone(),
            keyword,
            platform,
            memory: shared_memory(),
            entries: entries(),
            support: Mutex::new(None),
            windows: Cache::new(),
            notifier: std::sync::OnceLock::new(),
        }
    }

    /// Uses `memory` instead of the process-wide one (tests).
    #[cfg(test)]
    fn with_memory(mut self, memory: &'static RestoreMemory) -> Self {
        self.memory = memory;
        self
    }

    fn notifier(&self) -> Option<ResultsNotifier> {
        self.notifier.get().cloned()
    }

    /// Whether window management works now (asked at most every few seconds).
    fn support(&self) -> WindowSupport {
        let mut memo = self
            .support
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((asked, support)) = memo.as_ref() {
            if asked.elapsed() < SUPPORT_TTL {
                return support.clone();
            }
        }
        let support = self.platform.window_support();
        *memo = Some((Instant::now(), support.clone()));
        support
    }

    fn row(&self, entry: &Entry, score: f64) -> ResultItem {
        let command = entry.command;
        ResultItem::new(
            LAYOUTS_ID,
            command.key(),
            entry.title,
            Action::Custom {
                payload: format!("{PAYLOAD_PREFIX}{}", command.key()),
            },
        )
        .with_subtitle(describe(command).subtitle)
        .with_icon(IconSource::builtin("desktop"))
        .with_score(score)
    }

    /// The row that explains why window management does not work.
    fn unavailable_row(&self, reason: &str) -> ResultItem {
        ResultItem::new(
            self.id(),
            "unavailable",
            text::UNAVAILABLE_TITLE,
            Action::Custom {
                payload: HINT_PAYLOAD.to_owned(),
            },
        )
        .with_subtitle(reason)
        .with_icon(IconSource::builtin("desktop"))
        .with_score(score::KEYWORD)
    }

    fn layout_rows(&self, input: &str) -> Vec<ResultItem> {
        let query = normalize(input);
        if input.trim().is_empty() {
            return self
                .entries
                .iter()
                .enumerate()
                .map(|(i, entry)| self.row(entry, score::KEYWORD - 100.0 - i as f64))
                .collect();
        }
        if query.chars().count() < MIN_QUERY_CHARS {
            return Vec::new();
        }
        let mut fuzzy = FuzzyQuery::new(&query);
        if fuzzy.is_empty() {
            return Vec::new();
        }
        let mut scored: Vec<(f64, &Entry)> = Vec::new();
        for entry in &self.entries {
            let title = entry.title_lower.as_str();
            let exact = title == query || entry.aliases.iter().any(|alias| *alias == query);
            let mut best = fuzzy.score(entry.title).map(|s| {
                f64::from(s) + name_bonus(title, &query) - title.len() as f64 * LENGTH_PENALTY
            });
            for alias in entry.aliases {
                if let Some(s) = fuzzy.score(alias) {
                    let s = f64::from(s) * ALIAS_WEIGHT + name_bonus(alias, &query);
                    best = Some(best.map_or(s, |b| b.max(s)));
                }
            }
            if let Some(mut best) = best {
                if exact {
                    best += EXACT_BONUS;
                }
                scored.push((best, entry));
            }
        }
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        scored
            .into_iter()
            .map(|(score, entry)| self.row(entry, score))
            .collect()
    }

    fn window_rows(&self, input: &str) -> Vec<ResultItem> {
        let platform = self.platform.clone();
        self.windows.refresh_if_stale(
            LIST_TTL,
            move || {
                platform
                    .list_windows()
                    .map(|windows| {
                        windows
                            .into_iter()
                            .map(|window| WindowRow {
                                id: window.id.as_str().to_owned(),
                                title: window.title,
                                app: window.app,
                                minimized: window.minimized,
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            },
            self.notifier(),
            SWITCHER_ID,
        );
        let windows = self.windows.get();
        let query = input.trim();
        let mut fuzzy = FuzzyQuery::new(query);
        let query_lower = query.to_lowercase();
        let mut scored: Vec<(f64, &WindowRow)> = Vec::new();
        for (index, window) in windows.iter().enumerate() {
            if fuzzy.is_empty() {
                // Most recently used first, as the system lists them.
                scored.push((
                    score::KEYWORD + (MAX_ROWS * 4) as f64 - index as f64,
                    window,
                ));
                continue;
            }
            let title = fuzzy
                .score(&window.title)
                .map(|s| f64::from(s) + name_bonus(&window.title.to_lowercase(), &query_lower));
            let app = fuzzy.score(&window.app).map(|s| {
                f64::from(s) * ALIAS_WEIGHT + name_bonus(&window.app.to_lowercase(), &query_lower)
            });
            if let Some(best) = [title, app].into_iter().flatten().reduce(f64::max) {
                scored.push((score::KEYWORD + best, window));
            }
        }
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        scored.truncate(MAX_ROWS);
        scored
            .into_iter()
            .map(|(score, window)| self.window_row(window, score))
            .collect()
    }

    fn window_row(&self, window: &WindowRow, score: f64) -> ResultItem {
        let title = if window.title.is_empty() {
            text::UNTITLED
        } else {
            window.title.as_str()
        };
        let mut subtitle = window.app.clone();
        if window.minimized {
            if !subtitle.is_empty() {
                subtitle.push_str(" \u{b7} ");
            }
            subtitle.push_str(text::MINIMIZED);
        }
        ResultItem::new(
            SWITCHER_ID,
            &window.id,
            title,
            Action::Custom {
                payload: format!("{FOCUS_PREFIX}{}", window.id),
            },
        )
        .with_subtitle(subtitle)
        .with_icon(IconSource::builtin("app"))
        .with_score(score)
    }

    /// Whether a layout payload may run: valid and the plugin is switched on.
    fn run_layout(&self, command: WindowCommand) -> PluginResult<()> {
        apply_command(
            self.platform.as_ref(),
            self.memory,
            command,
            self.config.gap,
        )
        .map_err(PluginError::other)
    }
}

impl Plugin for WindowManagerPlugin {
    fn id(&self) -> &str {
        match self.mode {
            Mode::Layouts => LAYOUTS_ID,
            Mode::Switcher => SWITCHER_ID,
        }
    }

    fn name(&self) -> &str {
        match self.mode {
            Mode::Layouts => text::LAYOUTS_NAME,
            Mode::Switcher => text::SWITCHER_NAME,
        }
    }

    fn description(&self) -> &str {
        match self.mode {
            Mode::Layouts => text::LAYOUTS_DESCRIPTION,
            Mode::Switcher => text::SWITCHER_DESCRIPTION,
        }
    }

    fn keyword(&self) -> Option<&str> {
        self.keyword.as_deref()
    }

    fn global(&self) -> bool {
        match self.mode {
            Mode::Layouts => self.config.enabled && self.config.global,
            Mode::Switcher => false,
        }
    }

    fn keyword_row(&self) -> Option<ResultItem> {
        let keyword = self.keyword.as_deref()?;
        let (title, subtitle) = match self.mode {
            Mode::Layouts => (text::LAYOUTS_KEYWORD_TITLE, text::LAYOUTS_KEYWORD_SUBTITLE),
            Mode::Switcher => (
                text::SWITCHER_KEYWORD_TITLE,
                text::SWITCHER_KEYWORD_SUBTITLE,
            ),
        };
        Some(
            ResultItem::new(
                self.id(),
                "keyword",
                title,
                Action::Custom {
                    payload: HINT_PAYLOAD.to_owned(),
                },
            )
            .with_subtitle(subtitle)
            .with_icon(IconSource::builtin(match self.mode {
                Mode::Layouts => "desktop",
                Mode::Switcher => "app",
            }))
            .with_autocomplete(format!("{keyword} ")),
        )
    }

    fn query(&self, input: &str) -> Vec<ResultItem> {
        if !self.config.enabled || (self.keyword.is_none() && self.mode == Mode::Switcher) {
            return Vec::new();
        }
        if let WindowSupport::Unavailable(reason) = self.support() {
            // A plain query that happens to match must not show an error row;
            // only the keyword search explains.
            return if self.global() {
                Vec::new()
            } else {
                vec![self.unavailable_row(&reason)]
            };
        }
        match self.mode {
            Mode::Layouts => self.layout_rows(input),
            Mode::Switcher => {
                let rows = self.window_rows(input);
                if rows.is_empty() && input.trim().is_empty() && self.windows.get().is_empty() {
                    // Before the first list arrives, or on a desktop without
                    // windows: say so rather than showing nothing.
                    return vec![ResultItem::new(
                        SWITCHER_ID,
                        "none",
                        text::NO_WINDOWS,
                        Action::Custom {
                            payload: HINT_PAYLOAD.to_owned(),
                        },
                    )
                    .with_subtitle(text::NO_WINDOWS_HINT)
                    .with_icon(IconSource::builtin("app"))
                    .with_score(score::KEYWORD)];
                }
                rows
            }
        }
    }

    /// `windows:<layout key>` (`windows:left`, `windows:next_display`), so a
    /// `[[hotkey]]` can run a layout directly.
    fn resolve(&self, id: &str) -> Option<ResultItem> {
        if self.mode != Mode::Layouts || !self.config.enabled {
            return None;
        }
        let command = WindowCommand::from_key(id.strip_prefix("windows:")?)?;
        let entry = self.entries.iter().find(|entry| entry.command == command)?;
        Some(self.row(entry, 0.0))
    }

    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        let Action::Custom { payload } = &item.action else {
            return Err(PluginError::Unsupported(item.id.clone()));
        };
        if payload == HINT_PAYLOAD {
            return Err(PluginError::Message(match self.support() {
                WindowSupport::Unavailable(reason) => reason,
                WindowSupport::Available => match self.mode {
                    Mode::Layouts => "Press Tab to complete the keyword, then type a layout",
                    Mode::Switcher => "Press Tab to complete the keyword, then type a window",
                }
                .to_owned(),
            }));
        }
        if !self.config.enabled {
            return Err(PluginError::Message(format!(
                "{} {}",
                self.name(),
                text::NOT_AVAILABLE_HERE
            )));
        }
        if let Some(id) = payload.strip_prefix(FOCUS_PREFIX) {
            let window = WindowId::new(id)
                .ok_or_else(|| PluginError::Message(text::BAD_WINDOW.to_owned()))?;
            return self
                .platform
                .focus_window(&window)
                .map_err(PluginError::other);
        }
        let command = payload
            .strip_prefix(PAYLOAD_PREFIX)
            .and_then(WindowCommand::from_key)
            .ok_or_else(|| PluginError::Unsupported(item.id.clone()))?;
        self.run_layout(command)
    }

    /// Layouts are remembered like any other result (the ones you use often
    /// rank first); window rows are not: their ids are handles that mean
    /// nothing next time, and the titles are private.
    fn tracks_usage(&self) -> bool {
        self.mode == Mode::Layouts
    }

    fn attach_notifier(&self, notifier: ResultsNotifier) {
        let _ = self.notifier.set(notifier);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use sevak_core::window_layout::{Monitor, Rect};
    use sevak_platform::window_manager::{WindowInfo, WindowState};
    use sevak_platform::{PlatformError, Result as PlatformResult};

    use super::*;
    use crate::test_util::MockPlatform;

    fn id(text: &str) -> WindowId {
        WindowId::new(text).unwrap()
    }

    fn leak_memory() -> &'static RestoreMemory {
        Box::leak(Box::new(RestoreMemory::new()))
    }

    /// A desktop with one display and a few windows, recording what is done.
    struct Desktop {
        support: Mutex<WindowSupport>,
        windows: Vec<WindowInfo>,
        target: Mutex<WindowState>,
        placed: Mutex<Vec<Rect>>,
        focused: Mutex<Vec<String>>,
        list_calls: Mutex<usize>,
    }

    type Mutex<T> = std::sync::Mutex<T>;

    impl Desktop {
        fn new() -> Arc<Self> {
            let windows = [
                ("1", "Release notes - Firefox", "firefox", false),
                ("2", "main.rs - sevak - Visual Studio Code", "Code", false),
                ("3", "Terminal", "wt", true),
            ]
            .map(|(window, title, app, minimized)| WindowInfo {
                id: id(window),
                title: title.into(),
                app: app.into(),
                minimized,
            })
            .to_vec();
            Arc::new(Self {
                support: Mutex::new(WindowSupport::Available),
                windows,
                target: Mutex::new(WindowState {
                    id: id("9"),
                    title: "Notes".into(),
                    app: "notes".into(),
                    rect: Rect::new(200, 100, 800, 600),
                    maximized: false,
                    minimized: false,
                }),
                placed: Mutex::new(Vec::new()),
                focused: Mutex::new(Vec::new()),
                list_calls: Mutex::new(0),
            })
        }
    }

    impl PlatformProvider for Desktop {
        fn list_applications(&self) -> PlatformResult<Vec<sevak_core::AppEntry>> {
            Ok(Vec::new())
        }
        fn launch(&self, _: &sevak_core::LaunchTarget) -> PlatformResult<()> {
            Ok(())
        }
        fn load_icon(
            &self,
            _: &sevak_core::IconSource,
            _: u32,
        ) -> PlatformResult<sevak_core::IconData> {
            Err(PlatformError::Unsupported("icons"))
        }
        fn window_support(&self) -> WindowSupport {
            self.support.lock().unwrap().clone()
        }
        fn list_windows(&self) -> PlatformResult<Vec<WindowInfo>> {
            *self.list_calls.lock().unwrap() += 1;
            Ok(self.windows.clone())
        }
        fn focus_window(&self, window: &WindowId) -> PlatformResult<()> {
            self.focused.lock().unwrap().push(window.to_string());
            Ok(())
        }
        fn target_window(&self) -> PlatformResult<WindowState> {
            Ok(self.target.lock().unwrap().clone())
        }
        fn window_state(&self, _: &WindowId) -> PlatformResult<WindowState> {
            Ok(self.target.lock().unwrap().clone())
        }
        fn set_window_rect(&self, _: &WindowId, rect: Rect) -> PlatformResult<()> {
            self.placed.lock().unwrap().push(rect);
            self.target.lock().unwrap().rect = rect;
            Ok(())
        }
        fn list_monitors(&self) -> PlatformResult<Vec<Monitor>> {
            let mut primary = Monitor::new(
                "1",
                Rect::new(0, 0, 1920, 1080),
                Rect::new(0, 0, 1920, 1040),
            );
            primary.primary = true;
            Ok(vec![primary])
        }
    }

    fn layouts(desktop: &Arc<Desktop>, config: &WindowManagementConfig) -> WindowManagerPlugin {
        WindowManagerPlugin::layouts(config, desktop.clone()).with_memory(leak_memory())
    }

    fn switcher(desktop: &Arc<Desktop>) -> WindowManagerPlugin {
        WindowManagerPlugin::switcher(&WindowManagementConfig::default(), desktop.clone())
    }

    fn first(plugin: &WindowManagerPlugin, input: &str) -> ResultItem {
        plugin
            .query(input)
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("no result for {input:?}"))
    }

    /// Runs the query again until the background list arrives, the way the
    /// shell re-runs it when the notifier fires.
    fn query_after_load(plugin: &WindowManagerPlugin, input: &str) -> Vec<ResultItem> {
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        plugin.attach_notifier(Arc::new(move |_| {
            let _ = tx.lock().unwrap().send(());
        }));
        plugin.query(input);
        // Check emptiness after the query: the list may arrive between the two,
        // and the rows returned by that first query would then be stale.
        if plugin.windows.get().is_empty() {
            rx.recv_timeout(Duration::from_secs(5)).expect("a refresh");
        }
        plugin.query(input)
    }

    #[test]
    fn the_spec_commands_find_their_layouts() {
        let desktop = Desktop::new();
        let plugin = layouts(&desktop, &WindowManagementConfig::default());
        for (typed, key) in [
            ("left", "left"),
            ("right", "right"),
            ("top", "top"),
            ("bottom", "bottom"),
            ("max", "maximize"),
            ("maximize", "maximize"),
            ("center", "center"),
            ("restore", "restore"),
            ("next display", "next_display"),
            ("previous display", "previous_display"),
            ("almost-max", "almost_maximize"),
            ("almost max", "almost_maximize"),
            ("top left", "top_left"),
            ("TR", "top_right"),
            ("bottom left", "bottom_left"),
            ("br", "bottom_right"),
            ("left third", "left_third"),
            ("center third", "center_third"),
            ("right third", "right_third"),
            ("left two thirds", "left_two_thirds"),
            ("right two thirds", "right_two_thirds"),
            ("  Left  ", "left"),
        ] {
            let item = first(&plugin, typed);
            assert_eq!(
                item.id,
                format!("windows:{key}"),
                "{typed:?} -> {}",
                item.title
            );
            assert_eq!(
                item.action,
                Action::Custom {
                    payload: format!("win:{key}")
                }
            );
        }
    }

    #[test]
    fn the_bare_keyword_lists_every_command_in_order() {
        let desktop = Desktop::new();
        let plugin = layouts(&desktop, &WindowManagementConfig::default());
        let rows = plugin.query("");
        assert_eq!(rows.len(), WindowCommand::all().len());
        assert!(rows.windows(2).all(|w| w[0].score > w[1].score));
        assert_eq!(rows[0].id, "windows:left");
        assert!(rows.iter().all(|row| row.plugin_id == "windows"));
        // Titles and ids are unique.
        let mut ids: Vec<_> = rows.iter().map(|r| r.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), rows.len());
    }

    #[test]
    fn unrelated_and_too_short_queries_match_nothing() {
        let desktop = Desktop::new();
        let plugin = layouts(&desktop, &WindowManagementConfig::default());
        assert!(plugin.query("l").is_empty());
        assert!(plugin.query("firefox").is_empty());
        assert!(plugin.query("qqqq").is_empty());
        assert!(plugin.query("-").is_empty());
    }

    #[test]
    fn keywords_and_global_follow_the_config() {
        let desktop = Desktop::new();
        let plugins =
            WindowManagerPlugin::instances(&WindowManagementConfig::default(), desktop.clone());
        let summary: Vec<_> = plugins
            .iter()
            .map(|p| {
                (
                    p.id().to_owned(),
                    p.keyword().map(str::to_owned),
                    p.global(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("windows".into(), Some("win".into()), false),
                ("windows:switch".into(), Some("w".into()), false)
            ]
        );
        let config = WindowManagementConfig {
            keyword: " snap ".into(),
            switcher_keyword: String::new(),
            global: true,
            ..WindowManagementConfig::default()
        };
        let plugins = WindowManagerPlugin::instances(&config, desktop.clone());
        assert_eq!(plugins[0].keyword(), Some("snap"));
        assert!(plugins[0].global());
        assert_eq!(
            plugins[1].keyword(),
            None,
            "an empty keyword turns the switcher off"
        );
        assert!(plugins[1].query("firefox").is_empty());
        assert!(!plugins[1].global());
        // Row for the bare keyword completes it.
        let row = plugins[0].keyword_row().unwrap();
        assert_eq!(row.autocomplete.as_deref(), Some("snap "));
        assert!(plugins[1].keyword_row().is_none());
    }

    #[test]
    fn a_disabled_feature_owns_no_keyword_and_answers_nothing() {
        let desktop = Desktop::new();
        let config = WindowManagementConfig {
            enabled: false,
            global: true,
            ..WindowManagementConfig::default()
        };
        for plugin in WindowManagerPlugin::instances(&config, desktop.clone()) {
            assert_eq!(plugin.keyword(), None);
            assert!(!plugin.global());
            assert!(plugin.query("left").is_empty());
            assert!(plugin.keyword_row().is_none());
            assert!(plugin.resolve("windows:left").is_none());
        }
        let plugin = WindowManagerPlugin::layouts(&config, desktop.clone());
        let item = ResultItem::new(
            "windows",
            "left",
            "Left half",
            Action::Custom {
                payload: "win:left".into(),
            },
        );
        assert!(plugin.execute(&item).is_err());
        assert!(desktop.placed.lock().unwrap().is_empty());
    }

    #[test]
    fn enter_on_a_layout_moves_the_remembered_window() {
        let desktop = Desktop::new();
        let plugin = layouts(&desktop, &WindowManagementConfig::default());
        plugin.execute(&first(&plugin, "left")).unwrap();
        plugin.execute(&first(&plugin, "top right")).unwrap();
        plugin.execute(&first(&plugin, "restore")).unwrap();
        assert_eq!(
            *desktop.placed.lock().unwrap(),
            [
                Rect::new(0, 0, 960, 1040),
                Rect::new(960, 0, 960, 520),
                Rect::new(200, 100, 800, 600),
            ]
        );
    }

    #[test]
    fn the_gap_setting_reaches_the_layout() {
        let desktop = Desktop::new();
        let config = WindowManagementConfig {
            gap: 10,
            ..WindowManagementConfig::default()
        };
        let plugin = layouts(&desktop, &config);
        plugin.execute(&first(&plugin, "max")).unwrap();
        assert_eq!(
            *desktop.placed.lock().unwrap(),
            [Rect::new(10, 10, 1900, 1020)]
        );
    }

    #[test]
    fn hotkeys_resolve_layouts_by_id() {
        let desktop = Desktop::new();
        let plugin = layouts(&desktop, &WindowManagementConfig::default());
        let item = plugin.resolve("windows:almost_maximize").unwrap();
        assert_eq!(item.title, "Almost maximize");
        plugin.execute(&item).unwrap();
        assert_eq!(
            *desktop.placed.lock().unwrap(),
            [Rect::new(96, 52, 1728, 936)]
        );
        for bad in [
            "windows:",
            "windows:nope",
            "windows:left;calc",
            "windows:focus:1",
            "tasks:left",
            "windows:switch:1",
        ] {
            assert!(plugin.resolve(bad).is_none(), "{bad}");
        }
        // The switcher resolves nothing.
        assert!(switcher(&desktop).resolve("windows:left").is_none());
    }

    #[test]
    fn unavailable_window_management_is_explained_not_failed_silently() {
        let desktop = Desktop::new();
        *desktop.support.lock().unwrap() =
            WindowSupport::Unavailable("Wayland does not allow this".into());
        let plugin = layouts(&desktop, &WindowManagementConfig::default());
        let rows = plugin.query("left");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, text::UNAVAILABLE_TITLE);
        assert_eq!(rows[0].subtitle, "Wayland does not allow this");
        let error = plugin.execute(&rows[0]).unwrap_err();
        assert_eq!(error.to_string(), "Wayland does not allow this");
        // The switcher explains the same way.
        let rows = switcher(&desktop).query("fire");
        assert_eq!(rows[0].subtitle, "Wayland does not allow this");
        assert_eq!(
            *desktop.list_calls.lock().unwrap(),
            0,
            "no OS listing was started"
        );
        // Hotkey path: the layout row is still resolved, and running it shows the reason.
        let item = plugin.resolve("windows:left").unwrap();
        let error = plugin.execute(&item).unwrap_err();
        assert_eq!(error.to_string(), "Wayland does not allow this");
        assert!(desktop.placed.lock().unwrap().is_empty());
        // A global plugin stays quiet for plain queries.
        let global = WindowManagerPlugin::layouts(
            &WindowManagementConfig {
                global: true,
                ..WindowManagementConfig::default()
            },
            desktop.clone(),
        );
        assert!(global.query("left").is_empty());
    }

    #[test]
    fn the_switcher_lists_windows_most_recent_first_and_finds_them_by_title_or_app() {
        let desktop = Desktop::new();
        let plugin = switcher(&desktop);
        let rows = query_after_load(&plugin, "");
        assert_eq!(
            rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
            [
                "Release notes - Firefox",
                "main.rs - sevak - Visual Studio Code",
                "Terminal"
            ]
        );
        assert!(rows.windows(2).all(|w| w[0].score > w[1].score));
        assert!(rows.iter().all(|r| r.score >= score::KEYWORD));
        assert_eq!(rows[2].subtitle, "wt \u{b7} minimized");
        assert_eq!(rows[0].subtitle, "firefox");
        assert_eq!(rows[0].id, "windows:switch:1");
        assert_eq!(
            rows[0].action,
            Action::Custom {
                payload: "win:focus:1".into()
            }
        );

        assert_eq!(first(&plugin, "release").title, "Release notes - Firefox");
        // By app name even though the title lacks it.
        assert_eq!(
            first(&plugin, "code").title,
            "main.rs - sevak - Visual Studio Code"
        );
        assert_eq!(first(&plugin, "wt").title, "Terminal");
        assert!(plugin.query("zzzz").is_empty());
    }

    #[test]
    fn the_window_list_is_not_asked_for_on_every_keystroke() {
        let desktop = Desktop::new();
        let plugin = switcher(&desktop);
        query_after_load(&plugin, "");
        for typed in ["f", "fi", "fir", "fire"] {
            plugin.query(typed);
        }
        assert_eq!(*desktop.list_calls.lock().unwrap(), 1);
    }

    #[test]
    fn enter_on_a_window_focuses_exactly_that_window() {
        let desktop = Desktop::new();
        let plugin = switcher(&desktop);
        let rows = query_after_load(&plugin, "terminal");
        plugin.execute(&rows[0]).unwrap();
        assert_eq!(*desktop.focused.lock().unwrap(), ["3"]);
    }

    #[test]
    fn payloads_outside_the_closed_vocabulary_are_refused() {
        let desktop = Desktop::new();
        let plugin = layouts(&desktop, &WindowManagementConfig::default());
        let run = |payload: &str| {
            plugin.execute(&ResultItem::new(
                "windows",
                "x",
                "x",
                Action::Custom {
                    payload: payload.into(),
                },
            ))
        };
        for bad in [
            "",
            "left",
            "win:",
            "win:nope",
            "win:left ",
            "win:LEFT",
            "win:left;rm -rf /",
            "win:focus:",
            "win:focus:a b",
            "win:focus:$(calc)",
            "win:focus:../../x",
            "task:dark_mode",
            "win:focus:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ] {
            assert!(run(bad).is_err(), "{bad:?}");
        }
        assert!(desktop.placed.lock().unwrap().is_empty());
        assert!(desktop.focused.lock().unwrap().is_empty());
        // Other action kinds are not this plugin's.
        let other = ResultItem::new("windows", "x", "x", Action::CopyText { text: "x".into() });
        assert!(matches!(
            plugin.execute(&other),
            Err(PluginError::Unsupported(_))
        ));
    }

    #[test]
    fn hints_complete_the_keyword_and_do_nothing_else() {
        let desktop = Desktop::new();
        let plugin = layouts(&desktop, &WindowManagementConfig::default());
        let row = plugin.keyword_row().unwrap();
        assert_eq!(row.autocomplete.as_deref(), Some("win "));
        let error = plugin.execute(&row).unwrap_err();
        assert!(error.to_string().contains("Tab"), "{error}");
        assert!(desktop.placed.lock().unwrap().is_empty());
    }

    #[test]
    fn the_default_platform_has_no_window_management_and_says_so() {
        let plugin =
            WindowManagerPlugin::layouts(&WindowManagementConfig::default(), MockPlatform::empty());
        let rows = plugin.query("left");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, text::UNAVAILABLE_TITLE);
    }

    #[test]
    fn only_layouts_are_remembered_in_the_usage_statistics() {
        let desktop = Desktop::new();
        let config = WindowManagementConfig::default();
        assert!(WindowManagerPlugin::layouts(&config, desktop.clone()).tracks_usage());
        assert!(!WindowManagerPlugin::switcher(&config, desktop).tracks_usage());
    }

    #[test]
    fn normalizing_input() {
        assert_eq!(normalize("  Almost-Max "), "almost max");
        assert_eq!(normalize("next_display"), "next display");
        assert_eq!(normalize("A   B"), "a b");
        assert_eq!(normalize(""), "");
    }
}
