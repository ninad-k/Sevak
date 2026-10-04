# Extensions: browse and install

**Settings → Extensions** (and the `ext` keyword in the launcher) lists what the
Sevak gallery offers and installs it for you: [workflows](../workflows.md),
[script plugins](script-plugins.md), **native extensions** (programs written
in Rust, see [Writing extensions in Rust](../writing-extensions-in-rust.md)) and
[themes](../themes.md). You can see who published each one, its version,
licence and permissions, and where it comes from, install it, update it, switch
it off and remove it, all without leaving Sevak.

It is **opt-in and on request**, and it adds no new way for something to run:

- Opening the page requests nothing. It shows what you installed and the list
  you loaded last time, with how old it is.
- **Load the list** downloads two small files, the package list and the theme
  list, from Sevak's repository on GitHub **at the tag of your Sevak version**
  (a build without a published release uses the latest release and says so).
  Files are requested only from Sevak's own repository, also when a request is
  redirected. See [Gallery trust](../security/gallery-trust.md).
- **Install** downloads that one package over HTTPS, checks it against the
  SHA-256 in the list and **discards it if it does not match**, refuses unsafe
  paths, links and oversized files, and only then writes it to your plugins,
  workflows or themes folder. For a native extension the package's own manifest
  must also say what the list said (version, publisher, licence, permissions).
- **Nothing runs because you installed it.** A new workflow, script plugin or
  native extension is not allowed yet: Sevak shows its permission dialog first,
  as for anything you drop into the folders by hand. A native extension's dialog
  is titled **Sevak: new native extension** and shows the publisher, its
  declared permissions and the program's SHA-256.
- No account, no identifier and no data other than the requests themselves.

## The page

**Discover** lists the loaded list. Search by name, author or what it does, and
filter by kind. Rows have an **Install** button (or **Update**); a native
extension has **Review…** instead, which opens its details: publisher, version,
licence, the permissions it declares, the builds it has (and which one your
computer uses), where its source code is, where it is downloaded from and the
full SHA-256, with a plain statement of what a native extension is. Only then
is there an **Install native extension** button.

Items that this Sevak or computer cannot use are shown with the reason ("Needs
Sevak 0.4.0 or newer", "No build for this computer").

**Installed** lists what you installed from here, with its status (**Ready**,
**Waiting for your permission**, **Switched off**, **Not loaded** and why), an
**Update to v...** indicator when the loaded list has a newer version (also
shown on the tab), **Update**, **Review…** (asks the permission question again
if you said "Not now"), a switch, and **Uninstall**.

- **Update** replaces the folder as one step: the new version is checked and
  written aside first, and if anything fails the old version stays. An updated
  workflow or plugin that can run code, and any updated native extension (it is
  a new program), asks for your permission again.
- **Uninstall** asks first, then removes the folder, the files the extension
  saved (its data folder) and your recorded permission for it. Only things this
  page installed can be removed here, so a folder you made yourself is never
  touched. An installed theme that is in use cannot be removed; choose another
  one first.
- Plugins and workflows you added by hand are under **Plugins** and
  **Workflows**; this page lists only what it installed.

If the gallery cannot be reached the page says so, keeps showing the list you
loaded earlier (and what is installed), and everything that does not need the
network keeps working.

## From the launcher

Type `ext` or `store`, a space and a name:

| You see | Enter |
|---|---|
| **Load the extension list** (until a list has been loaded) | asks, then downloads the list once |
| `<name>` (a kind, the publisher, the version) | asks, then installs it in the background |
| **Update `<name>`** | asks, then updates it |
| an installed or unavailable item | opens Settings → Extensions |

Typing never uses the network or the disk: the rows come from the list loaded
earlier. The confirmation says what you are installing and, for a native
extension, that it is a compiled program that is not sandboxed and never runs
until you allow it. When the work finishes Sevak shows a notification and
asks the usual permission question.

Both keywords can be switched off under **Settings → Plugins** (`extensions`
turns off both). If a script plugin or workflow of yours already uses `ext` or
`store`, Settings warns about the shared keyword, as for any other.

## What to know before installing a native extension

A native extension is a program, not a script you can read. Sevak shows who
published it and what it says it does, but it **cannot sandbox it or check it**:
once you allow it, it runs with your account's permissions like any program you
install, whatever permissions it lists. Allow only extensions whose publisher
you trust. The permissions are the author's statement, shown so you can notice a
mismatch ("a calculator that wants the network"). See the
[security model](../writing-extensions-in-rust.md#security-model) for what Sevak
does and does not do.

## Files

| File | Where | What |
|---|---|---|
| `installed-extensions.json` | data folder | what this page installed: id, version, folder, the checksums |
| `extensions-catalog.json` | data folder | the list you loaded last, so the page works offline |

Both are small and owner-only on Linux and macOS. Deleting them does not delete
what is installed; the page just stops listing it as installed from here.
