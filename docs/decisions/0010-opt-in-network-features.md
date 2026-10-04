# ADR-0010: Network features only on the user's action or opt-in

**Status:** Accepted

## Context

[ADR-0001](0001-local-first-no-telemetry.md) says Sevak does not phone home,
yet some features are useful only with the network: currency rates, update
checks, community themes and workflows. Each such feature is a request that
discloses an IP address and a timestamp, and a download that brings in code or
data from outside.

## Decision

Sevak itself makes network requests in exactly these cases, all listed in
[Privacy](../privacy.md):

| Feature | Trigger | Request |
|---|---|---|
| Web search, links, bookmarks | The user presses Enter on the result | None by Sevak: the address opens in the user's browser |
| Currency conversion | `[calculator] currency = true` (off by default) | One daily GET of the ECB reference-rates XML, cached for 24 hours |
| Update check | `[general] check_for_updates` (on by default; can be switched off) | A GET of `latest.json` on GitHub Releases. Installing needs a separate yes in a dialog and a valid update signature |
| Theme gallery | The user clicks *Browse online themes* | The theme index, then one theme file |
| Workflow gallery | The user presses *Load gallery* | The gallery index, then one package |

Rules that follow from this:

- Nothing is fetched in the background except the update check and, when
  enabled, the ECB rates. Galleries are never contacted at startup.
- Downloads go through one function (`fetch_https`,
  `crates/sevak-plugins/src/net.rs`): HTTPS only, redirects only to HTTPS,
  a 20-second timeout, a size cap, no cookies, and a user agent that carries
  only the program name and version. A new caller belongs in the privacy page.
- A downloaded gallery file is checked against the SHA-256 in the index
  before anything is written, and an installed script plugin or workflow still
  has to be approved before it runs ([ADR-0005](0005-approval-before-run.md)).
- The update check is the single background request that is on by default,
  because an updater that must be found and enabled is rarely enabled and
  security fixes would not reach users. It sends no data beyond the request
  itself.

## Consequences

- Network behaviour can be audited by searching for the one download helper
  and the updater.
- The gallery index and its packages come from the same repository, so the
  SHA-256 check protects the transfer, not against a compromised repository;
  approval before running is the control for that case (see the
  [threat model](../security/threat-model.md#gallery)).
- Features that need continuous connectivity (sync, live search suggestions)
  do not fit this rule and are not offered.
