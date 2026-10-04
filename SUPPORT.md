# Support and community

## Ask a question

[**GitHub Discussions**](https://github.com/ninad-k/Sevak/discussions) is the right place for:

- "How do I...?" setup and usage questions (Q&A category)
- Ideas and early feedback (Ideas category)
- Showing off your setup and workflows

## Report a bug

Open a [bug report](https://github.com/ninad-k/Sevak/issues/new?template=bug_report.yml) on GitHub Issues and include:

- Your Sevak version (run `sevak --version`)
- Your operating system and version (for example "Windows 11 24H2", "macOS 15.1", "Ubuntu 24.04")
- What you did, what you expected, and what happened instead
- Relevant logs (remove anything private first; locations are in [docs/install.md](docs/install.md))
- The relevant parts of your `config.toml` if you changed any settings

A diagnostics report option is coming; more details will be added when available.

## Report a security issue

**Please do not open a public GitHub issue for security problems.** Report privately through GitHub:
[**Report a vulnerability**](https://github.com/ninad-k/Sevak/security/advisories/new) (the Security tab of the repository).

See [SECURITY.md](SECURITY.md) for details on what is in scope and what to include.

## Request a feature

Open a [feature request](https://github.com/ninad-k/Sevak/issues/new?template=feature_request.yml) on GitHub Issues.
First, read [ROADMAP.md](ROADMAP.md) to see what is already planned.

## Supported versions

Only the [latest release](https://github.com/ninad-k/Sevak/releases/latest) gets bug fixes and security patches.
Updates are free and come with the installer and update checks in the app.

## Out of scope

Sevak is a launcher and cannot be the primary tool for:

- Organizing or managing files (that is what file managers are for; Sevak can open them)
- Managing passwords or secrets (use a dedicated password manager; Sevak supports 1Password as a read-only lookup)
- Real-time data fetching and monitoring (the clipboard history and workflow galleries fetch only when you ask)
- System administration tasks at scale (Sevak is for your own desktop, not managing many machines)
- Replacing your shell or terminal (Sevak runs commands, but scripting is not its focus)

## Docs and community

- [User guide](docs/usage.md) – how to search, launch and use actions
- [Keyboard reference](docs/keyboard.md) – every shortcut
- [Configuration reference](docs/configuration.md) – every setting in `config.toml`
- [Troubleshooting](docs/troubleshooting.md) – solving common problems
- [Privacy](docs/privacy.md) – what Sevak sends over the network
- [Developer guide](docs/development.md) – building from source, writing plugins
