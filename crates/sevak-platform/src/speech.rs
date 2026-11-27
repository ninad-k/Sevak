//! Text-to-speech and sound playback through the tools each OS ships.
//!
//! Both are fire-and-forget: the program is started and a thread waits for it,
//! so the caller never blocks while the speech or sound plays. Nothing goes
//! through a shell. Text reaches the speech program on standard input where the
//! tool supports it (and after `--` otherwise), and the Windows PowerShell
//! scripts are fixed strings that read the text, voice and path from standard
//! input or environment variables, so user text is never part of a command
//! line that gets parsed.
//!
//! The command tables are pure functions compiled on every OS, so one test
//! module covers all three.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::error::{PlatformError, Result};
use crate::tasks::{find_tool, Os};

/// Longest text handed to a speech program, in bytes.
const MAX_SPEECH_BYTES: usize = 64 * 1024;

/// Environment variable carrying the voice name to the Windows script.
const VOICE_VAR: &str = "SEVAK_SPEAK_VOICE";
/// Environment variable carrying the sound file path to the Windows script.
const SOUND_VAR: &str = "SEVAK_SOUND_FILE";

const WINDOWS_SPEAK: &str = "[Console]::InputEncoding = [System.Text.Encoding]::UTF8; \
Add-Type -AssemblyName System.Speech; \
$text = [Console]::In.ReadToEnd(); \
$synth = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
if ($env:SEVAK_SPEAK_VOICE) { $synth.SelectVoice($env:SEVAK_SPEAK_VOICE) }; \
$synth.Speak($text)";

const WINDOWS_WAV: &str = "(New-Object Media.SoundPlayer $env:SEVAK_SOUND_FILE).PlaySync()";

const WINDOWS_MEDIA: &str = "Add-Type -AssemblyName PresentationCore; \
$player = New-Object System.Windows.Media.MediaPlayer; \
$player.Open([Uri]$env:SEVAK_SOUND_FILE); \
$player.Play(); \
$waited = 0; \
while (-not $player.NaturalDuration.HasTimeSpan -and $waited -lt 50) { Start-Sleep -Milliseconds 100; $waited++ }; \
if ($player.NaturalDuration.HasTimeSpan) { Start-Sleep -Milliseconds ([int]$player.NaturalDuration.TimeSpan.TotalMilliseconds + 200) }; \
$player.Close()";

/// One program to start: argv, optional standard input and extra environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub program: String,
    pub args: Vec<String>,
    pub stdin: Option<String>,
    pub env: Vec<(String, String)>,
}

impl Invocation {
    fn new(program: &str, args: &[&str]) -> Self {
        Self {
            program: program.to_owned(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            stdin: None,
            env: Vec::new(),
        }
    }

    fn with_stdin(mut self, text: &str) -> Self {
        self.stdin = Some(text.to_owned());
        self
    }

    fn with_env(mut self, name: &str, value: &str) -> Self {
        self.env.push((name.to_owned(), value.to_owned()));
        self
    }
}

fn powershell(script: &str) -> Invocation {
    Invocation::new(
        "powershell.exe",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ],
    )
}

/// A voice name is data for the tool, never an option: no leading dash, no
/// control characters, and a sane length.
fn clean_voice(voice: Option<&str>) -> Result<Option<String>> {
    let Some(voice) = voice.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    if voice.starts_with('-') || voice.chars().any(char::is_control) || voice.chars().count() > 100
    {
        return Err(PlatformError::Message(
            "that voice name is not valid".to_owned(),
        ));
    }
    Ok(Some(voice.to_owned()))
}

/// The programs that can speak `text` on `os`, best first. The caller starts
/// the first one that is installed.
pub fn speak_candidates(os: Os, text: &str, voice: Option<&str>) -> Result<Vec<Invocation>> {
    let voice = clean_voice(voice)?;
    let text = truncate(text);
    Ok(match os {
        Os::Mac => {
            let mut args = vec!["-f", "-"];
            if let Some(voice) = voice.as_deref() {
                args.extend(["-v", voice]);
            }
            vec![Invocation::new("say", &args).with_stdin(text)]
        }
        Os::Windows => {
            let mut run = powershell(WINDOWS_SPEAK).with_stdin(text);
            if let Some(voice) = voice.as_deref() {
                run = run.with_env(VOICE_VAR, voice);
            }
            vec![run]
        }
        Os::Linux => {
            let mut spd = vec!["--wait"];
            let mut espeak = Vec::new();
            if let Some(voice) = voice.as_deref() {
                spd.extend(["-y", voice]);
                espeak.extend(["-v", voice]);
            }
            spd.extend(["--", text]);
            espeak.extend(["--", text]);
            vec![
                Invocation::new("spd-say", &spd),
                Invocation::new("espeak-ng", &espeak),
                Invocation::new("espeak", &espeak),
            ]
        }
    })
}

/// The programs that can play the sound file `path` on `os`, best first.
pub fn sound_candidates(os: Os, path: &Path) -> Result<Vec<Invocation>> {
    let text = path.to_string_lossy();
    if text.starts_with('-') || text.chars().any(char::is_control) {
        return Err(PlatformError::Message(
            "that sound file path is not valid".to_owned(),
        ));
    }
    let text = text.as_ref();
    Ok(match os {
        Os::Mac => vec![Invocation::new("afplay", &[text])],
        Os::Windows => {
            let wav = path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"));
            let script = if wav { WINDOWS_WAV } else { WINDOWS_MEDIA };
            vec![powershell(script).with_env(SOUND_VAR, text)]
        }
        Os::Linux => vec![
            Invocation::new("paplay", &["--", text]),
            Invocation::new("pw-play", &["--", text]),
            Invocation::new("aplay", &["-q", "--", text]),
        ],
    })
}

fn truncate(text: &str) -> &str {
    if text.len() <= MAX_SPEECH_BYTES {
        return text;
    }
    let mut end = MAX_SPEECH_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Speaks `text` with the system's speech engine without waiting for it.
pub fn speak(text: &str, voice: Option<&str>) -> Result<()> {
    if text.trim().is_empty() {
        return Ok(());
    }
    start_first(speak_candidates(Os::current(), text, voice)?, "speech")
}

/// Plays the sound file `path` without waiting for it.
pub fn play_sound(path: &Path) -> Result<()> {
    start_first(sound_candidates(Os::current(), path)?, "playing sounds")
}

/// Starts the first candidate whose program is installed.
fn start_first(candidates: Vec<Invocation>, what: &'static str) -> Result<()> {
    for candidate in &candidates {
        if let Some(program) = resolve(&candidate.program) {
            return spawn(&program, candidate);
        }
    }
    Err(PlatformError::Message(format!(
        "no program for {what} was found (tried {})",
        candidates
            .iter()
            .map(|c| c.program.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    )))
}

fn resolve(program: &str) -> Option<std::path::PathBuf> {
    if cfg!(windows) {
        // `powershell.exe` is found by the system; no PATH probing needed.
        Some(program.into())
    } else {
        find_tool(program)
    }
}

fn spawn(program: &Path, run: &Invocation) -> Result<()> {
    let mut command = Command::new(program);
    command
        .args(&run.args)
        .envs(run.env.iter().map(|(k, v)| (k, v)))
        .stdin(if run.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    crate::process::configure_helper_command(&mut command);
    let mut child = command.spawn()?;
    let stdin = run.stdin.clone();
    let pipe = child.stdin.take();
    // A thread of its own writes the text and reaps the child, so the caller
    // never waits and the child never lingers as a zombie.
    let waiter = std::thread::Builder::new()
        .name("sevak-speech".into())
        .spawn(move || {
            if let (Some(text), Some(mut pipe)) = (stdin, pipe) {
                let _ = pipe.write_all(text.as_bytes());
            }
            let _ = child.wait();
        });
    if let Err(err) = waiter {
        tracing::debug!(%err, "could not start speech waiter thread");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_speaks_through_stdin_so_text_is_never_an_option() {
        let runs = speak_candidates(Os::Mac, "-v evil; rm -rf /", Some("Samantha")).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].program, "say");
        assert_eq!(runs[0].args, ["-f", "-", "-v", "Samantha"]);
        assert_eq!(runs[0].stdin.as_deref(), Some("-v evil; rm -rf /"));
    }

    #[test]
    fn windows_script_is_fixed_and_data_travels_out_of_band() {
        let text = "'; Remove-Item C:\\ -Recurse; '";
        let runs = speak_candidates(Os::Windows, text, Some("Microsoft Zira")).unwrap();
        let run = &runs[0];
        assert_eq!(run.program, "powershell.exe");
        assert!(!run.args.iter().any(|a| a.contains("Remove-Item")));
        assert!(!run.args.iter().any(|a| a.contains("Zira")));
        assert_eq!(run.stdin.as_deref(), Some(text));
        assert_eq!(
            run.env,
            [(VOICE_VAR.to_owned(), "Microsoft Zira".to_owned())]
        );
    }

    #[test]
    fn linux_tries_spd_say_then_espeak_with_text_after_double_dash() {
        let runs = speak_candidates(Os::Linux, "--version", Some("en")).unwrap();
        let programs: Vec<&str> = runs.iter().map(|r| r.program.as_str()).collect();
        assert_eq!(programs, ["spd-say", "espeak-ng", "espeak"]);
        assert_eq!(runs[0].args, ["--wait", "-y", "en", "--", "--version"]);
        assert_eq!(runs[1].args, ["-v", "en", "--", "--version"]);
    }

    #[test]
    fn a_voice_that_looks_like_an_option_is_refused() {
        assert!(speak_candidates(Os::Mac, "hi", Some("-o /etc/passwd")).is_err());
        assert!(speak_candidates(Os::Mac, "hi", Some("a\nb")).is_err());
        // Blank means the default voice.
        let runs = speak_candidates(Os::Mac, "hi", Some("  ")).unwrap();
        assert_eq!(runs[0].args, ["-f", "-"]);
    }

    #[test]
    fn sounds_use_each_systems_player() {
        let path = Path::new("/tmp/a b.wav");
        let mac = sound_candidates(Os::Mac, path).unwrap();
        assert_eq!(mac[0].program, "afplay");
        assert_eq!(mac[0].args, ["/tmp/a b.wav"]);
        let linux = sound_candidates(Os::Linux, path).unwrap();
        let programs: Vec<&str> = linux.iter().map(|r| r.program.as_str()).collect();
        assert_eq!(programs, ["paplay", "pw-play", "aplay"]);
        assert_eq!(linux[2].args, ["-q", "--", "/tmp/a b.wav"]);
    }

    #[test]
    fn windows_sound_path_goes_in_the_environment() {
        let wav = sound_candidates(Os::Windows, Path::new("C:\\x'y.wav")).unwrap();
        assert!(wav[0].args.last().unwrap().contains("SoundPlayer"));
        assert!(!wav[0].args.iter().any(|a| a.contains("x'y")));
        assert_eq!(
            wav[0].env,
            [(SOUND_VAR.to_owned(), "C:\\x'y.wav".to_owned())]
        );
        let mp3 = sound_candidates(Os::Windows, Path::new("C:\\a.mp3")).unwrap();
        assert!(mp3[0].args.last().unwrap().contains("MediaPlayer"));
    }

    #[test]
    fn option_like_sound_paths_are_refused() {
        assert!(sound_candidates(Os::Mac, Path::new("-f.wav")).is_err());
    }

    #[test]
    fn empty_text_is_not_spoken() {
        assert!(speak("   ", None).is_ok());
    }

    #[test]
    fn long_text_is_cut_on_a_character_boundary() {
        let text = "é".repeat(MAX_SPEECH_BYTES);
        assert!(truncate(&text).len() <= MAX_SPEECH_BYTES);
        assert!(text.starts_with(truncate(&text)));
    }
}
