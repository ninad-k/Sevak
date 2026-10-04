# cron

A [native extension](../../../docs/writing-extensions-in-rust.md) for Sevak that
explains a cron expression and lists when it runs next. Type `cron` and the
five fields:

```text
cron */15 9-17 * * 1-5      Every 15 minutes, between 09:00 and 17:59, on Monday through Friday
cron 0 0 1 jan *            At 00:00, on day 1 of the month, in January
cron @daily                 At 00:00, every day
cron 0 9 * * * +05:30       the same, read in UTC+05:30
```

You get the meaning in words (Enter copies the normalized expression) and the
**next five run times** (Enter copies one as an ISO 8601 timestamp).

- Fields: minute, hour, day of month, month, day of week. Lists (`1,15`),
  ranges (`9-17`), steps (`*/5`, `10-30/5`, `5/20`), month and weekday names
  (`jan`, `mon-fri`), `?` for `*`, Sunday as `0` or `7`, and the shortcuts
  `@hourly @daily @midnight @weekly @monthly @yearly @annually`.
- Like Vixie cron, when **both** day of month and day of week are restricted a
  day matches if **either** does (`0 0 13 * fri` is the 13th and every Friday).
- **Times are UTC**, or a fixed offset if the expression ends with one
  (`+05:30`, `-8`, `utc`). Named time zones and daylight saving time are not
  known to it; say so before relying on a schedule that crosses a DST change.
- Not supported, and said so: a seconds field (Quartz style, six fields),
  `L` `W` `#`, and `@reboot`. An expression that can never run (`0 0 30 2 *`)
  is reported as "Never runs".

## Permissions: none

It reads the text you type and the system clock. It opens no network
connection, touches no file and starts no program. The source is one file,
[`src/main.rs`](src/main.rs).

## Dependencies

Only `sevak-extension-sdk` (Apache-2.0, this repository) and its dependencies;
the date arithmetic is written out. Checked with `cargo deny` against the
repository's `deny.toml`.

| Crate | Licence |
|---|---|
| sevak-extension-sdk | Apache-2.0 |
| serde, serde_core, serde_derive, serde_json | MIT OR Apache-2.0 |
| itoa, proc-macro2, quote, syn | MIT OR Apache-2.0 |
| unicode-ident | (MIT OR Apache-2.0) AND Unicode-3.0 |
| memchr | Unlicense OR MIT |
| zmij | MIT |

## Build and test

```sh
cargo test --locked
cargo build --release --locked
cargo run -p sevak-ext -- validate examples/native/cron    # from the repository root
```

Packages for all platforms are built by
[`.github/workflows/native-extensions.yml`](../../../.github/workflows/native-extensions.yml).

Licence: Apache-2.0.
