//! Windows media control through the system media transport controls (SMTC),
//! the session behind the volume flyout and the lock screen, with the media
//! keys as a fallback for players that do not register a session.

use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY;

use super::paste::{key_input, send_inputs};
use super::tasks::on_mta_thread;
use crate::error::{PlatformError, Result};
use crate::media::{clean, friendly_app_name, MediaCommand, NowPlaying};
use crate::tasks::vk;

fn current_session() -> windows::core::Result<Option<Session>> {
    let manager = Manager::RequestAsync()?.join()?;
    // No session at all comes back as an error, which means "nothing playing".
    Ok(manager.GetCurrentSession().ok())
}

pub(crate) fn now_playing() -> Result<Option<NowPlaying>> {
    on_mta_thread(|| {
        let Some(session) = current_session()? else {
            return Ok(None);
        };
        let properties = session.TryGetMediaPropertiesAsync()?.join()?;
        let title = properties.Title()?.to_string();
        if title.trim().is_empty() {
            return Ok(None);
        }
        let playing = session
            .GetPlaybackInfo()
            .and_then(|info| info.PlaybackStatus())
            .is_ok_and(|status| status == Status::Playing);
        let app = session
            .SourceAppUserModelId()
            .map(|id| friendly_app_name(&id.to_string()))
            .unwrap_or_default();
        Ok(Some(NowPlaying {
            title,
            artist: properties
                .Artist()
                .map(|a| a.to_string())
                .unwrap_or_default(),
            app,
            playing,
        }))
    })
    .map(|track| track.map(clean))
    .map_err(|err| PlatformError::Os {
        operation: "media session",
        message: err.to_string(),
    })
}

pub(crate) fn control(command: MediaCommand) -> Result<()> {
    // Ask the current session first: it is exact. `false` or an error (no
    // session, or the player ignores the request) falls through to the key.
    let handled = on_mta_thread(|| {
        let Some(session) = current_session()? else {
            return Ok(false);
        };
        match command {
            MediaCommand::PlayPause => session.TryTogglePlayPauseAsync()?.join(),
            MediaCommand::Next => session.TrySkipNextAsync()?.join(),
            MediaCommand::Previous => session.TrySkipPreviousAsync()?.join(),
            MediaCommand::Stop => session.TryStopAsync()?.join(),
        }
    })
    .unwrap_or(false);
    if handled {
        return Ok(());
    }
    let key = VIRTUAL_KEY(match command {
        MediaCommand::PlayPause => vk::MEDIA_PLAY_PAUSE,
        MediaCommand::Next => vk::MEDIA_NEXT,
        MediaCommand::Previous => vk::MEDIA_PREV,
        MediaCommand::Stop => vk::MEDIA_STOP,
    });
    send_inputs(&[key_input(key, false), key_input(key, true)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_playing_query_answers_without_changing_anything() {
        // Read-only. Whether something is playing depends on the machine; the
        // call must answer either way, and what it returns must be clean text.
        // Asked twice: see the radio test.
        let _ = now_playing();
        if let Ok(Some(track)) = now_playing() {
            assert!(!track.title.is_empty());
            assert!(!track.title.contains(char::is_control));
            assert!(!track.artist.contains(char::is_control));
        }
    }
}
