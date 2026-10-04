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
