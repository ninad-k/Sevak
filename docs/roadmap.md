# Roadmap

**This roadmap is a statement of intent, not a commitment.** Priorities can shift based on community feedback, technical constraints and the maintainer's time.

## Implemented on this branch

These features are present in the current branch. A feature being present here
does not mean a release containing it is published yet.

- Branded Windows installer with per-user and per-machine installation, upgrade,
  repair and uninstall flows
- Settings pages for Contacts, Clipboard, Tasks and media, and Dictionary
- Safari bookmark indexing and previews for PDFs and other supported files
- Media controls and now-playing results
- Security scanning and dependency checks, a threat model, and private security
  reporting guidance
- Local diagnostics reports for troubleshooting
- Licence and SBOM documentation; stable releases generate SBOMs and provenance
  attestations
- Automated tests, coverage reporting and a latency budget in CI
- Marketplace site, gallery packages, and in-app gallery and extension pages
  (integrated on this branch; availability in a published release follows the
  release process)

## Next (planned, awaiting resources)

- Package-manager publishing: Homebrew, Scoop, winget, AUR ([issue #12](https://github.com/ninad-k/Sevak/issues/12))
  - Needs package-maintainer relationships and account access
- Windows code signing with SignPath ([issue #10](https://github.com/ninad-k/Sevak/issues/10))
  - Needs SignPath account, tokens and certificate
- macOS code signing and notarization
  - Needs Apple Developer account (paid)
- Hand-testing on macOS and Linux desktops to ensure reliability
- Accessibility review (screen reader support and testing)

## Later (under consideration)

- Alfred Remote-style phone companion app (not actively planned)
- Music library browsing and filtering
- Richer file search filters
- Localisation for other languages
- More community gallery themes, workflows, plugins and extensions (the initial
  marketplace, gallery packages, submission and review process exist; see
  [Publishing to the gallery](marketplace/publishing.md))
- Gallery revocation and deprecation notices in the app: today a removed entry stays listed in builds released before the removal and stays installed where it was installed, and the optional `deprecated` / `replaced_by` index fields are not shown. A fix would be a signed revocation list checked only when the user opens the gallery or the extensions page, plus a notice and an offer to uninstall. Not started; see [updating and removal](marketplace/updating-and-removal.md#takedown)
- Signed gallery indexes (see [Gallery trust](security/gallery-trust.md#future-sign-the-indexes))

## How to propose a feature

1. Check [GitHub Discussions](https://github.com/ninad-k/Sevak/discussions) — others may have suggested it already.
2. Search [open issues](https://github.com/ninad-k/Sevak/issues) to see if it is already tracked.
3. Open a [feature request](https://github.com/ninad-k/Sevak/issues/new?template=feature_request.yml) and describe the problem and your proposed solution.

Features that are useful to most people, perform quickly and work offline have the best chance of being adopted.
