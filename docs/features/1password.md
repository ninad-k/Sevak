# 1Password

Find your 1Password logins by title or website and open the site or the item: `1p github`. Sevak uses the official [1Password command-line tool (`op`)](https://developer.1password.com/docs/cli/get-started/) and **never reads a password, one-time code, note or any other secret**.

This plugin is **off by default**.

!!! note "Trademark"
    1Password is a trademark of AgileBits Inc. Sevak is not affiliated with or
    endorsed by 1Password; it only runs the official `op` tool you install.

## Turn it on

1. Install the 1Password CLI (`op`).
2. In the 1Password app, turn on *Settings → Developer → Integrate with 1Password CLI*.
3. Open **Settings → Integrations**, switch on **Search 1Password logins** and press **Save**. Or add to `config.toml`, then choose **Reload index**:

```toml
[onepassword]
enabled = true
```

## How to use it

Type `1p` followed by a space and part of a login's title or website.

| Key | Action |
|---|---|
| ++enter++ | Open the login's website in your browser (opens the item in 1Password if it has no website) |
| ++ctrl+enter++ | Open the item in the 1Password app |
| ++shift+enter++ | Copy the username, when 1Password lists one |
| ++arrow-right++ or ++ctrl+k++ | Also: copy the website address |

`op` runs only when you type `1p` followed by a space, never at startup or while you type a normal search. The first time (and again after `cache_minutes`, default 10) 1Password may ask you to unlock with Touch ID, Windows Hello or your system password; the row says "Asking 1Password…" until it answers. If you dismiss the prompt, Sevak does not ask again by itself: press ++enter++ on the "Try again" row.

## Options

| Setting | Default | What it does | Config section |
|---|---|---|---|
| Enabled | `false` | Turn the plugin on | [`[onepassword] enabled`](../configuration.md#onepassword) |
| Keyword | `1p` | Keyword to search logins | [`[onepassword] keyword`](../configuration.md#onepassword) |
| `op` path | `""` | Path to `op`; empty searches `PATH` and the usual install folders | [`[onepassword] op_path`](../configuration.md#onepassword) |
| Account | `""` | Which account when several are signed in (address, short name or ID); empty uses `op`'s default | [`[onepassword] account`](../configuration.md#onepassword) |
| Cache | `10` | Minutes the list of logins is kept in memory (1–1440) | [`[onepassword] cache_minutes`](../configuration.md#onepassword) |

## Privacy

Sevak asks `op` only for the list of logins (`op item list --categories Login`), which contains titles, vault names, website addresses and usernames, and keeps that list in memory. To fill in a password, open the item in 1Password. What you search for and pick here is kept out of the search history and usage statistics. `op` is 1Password's own program and talks to 1Password as it normally does; Sevak itself sends nothing.

## Troubleshooting

See [`c`, `1p`, `define` or `spell` does nothing](../troubleshooting.md#c-1p-define-or-spell-does-nothing).
