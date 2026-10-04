# Pomodoro for Sevak

Requires Node.js 22+ on PATH. Install this folder as a script plugin, reload
Sevak, and allow its Node command when prompted.

| Command | Enter does |
|---|---|
| `pomo start 25` | Starts a 25-minute focus timer |
| `pomo break 5` | Starts a 5-minute break |
| `pomo pause` | Pauses a running timer |
| `pomo resume` | Resumes a paused timer |
| `pomo reset` | Stops the timer, preserving the completed count |
| `pomo status` | Shows remaining time and completed focus sessions |

Durations are whole minutes from 1 to 180; defaults are 25 for focus and 5 for
a break. Starting another timer replaces the current one only when you press
Enter. Typing a start/pause/reset command does not execute it.

The extension saves a deadline in `SEVAK_PLUGIN_DATA/timer.json`. Elapsed time
survives an idle plugin shutdown or Sevak restart. It recognizes completion
when next queried and counts each completed focus session once. Breaks start
manually. There are **no background alarms, OS notifications or live countdown**;
reopen or retype `pomo status` to refresh (Sevak may cache a query for two seconds).
This plugin makes no network requests.

If the state file is damaged, the plugin displays an explanation and leaves
the file intact. Back it up or rename it to reset. Find the data folder under
Sevak's data directory, in `plugins/pomodoro/`.
