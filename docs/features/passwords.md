# Password generator

Make strong random passwords and passphrases, offline. Type `pass`, `pass 24` or `pass 6 words`.

## How to use it

| You type | You get |
|---|---|
| `pass` | A 20-character password, one with letters and digits only, and a six-word passphrase |
| `pass 24` | 24-character passwords, with and without symbols |
| `pass 6 words`, `pass words` | Three passphrases, such as `maple-otter-lantern-quiet-harbor-zest` |
| `pass 16 nosymbols`, `pass 16 no symbols` | Passwords of letters and digits only |
| `pass symbols` | Passwords with symbols, even when `[passwords] symbols` is off |
| `pass pin`, `pass pin 8` | Numbers of six digits, or as many as you ask for |

Each row shows how strong the secret is, in **bits of entropy**: the number of guesses an attacker who knows exactly how it was made would need, as a power of two. 80 bits or more is out of reach for guessing; the default password has about 125 and the default passphrase about 66.

++enter++ copies the highlighted one. It is marked as a secret on the clipboard, so clipboard histories leave it out: Sevak's own [clipboard history](clipboard.md), Windows' `Win+V` history and cloud clipboard, and macOS and KDE clipboard managers that honour the marker.

## How passwords are made

- Every character and word comes from the operating system's secure random number generator, drawn so that each choice is equally likely.
- Passwords use lowercase and uppercase letters, digits and (unless you turn them off) the symbols `!#$%&*+-.:=?@^_~`, which forms and shells accept without quoting. Every password has at least one of each kind. The strength shown takes that rule into account.
- Passphrases use Sevak's own list of 2,157 common English words of three to eight letters, joined by `-`. Each word adds about 11 bits.

## Options

These are in **Settings → Integrations**.

| Setting | Default | What it does | Config section |
|---|---|---|---|
| Keyword | `pass` | Keyword for the generator | [`[passwords] keyword`](../configuration.md#passwords) |
| Password length | `20` | Characters in a password (4-128) | [`[passwords] length`](../configuration.md#passwords) |
| Passphrase words | `6` | Words in a passphrase (2-20) | [`[passwords] words`](../configuration.md#passwords) |
| Include symbols | on | Add symbols to passwords | [`[passwords] symbols`](../configuration.md#passwords) |

The plugin is on by default. Turn it off by adding `"pwgen"` to [`[plugins] disabled`](../configuration.md#plugins).

## Privacy

Generated passwords are made on your computer and never sent, saved or logged. Neither they nor what you typed to make them are recorded in the usage statistics or the search history.
