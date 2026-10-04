# rust-hello

An example Sevak extension written in Rust with
[`sevak-extension-sdk`](../../crates/sevak-extension-sdk). Type `rh Ada` for
greetings; **Remember Ada** saves the name in the extension's data folder and
`rh` alone lists the saved names.

It is also the source of the project template: `templates/rust-extension/src/main.rs`
is this file's `src/main.rs`, and a test keeps them identical.

```sh
cargo test -p rust-hello                 # unit tests, and the program through Sevak's real plugin host
cargo build -p rust-hello --release
cargo run -p sevak-ext -- validate examples/rust-hello
cargo run -p sevak-ext -- pack examples/rust-hello \
    --binary linux-x86_64=target/release/rust-hello --out dist
```

The guide is [docs/writing-extensions-in-rust.md](../../docs/writing-extensions-in-rust.md).
