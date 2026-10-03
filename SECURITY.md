# Security policy

## Supported versions

Sevak releases continuously from `main`. Only the
[latest release](https://github.com/ninad-k/Sevak/releases/latest) gets
security fixes; please update before reporting.

## Reporting a vulnerability

**Please do not open a public issue for security problems.**

Report privately through GitHub:
[**Report a vulnerability**](https://github.com/ninad-k/Sevak/security/advisories/new)
(the Security tab of the repository). Include:

- what an attacker can do, and what they need first (local access, a crafted
  file in a searched folder, a malicious config, ...);
- steps to reproduce, with the Sevak version and OS;
- any logs (their location per OS is in
  [docs/install.md](docs/install.md)). Remove anything private first.

You should get a reply within a week. Once the problem is confirmed, a fix is
released as soon as practical, and you are credited in the advisory unless you
prefer not to be.

## Scope

Things that are in scope include:

- running programs or opening links the user did not choose (for example
  through crafted file names, search results or config values);
- the web view reaching Tauri commands, files or URLs it should not;
- the installers or the release process (for example tampered downloads).

Release downloads can be checked against `SHA256SUMS.txt` on each release page.
The builds are not yet code-signed; see [docs/install.md](docs/install.md).

Out of scope: problems that need an attacker who already controls your user
account (they can edit `config.toml` or run programs anyway), and the operating
system asking for confirmation before running an unsigned app.
