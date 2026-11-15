# World clock and time zones

See the time anywhere and convert times between time zones, offline. Type `time`, `time tokyo` or `time 3pm pst in ist`.

## How to use it

| You type | You get |
|---|---|
| `time` | The time here, then in each of your cities |
| `time tokyo`, `time in london` | The time there now; a partial name (`time san`) lists the places that match |
| `time 3pm` | 3 PM here, in each of your cities |
| `time 9:30 tokyo` | 9:30 in Tokyo, here and in your cities |
| `time 3pm pst in ist`, `time 14:30 berlin to new york` | The time converted from one place to another |
| `time tokyo to london` | The time in London now, and how far apart they are |

Each row shows the date, the 12-hour time, the zone's abbreviation and UTC offset, and how far ahead or behind it is. ++enter++ copies the answer, for example `21:00 JST (Tokyo), Thu 15 Jan`.

Times can be written `15`, `15:30`, `1530`, `3pm`, `3 pm`, `3:30 p.m.`, `noon`, `midnight` or `now`. Between the two places, `in`, `to`, `into`, `->`, `>` or `=` all work.

## Places

- **Cities** of the IANA time-zone database (`Kolkata`, `Buenos Aires`, `Reykjavik`) and its full names (`Europe/Berlin`).
- **More cities and countries** that share a zone with one of those: `San Francisco`, `Mumbai`, `Munich`, `Japan`, `UK`, and others.
- **Abbreviations**: `PST`, `EST`, `CST`, `MST`, `UTC`, `GMT`, `BST`, `CET`, `IST` (India), `JST`, `AEST` and more. An abbreviation means the region's time with daylight saving time included. In July `3pm pst` is 3 PM Pacific *daylight* time, and the answer says `PDT`. `CST` is US Central time.

## Daylight saving time

The time-zone rules (the IANA database, version 2025b) are built into Sevak, so the conversion is right on either side of a clock change and nothing is downloaded. A time like `3pm` means today's date in that place. If the clocks skip that time (02:30 on the night they jump from 02:00 to 03:00), Sevak moves it forward by the jump and says so. If the time happens twice (when the clocks go back), Sevak shows the first one and says so.

Your own time zone comes from the operating system. Sevak checks it again on every index reload, so it notices when you travel.

## Options

These are in **Settings → Integrations**.

| Setting | Default | What it does | Config section |
|---|---|---|---|
| Keyword | `time` | Keyword for the clock | [`[clock] keyword`](../configuration.md#clock) |
| Cities | London, New York, Tokyo | What a bare `time` lists | [`[clock] cities`](../configuration.md#clock) |

The plugin is on by default. Turn it off by adding `"clock"` to [`[plugins] disabled`](../configuration.md#plugins).

## Privacy

Everything happens on your computer. Nothing is sent anywhere.
