# Gallery trust

This page describes where the two online galleries (workflows and script
plugins, and themes) get their files, what is checked before anything is
installed, and what is not protected yet. It is a design note for contributors
and for people judging the risk; the threat model links here.

## What the galleries are

Both galleries are lists kept in Sevak's repository (`gallery/index.json` and
`gallery/themes.json`) next to the files they list (`gallery/packages/*.zip`,
`gallery/themes/*.toml`). Nothing is requested until the user presses **Load
gallery**, **Load the list**, **Refresh the list**, **Check extension advisories**,
or **Browse online themes**. Catalog actions in Gallery and Extensions also
request the signed advisory feed described below. Package bytes are downloaded
only after **Install** or **Update**. An installed workflow or script plugin still
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
Builds of that kind read the lists at their own tags, which still hold format 1,
so they are not affected by the repository moving on.

The committed lists use format 2 only, with every package and theme named by a
path. `node scripts/gallery-check.mjs` (and the repository's tests) fail on an
absolute address, a path that is not under `gallery/packages/` or
`gallery/themes/`, a missing file, a stale hash or a file the list does not name.

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

### 5. Native extensions

A gallery entry of kind `native` is a compiled program, so it has stricter
handling on top of rules 1 to 4. The index stays **format 2**: an entry of a kind
an older Sevak does not know is skipped there, so older builds simply do not list
native extensions.

- **One package per platform.** The entry lists a `platforms` table
  (`windows-x86_64`, `macos-aarch64`, `linux-x86_64`, ...), each with a path under
  `gallery/extensions/<id>/` and its SHA-256, so a user downloads only the build
  for their computer. The paths are resolved against the release tag like every
  other source (rule 2); an entry that names another host, branch or release is
  skipped. `scripts/gallery-check.mjs` and the repository's tests refuse a
  package that is not listed, a hash that does not match, a package over 10 MiB
  and a file name that is not `<id>-<version>-<platform>.sevakext`.
- **Limits.** A package is at most 32 MiB when downloaded (the gallery asks for
  10 MiB), 100 files, 64 MiB per file and 128 MiB unpacked. Paths follow the same
  rules as rule 4. The package carries `checksums.sha256` covering every other
  file, and a file that is missing from it, extra to it or different from it
  discards the package. Only programs the manifest declares are accepted.
- **The package says what the list says.** The page shows the index entry and the
  Allow dialog shows the package's own `plugin.toml`. Before anything is
  written Sevak compares the version, publisher, licence and permissions in both
  and refuses to install if they differ; the package's `id` must also be the
  entry's. A test checks every committed entry the same way.
- **Installed is not allowed.** The folder is new and unapproved. The approval is
  bound to the program's bytes, so an update (a new program) asks again, and
  declared permissions are information, not a limit. See
  [Writing extensions in Rust](../writing-extensions-in-rust.md#security-model).
- **Tag pinning means a release carries the list.** An entry merged to `main`
  reaches users with the next release. That is a delay, not a hole: a shipped
  build cannot be made to list or install something that was added later.
- **Review is the human check.** Reviewers look for public source that builds the
  committed binaries, permissions that match the code, an open-source licence
  and no obfuscation. A review is not an audit; package and catalog signatures
  remain separate future work (below).
- **Updates and removal** are the same code path as install for every kind:
  an update is written aside and swapped in as one step, with the old version put
  back if the swap fails, and removal deletes only what a receipt (the store's
  own record in the data folder) says it installed, only inside the managed
  folders, and only if the folder still holds a manifest.

### 6. Signed revocation notices

The package catalogs remain pinned to their release. A separate advisory channel
lets builds with this feature learn about a removed package without changing that
pin. Only an explicit **Load gallery**, **Load/Refresh the list**, launcher **Load
the extension list**, or **Check extension advisories** requests these two assets
(**Check extension advisories** is the same request as **Refresh the list**, so
it then also reloads the pinned package and theme lists):

```text
https://github.com/ninad-k/Sevak/releases/download/extension-advisories/extension-revocations.json
https://github.com/ninad-k/Sevak/releases/download/extension-advisories/extension-revocations.json.sig
```

There is no startup request, timer, installed-package upload, or request while
typing. Simply opening Extensions reads local state. The theme editor's separate
online gallery does not fetch this feed.

- The two downloads run without holding the advisory state, so the pages'
  lists never wait on the network; verification, the rollback check and the
  swap happen afterwards, serialized with install commits.
- The JSON is limited to **128 KiB** and the detached signature to **8 KiB**.
  Sevak verifies the exact downloaded bytes with minisign before parsing them.
  The trust anchor is the updater public key embedded when Sevak was built (a
  copy in the sevak-plugins crate that a test keeps equal to
  	auri.conf.json's plugins.updater.pubkey);
  neither a downloaded document nor a local cache can supply another key. The
  signature uses the Tauri signer's base64 format. Existing download-host and
  redirect restrictions still apply.
- The strict format-1 envelope carries Unix-second `issued_at`, `expires_at` and
  up to 256 notices. Validity is at most 30 days; malformed metadata, unknown
  fields and responses older than the verified saved copy are rejected. The
  local clock only labels freshness: an authentic feed that looks expired or
  is issued more than five minutes ahead is kept and enforced, but reported as
  stale, so a wrong clock cannot switch revocations off. A package id has
  one record containing exact versions, or a sole `"*"` for all versions.
  Versions compare as semantic versions where both parse (`1.0` equals
  `1.0.0`), otherwise exactly. `"*"` also covers any later fixed version,
  so it blocks the update to the fix too; notices should list exact versions
  unless no version may be installed. Theme notices carry `"kind": "theme"` and the sole `"*"`, and
  never match a package of the same id (or the reverse). Reasons are bounded plain text; optional advisory links must be
  Sevak GitHub security-advisory addresses.
- The data folder's `extension-advisories.json` contains the original body and
  signature. Reads are bounded, and the signature and schema are checked again
  after restart. A failed download, signature mismatch, older response or
  invalid response never replaces the previous verified notices. If writing the
  cache fails, newly authenticated notices still apply in the current session
  and the page explains that they could not be saved. **Previously
  authenticated revocations remain effective after expiry**; the page labels
  freshness unknown/stale rather than silently declaring a clean result.
- A known revoked version cannot be installed or updated from these galleries.
  Each install rechecks after downloading, and its final writes are serialized
  with advisory replacement, so a newly learned notice cannot be bypassed by an
  in-flight download. The separate Appearance theme gallery also enforces known
  cached notices when installing, without fetching the feed itself, and records
  the same receipt as Extensions, so later notices reach that copy.
  The Installed tab warns about matching managed copies even if their entry is
  no longer listed and offers **Uninstall**. An update to a version not covered
  by the notice remains possible. Existing copies are never removed, disabled,
  or executed by the advisory check. Manually installed or older Gallery copies
  without an Extensions receipt need manual removal from their folder. Their
  version is unknown, so the Installed tab lists those that a `"*"` notice
  covers, with their path (a package folder by its catalog folder or id, a
  theme file only while the theme list is loaded), and Gallery warns about any
  notice for such a copy. New installs made from Gallery now receive the same
  managed receipt as Extensions.
- An unavailable or never-published feed displays **unknown** status. It does
  not block all unrelated installs. Even a current verified feed only proves
  who signed those notices, not that other extensions are safe. Offline users
  cannot learn new notices; builds predating this feature cannot use it.

Publication and expiry renewal are an explicit maintainer operation, documented
in [Updating and removal](../marketplace/updating-and-removal.md#publish-and-renew-advisories).
Shipping this code does not publish the channel. Until its first signed upload,
users see unavailable/unknown status. The dedicated channel must remain mutable;
ordinary application releases may remain immutable.

## What this does not protect against

- **No package/catalog signature yet.** The hash in an index comes from the same repository as
  the file it covers. An attacker who can publish a release (or move an
  existing tag) can change both. Pinning to a tag limits the window to
  published releases; it does not remove the trust in the repository and its
  release process. The separate advisory signature does not authenticate those
  indexes or prove that an installed program is harmless.
- **Tags can be moved** unless the repository protects them. Protecting
  `v*` tags (and, where available, immutable releases) in the repository
  settings is part of what makes the pin meaningful; that setting is outside
  this repository.
- **The latest-release fallback trusts the same account as the pin**, and a
  fallback build can show a list that does not match the build's own
  behaviour.
- **GitHub and TLS.** The connection is HTTPS with the system's trusted roots;
  a compromise of those, or of the account, is not addressed here.
- **A native extension is not sandboxed.** The gallery makes sure the file
  you get is the file listed, not that the program is harmless; see rule 5.
- **What an installed package does** is decided by the Allow dialog and the
  script-plugin rules, not by the gallery (see the
  [threat model](threat-model.md) and
  [Script and workflow trust](script-workflow-trust.md)). A workflow that
  pastes into another app waits for approval, like one that runs a script, so
  some gallery workflows that run no code (`tidy-text`, `markdown-tools`) ask
  once after installing.

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
