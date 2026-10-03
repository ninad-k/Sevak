//! Media controls: play/pause, next, previous, stop, and a "now playing" row.
//!
//! Typing `play`, `pause`, `next` or `music` finds the buttons the way typing
//! `lock` finds the system commands, and `play ` (the keyword, then a space)
//! lists them all together with the track that is playing: title, artist and
//! the app. Enter on that row plays or pauses; Shift and Alt skip forward and
//! back.
//!
//! What is offered depends on the machine (the platform provider reports which
//! buttons can work: Linux needs `playerctl`). The track is read from the
//! system's media player on request, never on the typing path: a query only
//! reads the last answer from a [`Cache`] and starts a background refresh when
//! it is stale, and the notifier makes the shell show the fresh track a moment
//! later. Nothing is stored or sent anywhere.

use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use sevak_core::config::MediaConfig;
use sevak_core::model::score;
use sevak_core::{
    Action, FuzzyQuery, IconSource, Modifier, Plugin, PluginError, PluginResult, ResultItem,
    ResultsNotifier,
};
use sevak_platform::{MediaCommand, NowPlaying, PlatformProvider};

use crate::apps::name_bonus;
use crate::live::Cache;

const PLUGIN_ID: &str = "media";
/// Prefix of the [`Action::Custom`] payload.
const PAYLOAD_PREFIX: &str = "media:";
/// Queries shorter than this match no button.
const MIN_QUERY_CHARS: usize = 2;
/// Aliases are weaker evidence than the title.
const ALIAS_WEIGHT: f64 = 0.9;
/// Breaks ties in favour of shorter titles.
const LENGTH_PENALTY: f64 = 0.01;
/// How long the track is trusted before a query starts a refresh.
const NOW_PLAYING_TTL: Duration = Duration::from_secs(2);
/// Words that ask for the track that is playing. A query must be the start of
/// one of them (at least [`MIN_TRACK_QUERY_CHARS`] letters), so `play` and
/// `pause` find the buttons only.
const NOW_PLAYING_WORDS: [&str; 3] = ["now playing", "music", "song"];
const MIN_TRACK_QUERY_CHARS: usize = 3;
/// Ranks the track row above the buttons that came up with it.
const TRACK_SCORE: f64 = 1000.0;

/// Whether `query` (lowercase) asks for the track that is playing.
fn asks_for_track(query: &str) -> bool {
    query.chars().count() >= MIN_TRACK_QUERY_CHARS
        && NOW_PLAYING_WORDS.iter().any(|word| word.starts_with(query))
}

struct Button {
    command: MediaCommand,
    title: &'static str,
    subtitle: &'static str,
    aliases: &'static [&'static str],
    icon: &'static str,
}

const BUTTONS: [Button; 4] = [
    Button {
        command: MediaCommand::PlayPause,
        title: "Play / Pause",
        subtitle: "Start or pause the music or video that is playing",
        aliases: &["play", "pause", "play pause", "resume", "toggle playback"],
        icon: "play",
    },
    Button {
        command: MediaCommand::Next,
        title: "Next track",
        subtitle: "Skip to the next song or video",
        aliases: &["next", "skip", "next song", "skip track", "forward"],
        icon: "next",
    },
    Button {
        command: MediaCommand::Previous,
        title: "Previous track",
        subtitle: "Go back to the previous song or video",
        aliases: &["previous", "prev", "back", "previous song", "last track"],
        icon: "previous",
    },
    Button {
        command: MediaCommand::Stop,
        title: "Stop playback",
        subtitle: "Stop the music or video that is playing",
        aliases: &["stop", "stop music", "stop playing"],
        icon: "stop",
    },
];

fn button(command: MediaCommand) -> &'static Button {
    BUTTONS
        .iter()
        .find(|button| button.command == command)
        .expect("every command has a button")
}

fn payload(command: MediaCommand) -> Action {
    Action::Custom {
        payload: format!("{PAYLOAD_PREFIX}{}", command.key()),
    }
}

/// The media buttons and the playing track.
pub struct MediaPlugin {
    platform: Arc<dyn PlatformProvider>,
    config: MediaConfig,
    keyword: Option<String>,
    /// Rebuilt by [`Plugin::refresh`]; queries read a snapshot.
    controls: RwLock<Arc<Vec<MediaCommand>>>,
    /// Whether the platform can say what is playing (also from `refresh`).
    can_query: RwLock<bool>,
    now_playing: Arc<Cache<Option<NowPlaying>>>,
    notifier: OnceLock<ResultsNotifier>,
}

impl MediaPlugin {
    pub fn new(config: MediaConfig, platform: Arc<dyn PlatformProvider>) -> Self {
        let keyword = Some(config.keyword.trim().to_owned()).filter(|k| !k.is_empty());
        Self {
            platform,
            config,
            keyword,
            controls: RwLock::new(Arc::new(Vec::new())),
            can_query: RwLock::new(false),
            now_playing: Cache::new(),
            notifier: OnceLock::new(),
        }
    }

    fn controls(&self) -> Arc<Vec<MediaCommand>> {
        self.controls
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn shows_track(&self) -> bool {
        self.config.now_playing
            && *self
                .can_query
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn button_row(&self, command: MediaCommand, score: f64) -> ResultItem {
        let button = button(command);
        ResultItem::new(PLUGIN_ID, command.key(), button.title, payload(command))
            .with_subtitle(button.subtitle)
            .with_icon(IconSource::builtin(button.icon))
            .with_score(score)
    }

    /// The row for the track that is playing. Enter plays or pauses; Shift and
    /// Alt skip, when those buttons exist.
    fn track_row(&self, controls: &[MediaCommand], track: &NowPlaying, score: f64) -> ResultItem {
        let title = if track.artist.is_empty() {
            track.title.clone()
        } else {
            format!("{} \u{2014} {}", track.title, track.artist)
        };
        let state = if track.playing { "Playing" } else { "Paused" };
        let subtitle = if track.app.is_empty() {
            state.to_owned()
        } else {
            format!("{state} in {}", track.app)
        };
        let mut item = ResultItem::new(
            PLUGIN_ID,
            "now_playing",
            title,
            payload(MediaCommand::PlayPause),
        )
        .with_subtitle(subtitle)
        .with_icon(IconSource::builtin("note"))
        .with_score(score);
        if controls.contains(&MediaCommand::Next) {
            item = item.with_secondary(
                "Next track",
                Some(Modifier::Shift),
                payload(MediaCommand::Next),
            );
        }
        if controls.contains(&MediaCommand::Previous) {
            item = item.with_secondary(
                "Previous track",
                Some(Modifier::Alt),
                payload(MediaCommand::Previous),
            );
        }
        item
    }

    /// Starts a background read of the track unless the last one is recent.
    fn refresh_track(&self) {
        let platform = self.platform.clone();
        self.now_playing.refresh_if_stale(
            NOW_PLAYING_TTL,
            move || platform.now_playing().ok().flatten(),
            self.notifier.get().cloned(),
            PLUGIN_ID,
        );
    }
}

impl Plugin for MediaPlugin {
    fn id(&self) -> &str {
        PLUGIN_ID
    }

    fn name(&self) -> &str {
        "Media controls"
    }

    fn description(&self) -> &str {
        "Play/pause, next, previous and stop, and the track that is playing; type `play`."
    }

    fn keyword(&self) -> Option<&str> {
        self.keyword.as_deref()
    }

    fn global(&self) -> bool {
        self.config.global || self.keyword.is_none()
    }

    fn keyword_row(&self) -> Option<ResultItem> {
        let keyword = self.keyword.as_deref()?;
        if self.controls().is_empty() {
            return None;
        }
        Some(
            ResultItem::new(
                PLUGIN_ID,
                "home",
                "Media controls",
                payload(MediaCommand::PlayPause),
            )
            .with_subtitle("Press Tab to list the buttons and the track that is playing")
            .with_icon(IconSource::builtin("play"))
            .with_autocomplete(format!("{keyword} ")),
        )
    }

    fn query(&self, input: &str) -> Vec<ResultItem> {
        let input = input.trim();
        let controls = self.controls();
        if controls.is_empty() {
            return Vec::new();
        }
        let shows_track = self.shows_track();

        if input.is_empty() {
            // The keyword alone: the track, then every button.
            let mut rows = Vec::new();
            if shows_track {
                self.refresh_track();
                if let Some(track) = self.now_playing.get().as_ref() {
                    rows.push(self.track_row(&controls, track, score::KEYWORD));
                }
            }
            rows.extend(
                controls.iter().enumerate().map(|(i, command)| {
                    self.button_row(*command, score::KEYWORD - 10.0 - i as f64)
                }),
            );
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
        let mut rows: Vec<ResultItem> = Vec::new();

        for command in controls.iter() {
            let button = button(*command);
            let title_lower = button.title.to_lowercase();
            let mut best = query.score(button.title).map(|s| {
                f64::from(s) + name_bonus(&title_lower, &query_lower)
                    - title_lower.len() as f64 * LENGTH_PENALTY
            });
            for alias in button.aliases {
                if let Some(s) = query.score(alias) {
                    let s = f64::from(s) * ALIAS_WEIGHT + name_bonus(alias, &query_lower);
                    best = Some(best.map_or(s, |b| b.max(s)));
                }
            }
            if let Some(best) = best {
                rows.push(self.button_row(*command, best));
            }
        }

        // The track answers to "music", "now playing" and the like.
        if shows_track && asks_for_track(&query_lower) {
            self.refresh_track();
            if let Some(track) = self.now_playing.get().as_ref() {
                rows.push(self.track_row(&controls, track, TRACK_SCORE));
            }
        }
        rows.sort_by(|a, b| b.score.total_cmp(&a.score));
        rows
    }

    /// `media:<key>` (`media:play_pause`, `media:next`, ...) for a button this
    /// machine offers. The playing-track row is dynamic and not resolvable.
    fn resolve(&self, id: &str) -> Option<ResultItem> {
        let command = MediaCommand::from_key(id.strip_prefix("media:")?)?;
        self.controls()
            .contains(&command)
            .then(|| self.button_row(command, 0.0))
    }

    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        let Action::Custom { payload } = &item.action else {
            return Err(PluginError::Unsupported(item.id.clone()));
        };
        let command = payload
            .strip_prefix(PAYLOAD_PREFIX)
            .and_then(MediaCommand::from_key)
            .ok_or_else(|| PluginError::Unsupported(item.id.clone()))?;
        if !self.controls().contains(&command) {
            return Err(PluginError::Message(
                "That media button is not available on this system".to_owned(),
            ));
        }
        self.platform
            .media_control(command)
            .map_err(PluginError::other)
    }

    fn refresh(&self) -> PluginResult<()> {
        *self
            .controls
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) =
            Arc::new(self.platform.supported_media_commands());
        let can_query = self.config.now_playing && self.platform.now_playing_available();
        *self
            .can_query
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = can_query;
        if can_query {
            // On a background thread already: warm the cache so the first
            // `play ` shows the track at once.
            self.now_playing
                .store(self.platform.now_playing().ok().flatten());
        }
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

    fn platform(commands: &[MediaCommand], track: Option<NowPlaying>) -> Arc<MockPlatform> {
        let platform = MockPlatform::empty();
        *platform.media_commands.lock().unwrap() = commands.to_vec();
        *platform.now_playing_supported.lock().unwrap() = true;
        *platform.playing.lock().unwrap() = track;
        platform
    }

    fn song() -> NowPlaying {
        NowPlaying {
            title: "Blue in Green".into(),
            artist: "Miles Davis".into(),
            app: "Spotify".into(),
            playing: true,
        }
    }

    fn plugin(config: MediaConfig, platform: &Arc<MockPlatform>) -> MediaPlugin {
        let plugin = MediaPlugin::new(config, platform.clone());
        plugin.refresh().unwrap();
        plugin
    }

    fn all() -> Arc<MockPlatform> {
        platform(&MediaCommand::ALL, Some(song()))
    }

    fn titles(plugin: &MediaPlugin, input: &str) -> Vec<String> {
        plugin.query(input).into_iter().map(|i| i.title).collect()
    }

    fn first(plugin: &MediaPlugin, input: &str) -> ResultItem {
        plugin
            .query(input)
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("no result for {input:?}"))
    }

    #[test]
    fn metadata_and_keyword_follow_the_config() {
        let plugin = MediaPlugin::new(MediaConfig::default(), all());
        assert_eq!(plugin.id(), "media");
        assert_eq!(plugin.keyword(), Some("play"));
        assert!(plugin.global());
        let config = MediaConfig {
            keyword: " ".into(),
            global: false,
            now_playing: true,
        };
        let plugin = MediaPlugin::new(config, all());
        assert_eq!(plugin.keyword(), None);
        assert!(plugin.global());
    }

    #[test]
    fn nothing_is_offered_before_the_first_refresh_or_without_controls() {
        let plugin = MediaPlugin::new(MediaConfig::default(), all());
        assert!(plugin.query("pause").is_empty());
        assert!(plugin.query("").is_empty());
        assert!(plugin.keyword_row().is_none());
        // Linux without playerctl: no buttons, nothing to show.
        let bare = platform(&[], None);
        *bare.now_playing_supported.lock().unwrap() = false;
        let plugin = plugin_for(&bare);
        assert!(plugin.query("pause").is_empty());
        assert!(plugin.query("").is_empty());
        assert!(plugin.resolve("media:play_pause").is_none());
    }

    fn plugin_for(platform: &Arc<MockPlatform>) -> MediaPlugin {
        plugin(MediaConfig::default(), platform)
    }

    #[test]
    fn finds_buttons_by_title_and_alias() {
        let plugin = plugin_for(&all());
        assert_eq!(first(&plugin, "pause").title, "Play / Pause");
        assert_eq!(first(&plugin, "play").title, "Play / Pause");
        assert_eq!(first(&plugin, "resume").title, "Play / Pause");
        assert_eq!(first(&plugin, "next").title, "Next track");
        assert_eq!(first(&plugin, "skip").title, "Next track");
        assert_eq!(first(&plugin, "previous").title, "Previous track");
        assert_eq!(first(&plugin, "prev").title, "Previous track");
        assert_eq!(first(&plugin, "stop music").title, "Stop playback");
    }

    #[test]
    fn short_and_unrelated_queries_match_nothing() {
        let plugin = plugin_for(&all());
        assert!(plugin.query("p").is_empty());
        assert!(plugin.query("firefox").is_empty());
    }

    #[test]
    fn only_supported_buttons_are_offered() {
        let platform = platform(&[MediaCommand::PlayPause, MediaCommand::Next], None);
        let plugin = plugin_for(&platform);
        assert_eq!(first(&plugin, "pause").title, "Play / Pause");
        assert_eq!(first(&plugin, "next").title, "Next track");
        assert!(titles(&plugin, "previous")
            .iter()
            .all(|t| t != "Previous track"));
        assert!(titles(&plugin, "stop").iter().all(|t| t != "Stop playback"));
        assert!(plugin.resolve("media:stop").is_none());
    }

    #[test]
    fn the_keyword_alone_lists_the_track_then_every_button() {
        let plugin = plugin_for(&all());
        let rows = plugin.query("");
        assert_eq!(
            rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
            [
                "Blue in Green \u{2014} Miles Davis",
                "Play / Pause",
                "Next track",
                "Previous track",
                "Stop playback"
            ]
        );
        assert!(rows.windows(2).all(|w| w[0].score > w[1].score));
        let track = &rows[0];
        assert_eq!(track.id, "media:now_playing");
        assert_eq!(track.subtitle, "Playing in Spotify");
        assert_eq!(track.action, payload(MediaCommand::PlayPause));
        // Shift skips forward, Alt goes back.
        assert_eq!(track.secondary.len(), 2);
        assert_eq!(track.secondary[0].modifier, Some(Modifier::Shift));
        assert_eq!(track.secondary[0].action, payload(MediaCommand::Next));
        assert_eq!(track.secondary[1].modifier, Some(Modifier::Alt));
        assert_eq!(track.secondary[1].action, payload(MediaCommand::Previous));
    }

    #[test]
    fn the_track_row_reads_naturally() {
        let plugin = plugin_for(&all());
        let controls = [MediaCommand::PlayPause];
        let paused = NowPlaying {
            title: "Podcast".into(),
            artist: String::new(),
            app: String::new(),
            playing: false,
        };
        let row = plugin.track_row(&controls, &paused, 1.0);
        assert_eq!(row.title, "Podcast");
        assert_eq!(row.subtitle, "Paused");
        // No skip actions where the buttons do not exist.
        assert!(row.secondary.is_empty());
    }

    #[test]
    fn music_and_now_playing_find_the_track_first() {
        let plugin = plugin_for(&all());
        assert_eq!(first(&plugin, "music").id, "media:now_playing");
        assert_eq!(first(&plugin, "now playing").id, "media:now_playing");
        // A button query does not drag the track in.
        assert!(plugin
            .query("pause")
            .iter()
            .all(|row| row.id != "media:now_playing"));
    }

    #[test]
    fn only_track_words_ask_for_the_track() {
        for yes in ["now", "now p", "now playing", "mus", "music", "song"] {
            assert!(asks_for_track(yes), "{yes}");
        }
        for no in [
            "",
            "no",
            "play",
            "pause",
            "next",
            "musical",
            "tracks",
            "now playing x",
        ] {
            assert!(!asks_for_track(no), "{no}");
        }
    }

    #[test]
    fn nothing_playing_means_no_track_row() {
        let platform = platform(&MediaCommand::ALL, None);
        let plugin = plugin_for(&platform);
        assert!(plugin.query("").iter().all(|r| r.id != "media:now_playing"));
        assert!(plugin
            .query("music")
            .iter()
            .all(|r| r.id != "media:now_playing"));
    }

    #[test]
    fn the_track_can_be_switched_off() {
        let platform = all();
        let config = MediaConfig {
            now_playing: false,
            ..MediaConfig::default()
        };
        let plugin = plugin(config, &platform);
        assert!(plugin.query("").iter().all(|r| r.id != "media:now_playing"));
        // The platform is never asked.
        assert_eq!(*platform.now_playing_calls.lock().unwrap(), 0);
    }

    #[test]
    fn results_are_stable_custom_actions_with_icons() {
        let plugin = plugin_for(&all());
        let item = first(&plugin, "next");
        assert_eq!(item.id, "media:next");
        assert_eq!(item.plugin_id, "media");
        assert_eq!(
            item.action,
            Action::Custom {
                payload: "media:next".into()
            }
        );
        assert_eq!(item.icon, Some(IconSource::builtin("next")));
        assert!(item.score > 0.0);
    }

    #[test]
    fn keyword_row_completes_the_bare_keyword() {
        let plugin = plugin_for(&all());
        let row = plugin.keyword_row().unwrap();
        assert_eq!(row.autocomplete.as_deref(), Some("play "));
        assert_eq!(row.plugin_id, "media");
    }

    #[test]
    fn typing_never_waits_for_the_player() {
        let platform = all();
        let plugin = MediaPlugin::new(MediaConfig::default(), platform.clone());
        // `refresh` is the background warm-up: it asks the player once.
        plugin.refresh().unwrap();
        assert_eq!(*platform.now_playing_calls.lock().unwrap(), 1);
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        plugin
            .notifier
            .set(Arc::new(move |id| {
                let _ = tx.lock().unwrap().send(id.to_owned());
            }))
            .ok();

        // The cached track is shown at once, and a track change is announced
        // after a background read.
        *platform.playing.lock().unwrap() = Some(NowPlaying {
            title: "Next Song".into(),
            artist: "Someone".into(),
            app: "Spotify".into(),
            playing: true,
        });
        std::thread::sleep(NOW_PLAYING_TTL + Duration::from_millis(100));
        assert_eq!(
            plugin.query("")[0].title,
            "Blue in Green \u{2014} Miles Davis"
        );
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), "media");
        assert_eq!(plugin.query("")[0].title, "Next Song \u{2014} Someone");

        // Button queries never ask the player.
        let calls = *platform.now_playing_calls.lock().unwrap();
        plugin.query("pause");
        plugin.query("next");
        assert_eq!(*platform.now_playing_calls.lock().unwrap(), calls);
    }

    #[test]
    fn execute_presses_the_button_through_the_platform() {
        let platform = all();
        let plugin = plugin_for(&platform);
        plugin.execute(&first(&plugin, "pause")).unwrap();
        plugin.execute(&first(&plugin, "next")).unwrap();
        // The track row plays or pauses, and its secondary actions skip.
        let track = first(&plugin, "");
        plugin.execute(&track).unwrap();
        plugin
            .execute(&track.secondary_as_primary(1).unwrap())
            .unwrap();
        assert_eq!(
            *platform.media_pressed.lock().unwrap(),
            [
                MediaCommand::PlayPause,
                MediaCommand::Next,
                MediaCommand::PlayPause,
                MediaCommand::Previous
            ]
        );
        // Media buttons never ask first.
        assert_eq!(plugin.confirmation(&track), None);
    }

    #[test]
    fn execute_rejects_foreign_malformed_and_unavailable_actions() {
        let platform = platform(&[MediaCommand::PlayPause], None);
        let plugin = plugin_for(&platform);
        let custom = |payload: &str| {
            ResultItem::new(
                "media",
                "x",
                "X",
                Action::Custom {
                    payload: payload.into(),
                },
            )
        };
        for payload in ["media:eject", "media:", "task:next", "next"] {
            assert!(
                matches!(
                    plugin.execute(&custom(payload)),
                    Err(PluginError::Unsupported(_))
                ),
                "{payload}"
            );
        }
        assert!(matches!(
            plugin.execute(&custom("media:stop")),
            Err(PluginError::Message(_))
        ));
        let copy = ResultItem::new("media", "y", "Y", Action::CopyText { text: "t".into() });
        assert!(matches!(
            plugin.execute(&copy),
            Err(PluginError::Unsupported(_))
        ));
        assert!(platform.media_pressed.lock().unwrap().is_empty());
    }

    #[test]
    fn resolve_finds_offered_buttons_by_id() {
        let platform = platform(&[MediaCommand::PlayPause, MediaCommand::Next], None);
        let plugin = plugin_for(&platform);
        let item = plugin.resolve("media:play_pause").expect("offered button");
        assert_eq!(item.id, "media:play_pause");
        assert_eq!(item.action, first(&plugin, "pause").action);
        plugin.execute(&item).unwrap();
        assert_eq!(
            *platform.media_pressed.lock().unwrap(),
            [MediaCommand::PlayPause]
        );
        for id in [
            "media:stop",
            "media:previous",
            "media:now_playing",
            "media:nope",
            "media:",
            "tasks:next",
            "system:lock",
        ] {
            assert!(plugin.resolve(id).is_none(), "{id}");
        }
    }
}
