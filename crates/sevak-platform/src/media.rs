//! Media controls: play/pause, next, previous, stop, and what is playing.
//!
//! The control is a closed vocabulary ([`MediaCommand`]) and the queries are
//! read-only. Each OS uses its own way to reach the player the user is
//! listening to:
//!
//! - Windows: the system media transport controls (the same session the volume
//!   flyout shows), with the media keys as a fallback.
//! - macOS: media key events, which reach whatever app owns the keys, and
//!   Apple events to Music and Spotify for the title and artist. (The system's
//!   own now-playing service, MediaRemote, is private API.)
//! - Linux: MPRIS through `playerctl`.
//!
//! The parsers and command tables are pure functions compiled on every OS.

#[cfg(not(windows))]
use crate::error::PlatformError;
use crate::error::Result;
#[cfg(not(windows))]
use crate::system::CommandLine;
#[cfg(not(windows))]
use crate::tasks::capture;
use crate::tasks::{find_tool, Os};

/// What a media button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaCommand {
    PlayPause,
    Next,
    Previous,
    Stop,
}

impl MediaCommand {
    pub const ALL: [Self; 4] = [Self::PlayPause, Self::Next, Self::Previous, Self::Stop];

    /// Stable identifier used in result ids and `[media] disabled`.
    pub fn key(self) -> &'static str {
        match self {
            Self::PlayPause => "play_pause",
            Self::Next => "next",
            Self::Previous => "previous",
            Self::Stop => "stop",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|command| command.key() == key)
    }
}

/// The track a player reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    /// The player's name (`Spotify`, `Music`, `firefox`).
    pub app: String,
    pub playing: bool,
}

/// What this machine's media control needs, gathered by [`MediaEnv::detect`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaEnv {
    /// `playerctl` is installed (Linux).
    pub playerctl: bool,
    /// The Music and Spotify apps are installed (macOS).
    pub mac_players: Vec<MacPlayer>,
}

impl MediaEnv {
    pub fn detect() -> Self {
        Self {
            playerctl: find_tool("playerctl").is_some(),
            mac_players: MacPlayer::ALL
                .into_iter()
                .filter(|player| std::path::Path::new(player.bundle_path()).exists())
                .collect(),
        }
    }
}

/// The commands that can work on `os`.
pub fn supported_commands(os: Os, env: &MediaEnv) -> Vec<MediaCommand> {
    match os {
        Os::Windows => MediaCommand::ALL.to_vec(),
        // Stop has no media key; it is sent to Music or Spotify by name.
        Os::Mac => MediaCommand::ALL
            .into_iter()
            .filter(|command| *command != MediaCommand::Stop || !env.mac_players.is_empty())
            .collect(),
        Os::Linux if env.playerctl => MediaCommand::ALL.to_vec(),
        Os::Linux => Vec::new(),
    }
}

/// Whether a now-playing query can work on `os`.
pub fn can_query(os: Os, env: &MediaEnv) -> bool {
    match os {
        Os::Windows => true,
        Os::Mac => !env.mac_players.is_empty(),
        Os::Linux => env.playerctl,
    }
}

// ---------------------------------------------------------------------------
// macOS
// ---------------------------------------------------------------------------

/// The players on macOS that can be asked what is playing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacPlayer {
    Music,
    Spotify,
}

impl MacPlayer {
    pub const ALL: [Self; 2] = [Self::Music, Self::Spotify];

    pub fn app_name(self) -> &'static str {
        match self {
            Self::Music => "Music",
            Self::Spotify => "Spotify",
        }
    }

    fn bundle_path(self) -> &'static str {
        match self {
            Self::Music => "/System/Applications/Music.app",
            Self::Spotify => "/Applications/Spotify.app",
        }
    }
}

/// `NX_KEYTYPE_PLAY`, `NX_KEYTYPE_NEXT` and `NX_KEYTYPE_PREVIOUS`: the media
/// key codes of `IOKit/hidsystem/ev_keymap.h`. There is none for stop.
pub fn mac_media_key(command: MediaCommand) -> Option<i64> {
    match command {
        MediaCommand::PlayPause => Some(16),
        MediaCommand::Next => Some(17),
        MediaCommand::Previous => Some(18),
        MediaCommand::Stop => None,
    }
}

/// The `data1` field of a system-defined media key event: key code, then
/// the state (`0xA` down, `0xB` up) in the second byte.
pub fn mac_key_event_data(key: i64, down: bool) -> i64 {
    (key << 16) | ((if down { 0xA } else { 0xB }) << 8)
}

/// The AppleScript lines that print `state<TAB>title<TAB>artist` of the player,
/// or nothing when it is not running or has no track. Never launches the app.
pub fn mac_now_playing_script(player: MacPlayer) -> Vec<String> {
    let app = player.app_name();
    vec![
        format!(r#"if application "{app}" is running then"#),
        format!(r#"tell application "{app}""#),
        "if player state is playing or player state is paused then".to_owned(),
        r#"return (player state as text) & tab & (name of current track) & tab & (artist of current track)"#
            .to_owned(),
        "end if".to_owned(),
        "end tell".to_owned(),
        "end if".to_owned(),
        r#"return """#.to_owned(),
    ]
}

/// The AppleScript line that stops the player (Spotify has no stop, so it
/// pauses) if it is running.
pub fn mac_stop_script(player: MacPlayer) -> String {
    let (app, verb) = match player {
        MacPlayer::Music => ("Music", "stop"),
        MacPlayer::Spotify => ("Spotify", "pause"),
    };
    format!(r#"if application "{app}" is running then tell application "{app}" to {verb}"#)
}

/// The track in a `state<TAB>title<TAB>artist` line, as printed by
/// [`mac_now_playing_script`].
pub fn parse_mac_now_playing(output: &str, app: &str) -> Option<NowPlaying> {
    let mut fields = output.trim_end_matches(['\r', '\n']).splitn(3, '\t');
    let state = fields.next()?;
    let title = fields.next()?.trim();
    let artist = fields.next().unwrap_or("").trim();
    (!title.is_empty()).then(|| NowPlaying {
        title: title.to_owned(),
        artist: artist.to_owned(),
        app: app.to_owned(),
        playing: state.trim().eq_ignore_ascii_case("playing"),
    })
}

// ---------------------------------------------------------------------------
// Linux
// ---------------------------------------------------------------------------

/// `playerctl` arguments for a command. Applies to the most recently active
/// player.
pub fn playerctl_args(command: MediaCommand) -> &'static [&'static str] {
    match command {
        MediaCommand::PlayPause => &["play-pause"],
        MediaCommand::Next => &["next"],
        MediaCommand::Previous => &["previous"],
        MediaCommand::Stop => &["stop"],
    }
}

/// `playerctl metadata` arguments that print `status<TAB>title<TAB>artist<TAB>player`.
pub const PLAYERCTL_METADATA: [&str; 3] = [
    "metadata",
    "--format",
    "{{status}}\t{{title}}\t{{artist}}\t{{playerName}}",
];

/// The track in [`PLAYERCTL_METADATA`]'s output. Stopped players and empty
/// titles are "nothing playing".
pub fn parse_playerctl(output: &str) -> Option<NowPlaying> {
    let mut fields = output.trim_end_matches(['\r', '\n']).splitn(4, '\t');
    let status = fields.next()?.trim();
    let title = fields.next()?.trim();
    let artist = fields.next().unwrap_or("").trim();
    let app = fields.next().unwrap_or("").trim();
    if title.is_empty() || status.eq_ignore_ascii_case("stopped") {
        return None;
    }
    Some(NowPlaying {
        title: title.to_owned(),
        artist: artist.to_owned(),
        app: app.to_owned(),
        playing: status.eq_ignore_ascii_case("playing"),
    })
}

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

/// A readable player name from a Windows app user model id: `Spotify.exe`,
/// `SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify`,
/// `OpenAI.Codex_2p2nqsd0c76g0!App`, `Chrome`.
pub fn friendly_app_name(app_user_model_id: &str) -> String {
    let id = app_user_model_id.trim();
    let (package, app) = id.rsplit_once('!').unwrap_or(("", id));
    // Most Store apps are simply `!App`: the package then names them.
    let name = if app.is_empty() || app.eq_ignore_ascii_case("app") {
        package.split('_').next().unwrap_or(package)
    } else {
        app
    };
    let name = name
        .strip_suffix(".exe")
        .or_else(|| name.strip_suffix(".EXE"))
        .unwrap_or(name);
    // `Microsoft.ZuneMusic` is a package name: its last part is the app's.
    name.rsplit('.').next().unwrap_or(name).to_owned()
}

/// Single line of text for a player-supplied string: control characters and
/// runs of whitespace become single spaces, and the length is capped.
pub fn clean_text(text: &str) -> String {
    const MAX_CHARS: usize = 200;
    let mut out = String::new();
    for word in text.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    let cleaned: String = out.chars().filter(|c| !c.is_control()).collect();
    cleaned.chars().take(MAX_CHARS).collect()
}

// ---------------------------------------------------------------------------
// Dispatch to the current OS
// ---------------------------------------------------------------------------

/// The media commands that can work here. Probes `PATH` and the app folders:
/// call from a background thread.
pub fn available_commands() -> Vec<MediaCommand> {
    supported_commands(Os::current(), &MediaEnv::detect())
}

/// Whether [`now_playing`] can answer here.
pub fn now_playing_available() -> bool {
    can_query(Os::current(), &MediaEnv::detect())
}

/// What is playing, if anything. Talks to the player (WinRT, Apple events, a
/// D-Bus service): can take a moment, so call it from a background thread.
pub fn now_playing() -> Result<Option<NowPlaying>> {
    #[cfg(windows)]
    {
        crate::windows::media::now_playing()
    }
    #[cfg(target_os = "macos")]
    {
        let env = MediaEnv::detect();
        let mut paused = None;
        for player in env.mac_players {
            let mut args = Vec::new();
            for line in mac_now_playing_script(player) {
                args.push("-e".to_owned());
                args.push(line);
            }
            let Ok(output) = capture("/usr/bin/osascript", &args) else {
                continue;
            };
            if let Some(track) = parse_mac_now_playing(&output, player.app_name()) {
                if track.playing {
                    return Ok(Some(clean(track)));
                }
                paused.get_or_insert(track);
            }
        }
        Ok(paused.map(clean))
    }
    #[cfg(target_os = "linux")]
    {
        let Some(tool) = find_tool("playerctl") else {
            return Ok(None);
        };
        // Exits non-zero when no player is running: that is "nothing playing".
        Ok(capture(&tool.to_string_lossy(), &PLAYERCTL_METADATA)
            .ok()
            .and_then(|output| parse_playerctl(&output))
            .map(clean))
    }
}

pub(crate) fn clean(mut track: NowPlaying) -> NowPlaying {
    track.title = clean_text(&track.title);
    track.artist = clean_text(&track.artist);
    track.app = clean_text(&track.app);
    track
}

/// Presses the media button.
pub fn control(command: MediaCommand) -> Result<()> {
    #[cfg(windows)]
    {
        crate::windows::media::control(command)
    }
    #[cfg(target_os = "macos")]
    {
        match mac_media_key(command) {
            Some(key) => crate::macos::media::post_media_key(key),
            None => {
                let env = MediaEnv::detect();
                if env.mac_players.is_empty() {
                    return Err(PlatformError::Unsupported("stopping playback"));
                }
                for player in env.mac_players {
                    let line = CommandLine {
                        program: "/usr/bin/osascript".to_owned(),
                        args: vec!["-e".to_owned(), mac_stop_script(player)],
                    };
                    crate::process::run_checked(&line.program, &line.args, crate::system::GRACE)?;
                }
                Ok(())
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        let tool = find_tool("playerctl").ok_or(PlatformError::Unsupported(
            "media control (install playerctl)",
        ))?;
        let line = CommandLine {
            program: tool.to_string_lossy().into_owned(),
            args: playerctl_args(command)
                .iter()
                .map(|a| (*a).to_owned())
                .collect(),
        };
        crate::process::run_checked(&line.program, &line.args, crate::system::GRACE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_keys_round_trip() {
        for command in MediaCommand::ALL {
            assert_eq!(MediaCommand::from_key(command.key()), Some(command));
        }
        assert_eq!(MediaCommand::from_key("pause"), None);
    }

    #[test]
    fn availability_per_os() {
        let none = MediaEnv::default();
        assert_eq!(supported_commands(Os::Windows, &none), MediaCommand::ALL);
        assert!(can_query(Os::Windows, &none));

        // No player installed on a Mac: the keys still work, stop does not.
        let mac = supported_commands(Os::Mac, &none);
        assert!(!mac.contains(&MediaCommand::Stop));
        assert!(mac.contains(&MediaCommand::PlayPause));
        assert!(!can_query(Os::Mac, &none));
        let with_music = MediaEnv {
            mac_players: vec![MacPlayer::Music],
            ..MediaEnv::default()
        };
        assert_eq!(supported_commands(Os::Mac, &with_music), MediaCommand::ALL);
        assert!(can_query(Os::Mac, &with_music));

        assert!(supported_commands(Os::Linux, &none).is_empty());
        assert!(!can_query(Os::Linux, &none));
        let with_playerctl = MediaEnv {
            playerctl: true,
            ..MediaEnv::default()
        };
        assert_eq!(
            supported_commands(Os::Linux, &with_playerctl),
            MediaCommand::ALL
        );
        assert!(can_query(Os::Linux, &with_playerctl));
    }

    #[test]
    fn mac_media_keys_and_event_data() {
        assert_eq!(mac_media_key(MediaCommand::PlayPause), Some(16));
        assert_eq!(mac_media_key(MediaCommand::Next), Some(17));
        assert_eq!(mac_media_key(MediaCommand::Previous), Some(18));
        assert_eq!(mac_media_key(MediaCommand::Stop), None);
        assert_eq!(mac_key_event_data(16, true), 0x0010_0A00);
        assert_eq!(mac_key_event_data(16, false), 0x0010_0B00);
        assert_eq!(mac_key_event_data(17, true), 0x0011_0A00);
    }

    #[test]
    fn mac_scripts_name_the_player_and_never_launch_it() {
        let script = mac_now_playing_script(MacPlayer::Spotify);
        assert!(script[0].contains(r#"application "Spotify" is running"#));
        assert!(script[1].contains(r#"tell application "Spotify""#));
        assert_eq!(
            mac_stop_script(MacPlayer::Music),
            r#"if application "Music" is running then tell application "Music" to stop"#
        );
        assert!(mac_stop_script(MacPlayer::Spotify).ends_with("to pause"));
    }

    #[test]
    fn mac_output_parsing() {
        let track = parse_mac_now_playing("playing\tSong\tArtist\n", "Music").unwrap();
        assert_eq!(
            track,
            NowPlaying {
                title: "Song".into(),
                artist: "Artist".into(),
                app: "Music".into(),
                playing: true
            }
        );
        assert!(
            !parse_mac_now_playing("paused\tSong\tArtist", "Music")
                .unwrap()
                .playing
        );
        // A title with a tab in the artist field stays whole.
        assert_eq!(
            parse_mac_now_playing("playing\tS\tA\tB", "x")
                .unwrap()
                .artist,
            "A\tB"
        );
        assert_eq!(parse_mac_now_playing("", "Music"), None);
        assert_eq!(parse_mac_now_playing("playing\t\tArtist", "Music"), None);
        assert_eq!(parse_mac_now_playing("\n", "Music"), None);
    }

    #[test]
    fn playerctl_commands_and_parsing() {
        assert_eq!(playerctl_args(MediaCommand::PlayPause), ["play-pause"]);
        assert_eq!(playerctl_args(MediaCommand::Stop), ["stop"]);
        assert_eq!(PLAYERCTL_METADATA[1], "--format");
        assert_eq!(
            PLAYERCTL_METADATA[2],
            "{{status}}\t{{title}}\t{{artist}}\t{{playerName}}"
        );
        let track = parse_playerctl("Playing\tTitle\tArtist\tspotify\n").unwrap();
        assert_eq!(track.title, "Title");
        assert_eq!(track.artist, "Artist");
        assert_eq!(track.app, "spotify");
        assert!(track.playing);
        assert!(!parse_playerctl("Paused\tT\tA\tvlc").unwrap().playing);
        assert_eq!(parse_playerctl("Stopped\tT\tA\tvlc"), None);
        assert_eq!(parse_playerctl("Playing\t\tA\tvlc"), None);
        assert_eq!(parse_playerctl(""), None);
        // No artist (a podcast, a video).
        assert_eq!(
            parse_playerctl("Playing\tVideo\t\tfirefox").unwrap().artist,
            ""
        );
    }

    #[test]
    fn app_names_are_made_readable() {
        assert_eq!(friendly_app_name("Spotify.exe"), "Spotify");
        assert_eq!(
            friendly_app_name("SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify"),
            "Spotify"
        );
        assert_eq!(
            friendly_app_name("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic"),
            "ZuneMusic"
        );
        assert_eq!(friendly_app_name("OpenAI.Codex_2p2nqsd0c76g0!App"), "Codex");
        assert_eq!(
            friendly_app_name("Clipchamp.Clipchamp_yxz26nhyzhsrt!App"),
            "Clipchamp"
        );
        assert_eq!(friendly_app_name("chrome.exe"), "chrome");
        assert_eq!(friendly_app_name("Chrome"), "Chrome");
        assert_eq!(friendly_app_name(""), "");
    }

    #[test]
    fn player_text_is_one_clean_line() {
        assert_eq!(clean_text("  A \n\t B\u{7} "), "A B");
        assert_eq!(clean_text("x".repeat(500).as_str()).chars().count(), 200);
        assert_eq!(clean_text(""), "");
    }
}
