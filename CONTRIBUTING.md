# Contributing to Sevak

Thanks for helping. Bug reports, fixes, new plugins, docs and testing on
platforms the maintainer doesn't use daily (macOS, KDE, other Linux distros)
are all welcome.

## Before you start

- **Bugs and small fixes:** open a pull request directly, or an
  [issue](https://github.com/ninad-k/Sevak/issues/new/choose) first if you're
  not sure it is a bug.
- **New features and plugins:** open an issue first so we can agree on the
  approach before you spend time on it. Built-in plugins should be useful to
  most people, fast (they run on every keystroke) and work offline.
- **Questions and early ideas:** ask in
  [Discussions](https://github.com/ninad-k/Sevak/discussions) (Q&A for "how
  do I...?", Ideas for anything not yet concrete enough for a feature request).
- **Security problems:** do not open a public issue. See [SECURITY.md](SECURITY.md).

Please follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## Setting up

The prerequisites, the architecture overview and how to run the app are in
[docs/development.md](docs/development.md). In short:

```sh
npm ci
npm run tauri dev          # run with hot reload
```

To add a result source, read [docs/plugins.md](docs/plugins.md); the `uuid`
plugin (`crates/sevak-plugins/src/example_uuid.rs`) is a complete worked example.
If you would rather not write Rust, a script plugin needs only a `plugin.toml`
and a script; see [External plugins](docs/plugins.md#external-plugins) and the
examples in `examples/plugins/`.

## Making a change

1. Fork the repository and create a branch from `main`.
2. Keep the change focused: one fix or feature per pull request.
3. Add or update tests. Platform code has tests that run on its own OS in CI.
4. Update the docs (`README.md`, `docs/`) when behaviour or configuration changes.
5. Run what CI runs:

   ```sh
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   npm run check
   ```

CI also builds and tests on Windows, macOS, Ubuntu and Fedora, so you don't
need every OS locally. Say in the pull request which platforms you tested on
by hand.

The Rust version is pinned in `rust-toolchain.toml` (rustup uses it
automatically). If you add or update a dependency, run `cargo deny check`; see
[Security and supply chain](docs/development.md#security-and-supply-chain) for
the licence policy and how to handle an advisory.

## Pull request titles decide the version

`main` is protected: every change lands through a pull request with passing
CI, and **every merge publishes a release** automatically. The version bump
comes from [Conventional Commit](https://www.conventionalcommits.org) prefixes
in the pull request title and commit messages:

| Prefix | Example | Release |
|---|---|---|
| `fix:` | `fix: keep the hotkey when the new one is taken` | patch (0.1.0 → 0.1.1) |
| `feat:` | `feat(files): search hidden folders` | minor (0.1.0 → 0.2.0) |
| `feat!:` / `BREAKING CHANGE:` footer | `feat!: rename config keys` | major (minor while on 0.x) |
| `docs:`, `chore:`, `ci:`, `refactor:`, `test:` | `docs: macOS install notes` | patch, or none if only docs change |

Releases can also be staged (beta, then promote) and rolled back; see
[docs/releasing.md](docs/releasing.md).

Use the imperative mood and keep the title under about 70 characters. Pull
requests that only touch Markdown, `docs/` or `LICENSE` don't trigger a release.

## Style

- Rust: `rustfmt` defaults and no clippy warnings. Prefer small, documented
  functions; explain *why* in comments, not *what*.
- Keep OS-specific code in `crates/sevak-platform` behind
  `#[cfg(target_os = ...)]`, so the rest of the code needs no cfg gates.
- Svelte/TypeScript: follow the existing style; `npm run check` must pass.
- Don't add telemetry or network calls. Sevak works fully offline, apart from
  opening the web searches the user asks for.

## Triage and priorities

Issues are labeled with priority (P0–P3) to help contributors decide where to focus. Labels are synced from [.github/labels.json](.github/labels.json); the maintainer runs `scripts/sync-labels.sh` to keep them in sync. See [docs/development.md](docs/development.md) for more on managing the issue tracker.

- **P0** (critical): blocks a release or core functionality; gets a reply within days.
- **P1** (high): important feature or significant bug; best-effort response.
- **P2** (medium): nice-to-have improvement or minor issue; no response time promised.
- **P3** (low): ideas for later, low-impact issues; handled when there is time.

There is no response-time SLA. The maintainer is a single person and works on this in spare time.

## License

By contributing, you agree that your contributions are licensed under the
[Apache License 2.0](LICENSE), the same as the rest of the project.
