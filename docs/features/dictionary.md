# Dictionary and spelling

Look up a word's meanings or fix its spelling, offline. Type `define serendipity` or `spell recieve`.

## How to use it

### Define

`define serendipity` shows the meanings of a word, one row each with its part of speech.

| Key | Action |
|---|---|
| ++enter++ | Copy the definition |
| ++shift+enter++ | Copy the definition with the word |
| ++ctrl+l++ | Show it as Large Type |
| ++tab++ on a "Did you mean" row | Look up that word instead |

For a word that is not found, "Did you mean" rows offer close words. Inflected forms are explained through their base word (`running` shows `run`).

### Spell

`spell recieve` lists corrections, best first.

| Key | Action |
|---|---|
| ++enter++ | **Paste** the right spelling into the app you were using (copies where pasting is not possible) |
| ++ctrl+enter++ | Copy it |

Typing a whole sentence checks it and corrects the first wrong word. Capitalization is kept: `Recieve` gives `Receive`.

## Where the words come from

Everything is offline. macOS uses its own Dictionary for `define`, Windows its spell checker for `spell`, and otherwise Sevak uses a bundled English dictionary (WordNet, about 3 MB in the program). On Linux the word lists in `/usr/share/hunspell` also count as correct spellings.

## Options

All of these are in **Settings → Integrations**.

| Setting | Default | What it does | Config section |
|---|---|---|---|
| Define keyword | `define` | Keyword for definitions | [`[dictionary] define_keyword`](../configuration.md#dictionary) |
| Spell keyword | `spell` | Keyword for spelling | [`[dictionary] spell_keyword`](../configuration.md#dictionary) |
| Use system | `true` | Prefer the OS dictionary and spell checker; `false` always uses the bundled data | [`[dictionary] use_system`](../configuration.md#dictionary) |

The plugin is on by default. Turn it off by adding `"dict"` to [`[plugins] disabled`](../configuration.md#plugins).

## Privacy

The words you look up are not saved in the search history, and nothing is sent anywhere. The bundled dictionary's licence is in [THIRD_PARTY_NOTICES.md](https://github.com/ninad-k/Sevak/blob/main/THIRD_PARTY_NOTICES.md).

## Troubleshooting

See [`c`, `1p`, `define` or `spell` does nothing](../troubleshooting.md#c-1p-define-or-spell-does-nothing).
