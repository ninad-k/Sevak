# Security policy

Sevak is a local keyboard launcher. It reads what you type, copy and select,
launches programs, and can run scripts you approve, so security reports are
taken seriously. How Sevak is meant to resist attack, and where it
deliberately does not, is written down in the
[threat model](docs/security/threat-model.md); the reasoning behind its main
design choices is in the [design decisions](docs/decisions/index.md).

## Supported versions

Sevak releases continuously from `main`. Only the
[latest release](https://github.com/ninad-k/Sevak/releases/latest) gets
security fixes; please update before reporting (the tray menu's **Check for
updates** installs it). Older versions are not patched, and a fix is shipped as
a new release, not as a backport.

## Reporting a vulnerability

**Please do not open a public issue, discussion or pull request for a
security problem.**

Report privately through GitHub:
[**Report a vulnerability**](https://github.com/ninad-k/Sevak/security/advisories/new)
(the Security tab of the repository). Include:

- what an attacker can do, and what they need first (local access, a crafted
  file in a searched folder, a malicious config, a plugin from the gallery, ...);
- steps to reproduce, with the Sevak version and OS;
- any logs (their location per OS is in
  [docs/install.md](docs/install.md)). Remove anything private first.

### What to expect

Sevak is maintained by one person, so these are targets rather than
guarantees:

| Step | Target |
|---|---|
| Acknowledgement that the report arrived | within 7 days |
| First assessment (accepted, needs more information, or out of scope, with the reason) | within 14 days |
| Fix released for a confirmed critical or high severity issue | within 30 days |
| Fix released for a confirmed medium or low severity issue | within 90 days, usually sooner |

Fixes ship through the normal release (every merge to `main` publishes one),
and installed copies are offered the update within hours. Once a fix is
released, the advisory is published and you are credited, unless you prefer
not to be. If a fix will take longer than the target, you will be told why.
Please give us the chance to fix an issue before you disclose it; if there is
no response after the targets above, you may disclose it, and we would still
like to hear from you first.

### Safe harbour

If you act in good faith to find and report a vulnerability in Sevak, we will
not pursue or support legal action against you for it. Good faith means:

- you test on your own machine, accounts and data, and do not access, change
  or keep other people's data;
- you do not degrade the service of GitHub or anyone else (this includes the
  repository, its releases and the gallery files: do not push, or try to push,
  anything to them);
- you do not use social engineering, phishing or physical attacks against the
  maintainer or other users;
- you report the issue privately, give us reasonable time to fix it, and do not
  publish details before a fix is released or the targets above have passed.

This is the maintainer's statement for the Sevak project. It cannot bind
third parties such as GitHub, Microsoft, Apple or your employer.

## Scope

Sevak runs with your user's privileges and is meant to act on your behalf, so
many things a program "can do" are not vulnerabilities by themselves. The line
is **doing something you did not ask for, or letting someone else do it**.

In scope:

- **Running programs or opening links the user did not choose**, for example
  through crafted file names, search results, clipboard or selection content,
  bookmarks, contact names, theme or plugin metadata, or config values that
  Sevak did not mean to treat as commands.
- **Crossing the approval boundary**: a script plugin or workflow that runs
  without the user's approval, or that keeps running after the code it
  was approved for has changed (see the [threat model](docs/security/threat-model.md)).
- **The web view reaching what it should not**: Tauri commands, files or URLs
  beyond what the launcher needs, script injection into the window, a way to
  turn text from a result into markup or script.
- **Leaking what should stay private**: typed keystrokes, clipboard contents
  marked secret by the source app, selections or 1Password item data written
  to disk or logs, or sent over the network.
- **Gallery and update integrity**: installing or running something that fails
  its checksum or signature, escaping the install folder (path traversal,
  symlinks, archive bombs), or downgrade or redirect tricks in the updater.
- **The installers or the release process**, for example a tampered download
  that passes the documented checks.
- **Memory-safety or denial-of-service problems that a remote or low-privilege
  party can trigger** with a crafted file, clipboard item, plugin output or
  gallery package (for example unbounded memory or a hang that survives restart).

Out of scope:

- Problems that need an attacker who already controls your user account or can
  already write to your config or data folders: they can edit `config.toml`,
  add a snippet or a shell command, or run programs directly. Software running
  as you can also read your clipboard and screen; Sevak does not try to defend
  against that, and clipboard history is stored unencrypted by design (it is
  documented, and can be switched off).
- Scripts, plugins and workflows that do something harmful *after you approved
  them*. An approval is permission to run code with your privileges. Reports
  about a plugin's own bugs go to that plugin's author.
- The operating system asking for confirmation before running an unsigned app,
  and the SmartScreen or Gatekeeper warnings on the current unsigned builds.
- Weaknesses in third-party components with no demonstrated effect on Sevak
  (report them upstream), and best-practice gaps without an attack.
- Reports from automated scanners with no working demonstration, social
  engineering of the maintainer, and anything needing physical access to an
  unlocked machine.
- Features that are documented as not working in a given environment, such as
  Wayland not allowing global shortcuts or snippet expansion.

If you are not sure whether something is in scope, report it anyway.

## Verifying downloads

Every release page lists `SHA256SUMS.txt`, the SHA-256 of every file attached
to the release, including the installers and `latest.json`. Check a download
against it before you run it:

```sh
# Linux / macOS, in the folder with the download and SHA256SUMS.txt
sha256sum --ignore-missing -c SHA256SUMS.txt        # macOS: shasum -a 256 -c ...
```

```powershell
# Windows
(Get-FileHash .\Sevak_<version>_x64-setup.exe -Algorithm SHA256).Hash
# compare with the line for that file in SHA256SUMS.txt
```

A checksum that sits on the same page as the download guards against a
corrupted or truncated file, not against someone who can replace both. Two
stronger checks:

- **Updater signature.** Sevak installs updates itself only after verifying a
  [minisign](https://jedisct1.github.io/minisign/) signature with the public
  key compiled into the app. That key is in
  [`src-tauri/tauri.conf.json`](src-tauri/tauri.conf.json) (`plugins.updater.pubkey`;
  key id `B63493C2982845FF`). The signature of each updatable package is the
  `.sig` file next to it on the release page and the `signature` field in
  `latest.json`; both are base64-encoded minisign signatures, so decode the
  text before checking a package with the `minisign` tool.
- **Provenance.** Releases are built by
  [the release workflow](.github/workflows/release.yml) from `main`; the
  commit a release was built from is the one its tag points at.

The Windows and macOS builds are **not code-signed or notarized yet**; see
[docs/install.md](docs/install.md) for what your OS will show. Package-manager
copies (winget, Scoop, Homebrew, AUR) take their checksums from
`SHA256SUMS.txt`.
