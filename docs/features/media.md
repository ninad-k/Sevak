# Media controls

Press the media buttons of whatever is playing (a music app, a browser tab, a video player) and see the current track, without switching to the player.

## How to use it

Type `play`, `pause`, `next` or `skip`, `previous` or `back`, or `stop`. ++enter++ presses that button for the player you are listening to.

Type `play ` (with the trailing space) or `music` to see the buttons together with a **now playing** row: *title — artist*, and the app.

| Key on the now-playing row | Action |
|---|---|
| ++enter++ | Play or pause |
| ++shift+enter++ | Next track |
| ++alt+enter++ | Previous track |

## How it reaches the player

| | Windows | macOS | Linux |
|---|---|---|---|
| Buttons | The system media session (the one in the volume flyout), else the media keys | The media keys (play/pause, next, previous); stop is sent to Music or Spotify | [`playerctl`](https://github.com/altdesktop/playerctl) (MPRIS) |
| Now playing | The system media session | Music and Spotify (through Apple events, only if they are running) | `playerctl metadata` |

On Linux the buttons are not offered until `playerctl` is installed. On macOS the system's own now-playing service is private, so only Music and Spotify are asked for the track.

## Options

| Setting | Default | What it does | Config section |
|---|---|---|---|
| Keyword | `play` | `play ` lists the buttons and the track; `""` removes the keyword | [`[media] keyword`](../configuration.md#media) |
| Global | `true` | Also match `pause`, `next track`… in ordinary searches | [`[media] global`](../configuration.md#media) |
| Now playing | `true` | Show the playing track (title, artist, app) as a row | [`[media] now_playing`](../configuration.md#media) |

To turn the plugin off, add `"media"` to [`[plugins] disabled`](../configuration.md#plugins). Volume and mute are [automation tasks](tasks.md).

## Privacy

The track is read from your media player when you search for it, never stored or sent anywhere. Set `now_playing = false` to turn the row off.
