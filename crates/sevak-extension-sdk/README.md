# sevak-extension-sdk

Write [Sevak](https://github.com/ninad-k/Sevak) launcher extensions in Rust.

An extension is a native program. Sevak starts it, sends it what the user typed
after the extension's keyword, and shows the rows it answers with. This crate
speaks Sevak's script-plugin protocol (newline-delimited JSON on stdin and
stdout, protocol version 1), so you write only the function from a query to a
list of rows. Its only dependencies are `serde` and `serde_json`.

```rust
use sevak_extension_sdk::{run, Item, Query};

fn main() {
    run(|query: &Query| {
        if query.is_empty() {
            return Ok(vec![Item::new("Type your name").subtitle("Say hello")]);
        }
        Ok(vec![Item::new(format!("Hello, {}!", query.text())).copy_on_enter()])
    });
}
```

What a row can do when picked is a closed set that Sevak performs itself: copy
text, open a web or mail link, open a path, send a payload back to your program,
and (with the `launch` capability in the manifest) start an application.

An extension runs with the user's account permissions in a scrubbed
environment. It is **not sandboxed**, and the permissions it declares are shown
to the user, not enforced.

The full guide (project template, manifest, packaging with `sevak-ext`,
approval, publishing to the gallery) is
[docs/writing-extensions-in-rust.md](https://github.com/ninad-k/Sevak/blob/main/docs/writing-extensions-in-rust.md).

## Licence

Apache-2.0, like Sevak.
