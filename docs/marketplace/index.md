# Marketplace

The Sevak marketplace lists everything you can add to the launcher beyond what
ships in the box:

| Kind | What it is | Runs code? |
|---|---|---|
| **Workflows** | Keyword searches, Universal Actions and small automations built from the closed set of workflow nodes | No, unless the listing says it runs a script |
| **Script plugins** | Small programs in Python or Node.js that answer when you type their keyword | A script, after you allow it |
| **Native extensions** | Compiled programs written in Rust | A native program, after you allow it |
| **Themes** | Color files for the search bar and Settings | No |

Every listing states which of those applies, who wrote it, which licence it has,
and where its source is. Sevak downloads a package only when you click, and
refuses it unless its SHA-256 equals the one published in the list. See
[Gallery trust](../security/gallery-trust.md) for exactly what is checked and
what is not.

## Browse

- **On the web:** [ninad-k.github.io/Sevak/marketplace](https://ninad-k.github.io/Sevak/marketplace/).
  Search by name or keyword, filter by kind or tag, and open a listing to see
  what it does, how to install it, what permission to expect and the checksum to
  verify. The page is built from the same gallery files the app reads, and every
  listing is visible without JavaScript (the filters need it).
- **In the app:** **Settings → Gallery** for workflows and script plugins,
  **Settings → Extensions** for all kinds together (and native extensions), and
  **Settings → Appearance → Theme editor → Browse online themes** for themes.
  Each of those pages has a **Browse the marketplace** link to the web version.
- **For tools:** the same data as JSON at
  [`/marketplace/catalog.json`](https://ninad-k.github.io/Sevak/marketplace/catalog.json).

A new listing reaches the app with the next release, because released builds read
the gallery pinned to their own version, not `main`.

## Publish your own

Built something useful? Submit it:

1. Read the [publishing guide](https://ninad-k.github.io/Sevak/docs/marketplace/publishing/)
   for the rules, the package layout and the checks to run first.
2. Open a [submission](https://github.com/ninad-k/Sevak/issues/new?template=extension_submission.yml)
   (in the app: **Submit your extension** on the Gallery, Extensions and theme pages).
3. A maintainer reviews it against the gallery rules in public and merges it
   into the gallery, from where it appears on the marketplace.

## How the website is made

`scripts/build-marketplace.mjs` reads `gallery/index.json` and
`gallery/themes.json` and writes the pages. The output is not committed: the
Pages workflow runs the script before it publishes the site. To preview it, run
`npm run marketplace` and serve the `landing/` folder with any static file server
(for example `python -m http.server --directory landing`). All text from a
listing is escaped and only `https://` addresses are linked.
