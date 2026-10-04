# Sevak documentation media

[← Help center](../README.md)

## Gallery

| Asset | Purpose |
|---|---|
| [Overview](sevak-overview.png) | README hero and product introduction (shows Win+Space, plus the macOS and Linux keys) |
| [Feature gallery](sevak-features.png) | Apps, calculations, files and web search |
| [More features](sevak-features-more.png) | Preview pane, emoji grid, action panel and file buffer, one tile each (README) |
| [Settings](sevak-settings.png) | The actual Settings component |
| [Themes](sevak-themes.png) | Light and dark launcher examples |
| [Workflow](sevak-workflow.svg) | Accessible vector diagram of open → search → act |
| [12-second reel](sevak-12s.mp4) | 1080 × 1920, 30 fps, H.264/AAC |
| [Animated preview](sevak-demo.gif) | Small, silent GitHub-friendly reel preview |
| [Reel poster](sevak-reel-poster.png) | Static vertical cover |
| [Captions](sevak-12s.srt) | Plain-text transcript with timing |

Individual captures support the help pages. Launcher captures
(`launcher-*.png`) are one card each:

| Capture | Shows | Used on |
|---|---|---|
| [ready](launcher-ready.png), [apps](launcher-apps.png), [calculator](launcher-calculator.png), [files](launcher-files.png), [web](launcher-web.png), [light](launcher-light.png) | The search bar with a sample query | Quick start, home page, product page |
| [preview](launcher-preview.png) | The preview pane (Ctrl+Y) for a text file | More features tile |
| [actions](launcher-actions.png) | The action panel (Ctrl+K) for an app | More features tile |
| [grid](launcher-grid.png) | The emoji grid (`:`) | [Emoji picker](../features/emoji.md) |
| [buffer](launcher-buffer.png) | The file buffer strip | [Files](../features/files.md#file-buffer) |
| [selection](launcher-selection.png) | Universal Actions for a selected sentence | [Selection actions](../features/selection.md) |

Settings captures are [General](settings-general.png) (product page),
the [theme editor](settings-theme-editor.png) ([Themes](../themes.md)) and the
[workflow builder](settings-workflow-builder.png) ([Workflows](../workflows.md)).

The product page's social card (`landing/assets/og-image.png`, also saved as
`assets/brand/linkedin-launch.png`) is drawn by the same script.

## What the visuals show

The launcher and Settings images are rendered from the repository's actual
Svelte components in an isolated headless browser. Search results come from
[`fixtures.json`](../../scripts/docs-media/fixtures.json), not the user's
machine. Generic app glyphs, sample filenames and example paths are deliberate.

The reel is an **animated walkthrough**, not a native desktop screen recording
or a performance benchmark. It explains the supported sequence: open Sevak,
type a query, choose a result, press Enter. It includes a calculation example.
The short tonal sound cues are synthesized by the renderer; no stock music
or voice recording is used.

## Regenerate the media

The generator does not change app source, user settings or application state.
It serves the existing UI on a private local Vite port, substitutes documentation
fixtures in memory, captures the components, then renders the compositions.

Requirements:

- Project dependencies installed with `npm ci`.
- Node.js 22+ and the optional `playwright` and `@napi-rs/canvas` packages.
- Google Chrome, or a Chromium executable selected with `SEVAK_MEDIA_BROWSER`.
- FFmpeg on the command path when generating the video.

Install optional renderer packages without changing the app's dependency list:

```sh
npm install --no-save --package-lock=false playwright @napi-rs/canvas
node scripts/docs-media/render.mjs
node scripts/docs-media/render.mjs --video
```

`render.mjs` serves the UI on port 1436; set `SEVAK_MEDIA_PORT` to use another
free port (the app's own dev server uses 1420).

What each command regenerates:

| Command | Writes |
|---|---|
| `node scripts/docs-media/render.mjs` | Every `launcher-*.png` and `settings-*.png`, `sevak-overview.png`, `sevak-features.png`, `sevak-features-more.png`, `sevak-themes.png`, `sevak-settings.png`, `sevak-workflow.svg`, the social card |
| `node scripts/docs-media/render.mjs --video` | The above, plus `sevak-12s.mp4` and `sevak-reel-poster.png` |
| the `ffmpeg` command below | `sevak-demo.gif`, from the MP4 |
| edit by hand | `sevak-12s.srt` (captions; keep them in step with the text in `render.mjs`) |

The sample rows, previews and file buffer are in
[`fixtures.json`](../../scripts/docs-media/fixtures.json); the grid shows the
mock's real emoji, and the theme editor, workflow builder and Universal Actions
use the browser-preview data that ships with the UI. After regenerating, run
`npm run docs:media-check`: it fails on a missing image, an image without alt
text, or a file in this folder that nothing uses.

The default shortcut is Super+Space. The images show it as **Win+Space**
(Cmd+Space on macOS, Super+Space on Linux); change the text in `render.mjs`
if the default changes again.

The frosted-glass blur (Settings → Appearance) is drawn by the operating system
(Windows Acrylic, macOS vibrancy) behind a transparent window, so a headless
browser cannot show it truthfully. No image claims to; add one only from a
real screenshot.

If the renderer packages are installed elsewhere, set
`SEVAK_MEDIA_NODE_MODULES` to that `node_modules` directory.
`SEVAK_MEDIA_BROWSER` may name an alternate Chromium executable.
`FFMPEG` may name the FFmpeg executable. Windows renders with Segoe UI; other
systems use their available sans-serif font, so exact text metrics can differ.

Create the small GIF preview from the MP4:

```sh
ffmpeg -y -i docs/media/sevak-12s.mp4 -vf "fps=8,scale=320:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=3" -loop 0 docs/media/sevak-demo.gif
```

Temporary audio and verification output belong under `target/docs-media/`,
which is ignored by Git. Commit only the final media and reproducible source.

## Updating documentation

When the UI or behavior changes, update fixtures only to represent supported
behavior, regenerate the visuals, and inspect the images and video again.
Keep captions and the text alternative in the README useful for readers who
cannot view images or play video. Avoid adding performance or privacy claims
that the implementation does not support.
