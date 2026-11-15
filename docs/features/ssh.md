# SSH hosts

Connect to a server from your SSH config without remembering its name. Type `ssh ` and part of a host alias or host name: `ssh web`, `ssh 10.0`.

The plugin is on by default and needs no setup: it lists the `Host` entries of `~/.ssh/config` (on Windows, `%USERPROFILE%\.ssh\config`, the file Windows' own OpenSSH reads).

## How to use it

Each row shows the alias and what it connects to (`deploy@web.example.com:2222`). An empty query lists every host in the order of your config file.

| Key | Action |
|---|---|
| ++enter++ | Open a terminal running `ssh <host>` |
| ++alt+enter++ | Copy the `ssh <host>` command |
| ++ctrl+enter++ | Copy the host name |
| ++ctrl+l++ | Show the command as Large Type |

The terminal is the one chosen for [terminal commands](shell.md) (`[shell] terminal`), and `ssh` runs only when you press ++enter++.

To connect with one key, bind a [hotkey](../configuration.md#hotkey) to `ssh:<alias>`:

```toml
[[hotkey]]
key = "Ctrl+Alt+S"
run = "ssh:web"
```

## What is read

- `~/.ssh/config`, and the files it pulls in with `Include` (wildcards such as `config.d/*.conf`, `~`, and paths relative to `~/.ssh`, up to 16 levels like ssh itself).
- `Host` lines name the hosts. Patterns made only of wildcards (`Host *`, `Host *.corp`), negated ones (`!bastion`) and `Match` blocks name nothing to connect to and are skipped. `HostName`, `User` and `Port` are shown; the first value wins, as in ssh.
- With `known_hosts = true`, also the hosts in `~/.ssh/known_hosts` that are not in the config already. Hashed entries (`|1|…`, the default on many Linux systems) cannot be turned back into names and are skipped.

A host name is passed to `ssh` only if it is made of letters, digits, `.`, `_` and `-`; anything else (spaces, shell characters, a leading `-`) is left out, so a crafted config line cannot make the terminal run something else.

## Privacy

- Files are read only from inside `~/.ssh`: an `Include` of a file elsewhere, or a symbolic link that leads out of `~/.ssh`, is skipped. If your config is a link into a dotfiles folder, keep the real file in `~/.ssh` (or `Include` it from there).
- Private keys and ssh's other files are never opened, even when an `Include` wildcard matches them: `id_*`, `*.pem`, `*.key`, `*.ppk`, `*.pub`, `authorized_keys*` and `known_hosts*`.
- The hosts are kept in memory only and reread when Sevak refreshes its index. The log has counts, never names. Nothing is sent anywhere.

## Configuration

```toml
[ssh]
keyword = "ssh"
known_hosts = false
```

See [`[ssh]`](../configuration.md#ssh). Turn the plugin off with `"ssh"` in `[plugins] disabled` or under **Settings → Plugins**.
