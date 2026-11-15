# Publisher signing

Sevak has two separate signature checks. The updater's `.sig` files authorize
an update for Sevak. Windows Authenticode and Apple Developer ID identify the
publisher to the operating system. An updater signature does **not** make an
installer publisher-signed or notarized.

The release workflow supports publisher signing, checks for incomplete
configuration, and verifies the built artifacts before the draft release can
be published. Local builds and ordinary PR builds remain usable without
publisher credentials. Until the configuration below is provisioned, Windows
installers are unsigned and macOS bundles use an ad-hoc signature.

## Windows

Provision a signing provider and its runner integration first. Follow the
provider's current certificate/key-storage requirements; this repository does
not issue certificates or configure a provider account. The existing
[signing issue](https://github.com/ninad-k/Sevak/issues/10) tracks provisioning.

Configure these repository settings:

| Setting | Kind | Purpose |
|---|---|---|
| `WINDOWS_SIGN_COMMAND` | Secret | JSON object containing `cmd` and `args`, with exactly one standalone `%1` argument for the file to sign. |
| `WINDOWS_SIGNING_TOKEN` | Secret, if needed | Provider credential passed only to the release build step; the signing adapter reads it from the environment. |
| `WINDOWS_SIGNER_THUMBPRINT` | Variable | Expected publisher certificate SHA-1 thumbprint, 40 hexadecimal digits. This identifies the certificate; file signatures still use the provider's modern digest algorithm. |
| `REQUIRE_WINDOWS_SIGNING` | Variable | Set to `true` after provisioning, so accidentally missing credentials stop the release. |

The command is a provider hook, not an installed signing tool. Check the
provider adapter into the repository or add its pinned installation step before
the Tauri build, then configure the command to invoke it. It must synchronously
sign the supplied `.exe` or `.msi`, add a trusted timestamp, and return nonzero
on failure. Do not put credentials in command arguments: build tools can log
them. Use the token environment variable instead.

Tauri invokes the hook while bundling, so the embedded application, installers
and subsequent updater signatures all cover the correctly ordered artifacts.
`verify-windows-signatures.ps1` then requires a valid Authenticode signature,
the expected publisher certificate, and a timestamp on `sevak.exe`, the NSIS
installer and the MSI. A signing service or certificate change requires updating
the expected thumbprint deliberately.

See [Tauri's Windows signing documentation](https://v2.tauri.app/distribute/sign/windows/)
for supported providers and command configuration. Signing does not guarantee
that a newly distributed file immediately has SmartScreen reputation.

## macOS

Configure these settings for a Developer ID Application certificate and
notarization using an Apple ID's app-specific password:

| Setting | Kind | Purpose |
|---|---|---|
| `APPLE_CERTIFICATE` | Secret | Base64-encoded exported `.p12` certificate. |
| `APPLE_CERTIFICATE_PASSWORD` | Secret | Password protecting that export. |
| `APPLE_SIGNING_IDENTITY` | Variable | Full `Developer ID Application: …` identity. Ad-hoc `-` is rejected when publisher signing is configured. |
| `APPLE_ID` | Secret | Apple account used for notarization. |
| `APPLE_PASSWORD` | Secret | App-specific notarization password. |
| `APPLE_TEAM_ID` | Variable | Ten-character Apple team ID. |
| `REQUIRE_MACOS_SIGNING` | Variable | Set to `true` to require signing and notarization for every release. |

Tauri imports the certificate for the build and submits the app for
notarization. The release overlay replaces the local ad-hoc identity.
Publication requires `codesign --verify --deep --strict`, Gatekeeper's
`spctl --assess`, and `xcrun stapler validate` to succeed for the universal app.
The downloadable disk image is checked too: `codesign --verify --strict` and
`spctl -a -t open --context context:primary-signature` on the single `.dmg`.
These checks must run on macOS with real credentials; mocked script tests
establish configuration and error handling only.

Follow [Tauri's macOS signing guide](https://v2.tauri.app/distribute/sign/macos/)
to obtain and export the certificate. This workflow currently uses the Apple ID
notarization path; it does not provision Apple accounts or API keys.

## Validation and rollout

```sh
node --test scripts/release-signing.test.mjs
node scripts/release-signing.mjs prepare target/signing
```

The local command reports absent credentials as **not configured**. In Actions,
`verify` also emits a `::warning::` annotation for an unsigned Windows or macOS
build, so it shows on the run page; the step still passes unless
`REQUIRE_WINDOWS_SIGNING` / `REQUIRE_MACOS_SIGNING` is `true`. A partial
configuration or an enabled requirement without credentials is an error.

Verification runs after tauri-action has uploaded the files to the draft
release. A failed verification deletes that draft; never publish a draft by
hand (see [Releasing](releasing.md#failed-builds-and-unverified-draft-assets)).

On Windows, `npm run test:scripts` signs throwaway copies of a small PE file
with a temporary self-signed certificate (created in `Cert:\CurrentUser\My`,
removed afterwards, never trusted) and requires `verify-windows-signatures.ps1`
to reject unsigned, untrusted, wrong-certificate and untimestamped files. A
timestamped success case needs a network timestamp authority and is not run.
The generated overlay contains command/identity configuration, never the Apple
certificate, passwords or provider token. Do not attach it as a build artifact.

Use a [staged beta release](releasing.md#staged-flow) to validate real
signatures and installation before making signing required. Check the release
job's publisher-signing summary and test a freshly downloaded installer on a
separate machine. Keep the updater key and publisher credentials independent;
rotating either has different effects on existing installations.
