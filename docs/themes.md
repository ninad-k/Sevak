# Themes and appearance

Sevak follows your system's light or dark mode by default. Everything else is optional and lives in the `[appearance]` table of `config.toml` (the Settings window, Appearance page, edits the same values).

## Settings

```toml
[appearance]
theme = "system"       # "system", "light" or "dark"
accent = ""            # "#rgb", "#rrggbb", "rgb(r, g, b)" or ""; "" uses the theme's accent
font_size = 15         # 12–22 pixels; the size of result titles
font_family = ""       # "Fira Sans, sans-serif"; "" uses the system font
opacity = 100          # 30–100, percent opacity of the search bar's background
radius = 14            # 0–32, corner radius in pixels
custom_css = ""        # a stylesheet in the config folder; "" loads none
```

Changes apply when you save in **Settings**, or after "Reload index" in the tray menu when you edit the file by hand.

## Configuration through Settings

All these options are available in **Settings → Appearance**. The GUI guides you with hints for each setting.

### Validation

Every value is checked when it is applied. A value that is not valid is ignored (that one setting falls back to its default), a warning is written to the log and shown under **Settings → Appearance**:

| Setting | Valid range | Default |
|---|---|---|
| `theme` | `"system"`, `"light"` or `"dark"` | `"system"` |
| `accent` | `#rgb`, `#rrggbb`, `rgb(r, g, b)` or empty | empty (the theme's accent) |
| `font_size` | whole number 12–22 | 15 |
| `font_family` | names made of letters, digits, spaces and `- _ .`, separated by commas; up to 200 characters | empty (system font) |
| `opacity` | whole number 30–100 | 100 |
| `radius` | whole number 0–32 | 14 |
| `custom_css` | relative path inside the config folder | empty |

## Accent color

Setting `accent` derives related CSS variables:

- `--accent-strong`: 18% darker (for hover states)
- `--selected`: accent at 17% opacity (background of selected result)
- `--on-accent`: black or white, whichever reads better on the accent

## Opacity and styling

`opacity` fades only the search bar's background; text stays fully opaque. It requires a window that supports transparency, which Sevak uses for rounded corners and shadow anyway.

Sevak does not offer background blur (Acrylic, Mica, vibrancy) because it would blur the transparent margin around the bar, creating a square frame effect.

`font_size` scales the whole search bar (input, subtitles, row height), not only the result titles. It does not affect the Settings window.

`radius` sets corner rounding. `0` = sharp corners; `32` = very rounded.

## Custom stylesheet

`custom_css` names a file inside the config folder, for example `theme.css` next to `config.toml` (or `themes/dark-purple.css`). It is a plain CSS stylesheet loaded after Sevak's built-in theme and appearance settings, so it overrides all of them.

### Rules for custom stylesheets

- The path must be relative and stay inside the config folder: no absolute paths, no `..`, and a symlink pointing outside is refused
- The file must be UTF-8 and at most 64 KiB; otherwise it is ignored with a warning
- It is read again whenever the config is reloaded (tray menu) or saved (Settings)
- It cannot load anything from the network: Sevak's content security policy blocks remote stylesheets, fonts and images. Use fonts installed on your system and `data:` URLs
- If you keep the config folder in a synced folder, your theme travels with it

### Example theme

```css
/* theme.css: purple dark theme */
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

### Scoping to light or dark mode

The built-in light and dark values are applied with the same specificity as a plain `:root` rule, and your file comes later, so `:root { ... }` overrides everything.

To change a variable only in one mode:

```css
:root[data-theme="dark"] { --bg: #0a0a0a; }        /* when theme = "dark" */

:root[data-theme="light"] { --bg: #fafafa; }       /* when theme = "light" */

@media (prefers-color-scheme: dark) {              /* when theme = "system" and OS is dark */
  :root:not([data-theme]) { --bg: #0a0a0a; }
}

@media (prefers-color-scheme: light) {             /* when theme = "system" and OS is light */
  :root:not([data-theme]) { --bg: #fafafa; }
}
```

## CSS variables

Set by the light and dark themes (`ui/src/app.css`):

### Appearance

| Variable | Used for |
|---|---|
| `--bg` | Search bar and Settings window background |
| `--fg` | Main text color |
| `--muted` | Secondary text: subtitles, hints, placeholder |
| `--border` | Hairlines between sections and form fields |
| `--accent` | Caret, buttons, switches, slider thumbs, focus rings |
| `--accent-strong` | Hover and focus states of accented elements |
| `--on-accent` | Text and icons on an accent background |
| `--selected` | Background of the selected result row |
| `--tile` | Background behind result icons |
| `--shadow` | The search bar's drop shadow (a `box-shadow` value) |

### UI elements

| Variable | Used for |
|---|---|
| `--kbd-bg`, `--kbd-border` | Key-cap hints (++ctrl+k++, ++enter++ labels) |
| `--warn`, `--error`, `--ok` | Notice, error and success message colors |
| `--surface` | Panels and sections in the Settings window |
| `--input-bg`, `--input-border` | Text fields and select boxes in Settings |
| `--switch-off` | Toggle track when off; slider track |

### Computed from settings

These are set automatically from appearance settings (override them in your stylesheet if you prefer):

| Variable | Source | Default |
|---|---|---|
| `--font-family` | `font_family` setting | system font |
| `--font-size` | `font_size` setting | `15px` |
| `--font-scale` | `font_size` / 15 | multiplies other text sizes and row height |
| `--radius` | `radius` setting | `14px` |
| `--card-opacity` | `opacity` / 100 | `1` |

## Theme examples

Share your theme! Examples could include:

- High-contrast themes for accessibility
- Seasonal themes (holiday colors, etc.)
- Themed to match your wallpaper
- Minimal or dark themes

The class names inside Sevak's UI may change between versions, so override only CSS variables and avoid selectors like `.result` or `.row`.

## More customization

- [Custom hotkeys and keyboard shortcuts](keyboard.md)
- [Complete settings reference](settings.md)
- [Configuration guide](configuration.md)

## See also

- [Privacy: custom stylesheets cannot load from the network](privacy.md)
- [Troubleshooting: appearance warnings and validation errors](troubleshooting.md)
