# Contacts

Search your address book and copy an e-mail address, write an e-mail, call a number or open the contact card. Type `c ` or `@` and part of a name, address, number or company: `c ada`, `@lovelace`, `c 555 0100`.

This plugin is **off by default**.

## Turn it on

Add to `config.toml`, then choose **Reload index** from the tray menu:

```toml
[contacts]
enabled = true
vcard_files = ["~/contacts.vcf"]   # optional: .vcf files or folders of them
```

The Settings window has no fields for this section; its plugin list can still switch the plugin off.

## How to use it

Names, e-mail addresses, phone numbers and company names are searched. Each row shows the person with their main address, number and company.

| Key | Action |
|---|---|
| ++enter++ | Copy the e-mail address (the phone number if there is no address) |
| ++ctrl+enter++ | Write an e-mail (opens your mail program) |
| ++shift+enter++ | Copy the phone number |
| ++alt+enter++ | Call the number (`tel:`, handled by whichever phone app you have) |
| ++ctrl+l++ | Show the phone number as Large Type |
| ++arrow-right++ or ++ctrl+k++ | List these and more: open the contact card (macOS), copy other addresses and numbers |

## Where the contacts come from

| | Source |
|---|---|
| Everywhere | vCard (`.vcf`) files and folders listed in `vcard_files`. Export one from your address book, Outlook, Google Contacts or a phone. No permission needed. |
| macOS | The Contacts app. The first time you type `c`, a row asks to allow access; press ++enter++ and answer macOS's question. Until you do, only vCard files are searched. |
| Windows | The People store behind the Mail and People apps. If your account's contacts are not in it, export a vCard file instead. |
| Linux | Evolution's local address books (`~/.local/share/evolution/addressbook`). Other address books: export a vCard file. |

Set `use_system = false` to read vCard files only.

## Options

| Setting | Default | What it does | Config section |
|---|---|---|---|
| Enabled | `false` | Turn the plugin on | [`[contacts] enabled`](../configuration.md#contacts) |
| Keyword | `c` | Keyword to search contacts; the `@` keyword always works too | [`[contacts] keyword`](../configuration.md#contacts) |
| Use system | `true` | Also read the system address book (macOS Contacts, Windows People, Evolution) | [`[contacts] use_system`](../configuration.md#contacts) |
| vCard files | *(empty)* | `.vcf` files or folders of them, e.g. `["~/contacts.vcf"]` | [`[contacts] vcard_files`](../configuration.md#contacts) |

## Privacy

Contacts are loaded into memory when Sevak starts and every ten minutes, never written to disk, never logged and never sent anywhere. Searches and picks in this plugin are kept out of the search history and usage statistics, so no names end up in `usage.json`.

## Troubleshooting

See [`c`, `1p`, `define` or `spell` does nothing](../troubleshooting.md#c-1p-define-or-spell-does-nothing).
