# Script and workflow trust: changes

A running list of what changed in how Sevak treats script plugins and
workflows, newest section last. Each entry says what is different for you.
The [threat model](threat-model.md) says why these controls exist.

## Approvals are bound to the plugin's contents

A script plugin's approval used to be tied to its id and command line. It is
now tied to a SHA-256 over:

- the bytes of `plugin.toml`, the id, mode, format and command line;
- the bytes of every file the command names inside the plugin folder (the
  `script`, and arguments such as `main.py` in `["python3", "main.py"]`),
  plus the files listed in the manifest's new `files` key;
- the plugin folder's location.

**What you will notice**

- Editing the script, updating the plugin with `git pull` or a gallery
  re-install, moving or copying the folder, or a second folder that claims the
  same id and command all ask again. The Allow dialog says so.
- The first start of a version with this rule asks once for every existing
  script plugin ("the plugin's contents changed or this is the first review
  under the new rules"). The approvals file is now format 2.
- If a script changes on disk while Sevak is running, Sevak will not start it
  again until you choose **Reload index** and review it.
- When two folders claim one id, the one you have already allowed keeps it.

Workflows already worked this way; they now share the same hashing code, and
their existing approvals stay valid.

## Scripts get a scrubbed environment

Script plugins and workflow scripts used to inherit everything in Sevak's
environment, and a workflow could export a long but incomplete list of
variable names. Now:

- A script starts with a small base set: `PATH`, the home, profile and temp
  folders, the locale, `XDG_*`, the display variables and what Windows programs
  need to start. Tokens, cloud credentials, proxy settings and version-manager
  roots in Sevak's environment are no longer passed on.
- A plugin that needs a variable lists it in its manifest, for example
  `inherit_env = ["OPENAI_API_KEY"]`. The names are part of what the approval
  covers.
- Variables that make an interpreter or loader run something else (startup
  files, module and library search paths, option strings) can be neither
  inherited nor set by a workflow. A workflow node that sets one now fails
  validation instead of being silently passed on. Names are compared without
  regard to case.

**What you will notice:** a script that relied on a variable from Sevak's
environment (a proxy, `JAVA_HOME`, `PYENV_ROOT`, a token) stops seeing it
until its manifest lists it in `inherit_env`.

## Stopping a script stops everything it started

A timeout used to end only the process Sevak started; helpers it had started
kept running and kept the output pipes open. Now:

- Windows: each script process (and each workflow script node) runs in a job
  object. A kill ends the whole job, and the operating system ends it when
  Sevak exits or crashes.
- Linux and macOS: the script starts in its own process group and a kill
  signals the group.
- A script that ends by itself is left alone, and so is what it started.

Limits: a helper that starts its own session or process group escapes on Linux
and macOS; a crash of Sevak does not end the group there. The process is
assigned to its job a moment after it starts, so something it starts in that
instant escapes on Windows.

## Script results use a closed set of actions

Script plugin results are checked against the actions scripts are documented to
use: `copy_text`, `open_url`, `open_path`, `custom` (persistent plugins) and
`launch`. Anything else (pasting into other apps, putting files or images on
the clipboard, revealing in the file manager, elevating) is dropped. `open_url`
links must be `http`, `https` or `mailto` and carry no control characters.
`launch` now needs `capabilities = ["launch"]` in `plugin.toml`; the Allow
dialog lists it. A plugin that uses `launch` without declaring it stops
showing those rows until its manifest is updated (which asks for approval
again).

## The Allow dialogs show everything and cannot be spoofed

- Names, commands, folder names and node settings written by a plugin's or
  workflow's author are cleaned before they appear: control characters, line
  breaks and invisible or direction-changing characters are removed (they could
  fake extra lines or reorder text), and long text is cut with an ellipsis.
- The script plugin dialog shows the folder name with a short contents id, the
  location, the command, the files covered, the environment variables the
  plugin asks for, and the `launch` capability.
- The workflow dialog lists every command with its arguments, the names of the
  environment variables a script node sets, whether it sends text to standard
  input, and "also does:" lines for nodes that paste text or open files and
  links, followed by the folder name and a short contents id.
- **Paste nodes now need approval.** A node that pastes types text into
  whatever app was in front, which can be a terminal. A workflow with a Paste
  node asks once after upgrading, and its dialog says it types into other apps.
  Copy nodes do not need approval (they only fill the clipboard, like the
  copy action in the launcher).

## A script's error output is read with limits

A persistent script's standard error is written to Sevak's log. A line without
a line break, or endless output, could make Sevak's memory grow, and text that
was not valid UTF-8 stopped the reading. Now each line is cut at 1 KiB, at most
200 lines and 32 KiB are logged per process, and everything else is read and
dropped (so the script never blocks on a full pipe). One-shot and workflow
scripts already had a 64 KiB cap.
