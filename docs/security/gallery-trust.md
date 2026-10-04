# Gallery trust

This page describes where the two online galleries (workflows and script
plugins, and themes) get their files, what is checked before anything is
installed, and what is not protected yet. It is a design note for contributors
and for people judging the risk; the threat model links here.

## What the galleries are

Both galleries are lists kept in Sevak's repository (`gallery/index.json` and
`gallery/themes.json`) next to the files they list (`gallery/packages/*.zip`,
`gallery/themes/*.toml`). Nothing is requested until the user presses **Load
gallery** or **Browse online themes**, and nothing is downloaded until the user
presses **Install** on an entry. An installed workflow or script plugin still
has to be allowed in Sevak's own dialog before it runs.

## The rules

### 1. Only the Sevak repository

A file is requested only from

- `https://raw.githubusercontent.com/ninad-k/Sevak/` (repository files), or
- `https://github.com/ninad-k/Sevak/releases/download/` (release assets, kept
  for packages that may be published that way later).

Nothing else is accepted: another host, another repository, `http://`, a user
name or port in the address, a query string, or a path that only looks like it
is inside the repository (`..` and encoded dots are resolved before the check).

The rule is applied in three places, all using one function
(`sevak_core::gallery_source`):

| Where | What is checked |
|---|---|
| Parsing an index | Every entry's file address. An entry that names anything else is left out and counted as skipped. |
| The first request | The address handed to the download code (`sevak_plugins::net`). |
| Every redirect | The address of each hop. A download that started at a release asset may also be redirected to GitHub's release storage (`release-assets.githubusercontent.com`, `objects.githubusercontent.com`); no other redirect leaves the list. At most five redirects are followed. |

### 2. Pinned to the release of the running build

A build does not read `main`. It reads the lists at the tag of its own version:

```text
https://raw.githubusercontent.com/ninad-k/Sevak/v<version>/gallery/index.json
```

Entries in an index name their files by a path **relative to the repository
root at that tag** (`gallery/packages/duckduckgo.zip`), and Sevak joins the path
to the same tag. An index cannot point a build at `main`, at another tag or at
another place: an absolute address is accepted only if it lies below the same
tag's raw address or release-download folder. A change merged to `main` later
therefore does not alter what an already released build lists or installs; it
reaches users only in the next release, together with the code that reads it.

The index files carry a format number (`format` for packages, `version` for
themes). Format 2 is the tag-relative form described here. A build older than
this change reads format 1 and shows "update Sevak" when it meets format 2.

### 3. Builds without a tag

A build whose release tag does not exist (a local build, a development build, a
pre-release version such as `1.2.3-beta.1`) cannot read its own tag. It then
asks for the repository's **latest stable release** (the `releases/latest`
redirect, which skips drafts and pre-releases), accepts only a plain `vX.Y.Z`
tag from it, and reads the lists from that tag. The window says so:

> This build (version 0.1.0) has no published release v0.1.0, so the gallery of
> the latest release (v0.4.2) is shown. Its files are not from this build.

Only a missing tag (HTTP 404) or a build without a tag triggers this. A network
failure is reported as a failure and is never answered with different content.

### 4. Checked before anything is written

- The package or theme file is downloaded into memory, at most
  5 MiB (packages), 256 KiB (theme index) or the theme size limit.
- Its SHA-256 is compared with the one in the index **before** the file is
  unpacked or parsed; on a mismatch nothing is written.
- Packages are unpacked with strict path rules: no absolute paths, drive
  letters, `..`, links or control characters; no Windows device names (`CON`,
  `NUL`, `COM1` and the like, with or without an extension); at most 200 files,
  2 MiB per file and 10 MiB in total; one top-level folder named like the entry.
  The same device-name rule applies to gallery ids and folder names.
- A theme is never written as the bytes that arrived: it is parsed, validated
  and written as the canonical text of the parsed theme.
- A gallery theme never replaces a theme of the same name. The window reports a
  conflict; **Reinstall** on an already installed theme replaces exactly that
  theme and nothing else.

## What this does not protect against

- **No signature yet.** The hash in an index comes from the same repository as
  the file it covers. An attacker who can publish a release (or move an
  existing tag) can change both. Pinning to a tag limits the window to
  published releases; it does not remove the trust in the repository and its
  release process.
- **Tags can be moved** unless the repository protects them. Protecting
  `v*` tags (and, where available, immutable releases) in the repository
  settings is part of what makes the pin meaningful; that setting is outside
  this repository.
- **The latest-release fallback trusts the same account as the pin**, and a
  fallback build can show a list that does not match the build's own
  behaviour.
- **GitHub and TLS.** The connection is HTTPS with the system's trusted roots;
  a compromise of those, or of the account, is not addressed here.
- **What an installed package does** is decided by the Allow dialog and the
  script-plugin rules, not by the gallery (see the
  [threat model](threat-model.md)).

## Future: sign the indexes

The updater already verifies releases with a signing key
(`TAURI_SIGNING_PRIVATE_KEY`). The same key (or a dedicated one) can sign the
index files:

1. The release workflow signs `index.json` and `themes.json` and publishes the
   signature next to each (`index.json.minisig`).
2. Sevak embeds the public key, downloads index and signature, and verifies
   the signature **before parsing** the index.
3. A build then trusts the key rather than the repository's branches and tags;
   the hash in a signed index covers the package.

The format number already allows this to be added without breaking older
builds.
