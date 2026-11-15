# Calendar

See what is on today and the next few days, from calendar files on your computer. No account, no sign-in and no network: Sevak reads only the `.ics` files and folders you list.

## Set it up

Export your calendar as an `.ics` file (every calendar app can; or let a sync tool such as vdirsyncer keep a folder of them up to date), then add it under **Settings → Integrations → Calendar**, or in `config.toml`:

```toml
[calendar]
files = ["~/calendar.ics", "~/.calendars"]
```

A folder contributes the `.ics` files up to three levels below it. Choose **Reload index** from the tray menu after changing the files (Sevak also rereads them when it refreshes its index).

## How to use it

| Type | Shows |
|---|---|
| `cal` | What is on now and in the next 7 days (`days`) |
| `cal today`, `cal tomorrow`, `cal yesterday` | That day |
| `cal fri`, `cal friday` | The next Friday (today if it is Friday) |
| `cal 2026-12-24` | That date |
| `cal week` | The next seven days |
| `cal dentist` | Events whose title or place matches, in the next year |
| `cal fri review` | Matching events on that day |

Each row shows the time (`Today 09:30–09:45`, `Tue 24 Nov – Thu 26 Nov, all day`), the place and the file it came from. Times are shown in your computer's time zone.

| Key | Action |
|---|---|
| ++enter++ | Copy the event's details: title, time, place, link and notes |
| ++ctrl+enter++ | Open the `.ics` file |
| ++alt+enter++ | Show the `.ics` file in the file manager |
| ++shift+enter++ | Open the event's link (its `URL`, or a meeting link in its place), `http(s)` only |

++enter++ copies rather than opens the file because opening an `.ics` file usually imports it into your calendar app again.

## What is understood

- Events (`VEVENT`) with a start, and an end or duration; all-day and multi-day events; cancelled events are hidden.
- Time zones: `TZID` names from the IANA database (`Europe/Berlin`, also with a vendor prefix), the common Windows names Outlook writes (`W. Europe Standard Time`), UTC and floating times. An unknown zone is read as local time.
- Repeating events (`RRULE`): daily, weekly (with `BYDAY`), monthly (`BYMONTHDAY`, or `BYDAY` such as "first Monday" and "last Friday") and yearly, with `INTERVAL`, `COUNT` and `UNTIL`; `EXDATE` and `RDATE`; and single occurrences that were moved or cancelled (`RECURRENCE-ID`). Rarer rule parts (`BYSETPOS`, `BYWEEKNO`, hourly rules) are not applied, so such events can show extra or only their first occurrence.

Occurrences are worked out for the next 400 days.

## Privacy

- Only the files and folders in `files` are read. Inside a folder, a symbolic link that leads out of it is not followed. Each file is read with a size limit (32 MiB).
- Events are kept in memory only; the log has counts. Choosing an event is kept out of the usage statistics and the search history.
- Nothing is sent anywhere: there is no subscription or URL support on purpose.

## Configuration

```toml
[calendar]
keyword = "cal"
files = []
days = 7              # 1-60
```

See [`[calendar]`](../configuration.md#calendar). Turn the plugin off with `"calendar"` in `[plugins] disabled` or under **Settings → Plugins**.
