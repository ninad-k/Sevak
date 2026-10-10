# regex

A [native extension](../../../docs/writing-extensions-in-rust.md) for Sevak that
tests a regular expression against sample text. Type `regex`, the pattern,
`=>` and the text:

```text
regex (\d+)-(\d+) => call 555-1234 or 800-9999
regex (?i)rust => I like RUST
regex (?P<year>\d{4})-(?P<month>\d\d) => due 2026-10-04
```

You get a summary ("2 matches"; Enter copies all matches, one per line), then
one row per match (the first 20) with where it starts, in characters, and what
each capture group caught, by number or name. Enter copies a match. A pattern
alone says whether it compiles and what groups it has; a pattern that does not
compile shows the compiler's reason.

- Syntax is the Rust [`regex`](https://docs.rs/regex) crate's: Unicode aware,
  inline flags (`(?i)`, `(?s)`, `(?m)`, `(?x)`), `\b`, `\p{Greek}`, named groups
  `(?P<name>...)`. **No lookaround and no backreferences**, which is what lets
  matching run in time linear in the text: no pattern can hang the launcher
  (`(a+)+$` on thousands of `a` is instant). Other engines differ in details,
  so a pattern that works here may not in PCRE, JavaScript or Python.
- A compiled pattern is limited to 2 MiB, and counting stops at 10,000 matches.
- The sample is everything after the first `=>` (one space after it is the
  separator). It is a single line, so use `\n` in the pattern, not in the text.

## Permissions: none

It reads only the text you type. It opens no network connection, touches no
file and starts no program. The source is one file,
[`src/main.rs`](src/main.rs).

## Dependencies

Shipped: `sevak-extension-sdk` (Apache-2.0, this repository) and `regex` with
its dependencies. Checked with `cargo deny` against the repository's `deny.toml`.

| Crate | Licence |
|---|---|
| sevak-extension-sdk | Apache-2.0 |
| regex, regex-automata, regex-syntax | MIT OR Apache-2.0 |
| aho-corasick, memchr | Unlicense OR MIT |
| serde, serde_core, serde_derive, serde_json | MIT OR Apache-2.0 |
| itoa, proc-macro2, quote, syn | MIT OR Apache-2.0 |
| unicode-ident | (MIT OR Apache-2.0) AND Unicode-3.0 |
| zmij | MIT |

## Build and test

```sh
cargo test --locked
cargo build --release --locked
cargo run -p sevak-ext -- validate examples/native/regex   # from the repository root
```

Packages for all platforms are built by
[`.github/workflows/native-extensions.yml`](../../../.github/workflows/native-extensions.yml).
This one is about 1.3 MiB (the Unicode tables) against 0.3 MiB for the others.

Licence: Apache-2.0.
