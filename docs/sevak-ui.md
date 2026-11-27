# Sevak UI

Sevak UI is a small toolkit for building interfaces inside a [workflow](workflows.md).
You describe the interface in JSON; the **Show UI** node opens it, in the
launcher panel or in a window of its own, and the workflow continues from what
the user does. Buttons can set values, copy text, send a notification, run one
of the node's scripts (which can update the UI live) or hand control back to
the workflow.

## The node

| Field | Meaning |
|---|---|
| `target` | `launcher` (replaces the results; Esc dismisses it) or `window` (a window of its own) |
| `definition` | the UI as inline JSON |
| `file` | or a JSON file in the workflow folder |
| `scripts` | `{ "<id>": { "language": "shell\|python\|javascript\|powershell", "script": "..." } }`, the scripts `run` actions may start |
| `ports` | output names a `file` definition uses (an inline definition is read for them) |
| `titlebar`, `always_on_top` | window only |

**Outputs.** The node has one output per name used by an `output` action in the
definition, plus `closed`, which fires when the UI is dismissed (Esc, the
window's close button) or a `close` action runs. With a `file`, list the names in `ports` because the
builder does not read files.

The next nodes receive, as `{query}`: the `arg` of the `output` action, or, when
it has none, the JSON of the UI's state. After `closed` they receive the
argument the node itself got.

While the UI is open the workflow waits. The 15 minute run limit does not count
that time: it pauses while a UI is open and only covers the workflow's own work.

## The definition

```json
{
  "title": "Greeter",
  "width": 420,
  "height": 300,
  "state": { "name": "" },
  "templates": { "field": { "type": "textfield", "id": "{param:id}" } },
  "root": { "type": "column", "children": [ ... ] }
}
```

Limits: 1 MB in all (included files count), at most 2000 components, depth 32,
ids unique and made of letters, digits, `-` and `_`. Unknown types or fields,
missing required properties and `run` actions naming undeclared scripts are
errors, shown like any node error. The builder's **Validate** button runs the
same checks.

### Components

| `type` | Properties |
|---|---|
| `column`, `row`, `grid` | `children`, `gap`, `padding`, `align` (`start`, `center`, `end`, `stretch`); `grid` also `columns` (1 to 12) |
| `label` | `text`, `style` (`heading`, `body`, `caption`, `mono`) |
| `markdown` | `text`, drawn by Sevak's safe Markdown renderer |
| `button` | `label`, `style` (`primary`, `secondary`, `danger`), `disabled`, optional `id`, `events.click` |
| `checkbox` | `id`, `label` |
| `textfield`, `passwordfield` | `id`, `placeholder` |
| `textarea` | `id`, `placeholder`, `rows` |
| `picker` | `id`, `options: [{label, value}]` |
| `slider` | `id`, `min`, `max`, `step` |
| `progress` | `value` (0 to 1, or a template like `{state:p}`) or `indeterminate: true` |
| `image` | `path`, `alt` |
| `list` | `id`, `items: [{title, subtitle, arg}]`, `filter_by` (id of a textfield that filters the rows fuzzily), `multi` |
| `separator`, `spacer` | none |
| `use` | `template`, `params`; see below |

Inputs keep their value in the UI's **state** under their `id`: text for fields
and pickers, `true`/`false` for a checkbox, a number for a slider, the selected
`arg` (or title) for a list, an array of them when `multi`.

**Images** are files: a path relative to the workflow folder, or absolute. Sevak
loads them (png, jpeg, gif, webp, at most 4 MB, and the bytes must really be
that format) and gives the renderer `data:` URLs. Remote URLs and `data:` URLs
in the definition are refused.

### Placeholders

In every text property: `{state:id}`, `{var:name}` (the workflow's variables),
`{query}` (the node's argument), also mixed: `"{var:a} is {state:b}"`. The initial values in
`state` may use `{query}` and `{var:name}` too.

### Templates and includes

`templates` are named components. `{"type": "use", "template": "field", "params": {"id": "email"}}`
copies the template and replaces `{param:id}` in its strings. A string that is
only `{param:x}` takes the parameter's own JSON value (a number or a sub-tree).
Templates may use other templates; recursion and runaway expansion (too many
components, or more than 4 MB copied) are refused.

A string value that is exactly `{file:parts/row.json}` is replaced by that
JSON file's content, which may include others. The path must stay inside the
workflow folder (no `..`, no absolute path, links pointing out are refused).

### Events

`events` maps `click` (buttons), `change` (inputs), `submit` (Enter in a
textfield) and `select` (a list row) to an action or a list of actions, run in
order. Each action does exactly one thing:

| Action | Does |
|---|---|
| `{"set": {"id": "template"}}` | sets state values (text values are templates) |
| `{"output": "port"}` / `{"output": "port", "arg": "template"}` | ends the UI and continues the workflow from that output |
| `{"close": true}` | ends the UI; the workflow continues from `closed` |
| `{"run": "script id"}` | runs one of the node's scripts and waits for it |
| `{"notify": "template"}` | shows a notification |
| `{"copy": "template"}` | copies text to the clipboard |

### Scripts

A script gets one JSON document on standard input:
`{"event": "click", "component": "<id>", "state": {...}, "vars": {...}}`.
It runs like a Run Script node: same environment, 10 second limit, the
workflow folder as working directory. Its standard output may be JSON with any of:

* `{"set": {"id": value}}` updates state (the UI redraws)
* `{"root": { ...component... }}` replaces the whole UI (templates of the definition still work)
* `{"notify": "text"}` shows a notification
* `{"close": true}` closes the UI

Nothing printed is fine. Anything printed that is not such JSON, a non-zero exit or a
timeout shows the error in the UI, which stays open.

Languages: `shell` (sh; not on Windows), `python`, `javascript` (Node), `powershell`
(pwsh). The interpreter must be on `PATH`.

## Security

* A UI **without scripts** runs no code: it needs no approval.
* A UI **with scripts** needs your Allow like any code-running node. The Allow
  covers the scripts' text, the UI's file and every file it includes; changing
  any of them asks again. The approval text lists each script.
* Only the node's own declared scripts can ever run; the UI cannot name a
  program, a path or a command line.
* The renderer never uses raw HTML except for Markdown, which goes through
  Sevak's sanitising renderer (links open in your browser, after a click).
* Images are `data:` URLs made by Sevak from local files, never remote.
* **Web views are intentionally not supported.** There is no component that loads a page,
  no navigation, no remote content, so a UI definition can neither load code from the network nor
  phone home.
* A UI window has no plugin permissions at all (`capabilities/sevak-ui.json`);
  it can only call Sevak's own `sevak_ui_session` and `sevak_ui_event` commands,
  and the shell closes it. The launcher target uses the launcher's existing permissions.
* What the user types in a UI is held in memory, sent only to the workflow and its scripts,
  and never logged.

## Example

[`examples/workflows/text-transformer`](https://github.com/ninad-k/Sevak/tree/main/examples/workflows/text-transformer)
types `tx` in the launcher and opens a text box, a picker (uppercase, lowercase,
title case, slug) and buttons. **Transform** runs a Python script that reads
the state and answers `{"set": {"result": ...}}`; the result label shows it.
**Copy** copies it, **Done** continues the workflow with the result, which the
next node copies.
