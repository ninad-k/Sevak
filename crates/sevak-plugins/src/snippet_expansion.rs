//! Snippet expansion as you type: type a snippet's `keyword` in any app and
//! Sevak replaces it with the snippet's text (see `[snippets]` in the config).
//!
//! Two halves:
//!
//! - [`Matcher`] is the pure part. It is fed [`KeyEvent`]s, keeps the last few
//!   characters typed, and says when they end with a keyword.
//! - [`SnippetExpansion`] is the running service: it starts the platform's key
//!   listener, feeds the matcher on a worker thread, checks where the user is
//!   typing, and asks the platform to swap the keyword for the text.
//!
//! # What is kept, and for how long
//!
//! This feature watches keystrokes, so the rules are strict:
//!
//! - The only copy of what was typed is the matcher's buffer: the last
//!   [`BUFFER_CHARS`] characters, in memory. It is never written to disk, never
//!   logged and never sent anywhere; the types here do not implement `Debug`
//!   output that shows it.
//! - The buffer is wiped when the caret may have moved (a click, a key that is
//!   not typing, a change of focus), when the app typed into is not one to
//!   watch, and after every expansion.
//! - Nothing is observed while `[snippets] auto_expand` is off: the key listener
//!   is not even started.
//! - Characters typed in Sevak's own windows, in a terminal (unless
//!   `expand_in_terminals`), in an app listed in `ignore_apps`, or in a password
//!   box the platform can recognise never reach the buffer.
//!
//! # Matching
//!
//! A snippet expands from `prefix + keyword`. A trigger that starts with a letter
//! or digit only counts at the start of a word (`sig` does not fire inside
//! `assign`); one that starts with punctuation, such as `;sig`, fires anywhere.
//! With several triggers matching, the longest wins. In `immediate` mode the
//! trigger fires on its last character, in `delimiter` mode when a character that
//! is not a letter or digit follows it (that character is kept after the text).
//! Enter and Tab are not delimiters: the app has already acted on them (sent the
//! message, moved the focus) by the time Sevak could.

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use chrono::Local;
use sevak_core::config::{ExpandOn, Snippet, SnippetsConfig};
use sevak_core::Config;
use sevak_platform::capture::is_terminal;
use sevak_platform::{KeyEvent, KeyListener, KeyListenerSupport, PlatformProvider, TypingTarget};

use crate::example_uuid::random_uuid_v4;
use crate::snippets::{expand, Env};

/// How many of the latest typed characters are remembered.
pub const BUFFER_CHARS: usize = 64;

/// How long the answer to "where is the user typing?" is reused between
/// keystrokes. A click or a change of focus resets it at once.
const TARGET_CACHE: Duration = Duration::from_millis(250);

/// How often the worker looks at its stop flag when nothing is typed.
const IDLE_POLL: Duration = Duration::from_millis(250);

/// The last few typed characters. Wiped, not just emptied, when cleared or
/// dropped.
struct Buffer {
    chars: Vec<char>,
}

impl Buffer {
    fn new() -> Self {
        Self {
            chars: Vec::with_capacity(BUFFER_CHARS),
        }
    }

    fn push(&mut self, c: char) {
        if self.chars.len() == BUFFER_CHARS {
            self.chars.remove(0);
        }
        self.chars.push(c);
    }

    fn pop(&mut self) {
        self.chars.pop();
    }

    /// Overwrites every remembered character before forgetting it.
    fn wipe(&mut self) {
        self.chars.fill('\0');
        self.chars.clear();
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        self.wipe();
    }
}

/// One keyword that expands.
#[derive(Clone)]
struct Trigger {
    /// `prefix + keyword`.
    chars: Vec<char>,
    /// Index into the snippets the matcher was built from.
    snippet: usize,
}

/// A keyword was typed: remove it (and the delimiter that followed, if any) and
/// put the snippet in its place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// Which snippet, as an index into the list given to [`Matcher::new`].
    pub snippet: usize,
    /// How many Backspaces remove what was typed: the trigger, plus the
    /// delimiter that fired it. Counted in typed characters, so an emoji or a
    /// letter with an accent is one, whatever its length in bytes.
    pub delete: usize,
    /// The delimiter that fired the match (`delimiter` mode); it goes back after
    /// the expanded text.
    pub delimiter: Option<char>,
}

/// Finds snippet keywords in a stream of typed characters. See the module
/// documentation.
pub struct Matcher {
    triggers: Vec<Trigger>,
    expand_on: ExpandOn,
    case_sensitive: bool,
    buffer: Buffer,
}

// By hand: the buffer holds what the user typed.
impl fmt::Debug for Matcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Matcher")
            .field("triggers", &self.triggers.len())
            .field("expand_on", &self.expand_on)
            .field("case_sensitive", &self.case_sensitive)
            .finish_non_exhaustive()
    }
}

impl Matcher {
    /// A matcher for the snippets that have a keyword. Snippets without one, with
    /// a keyword containing a line break, or longer than the buffer are skipped,
    /// and so is a keyword that repeats an earlier one.
    pub fn new(snippets: &[Snippet], settings: &SnippetsConfig) -> Self {
        let mut triggers: Vec<Trigger> = Vec::new();
        for (index, snippet) in snippets.iter().enumerate() {
            let Some(keyword) = snippet.keyword.as_deref().filter(|k| !k.is_empty()) else {
                continue;
            };
            let chars: Vec<char> = settings.prefix.chars().chain(keyword.chars()).collect();
            let usable = chars.len() < BUFFER_CHARS && !chars.iter().any(|c| c.is_control());
            let repeated = triggers.iter().any(|t| {
                t.chars.len() == chars.len()
                    && t.chars
                        .iter()
                        .zip(&chars)
                        .all(|(a, b)| same(*a, *b, settings.case_sensitive))
            });
            if usable && !repeated {
                triggers.push(Trigger {
                    chars,
                    snippet: index,
                });
            }
        }
        Self {
            triggers,
            expand_on: settings.expand_on,
            case_sensitive: settings.case_sensitive,
            buffer: Buffer::new(),
        }
    }

    /// True if no snippet has a usable keyword (nothing could ever match).
    pub fn is_empty(&self) -> bool {
        self.triggers.is_empty()
    }

    /// Forgets everything typed so far.
    pub fn reset(&mut self) {
        self.buffer.wipe();
    }

    /// Takes one event. A [`Match`] means the text before the caret now ends with
    /// a keyword; the buffer has been wiped for the expansion.
    pub fn feed(&mut self, event: KeyEvent) -> Option<Match> {
        match event {
            KeyEvent::Reset => {
                self.reset();
                None
            }
            KeyEvent::Backspace => {
                self.buffer.pop();
                None
            }
            KeyEvent::Char(c) => match self.expand_on {
                ExpandOn::Immediate => {
                    self.buffer.push(c);
                    let found = self.find()?.clone();
                    self.reset();
                    Some(Match {
                        snippet: found.snippet,
                        delete: found.chars.len(),
                        delimiter: None,
                    })
                }
                ExpandOn::Delimiter if is_delimiter(c) => {
                    let found = self.find().cloned();
                    match found {
                        Some(found) => {
                            self.reset();
                            Some(Match {
                                snippet: found.snippet,
                                // The keyword and the delimiter just typed.
                                delete: found.chars.len() + 1,
                                delimiter: Some(c),
                            })
                        }
                        None => {
                            // Kept: it is the boundary before the next word.
                            self.buffer.push(c);
                            None
                        }
                    }
                }
                ExpandOn::Delimiter => {
                    self.buffer.push(c);
                    None
                }
            },
        }
    }

    /// The longest trigger the buffer ends with (the first one, among equals).
    fn find(&self) -> Option<&Trigger> {
        let typed = &self.buffer.chars;
        self.triggers
            .iter()
            .filter(|trigger| self.ends_with(typed, trigger))
            .fold(None, |best: Option<&Trigger>, trigger| match best {
                Some(best) if best.chars.len() >= trigger.chars.len() => Some(best),
                _ => Some(trigger),
            })
    }

    fn ends_with(&self, typed: &[char], trigger: &Trigger) -> bool {
        let len = trigger.chars.len();
        if typed.len() < len {
            return false;
        }
        let start = typed.len() - len;
        if !typed[start..]
            .iter()
            .zip(&trigger.chars)
            .all(|(a, b)| same(*a, *b, self.case_sensitive))
        {
            return false;
        }
        // A trigger that starts like a word must start a word: `sig` is not
        // "assign". Nothing before it (just after a reset) counts as a boundary.
        let starts_like_a_word = trigger.chars.first().is_some_and(|c| c.is_alphanumeric());
        !starts_like_a_word || start == 0 || !typed[start - 1].is_alphanumeric()
    }
}

/// True for a character that ends a word in `delimiter` mode.
fn is_delimiter(c: char) -> bool {
    !c.is_alphanumeric()
}

fn same(typed: char, wanted: char, case_sensitive: bool) -> bool {
    typed == wanted || (!case_sensitive && typed.to_lowercase().eq(wanted.to_lowercase()))
}

/// Why expansion is not (or not fully) running, for Settings and the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpansionState {
    /// `auto_expand` is off.
    Off,
    /// Watching, with this many keywords.
    Running { keywords: usize },
    /// On, but no snippet has a keyword, so there is nothing to watch for.
    NoKeywords,
    /// The OS wants a permission first (macOS); the text says which.
    NeedsPermission(String),
    /// Not possible on this system (Wayland); the text says why.
    Unavailable(String),
    /// The listener could not be started.
    Failed(String),
}

impl ExpansionState {
    /// A sentence for the user, or `None` when everything is as configured.
    pub fn problem(&self) -> Option<&str> {
        match self {
            Self::NeedsPermission(text) | Self::Unavailable(text) | Self::Failed(text) => {
                Some(text)
            }
            Self::NoKeywords => {
                Some("No snippet has a keyword yet, so there is nothing to expand.")
            }
            Self::Off | Self::Running { .. } => None,
        }
    }
}

/// The running service. Dropping it stops watching the keyboard.
pub struct SnippetExpansion {
    // Order matters: the listener goes first so no more events arrive.
    listener: Option<KeyListener>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl fmt::Debug for SnippetExpansion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SnippetExpansion")
    }
}

impl SnippetExpansion {
    /// Starts watching the keyboard if `[snippets] auto_expand` is on and it can
    /// work, and says what happened. The service is `Some` only when it runs.
    pub fn start(
        config: &Config,
        platform: Arc<dyn PlatformProvider>,
    ) -> (Option<Self>, ExpansionState) {
        if !config.snippets.auto_expand {
            return (None, ExpansionState::Off);
        }
        let matcher = Matcher::new(&config.snippet, &config.snippets);
        if matcher.is_empty() {
            return (None, ExpansionState::NoKeywords);
        }
        match platform.key_listener_support() {
            KeyListenerSupport::Available => {}
            KeyListenerSupport::NeedsPermission(text) => {
                // Lets the OS show its prompt, so Sevak is in the list to switch on.
                platform.request_key_listener_permission();
                return (None, ExpansionState::NeedsPermission(text));
            }
            KeyListenerSupport::Unavailable(text) => {
                return (None, ExpansionState::Unavailable(text))
            }
        }

        let keywords = matcher.triggers.len();
        let (tx, rx) = mpsc::channel::<KeyEvent>();
        let stop = Arc::new(AtomicBool::new(false));
        // How many events the listener has sent; the worker compares it with how
        // many it has handled to tell whether typing went on after a match.
        let sent = Arc::new(AtomicU64::new(0));
        let worker = Worker {
            rx,
            matcher,
            platform: platform.clone(),
            snippets: config.snippet.clone(),
            settings: config.snippets.clone(),
            stop: stop.clone(),
            sent: sent.clone(),
            handled: 0,
            target: None,
        };
        let handle = match std::thread::Builder::new()
            .name("sevak-expand".to_owned())
            .spawn(move || worker.run())
        {
            Ok(handle) => handle,
            Err(err) => {
                return (
                    None,
                    ExpansionState::Failed(format!("Could not start the expansion thread: {err}")),
                )
            }
        };

        let sink = Box::new(move |event: KeyEvent| {
            sent.fetch_add(1, Ordering::SeqCst);
            // The listener's thread must never wait; a gone worker is not an error.
            let _ = tx.send(event);
        });
        match platform.start_key_listener(sink) {
            Ok(listener) => {
                tracing::info!(keywords, "snippet expansion is on: watching typed keywords");
                (
                    Some(Self {
                        listener: Some(listener),
                        stop,
                        worker: Some(handle),
                    }),
                    ExpansionState::Running { keywords },
                )
            }
            Err(err) => {
                stop.store(true, Ordering::SeqCst);
                let _ = handle.join();
                tracing::warn!("snippet expansion could not start: {err}");
                (
                    None,
                    ExpansionState::Failed(format!("Could not watch the keyboard: {err}")),
                )
            }
        }
    }
}

impl Drop for SnippetExpansion {
    fn drop(&mut self) {
        drop(self.listener.take());
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        tracing::info!("snippet expansion is off");
    }
}

/// A [`TypingTarget`] and when it was looked up.
struct Looked {
    at: Instant,
    target: TypingTarget,
}

/// The thread that turns events into expansions.
struct Worker {
    rx: Receiver<KeyEvent>,
    matcher: Matcher,
    platform: Arc<dyn PlatformProvider>,
    snippets: Vec<Snippet>,
    settings: SnippetsConfig,
    stop: Arc<AtomicBool>,
    /// Events the listener has sent so far, and how many of them this worker has
    /// taken off the queue.
    sent: Arc<AtomicU64>,
    handled: u64,
    target: Option<Looked>,
}

impl Worker {
    fn run(mut self) {
        while !self.stop.load(Ordering::SeqCst) {
            match self.rx.recv_timeout(IDLE_POLL) {
                Ok(event) => {
                    self.handled += 1;
                    self.handle(event);
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        self.matcher.reset();
    }

    fn handle(&mut self, event: KeyEvent) {
        if event == KeyEvent::Reset {
            self.matcher.reset();
            // A click or a change of focus: ask again where typing goes.
            self.target = None;
            return;
        }
        if !self.watching_here() {
            return;
        }
        if let Some(found) = self.matcher.feed(event) {
            self.expand(&found);
        }
    }

    /// Whether the app being typed into is one to watch. When the answer
    /// changes from the last one the buffer is wiped, and while it is "no" no
    /// character reaches the buffer.
    fn watching_here(&mut self) -> bool {
        let stale = self
            .target
            .as_ref()
            .is_none_or(|looked| looked.at.elapsed() >= TARGET_CACHE);
        if stale {
            let target = self.platform.typing_target();
            let changed = self
                .target
                .as_ref()
                .is_none_or(|looked| looked.target != target);
            self.target = Some(Looked {
                at: Instant::now(),
                target,
            });
            if changed {
                self.matcher.reset();
            }
        }
        let allowed = self
            .target
            .as_ref()
            .is_some_and(|looked| allowed(&looked.target, &self.settings));
        if !allowed {
            self.matcher.reset();
        }
        allowed
    }

    fn expand(&mut self, found: &Match) {
        // The cached answer may be a moment old: ask again before pressing keys.
        let target = self.platform.typing_target();
        let Some(snippet) = self.snippets.get(found.snippet) else {
            return;
        };
        if allowed(&target, &self.settings) {
            let mut text = self.fill_in(&snippet.text);
            if let Some(delimiter) = found.delimiter {
                text.push(delimiter);
            }
            if !text.is_empty() {
                // Backspaces delete whatever is before the caret, so if the user
                // has typed on since the keyword was seen they must not be pressed.
                let (sent, handled) = (self.sent.clone(), self.handled);
                let typing_stopped = move || sent.load(Ordering::SeqCst) == handled;
                match self
                    .platform
                    .replace_typed_text(found.delete, &text, &typing_stopped)
                {
                    Ok(true) => tracing::debug!("expanded a snippet"),
                    Ok(false) => tracing::debug!("typing went on; a snippet was not expanded"),
                    Err(err) => tracing::warn!("could not expand a snippet: {err}"),
                }
            }
        }
        // Whatever was typed while the expansion ran is in an unknown place now.
        while self.rx.try_recv().is_ok() {
            self.handled += 1;
        }
        self.matcher.reset();
        self.target = None;
    }

    /// The snippet's text with `{date}`, `{clipboard}` and the other placeholders
    /// filled in, as when it is pasted from the launcher.
    fn fill_in(&self, template: &str) -> String {
        let clipboard = || self.platform.clipboard_text().ok().flatten();
        let env = Env {
            now: Local::now().fixed_offset(),
            clipboard: &clipboard,
            uuid: &random_uuid_v4,
        };
        expand(template, &env)
    }
}

/// Says once, at debug level and without any typed text, that an app that could
/// not be identified was skipped.
fn note_unknown_app() {
    static NOTED: AtomicBool = AtomicBool::new(false);
    if !NOTED.swap(true, Ordering::Relaxed) {
        tracing::debug!(
            "the app being typed into could not be identified; snippets are not expanded there"
        );
    }
}

/// Whether typing into `target` may be watched and expanded.
fn allowed(target: &TypingTarget, settings: &SnippetsConfig) -> bool {
    if target.own_window || target.private {
        return false;
    }
    let Some(app) = &target.app else {
        // The app cannot be told (an elevated or protected process, a window
        // without a class, an app without a bundle id): `ignore_apps` and the
        // terminal rule cannot be applied, so nothing is watched or expanded.
        note_unknown_app();
        return false;
    };
    if app.matches_any(&settings.ignore_apps) {
        return false;
    }
    settings.expand_in_terminals || !is_terminal(app)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Mutex;

    use sevak_core::{AppEntry, IconData, IconSource, LaunchTarget};
    use sevak_platform::{ForegroundApp, KeySink, PlatformError, Result};

    use super::*;

    fn snippet(name: &str, keyword: Option<&str>, text: &str) -> Snippet {
        Snippet {
            name: name.to_owned(),
            keyword: keyword.map(str::to_owned),
            text: text.to_owned(),
        }
    }

    fn settings(prefix: &str, expand_on: ExpandOn, case_sensitive: bool) -> SnippetsConfig {
        SnippetsConfig {
            auto_expand: true,
            prefix: prefix.to_owned(),
            expand_on,
            case_sensitive,
            ..SnippetsConfig::default()
        }
    }

    fn matcher(keywords: &[&str], settings: &SnippetsConfig) -> Matcher {
        let snippets: Vec<Snippet> = keywords
            .iter()
            .map(|keyword| snippet(keyword, Some(keyword), "text"))
            .collect();
        Matcher::new(&snippets, settings)
    }

    /// Types `text` and returns the first match with the number of characters
    /// typed when it fired.
    fn type_text(matcher: &mut Matcher, text: &str) -> Option<(usize, Match)> {
        text.chars()
            .enumerate()
            .find_map(|(i, c)| matcher.feed(KeyEvent::Char(c)).map(|m| (i + 1, m)))
    }

    fn immediate() -> SnippetsConfig {
        settings("", ExpandOn::Immediate, true)
    }

    fn delimited() -> SnippetsConfig {
        settings("", ExpandOn::Delimiter, true)
    }

    #[test]
    fn typing_a_keyword_fires_on_its_last_character() {
        let mut m = matcher(&["sig"], &immediate());
        let (at, found) = type_text(&mut m, "sig").unwrap();
        assert_eq!(at, 3);
        assert_eq!(found.snippet, 0);
        assert_eq!(found.delete, 3);
        assert_eq!(found.delimiter, None);
    }

    #[test]
    fn the_buffer_is_wiped_by_a_match() {
        let mut m = matcher(&["sig"], &immediate());
        type_text(&mut m, "sig").unwrap();
        // Typing "ig" now must not complete the old "s".
        assert_eq!(type_text(&mut m, "ig"), None);
        assert!(m.buffer.chars.len() == 2);
    }

    #[test]
    fn a_keyword_is_found_after_other_text() {
        let mut m = matcher(&["sig"], &immediate());
        let (_, found) = type_text(&mut m, "hello sig").unwrap();
        assert_eq!(found.delete, 3);
    }

    #[test]
    fn a_word_like_keyword_does_not_fire_inside_a_word() {
        let mut m = matcher(&["sig"], &immediate());
        assert_eq!(type_text(&mut m, "assig"), None);
        assert_eq!(type_text(&mut m, "design"), None);
        // After a non-letter it does.
        let mut m = matcher(&["sig"], &immediate());
        assert!(type_text(&mut m, "(sig").is_some());
        let mut m = matcher(&["sig"], &immediate());
        assert!(type_text(&mut m, "1,sig").is_some());
    }

    #[test]
    fn a_punctuation_trigger_fires_anywhere() {
        let mut m = matcher(&["sig"], &settings(";", ExpandOn::Immediate, true));
        let (_, found) = type_text(&mut m, "assign;sig").unwrap();
        assert_eq!(found.delete, 4, "the prefix is deleted with the keyword");
        // Without the prefix nothing happens.
        let mut m = matcher(&["sig"], &settings(";", ExpandOn::Immediate, true));
        assert_eq!(type_text(&mut m, "sig"), None);
    }

    #[test]
    fn a_letter_prefix_is_still_a_word_start() {
        let mut m = matcher(&["sig"], &settings("x", ExpandOn::Immediate, true));
        assert!(type_text(&mut m, "xsig").is_some());
        let mut m = matcher(&["sig"], &settings("x", ExpandOn::Immediate, true));
        assert_eq!(type_text(&mut m, "boxsig"), None);
    }

    #[test]
    fn case_matters_unless_told_otherwise() {
        let mut m = matcher(&["sig"], &immediate());
        assert_eq!(type_text(&mut m, "SIG"), None);
        let mut m = matcher(&["sig"], &settings("", ExpandOn::Immediate, false));
        assert!(type_text(&mut m, "SIG").is_some());
        let mut m = matcher(&["Sig"], &settings("", ExpandOn::Immediate, false));
        assert!(type_text(&mut m, "sIG").is_some());
    }

    #[test]
    fn the_longest_keyword_wins() {
        let mut m = matcher(&[";a", ";ba"], &immediate());
        let (_, found) = type_text(&mut m, ";ba").unwrap();
        assert_eq!(found.snippet, 1);
        assert_eq!(found.delete, 3);
    }

    #[test]
    fn a_keyword_that_is_the_start_of_another_fires_first_in_immediate_mode() {
        // The documented caveat of immediate mode.
        let mut m = matcher(&[";a", ";ab"], &immediate());
        let (at, found) = type_text(&mut m, ";ab").unwrap();
        assert_eq!((at, found.snippet), (2, 0));
        // `delimiter` mode is how both stay reachable.
        let mut m = matcher(&[";a", ";ab"], &delimited());
        let (_, found) = type_text(&mut m, ";ab ").unwrap();
        assert_eq!(found.snippet, 1);
    }

    #[test]
    fn delimiter_mode_waits_for_a_delimiter_and_keeps_it() {
        let mut m = matcher(&["sig"], &delimited());
        assert_eq!(type_text(&mut m, "sig"), None);
        let found = m.feed(KeyEvent::Char(' ')).unwrap();
        assert_eq!(found.delete, 4, "the keyword and the space");
        assert_eq!(found.delimiter, Some(' '));

        for delimiter in ['.', ',', '!', ')', '-'] {
            let mut m = matcher(&["sig"], &delimited());
            type_text(&mut m, "sig");
            let found = m.feed(KeyEvent::Char(delimiter)).unwrap();
            assert_eq!(found.delimiter, Some(delimiter));
        }
    }

    #[test]
    fn delimiter_mode_does_not_fire_without_the_keyword() {
        let mut m = matcher(&["sig"], &delimited());
        assert_eq!(type_text(&mut m, "signal "), None);
        assert_eq!(type_text(&mut m, "assig "), None);
        assert_eq!(type_text(&mut m, "sig "), Some((4, found_for(0, 4, ' '))));
    }

    fn found_for(snippet: usize, delete: usize, delimiter: char) -> Match {
        Match {
            snippet,
            delete,
            delimiter: Some(delimiter),
        }
    }

    #[test]
    fn delimiter_mode_with_a_prefix() {
        let mut m = matcher(&["sig"], &settings(";", ExpandOn::Delimiter, true));
        let (at, found) = type_text(&mut m, "x ;sig ").unwrap();
        assert_eq!(at, 7);
        assert_eq!(found, found_for(0, 5, ' '));
    }

    #[test]
    fn a_delimiter_that_fired_nothing_is_a_word_boundary() {
        let mut m = matcher(&["sig"], &delimited());
        // "a sig " : the space before "sig" was kept, so "sig" starts a word.
        assert!(type_text(&mut m, "a sig ").is_some());
    }

    #[test]
    fn backspace_takes_back_the_last_character() {
        let mut m = matcher(&["sig"], &immediate());
        type_text(&mut m, "sib");
        m.feed(KeyEvent::Backspace);
        assert!(m.feed(KeyEvent::Char('g')).is_some());
    }

    #[test]
    fn backspace_on_an_empty_buffer_is_harmless() {
        let mut m = matcher(&["sig"], &immediate());
        m.feed(KeyEvent::Backspace);
        m.feed(KeyEvent::Backspace);
        assert!(type_text(&mut m, "sig").is_some());
    }

    #[test]
    fn a_reset_forgets_what_was_typed() {
        let mut m = matcher(&["sig"], &immediate());
        type_text(&mut m, "si");
        m.feed(KeyEvent::Reset);
        assert_eq!(m.feed(KeyEvent::Char('g')), None);
        // A new word works as usual.
        assert!(type_text(&mut m, " sig").is_some());
    }

    #[test]
    fn the_buffer_keeps_only_the_latest_characters() {
        let mut m = matcher(&["sig"], &immediate());
        for _ in 0..(BUFFER_CHARS * 3) {
            m.feed(KeyEvent::Char('x'));
        }
        assert_eq!(m.buffer.chars.len(), BUFFER_CHARS);
        // Still works at the end of a full buffer, and the boundary is seen.
        m.feed(KeyEvent::Char(' '));
        assert!(type_text(&mut m, "sig").is_some());
    }

    #[test]
    fn a_wipe_leaves_nothing_behind() {
        let mut m = matcher(&["sig"], &immediate());
        type_text(&mut m, "secret");
        m.reset();
        assert!(m.buffer.chars.is_empty());
        // A keyword started before the wipe cannot be finished after it.
        let mut m = matcher(&["secret"], &immediate());
        type_text(&mut m, "secre");
        m.reset();
        assert_eq!(m.feed(KeyEvent::Char('t')), None);
    }

    #[test]
    fn backspace_counts_are_in_typed_characters() {
        // Multi-byte and astral characters are one Backspace each.
        let keywords = ["日本語", "😀ok", "café", "a\u{301}b"];
        for keyword in keywords {
            let mut m = matcher(&[keyword], &settings(";", ExpandOn::Immediate, true));
            let typed = format!(";{keyword}");
            let (_, found) = type_text(&mut m, &typed).unwrap();
            assert_eq!(found.delete, typed.chars().count(), "{keyword}");
            assert_ne!(found.delete, typed.len(), "bytes are not characters");
        }
        let mut m = matcher(&["😀ok"], &settings("", ExpandOn::Delimiter, true));
        let (_, found) = type_text(&mut m, "😀ok ").unwrap();
        assert_eq!(found.delete, 4, "2 + 1 characters and the delimiter");
    }

    #[test]
    fn case_folding_does_not_change_the_count() {
        // 'İ' lowercases to two characters; one key press is still one deletion.
        let mut m = matcher(&["İx"], &settings("", ExpandOn::Immediate, false));
        let (_, found) = type_text(&mut m, "İx").unwrap();
        assert_eq!(found.delete, 2);
    }

    #[test]
    fn unusable_keywords_are_skipped() {
        let snippets = vec![
            snippet("none", None, "x"),
            snippet("blank", Some(""), "x"),
            snippet("newline", Some("a\nb"), "x"),
            snippet("long", Some(&"k".repeat(BUFFER_CHARS)), "x"),
            snippet("ok", Some("ok"), "x"),
            snippet("again", Some("ok"), "y"),
            snippet("again, loudly", Some("OK"), "z"),
        ];
        let m = Matcher::new(&snippets, &settings("", ExpandOn::Immediate, false));
        assert_eq!(m.triggers.len(), 1);
        assert_eq!(m.triggers[0].snippet, 4);
        let m = Matcher::new(&snippets, &immediate());
        assert_eq!(
            m.triggers.len(),
            2,
            "case-sensitive: \"OK\" is another keyword"
        );
        assert!(Matcher::new(&[], &immediate()).is_empty());
    }

    #[test]
    fn debug_output_shows_no_typed_text() {
        let mut m = matcher(&["sig"], &immediate());
        type_text(&mut m, "hunter2");
        let shown = format!("{m:?}");
        assert!(!shown.contains("hunter"), "{shown}");
    }

    #[test]
    fn app_rules() {
        let mut config = SnippetsConfig {
            ignore_apps: vec!["KeePassXC".into()],
            ..SnippetsConfig::default()
        };
        let target = |app: Option<&str>| TypingTarget {
            app: app.map(ForegroundApp::new),
            ..TypingTarget::default()
        };
        assert!(allowed(&target(Some("Notepad")), &config));
        // An app that cannot be identified cannot be checked against the rules.
        assert!(!allowed(&target(None), &config));
        assert!(!allowed(&target(Some("keepassxc")), &config));
        assert!(!allowed(&target(Some("WindowsTerminal")), &config));
        assert!(!allowed(&target(Some("gnome-terminal-server")), &config));
        config.expand_in_terminals = true;
        assert!(allowed(&target(Some("WindowsTerminal")), &config));
        assert!(!allowed(&target(Some("KeePassXC")), &config));

        let own = TypingTarget {
            own_window: true,
            ..TypingTarget::default()
        };
        let private = TypingTarget {
            private: true,
            ..TypingTarget::default()
        };
        assert!(!allowed(&own, &config));
        assert!(!allowed(&private, &config));
    }

    // -- The service, against a fake platform -------------------------------

    /// A platform with a keyboard we can type on and a text field we can read.
    struct Fake {
        sink: Mutex<Option<KeySink>>,
        target: Mutex<TypingTarget>,
        replaced: Mutex<Vec<(usize, String)>>,
        support: KeyListenerSupport,
        clipboard: Option<String>,
        latency: Duration,
    }

    impl Fake {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                latency: Duration::ZERO,
                sink: Mutex::new(None),
                target: Mutex::new(TypingTarget {
                    app: Some(ForegroundApp::new("Notepad")),
                    ..TypingTarget::default()
                }),
                replaced: Mutex::new(Vec::new()),
                support: KeyListenerSupport::Available,
                clipboard: Some("from the clipboard".to_owned()),
            })
        }

        fn type_text(&self, text: &str) {
            for c in text.chars() {
                self.send(KeyEvent::Char(c));
            }
        }

        fn send(&self, event: KeyEvent) {
            if let Some(sink) = self.sink.lock().unwrap().as_ref() {
                sink(event);
            }
        }

        fn listening(&self) -> bool {
            self.sink.lock().unwrap().is_some()
        }

        /// Waits for the worker to have handled everything sent so far, by
        /// sending a marker it replaces last.
        fn replaced(&self) -> Vec<(usize, String)> {
            let deadline = Instant::now() + Duration::from_millis(400);
            let mut last = self.replaced.lock().unwrap().len();
            // Give the worker time to drain; stop early once it has gone quiet.
            while Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(40));
                let now = self.replaced.lock().unwrap().len();
                if now == last && now > 0 {
                    break;
                }
                last = now;
            }
            self.replaced.lock().unwrap().clone()
        }

        fn settle(&self) {
            std::thread::sleep(Duration::from_millis(150));
        }
    }

    impl PlatformProvider for Fake {
        fn list_applications(&self) -> Result<Vec<AppEntry>> {
            Ok(Vec::new())
        }
        fn launch(&self, _: &LaunchTarget) -> Result<()> {
            Ok(())
        }
        fn load_icon(&self, _: &IconSource, _: u32) -> Result<IconData> {
            Err(PlatformError::Unsupported("icons"))
        }
        fn open_path(&self, _: &Path) -> Result<()> {
            Ok(())
        }
        fn clipboard_text(&self) -> Result<Option<String>> {
            Ok(self.clipboard.clone())
        }
        fn key_listener_support(&self) -> KeyListenerSupport {
            self.support.clone()
        }
        fn start_key_listener(&self, sink: KeySink) -> Result<KeyListener> {
            *self.sink.lock().unwrap() = Some(sink);
            Ok(KeyListener::new(|| {}))
        }
        fn typing_target(&self) -> TypingTarget {
            self.target.lock().unwrap().clone()
        }
        fn replace_typed_text(
            &self,
            delete: usize,
            text: &str,
            still_current: &dyn Fn() -> bool,
        ) -> Result<bool> {
            // Replacing takes a moment on a real system; typing can go on in it.
            std::thread::sleep(self.latency);
            if !still_current() {
                return Ok(false);
            }
            self.replaced
                .lock()
                .unwrap()
                .push((delete, text.to_owned()));
            Ok(true)
        }
    }

    fn config(snippets: Vec<Snippet>, settings: SnippetsConfig) -> Config {
        Config {
            snippet: snippets,
            snippets: settings,
            ..Config::default()
        }
    }

    fn running(fake: &Arc<Fake>, config: &Config) -> SnippetExpansion {
        let (service, state) = SnippetExpansion::start(config, fake.clone());
        assert!(matches!(state, ExpansionState::Running { .. }), "{state:?}");
        service.expect("running")
    }

    #[test]
    fn nothing_is_watched_unless_turned_on() {
        let fake = Fake::new();
        let cfg = config(
            vec![snippet("Sig", Some("sig"), "Regards")],
            SnippetsConfig::default(),
        );
        let (service, state) = SnippetExpansion::start(&cfg, fake.clone());
        assert!(service.is_none());
        assert_eq!(state, ExpansionState::Off);
        assert!(!fake.listening(), "no key listener was even started");
    }

    #[test]
    fn nothing_is_watched_without_keywords() {
        let fake = Fake::new();
        let cfg = config(
            vec![snippet("Sig", None, "Regards")],
            settings("", ExpandOn::Immediate, true),
        );
        let (service, state) = SnippetExpansion::start(&cfg, fake.clone());
        assert!(service.is_none());
        assert_eq!(state, ExpansionState::NoKeywords);
        assert!(state.problem().is_some());
        assert!(!fake.listening());
    }

    #[test]
    fn a_missing_permission_or_platform_is_reported_not_watched() {
        for (support, expect_permission) in [
            (KeyListenerSupport::NeedsPermission("allow it".into()), true),
            (KeyListenerSupport::Unavailable("no Wayland".into()), false),
        ] {
            let fake = Arc::new(Fake {
                support,
                ..Arc::into_inner(Fake::new()).unwrap()
            });
            let cfg = config(
                vec![snippet("Sig", Some("sig"), "x")],
                settings("", ExpandOn::Immediate, true),
            );
            let (service, state) = SnippetExpansion::start(&cfg, fake.clone());
            assert!(service.is_none());
            assert_eq!(
                matches!(state, ExpansionState::NeedsPermission(_)),
                expect_permission
            );
            assert!(state.problem().is_some());
            assert!(!fake.listening());
        }
    }

    #[test]
    fn typing_a_keyword_replaces_it_with_the_text() {
        let fake = Fake::new();
        let cfg = config(
            vec![snippet("Sig", Some("sig"), "Best regards,\nNinad")],
            settings(";", ExpandOn::Immediate, true),
        );
        let _service = running(&fake, &cfg);
        fake.type_text("hello ;sig");
        assert_eq!(fake.replaced(), [(4, "Best regards,\nNinad".to_owned())]);
    }

    #[test]
    fn typing_that_goes_on_cancels_the_expansion() {
        // The keyword is complete, and before Sevak gets to press Backspace the
        // user types another character: deleting now would eat the wrong text.
        let fake = Arc::new(Fake {
            latency: Duration::from_millis(200),
            ..Arc::into_inner(Fake::new()).unwrap()
        });
        let cfg = config(vec![snippet("Sig", Some("sig"), "Regards")], immediate());
        let _service = running(&fake, &cfg);
        fake.type_text("sig");
        std::thread::sleep(Duration::from_millis(60));
        fake.type_text("x");
        fake.settle();
        std::thread::sleep(Duration::from_millis(300));
        assert!(fake.replaced.lock().unwrap().is_empty());

        // And the next keyword still works: the buffer was wiped, not wedged.
        fake.type_text(" sig");
        assert_eq!(fake.replaced(), [(3, "Regards".to_owned())]);
    }

    #[test]
    fn placeholders_are_filled_in() {
        let fake = Fake::new();
        let cfg = config(
            vec![snippet("Paste", Some("pp"), "[{clipboard}]")],
            immediate(),
        );
        let _service = running(&fake, &cfg);
        fake.type_text("pp");
        assert_eq!(fake.replaced(), [(2, "[from the clipboard]".to_owned())]);
    }

    #[test]
    fn the_delimiter_goes_back_after_the_text() {
        let fake = Fake::new();
        let cfg = config(vec![snippet("Sig", Some("sig"), "Regards")], delimited());
        let _service = running(&fake, &cfg);
        fake.type_text("sig!");
        assert_eq!(fake.replaced(), [(4, "Regards!".to_owned())]);
    }

    #[test]
    fn an_ignored_app_is_not_watched() {
        let fake = Fake::new();
        let mut s = immediate();
        s.ignore_apps = vec!["Notepad".to_owned()];
        let cfg = config(vec![snippet("Sig", Some("sig"), "Regards")], s);
        let _service = running(&fake, &cfg);
        fake.type_text("sig");
        fake.settle();
        assert!(fake.replaced.lock().unwrap().is_empty());
    }

    #[test]
    fn terminals_sevaks_own_windows_and_password_boxes_are_not_watched() {
        for target in [
            TypingTarget {
                app: Some(ForegroundApp::new("WindowsTerminal")),
                ..TypingTarget::default()
            },
            TypingTarget {
                own_window: true,
                ..TypingTarget::default()
            },
            TypingTarget {
                private: true,
                ..TypingTarget::default()
            },
        ] {
            let fake = Fake::new();
            *fake.target.lock().unwrap() = target;
            let cfg = config(vec![snippet("Sig", Some("sig"), "Regards")], immediate());
            let _service = running(&fake, &cfg);
            fake.type_text("sig");
            fake.settle();
            assert!(fake.replaced.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn an_app_that_cannot_be_identified_is_not_watched_or_expanded() {
        let fake = Fake::new();
        *fake.target.lock().unwrap() = TypingTarget::default();
        let cfg = config(vec![snippet("Sig", Some("sig"), "Regards")], immediate());
        let _service = running(&fake, &cfg);
        fake.type_text("sig");
        fake.settle();
        assert!(fake.replaced.lock().unwrap().is_empty());

        // Once the app is known again, typing works as usual.
        *fake.target.lock().unwrap() = TypingTarget {
            app: Some(ForegroundApp::new("Notepad")),
            ..TypingTarget::default()
        };
        fake.send(KeyEvent::Reset);
        fake.type_text("sig");
        fake.settle();
        assert_eq!(fake.replaced.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_keyword_started_elsewhere_does_not_finish_here() {
        let fake = Fake::new();
        let cfg = config(vec![snippet("Sig", Some("sig"), "Regards")], immediate());
        let _service = running(&fake, &cfg);
        // "si" typed in a private box, then the focus moves to a normal one and
        // the user types "g": the first two characters must not have been kept.
        *fake.target.lock().unwrap() = TypingTarget {
            private: true,
            ..TypingTarget::default()
        };
        fake.type_text("si");
        fake.settle();
        *fake.target.lock().unwrap() = TypingTarget {
            app: Some(ForegroundApp::new("Notepad")),
            ..TypingTarget::default()
        };
        fake.send(KeyEvent::Reset);
        fake.type_text("g");
        fake.settle();
        assert!(fake.replaced.lock().unwrap().is_empty());
    }

    #[test]
    fn a_reset_event_forgets_a_half_typed_keyword() {
        let fake = Fake::new();
        let cfg = config(vec![snippet("Sig", Some("sig"), "Regards")], immediate());
        let _service = running(&fake, &cfg);
        fake.type_text("si");
        fake.send(KeyEvent::Reset);
        fake.type_text("g");
        fake.settle();
        assert!(fake.replaced.lock().unwrap().is_empty());
    }

    #[test]
    fn dropping_the_service_stops_the_worker() {
        let fake = Fake::new();
        let cfg = config(vec![snippet("Sig", Some("sig"), "Regards")], immediate());
        let service = running(&fake, &cfg);
        drop(service);
        // The worker is gone, so typing now does nothing.
        fake.type_text("sig");
        fake.settle();
        assert!(fake.replaced.lock().unwrap().is_empty());
    }
}
