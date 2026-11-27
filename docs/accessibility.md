# Accessibility

Sevak is keyboard-first: everything in the launcher and in Settings works
without a mouse. This page lists what Sevak does for screen-reader and keyboard
users, how that is checked, what is still missing, and how to report a problem.

The target is [WCAG 2.2](https://www.w3.org/TR/WCAG22/) level AA.

## What is supported

### Launcher

- **The search box is an ARIA combobox.** It controls the result list (a
  `listbox` of `option`s, or the same for Grid View tiles) and points
  `aria-activedescendant` at the selected row, so focus never leaves the search
  box. A screen reader reads the row the arrow keys move to, and reads the new
  top row after each search.
- **The action panel** (++arrow-right++ or ++ctrl+k++) is a second listbox the
  search box points at while it is open. Its name says whose actions they are
  ("Actions for Firefox").
- **A polite live region** says how many results a search found ("5 results",
  "No results."), when the action panel, the Text View or Large Type opens, and
  that the application index is still filling. It waits until you stop typing,
  so you hear one count, not one per keystroke.
- Errors use `role="alert"`; file buffer progress is a named `progressbar`.
- Icons and key-cap hints are hidden from screen readers (`aria-hidden`); the
  same keys are listed on [Keyboard shortcuts](keyboard.md).

### Settings window

- The section list is an ARIA `tablist` with one tab stop: ++arrow-up++ /
  ++arrow-down++ move between sections, ++home++ / ++end++ jump to the first or
  last. A section with problems says how many ("General, 2 problems").
- Every control has an accessible name: switches (`role="switch"`), icon-only
  buttons such as ✕ ("Remove Google"), colour pickers and preset menus.
- A field with a problem is marked `aria-invalid` and linked to its message with
  `aria-describedby`, so the message is read with the field. Messages appear in
  an alert as you type.
- "Saved", startup status and similar notices are polite live regions.
- Focus is managed when the page changes under it: closing the workflow
  builder returns focus to the button that opened it; confirmation buttons
  ("Delete everything", "Replace now") take the focus and give it back on
  Cancel; removing an entry from a list moves focus to the next entry.
- Tab order follows the page: sections, then the section's controls from top to
  bottom, then the Save bar. There are no keyboard traps; ++escape++ closes an
  open extension card and the workflow builder.

### Visual

- **Focus is always visible**: a 2-pixel outline in `--focus-ring` (the theme's
  `accent_strong`), at least 3:1 against the background in every built-in theme.
- **Contrast**: the built-in themes keep text, subtitles, selected rows,
  error/warning/success messages and text on accent buttons at 4.5:1 or more,
  and the theme editor shows the contrast of your own theme as you edit it.
- **Reduced motion**: with *reduce motion* turned on in the operating system,
  Sevak drops its fades, slides and smooth scrolling.
- **Text size**: Settings → Appearance → Font size scales the launcher text.
- Both windows declare their language (`lang="en"`).

## How it is checked

- **Automated**: `npm test` renders the launcher and every Settings section in
  jsdom and runs [axe-core](https://github.com/dequelabs/axe-core) over them;
  any serious or critical violation fails the build
  (`ui/src/App.a11y.test.ts`, `ui/src/Settings.a11y.test.ts`). Unit tests cover
  the combobox semantics, the live-region wording
  (`ui/src/lib/announce.test.ts`), and the contrast of every built-in theme
  (`ui/src/lib/themes.test.ts`).
- **Screen readers: by design, not yet by hand.** The launcher follows the
  [ARIA combobox pattern](https://www.w3.org/WAI/ARIA/apg/patterns/combobox/)
  and Settings the [tabs pattern](https://www.w3.org/WAI/ARIA/apg/patterns/tabs/),
  which NVDA and JAWS (Windows, WebView2), VoiceOver (macOS, WKWebView) and Orca
  (Linux, WebKitGTK) support. Manual testing with each of them is still
  recommended before relying on it; reports are welcome.

## Keyboard map

The full list is on [Keyboard shortcuts](keyboard.md). The essentials:

| Where | Keys |
|---|---|
| Anywhere | ++win+space++ (++cmd+space++, ++super+space++) shows or hides the launcher |
| Launcher | Type to search; ++arrow-up++ / ++arrow-down++ select; ++enter++ runs; ++arrow-right++ or ++ctrl+k++ lists actions; ++escape++ goes back one step, then hides |
| Launcher views | ++shift++ or ++ctrl+y++ preview; ++ctrl+t++ Text View; ++ctrl+l++ Large Type |
| Settings | ++tab++ moves through controls; ++arrow-up++ / ++arrow-down++ on the section list; ++ctrl+s++ saves |
| Workflow builder | Arrow keys move a focused node; ++delete++ removes the selection; ++escape++ closes |

## Known gaps

- Not yet tested by hand with NVDA, JAWS, VoiceOver or Orca (see above).
- Large Type is announced through the live region, but focus stays in the
  search box, so a screen reader does not move into it.
- Some status messages in Settings pages (gallery installs, backup results,
  theme saves) appear together with their live region, which some screen
  readers do not announce; the result is still on the page.
- The tray menu and native dialogs (file pickers) are drawn by the operating
  system and follow its own accessibility support.
- Only English is available; see [Localisation](roadmap.md#later-under-consideration).

## Reporting a problem

Open a [bug report](https://github.com/ninad-k/Sevak/issues/new?template=bug_report.yml)
that starts with "Accessibility:" and gives your operating system, screen reader (and version), what you did, what you
heard or saw, and what you expected. Problems that block using Sevak with a
keyboard or a screen reader are treated as bugs, not feature requests.
