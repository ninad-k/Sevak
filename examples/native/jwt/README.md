# jwt

A [native extension](../../../docs/writing-extensions-in-rust.md) for Sevak that
decodes a JSON Web Token offline. Type `jwt`, paste a token:

- whether it is **expired**, not yet valid, or has no expiry (judged by the
  `exp` and `nbf` claims against the computer's clock);
- the **algorithm** and what it means (`HS256` is HMAC with SHA-256, `none`
  is flagged as unsigned), plus `typ` and `kid`;
- the **header** and **payload** as JSON (Enter copies them pretty-printed,
  Ctrl+T shows them in full);
- one row per **claim**, registered ones first (`iss`, `sub`, `aud`, `exp`,
  `nbf`, `iat`, `jti`), times shown as UTC dates with "in 2 hours" / "3 days
  ago"; Enter copies the value;
- the **signature**, as written in the token.

A pasted `Bearer ` prefix, quotes and line breaks are ignored. Bad input (two
parts, an encrypted five-part token, text that is not base64url or JSON) gets
a message, not an empty list.

**The signature is not verified.** This reads a token; it does not vouch for it.
Never trust a token because this shows sensible claims.

## Permissions: none

It reads only the text you type. It opens no network connection, touches no
file and starts no program. (Declared permissions are the author's statement,
which Sevak shows and does not enforce; the source is this one file,
[`src/main.rs`](src/main.rs).) A token is never written to the log.

## Dependencies

Shipped: `sevak-extension-sdk` (Apache-2.0, this repository) and `serde_json`
with its dependencies. Checked with `cargo deny` against the repository's
`deny.toml` (permissive licences only).

| Crate | Licence |
|---|---|
| sevak-extension-sdk | Apache-2.0 |
| serde, serde_core, serde_derive, serde_json | MIT OR Apache-2.0 |
| itoa, proc-macro2, quote | MIT OR Apache-2.0 |
| syn | MIT OR Apache-2.0 |
| unicode-ident | (MIT OR Apache-2.0) AND Unicode-3.0 |
| memchr | Unlicense OR MIT |
| zmij | MIT |

Base64url and the date arithmetic are written out (about 60 lines) rather than
pulled from crates. `Cargo.lock` is committed; the program is built from it.

## Build and test

```sh
cargo test --locked                 # unit tests, and the program through Sevak's real plugin host
cargo build --release --locked
cargo run -p sevak-ext -- validate examples/native/jwt     # from the repository root
```

The dev-dependencies (`sevak-core`, `sevak-plugins`, ...) are only for
`tests/host.rs`; none of them is in the program. Packages for all platforms are
built by [`.github/workflows/native-extensions.yml`](../../../.github/workflows/native-extensions.yml)
(see "Building and publishing native extensions with CI" in the guide).

Licence: Apache-2.0.
