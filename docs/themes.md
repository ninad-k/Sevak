# Themes and appearance

Sevak follows your system's light or dark mode by default. Everything else is
optional and lives in the `[appearance]` table of `config.toml` (the Settings
window, Appearance page, edits the same values).

```toml
[appearance]
theme = "system"       # "system", "light" or "dark"
accent = "#7c3aed"     # "#rrggbb", "#rgb" or "rgb(r, g, b)"; "" keeps the theme's
font_size = 15         # 12-22, pixels; the size of result titles
font_family = "Fira Sans, sans-serif"   # "" uses the system font
opacity = 100          # 30-100, percent opacity of the search bar's background
radius = 14            # 0-32, corner radius in pixels
theme_file = "themes/Nord.toml"   # a theme file in the config folder; "" uses none
custom_css = "theme.css"   # a stylesheet in the config folder; "" loads none
```

Changes apply when you save in Settings, or after "Reload index" in the tray
menu when you edit the file by hand.

There are four layers, each overriding the one before: the built-in light or
dark look, a [theme file](#theme-files-and-the-editor) (`theme_file`), the
settings above, and your [custom stylesheet](#custom-stylesheet).

## Validation

Every value is checked when it is applied. A value that is not valid is ignored
(that one setting falls back to its default, the rest still apply), a warning is
written to the log and the warning is shown under Settings, Appearance:

| Setting | Valid | Default |
|---|---|---|
| `accent` | `#rgb`, `#rrggbb`, `rgb(r, g, b)` (0-255) or empty | empty (the theme's accent) |
| `font_size` | whole number 12-22 | 15 |
| `font_family` | names made of letters, digits, spaces and `- _ .`, separated by commas (quotes optional), up to 200 characters | empty (system font) |
| `opacity` | whole number 30-100 | 100 |
| `radius` | whole number 0-32 | 14 |
| `theme_file` | a relative path inside the config folder to a valid theme file | empty |
| `custom_css` | a relative path inside the config folder | empty |

Setting `accent` also derives the related variables: `--accent-strong` (18 %
darker), `--selected` (the accent at 17 % opacity) and `--on-accent` (black or
white, whichever reads better on the accent).

`opacity` fades only the search bar's background; text stays fully opaque. It
needs a window that can be transparent, which Sevak already uses for its rounded
corners and shadow. There is no background blur: the operating system's blur
effects (Acrylic, Mica, vibrancy) would also blur the transparent margin around
the bar, which shows as a square frame, so Sevak does not offer it.

`font_size` scales the whole search bar (input, subtitles, row height), not only
the titles. It does not change the Settings window.

## Theme files and the editor

A theme is a small TOML file in the `themes` folder of your config folder
(Settings, Appearance, **Open themes folder**). Settings, Appearance,
**Theme editor** creates and edits them without touching the file:

- Eight themes ship with Sevak: Sevak Light, Sevak Dark, Nord, Dracula,
  Solarized Light, Solarized Dark, Gruvbox and High Contrast. Click one to
  preview it. Changing anything in a built-in makes a copy, because the
  built-in names stay as shipped.
- The preview is the launcher's own result row with sample rows, and updates as
  you edit. **Light / Dark** switches between a theme's two variants.
- Each color has a picker, a text field (`#rrggbb`, `#rrggbbaa`, `rgb()` or
  `rgba()`) and an opacity slider. Sliders set the font size, corner radius,
  background opacity, row height, search field size, icon size and window width;
  **Default** takes a size back out of the theme. The font is a free-text list;
  the **Installed fonts** button fills the suggestions from the system when the
  window allows it.
- **Contrast** shows the WCAG contrast ratio of the text on the background, of
  the selected row's text on the selection, of the subtext on the background and
  of the text on accent buttons. A pair under 4.5:1 (level AA) is flagged. It is a
  warning, not a block; every built-in theme passes all four.
- **Undo**, **Redo** and **Reset** step through your edits.
- **Save as...** writes `themes/<name>.toml`. **Apply** saves it if needed and sets
  `theme_file`; press **Save** in Settings to keep that choice.
  **Import...** and **Export...** copy a theme file from and to anywhere on disk
  (an imported file is validated and rewritten in canonical form).

A theme file:

```toml
name = "Nord"
author = "Sevak"
description = "Arctic dark colors."

[font]
family = "Fira Sans, sans-serif"   # optional; empty is the system font
size = 15                          # 12-22

[layout]
radius = 14          # 0-32
opacity = 100        # 30-100
row_height = 48      # 32-96, px
search_size = 22     # 14-40, px, the text you type
icon_size = 32       # 16-64, px
window_width = 720   # 400-1600, px

[dark]               # and/or [light]
background = "#2e3440"
text = "#eceff4"
subtext = "#bcc7d8"
border = "rgba(236, 239, 244, 0.12)"
accent = "#88c0d0"
accent_strong = "#709dab"
on_accent = "#2e3440"
selection = "rgba(136, 192, 208, 0.2)"
# selection_text = "#eceff4"   # optional: the selected row's title; default: text
tile = "rgba(236, 239, 244, 0.08)"
kbd_background = "rgba(236, 239, 244, 0.08)"
kbd_border = "rgba(236, 239, 244, 0.16)"
shadow = "0 8px 28px rgba(0, 0, 0, 0.55), 0 1px 3px rgba(0, 0, 0, 0.4)"
surface = "rgba(236, 239, 244, 0.05)"
input_background = "rgba(236, 239, 244, 0.06)"
input_border = "rgba(236, 239, 244, 0.24)"
switch_off = "rgba(236, 239, 244, 0.28)"
warn = "#ebcb8b"
error = "#e5848d"
ok = "#a3be8c"
```

| Key | CSS variable | Used for |
|---|---|---|
| `background` | `--bg` | Search bar and Settings background |
| `text` | `--fg` | Text |
| `subtext` | `--muted` | Subtitles, hints, placeholder |
| `border` | `--border` | Hairlines |
| `accent`, `accent_strong`, `on_accent` | `--accent`, `--accent-strong`, `--on-accent` | Caret, buttons, hover shade, text on accent |
| `selection` | `--selected` | Background of the selected row |
| `selection_text` | `--selected-fg` | Title of the selected row (default: `text`) |
| `tile`, `kbd_background`, `kbd_border` | `--tile`, `--kbd-bg`, `--kbd-border` | Icon tiles and key caps |
| `shadow` | `--shadow` | The bar's drop shadow |
| `surface`, `input_background`, `input_border`, `switch_off` | `--surface`, `--input-bg`, `--input-border`, `--switch-off` | Settings window |
| `warn`, `error`, `ok` | `--warn`, `--error`, `--ok` | Notices |

How a theme file is read:

- **Every value is checked.** A value that is not valid (a color that does not
  parse, a number out of range, a font name with odd characters, a shadow that is
  not a plain `box-shadow` list, an unknown key) is ignored with a warning shown
  under Settings, Appearance, and the rest of the file still applies. Only a
  file that is not TOML at all is refused. The file must be UTF-8 and at most
  64 KiB, and its path follows the same rules as `custom_css`.
- **One palette or two.** A theme with only `[dark]` or only `[light]` uses it in
  every mode (the `theme` setting does not switch it). With both, the light one
  is used in light mode and the dark one in dark mode. Colors a palette leaves
  out keep the Sevak Light or Sevak Dark value of that mode.
- **Your settings still win.** `font_size`, `font_family`, `opacity` and `radius`
  from the `[appearance]` table override the theme's values when they differ
  from their default (15, empty, 100, 14), and a non-empty `accent` replaces the
  theme's accent and the colors derived from it. The theme's `window_width` is
  used while `[window] width` is the default 720.
- **`custom_css` comes last**, so it can still override anything.

## Theme gallery

The gallery is a list of community themes kept in this repository
(`gallery/themes.json`, with the files in `gallery/themes/`). It is opt-in:

- Nothing is requested until you click **Browse online themes**. That one click
  makes a single `GET` of
  `https://raw.githubusercontent.com/ninad-k/Sevak/main/gallery/themes.json`
  (no cookies, no query, no identifying headers; 15-second timeout; 256 KiB
  limit). The list is only listed, not installed.
- **Install** downloads that theme's file (https only, 64 KiB limit) and saves it
  to your themes folder only if its SHA-256 matches the `sha256` in the list. A
  mismatch, a file that is not a valid theme, or a download that is too large is
  refused and nothing is written. The saved file is the validated theme in
  canonical form.
- Nothing is sent about you, and nothing is downloaded or updated in the
  background. Without a network you can still use the built-in themes and
  **Import...**.

To share a theme, put the file in `gallery/themes/`, add an entry to
`gallery/themes.json` and open a pull request; see
[gallery/README.md](../gallery/README.md).

## Custom stylesheet

`custom_css` names a file inside the config folder, for example `theme.css`
next to `config.toml` (or `themes/dark-purple.css`). It is a plain stylesheet
that is loaded after Sevak's built-in theme and after the settings above, so it
overrides all of them.

Rules:

- The path must be relative and stay inside the config folder: no absolute
  paths, no `..`, and a symlink that points outside the folder is refused.
- The file must be UTF-8 and at most 64 KiB; otherwise it is ignored with a
  warning.
- It is read again whenever the config is reloaded or saved.
- It cannot load anything from the network: Sevak's content security policy
  blocks remote stylesheets, fonts and images (this keeps the
  [privacy promise](../README.md#privacy-and-updates)). Use fonts installed on your system
  and `data:` URLs.
- If you keep the config folder in a synced folder
  ([Config location](configuration.md#keep-the-config-in-a-synced-folder)), your theme travels with it.

A theme mostly sets the variables below on `:root`:

```css
/* theme.css: a purple dark theme */
:root {
  --bg: #1b1030;
  --fg: #f1e9ff;
  --muted: #a99bc8;
  --accent: #a78bfa;
  --accent-strong: #8b5cf6;
  --on-accent: #1b1030;
  --selected: rgba(167, 139, 250, 0.2);
}
```

The built-in light and dark values are applied with the same specificity as a
plain `:root` rule, and your file comes later, so `:root { ... }` is enough. To
change a variable only in one mode, scope it:

```css
:root[data-theme="dark"] { --bg: #101010; }            /* theme = "dark" */
@media (prefers-color-scheme: dark) {                  /* theme = "system" */
  :root:not([data-theme]) { --bg: #101010; }
}
```

## CSS variables

Set by the light and dark themes (`ui/src/app.css`):

| Variable | Used for |
|---|---|
| `--bg` | Search bar and Settings background |
| `--fg` | Text |
| `--muted` | Secondary text: subtitles, hints, placeholder |
| `--border` | Hairlines between sections |
| `--accent` | Caret, buttons, switches, slider thumbs |
| `--accent-strong` | Hover and focus accent, slider thumbs |
| `--on-accent` | Text on an accent background |
| `--selected` | Background of the selected result row |
| `--tile` | Background behind result icons |
| `--kbd-bg`, `--kbd-border` | Key-cap hints such as the `Ctrl+1` labels |
| `--shadow` | The search bar's drop shadow (a `box-shadow` value) |
| `--warn`, `--error`, `--ok` | Notices, error messages and success marks |
| `--surface` | Panels in the Settings window |
| `--input-bg`, `--input-border` | Form controls in Settings |
| `--switch-off` | Toggle tracks and slider tracks in Settings |
| `--selected-fg` | Title of the selected result; when unset it follows `--fg` |

Set from the appearance settings (override them in your file if you prefer):

| Variable | Default | From |
|---|---|---|
| `--font-size` | `15px` | `font_size`: result title size |
| `--font-scale` | `1` | `font_size` / 15: multiplies the other text sizes and row height |
| `--radius` | `14px` | `radius`: corner radius of the search bar |
| `--card-opacity` | `1` | `opacity` / 100: opacity of the bar's background layer |
| `--row-h` | `max(48px, 48px * scale)` | Height of a result row; a theme's `row_height` |
| `--search-size` | `22px * scale` | Size of the text you type; the bar's height follows it; a theme's `search_size` |
| `--icon-size` | `32px` | Size of the result icons; a theme's `icon_size` |

`font-family` is set on `:root` directly.

These names are the stable interface for themes. The class names inside the
page are not and may change between versions.
