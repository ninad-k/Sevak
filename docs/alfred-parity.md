# Alfred 5 and 6 feature parity

This page maps the features in Alfred's 5.x and 6.x release notes
(<https://www.alfredapp.com/whats-new/> and the changelogs) to Sevak. Sevak is
its own cross-platform launcher. It borrows ideas and the Script Filter JSON
format, not Alfred's names, themes or assets.

## Added for parity

| Alfred feature | In Sevak | Where to read more |
|---|---|---|
| Alfred UI (JSON-defined windows) | **Sevak UI**: the `show_ui` workflow node, in the launcher or its own window, with templates, events and approved scripts that update the UI live | [Sevak UI](sevak-ui.md) |
| Typed query history, retention 24 h to 3 months, `!` keyword | `[search] history_retention`, `history_persist`, and the `!` history keyword | [Searching](usage.md) |
| Several keywords per feature (`\|\|`) | Any configurable keyword accepts `a\|\|b` | [Configuration](configuration.md) |
| `?` search of the preferences | `? ` searches Settings; `sevak --settings <section>` opens a section | [Settings](settings.md) |
| Usage tab, "Copy Stats" | Settings, Usage page | [Settings](settings.md) |
| Markdown tables and GitHub callouts | Safe built-in renderer in the preview, Text View and workflow text views | [Searching](usage.md) |
| "View in Alfred" Universal Action | "View in Sevak" for images, PDFs, Markdown and text | [Selection](features/selection.md) |
| Clipboard viewer item count and preview size; open from history | `[clipboard] viewer_rows`, `preview_chars`; Open link / Open / Show in folder | [Clipboard](features/clipboard.md) |
| `{snippet:}`, ISO date placeholders, clipboard modifiers | `{snippet:keyword}`, `{isodate}`, `{isotime}`, `{isodatetime}`, `{clipboard.lowercase}` | [Snippets](features/snippets.md) |
| Contact photos | vCard photos, in memory only | [Contacts](features/contacts.md) |
| Calculator Tab completion | Tab puts the bare answer back in the bar | [Calculator](features/calculator.md) |
| File search result order and limits | `[files] sort`, `max_results` | [Files](features/files.md) |
| Recent Documents | `recent` keyword on Windows, macOS and Linux | [Files](features/files.md) |
| New themes, window fade-in, separate preferences appearance | Sevak Glass Light/Dark, Sevak Violet; `open_animation`, `animation_ms`; `settings_theme` | [Themes](themes.md) |
| Workflow objects: Split Arg, Join Arg, Random, Expression, Write File, Debug, Reveal File, Call External, List Filter, File Filter, Running Apps, Dialog Conditional, Speak | Nodes of the same purpose, plus `play_sound` | [Workflows](workflows.md) |
| Date placeholders in workflows | `{date}`, `{isodate}`, `{date:%fmt}` and friends | [Workflows](workflows.md) |
| Script Filter JSON: `cache`, `loosereload`, `rerun`, `variables`, `skipknowledge`, `quicklookurl`, `text`, `match` | Supported; `strictmods` and `icon.resize` are accepted and ignored | [Workflows](workflows.md) |
| `alfred_workflow_keyword`, a cache folder apart from data | Set for scripts and workflows | [Workflows](workflows.md) |
| Workflow Configuration (text field, checkbox, popup, slider, file picker, required) | `[[config]]` in `workflow.toml`, a Configure form, values kept outside the workflow folder | [Workflows](workflows.md) |
| Delete a workflow's cache and data with it | "Delete with data" | [Workflows](workflows.md) |
| Editor: notes, disabled connections, alignment, keyboard navigation | Notes, disabled nodes and connections, align and distribute, Ctrl/Cmd+arrows, a shortcuts overlay | [Workflows](workflows.md), [Keyboard](keyboard.md) |
| Text-to-speech | `speak` node and a "Speak" Universal Action | [Workflows](workflows.md) |

Sevak already had these before this work: Universal Actions, the file buffer,
Grid and Text View, the preview pane, Large Type, snippets and their
expansion, the clipboard history, system commands, automation tasks, media
controls, contacts, 1Password, the dictionary, the gallery, themes, and
Script Filter compatibility.

## Not added

- **Music Mini Player and library browsing**: on the [roadmap](roadmap.md).
- **Web views in Sevak UI**: left out on purpose, so a UI never loads remote
  content.
- **macOS-only items**: Shortcuts runner, Passwords app search, AutoFill
  codes, Powerpack licensing and Alfred Remote.
- **Hiding individual default actions**.
- **`{var:}` in fallback search titles**: Sevak's fallbacks have no workflow
  variables.
