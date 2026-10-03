## What and why

<!-- What does this change, and why? Link the issue: "Fixes #123". -->

## How it was tested

<!-- Which platforms did you try it on by hand (Windows / macOS / Linux X11 / Wayland)? -->

## Checklist

- [ ] The title uses a Conventional Commit prefix (`fix:`, `feat:`, `docs:`, ...). It decides the release version; see [CONTRIBUTING.md](../CONTRIBUTING.md).
- [ ] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` and `npm run check` pass.
- [ ] Tests are added or updated for the change.
- [ ] Docs (`README.md`, `docs/`) are updated if behaviour or config changed.
