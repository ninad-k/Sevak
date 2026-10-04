# ADR-0005: Approval before a script plugin or workflow runs

**Status:** Accepted

## Context

Script plugins and workflows are the way users extend Sevak, and they arrive
by being copied into a folder, synced from elsewhere or installed from the
opt-in gallery. They run with the user's full privileges. A folder appearing in
the config directory must not be enough to start running its code, and the
user has to be told what they are agreeing to in terms they can judge.

## Decision

Nothing that can run code is started until the user has said yes in a native
dialog.

- **Script plugins** (`crates/sevak-plugins/src/script/approvals.rs`,
  `src-tauri/src/script_plugins.rs`): a plugin folder is loaded only if its id
  and exact command line (mode, format and command) were approved. Changing the
  command asks again.
- **Workflows** (`crates/sevak-plugins/src/workflow/host.rs`): a workflow that
  has a node that can run code or commands (script filter, run script, launch
  app, system command, terminal command, open file) is loaded only if the
  SHA-256 of *what can run* was approved: the whole configuration of those nodes,
  the connections between nodes, the workflow's variables and the contents of
  the script files those nodes name. Layout, titles and nodes that cannot run
  code are left out, so moving a box does not ask again but editing a script
  does. The dialog describes in plain language what starts the workflow and
  what it will run.
- The answer is stored by Sevak in `script-plugin-approvals.json` in the data
  folder, not in the hand-edited `config.toml`, and is only ever written from
  the native dialog (a compromised web view cannot approve anything:
  no IPC command records an approval).
- "Not now" is remembered for the session only. A damaged approvals file
  means nothing is approved.
- Gallery installs ([ADR-0010](0010-opt-in-network-features.md)) land as
  ordinary, unapproved folders and go through the same dialog.
- Built-in features that act on the user's own typing (the `>` shell command,
  system commands) ask in their own way: destructive ones show a confirmation
  (`Plugin::confirmation`).

## Consequences

- A malicious folder cannot run silently, and a changed workflow script cannot
  run on an old approval.
- The script-plugin approval covers the command, **not the script file**: if
  the file behind an approved command is replaced, the plugin keeps running
  (the documented caveat in [Writing plugins](../plugins.md)). Workflows are
  stricter. Aligning the two is on the hardening backlog in the
  [threat model](../security/threat-model.md#hardening-backlog).
- Approval is permission to run code with the user's privileges. It is not a
  sandbox, and a plugin that is approved and then misbehaves is outside what
  Sevak can prevent.
- The user is asked at the next reload after anything changes, which can be
  after the change was made by a sync tool the user forgot about; the prompt
  names the folder so it can be recognised.
- The approval store is a plain file the user's account can write, so it is
  not a defence against software that already runs as the user.
