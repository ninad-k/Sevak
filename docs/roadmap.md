# Roadmap

**This roadmap is a statement of intent, not a commitment.** Priorities can shift based on community feedback, technical constraints and the maintainer's time.

## Now (in progress)

- Branded Windows installer with per-user / per-PC installation choice and upgrade prompt
- Settings pages for plugins that are config-file-only today (Contacts, Clipboard, Tasks, Dictionary)
- Safari bookmarks and PDF previews
- More gallery themes, workflows and plugins
- Refreshed demo media and screenshots
- Security scanning, threat model review, and security advisory guidelines
- Diagnostics report for troubleshooting
- License and SBOM documentation
- Test infrastructure improvements (faster CI, better coverage)

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
- A larger community gallery with user-submitted workflows and plugins (the submission and review process exists: [Publishing to the gallery](marketplace/publishing.md))
- Gallery revocation and deprecation notices in the app: today a removed entry stays listed in builds released before the removal and stays installed where it was installed, and the optional `deprecated` / `replaced_by` index fields are not shown. A fix would be a signed revocation list checked only when the user opens the gallery or the extensions page, plus a notice and an offer to uninstall. Not started; see [updating and removal](marketplace/updating-and-removal.md#takedown)
- Signed gallery indexes (see [Gallery trust](security/gallery-trust.md#future-sign-the-indexes))

## How to propose a feature

1. Check [GitHub Discussions](https://github.com/ninad-k/Sevak/discussions) — others may have suggested it already.
2. Search [open issues](https://github.com/ninad-k/Sevak/issues) to see if it is already tracked.
3. Open a [feature request](https://github.com/ninad-k/Sevak/issues/new?template=feature_request.yml) and describe the problem and your proposed solution.

Features that are useful to most people, perform quickly and work offline have the best chance of being adopted.
