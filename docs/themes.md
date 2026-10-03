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
custom_css = "theme.css"   # a stylesheet in the config folder; "" loads none
```

Changes apply when you save in Settings, or after "Reload index" in the tray
menu when you edit the file by hand.

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

Set from the appearance settings (override them in your file if you prefer):

| Variable | Default | From |
|---|---|---|
| `--font-size` | `15px` | `font_size`: result title size |
| `--font-scale` | `1` | `font_size` / 15: multiplies the other text sizes and row height |
| `--radius` | `14px` | `radius`: corner radius of the search bar |
| `--card-opacity` | `1` | `opacity` / 100: opacity of the bar's background layer |

`font-family` is set on `:root` directly.

These names are the stable interface for themes. The class names inside the
page are not and may change between versions.
