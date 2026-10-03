# Snippets

Paste text you define: boilerplate, email signatures, code templates, or anything you type often. Type `s <name>` to find a snippet and paste it with placeholders filled in.

## How to use it

Define snippets in your `config.toml`:

```toml
[[snippet]]
name = "Email signature"
keyword = "sig"
text = "Best regards,\nNinad\n{date}"

[[snippet]]
name = "License header"
keyword = "lic"
text = "# Apache License 2.0 - https://example.com"

[[snippet]]
name = "Meeting notes"
text = """
## Attendees
- 

## Agenda
- 

## Action items
- 
"""
```

Then search by name or keyword:

| Type | Shown | Press ++enter++ |
|---|---|---|
| `s email` | "Email signature" (matches name) | Pastes the text with `{date}` replaced by today |
| `s sig` | "Email signature" (matches keyword) | Pastes the text |
| `s lic` | "License header" (exact keyword match ranked highest) | Pastes the text |
| `s meet` | "Meeting notes" (fuzzy match on name) | Pastes the text (no placeholders to fill) |

When you press ++enter++, Sevak hides, brings back the app that was in focus, and pastes the snippet (unless [pasting is unavailable](#pasting-platform-notes)).

### Keyword matching

A `keyword` is optional and gives the snippet an alternative search term:

- Type `s signature` → finds "Email signature" by name.
- Type `s sig` → finds "Email signature" by keyword (and ranks higher).
- Type `s email sig` → finds "Email signature" because it matches either term.

An exact match on a keyword outranks any fuzzy match on the name, so `s sig` shows "Email signature" first even if you have other snippets with "sig" in the name.

## Placeholders

Placeholders are **filled in when you press ++enter++**, not while you type. This means `{time}` gives the time of the paste, and `{clipboard}` reads the clipboard only then.

| Placeholder | Result | Example |
|---|---|---|
| `{date}` | Today in `YYYY-MM-DD` format | `2026-10-03` |
| `{time}` | Now in `HH:MM` format (24-hour) | `14:05` |
| `{datetime}` | Today and now together | `2026-10-03 14:05` |
| `{date:FORMAT}` | Today in [strftime](https://docs.rs/chrono/latest/chrono/format/strftime/index.html) format | `{date:%d %B %Y}` → `03 October 2026` |
| `{time:FORMAT}` | Now in strftime format | `{time:%H:%M:%S}` → `14:05:23` |
| `{datetime:FORMAT}` | Now in strftime format | `{datetime:%Y-%m-%d at %H:%M}` → `2026-10-03 at 14:05` |
| `{clipboard}` | The text on the clipboard when you pressed ++enter++ | *(whatever you had copied)* |
| `{uuid}` | A fresh random UUID v4 | `550e8400-e29b-41d4-a716-446655440000` |
| `{{` | A literal `{` | `{{key}}` → `{key}` |
| `}}` | A literal `}` | *(same)* |

Anything else in braces (`{unknown}`, `{}`, a lone `{`) is left exactly as written, so code snippets with braces usually just work:

```toml
[[snippet]]
name = "Python lambda"
text = "lambda x: {x * 2}"  # The {x * 2} is left as-is
```

### Common formats

A few handy strftime formats for custom dates and times:

```toml
[[snippet]]
name = "ISO week date"
text = "{date:%G-W%V}"                      # 2026-W40

[[snippet]]
name = "US date"
text = "{date:%m/%d/%Y}"                    # 10/03/2026

[[snippet]]
name = "File timestamp"
text = "backup_{datetime:%Y%m%d_%H%M%S}"    # backup_20261003_140523

[[snippet]]
name = "Markdown heading + date"
text = "## {date:%A, %B %-d, %Y}"           # ## Thursday, October 3, 2026
```

## Actions

- **++enter++**: Paste the selected snippet into the app you were in.
- **++ctrl+k++** (action panel): See all available actions for the selected snippet (typically just paste and copy).
- **++ctrl+c++**: Copy the selected snippet's text to the clipboard without pasting into the previous app. Placeholders are not filled.

## Options

| Setting | Default | What it does | Config section |
|---|---|---|---|
| Snippets | *(empty)* | List of [`[[snippet]]`](../configuration.md#snippet) entries with name, optional keyword, and text | [`[[snippet]]`](../configuration.md#snippet) |
| Restore clipboard | `false` | Put back the clipboard's previous text after pasting (also affects clipboard history) | [`[paste] restore_clipboard`](../configuration.md#paste) |

## Pasting: platform notes

=== "Windows"
    Pasting works in most applications. **Exception**: applications running with elevated privileges (Admin, UAC elevation) cannot receive simulated keypresses, so Sevak copies instead and the row says so.

=== "macOS"
    Requires *System Settings → Privacy & Security → Accessibility → Sevak* to paste. Without this permission, Sevak copies instead and the row shows "Copies to clipboard". Keyboard layouts that move the ++v++ key (e.g. Dvorak) also prevent pasting; use copy instead.

=== "Linux (X11)"
    Pasting works in most applications.

=== "Linux (Wayland)"
    Applications cannot simulate keypresses, so Sevak can only copy. The row shows "Copies to clipboard".

## Editing snippets

The **Settings** window does not let you edit snippets (to keep hand-written entries untouched, with proper formatting and comments). Edit them by opening `config.toml` directly:

1. Choose **Settings → Open config file** (or find it in [Files and data](../files-and-data.md)).
2. Edit the [`[[snippet]]`](../configuration.md#snippet) sections.
3. Save the file and choose **Reload index** from Sevak's tray or restart.

Each [`[[snippet]]`](../configuration.md#snippet) must have a `name` and `text`; the `keyword` is optional. Names do not have to be unique (if you have duplicates, Sevak adds a number like "Email #2" to each).

## Tips and troubleshooting

!!! tip "Shared snippets"
    Store `config.toml` in a synced folder (Dropbox, OneDrive, iCloud Drive, git) with `SEVAK_CONFIG_DIR=~/Dropbox/sevak`. The [`[[snippet]]`](../configuration.md#snippet) entries sync across devices. See [Configuration reference](../configuration.md).

!!! tip "Long multiline snippets"
    Use triple-quoted TOML strings to avoid escaping newlines:
    ```toml
    [[snippet]]
    name = "Meeting notes"
    text = """
    ## Attendees
    - 
    
    ## Agenda
    - 
    """
    ```

!!! tip "Generate UUIDs"
    Each time you paste a snippet with `{uuid}`, a fresh UUID is generated. Use it in multiple places in a snippet to get different UUIDs:
    ```toml
    [[snippet]]
    name = "Pair of UUIDs"
    text = "{uuid}\n{uuid}"
    ```

!!! warning "Escaping braces in JSON or code"
    Snippets containing braces (JSON, programming languages) can conflict with placeholders. Use `{{` and `}}` to escape them:
    ```toml
    [[snippet]]
    name = "JSON template"
    text = "{{\"user\": \"{clipboard}\"}}"
    # Pastes: {"user": "..."}
    ```
