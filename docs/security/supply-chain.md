# Supply chain: checksums, SBOM, provenance and licences

Every Sevak release carries several independent ways to check what you
downloaded and what is inside it. This page explains each one, what it proves
and what it does not.

| Check | What it tells you | Where |
|---|---|---|
| Checksums | The file is the one the release published, bit for bit | `SHA256SUMS.txt` on the release |
| Build provenance attestation | The file was attested by this repository's `Attest` workflow, signed through GitHub | `gh attestation verify` |
| Updater signature | An update Sevak downloads for itself was signed with the maintainer's key | checked by the app, automatically |
| SBOM | The list of components inside the app | `sevak-sbom-*.cdx.json` on the release |
| Third-party notices | The licence of every dependency | `THIRD_PARTY_NOTICES.md`, in the repository and in the installed app |

## Verify your download

### Checksums

The release lists the SHA-256 of every file in `SHA256SUMS.txt`. Download it next
to the installer and compare:

=== "Windows (PowerShell)"

    ```powershell
    Get-FileHash .\Sevak_<version>_x64-setup.exe -Algorithm SHA256
    Select-String "x64-setup" .\SHA256SUMS.txt
    ```

=== "macOS"

    ```bash
    shasum -a 256 Sevak_<version>_universal.dmg
    grep universal.dmg SHA256SUMS.txt
    ```

=== "Linux"

    ```bash
    sha256sum --check --ignore-missing SHA256SUMS.txt
    ```

The two hashes must be identical. A checksum shows that your copy was not
damaged or swapped in transit. It does not show who made the file, because
`SHA256SUMS.txt` comes from the same place as the installer; for that, use the
attestation below.

### Build provenance

Release installers also carry a
[SLSA build-provenance attestation](https://slsa.dev/spec/v1.0/provenance),
made with GitHub's artifact attestations. With the
[GitHub CLI](https://cli.github.com/) installed:

```bash
gh attestation verify Sevak_<version>_x64-setup.exe --repo ninad-k/Sevak
```

A successful check prints the attestation's signer and the commit it refers
to. Add `--signer-workflow ninad-k/Sevak/.github/workflows/attest.yml` to require
that it was this repository's attestation workflow that signed it. If you have
no network, download the attestation once with `gh attestation download` and
verify with `--bundle`.

!!! warning "What the attestation covers, honestly"
    The attestation is created by the `Attest` workflow **after** the release is
    published. That workflow downloads the published installers, checks them
    against `SHA256SUMS.txt`, and signs their digests, recording the repository
    and commit it ran from. So it proves that *this repository's release
    process published these exact bytes*, and it makes a later swap detectable.
    It is **not** a hermetic or reproducible rebuild, and it does not by itself
    prove that the release workflow's build jobs produced the bytes: for that,
    the attestation would have to be made inside the build jobs, which is a
    possible later step. Sevak's builds are also not bit-for-bit reproducible
    today. Read it as "published by this project's pipeline", not "independently
    rebuilt and confirmed".

### Updater signature

Sevak's own updater downloads the update listed in `latest.json` and installs it
only if its signature matches the public key embedded in the app
(`plugins.updater.pubkey` in `src-tauri/tauri.conf.json`). The private key is
held by the maintainer as a repository secret and never leaves it. You do not
need to do anything for this check; an update with a missing or wrong signature
is rejected. It protects automatic updates, not manual downloads.

### Installers are not code-signed yet

The Windows and macOS installers are not yet signed with a Windows or Apple
developer certificate, which is why your operating system warns you about them;
see [Installing Sevak](../install.md). The checks above are how you establish
trust in the meantime.

## Software bill of materials (SBOM)

Each release has two [CycloneDX](https://cyclonedx.org/) 1.5 SBOMs in JSON, added
after the release is published by the `SBOM` workflow:

| File | Contents |
|---|---|
| `sevak-sbom-rust.cdx.json` | The Rust crates of the `sevak` app, with versions, licences and package URLs, from `Cargo.lock`. Built for all targets, so it is a superset of what one platform's installer contains; build-only crates are left out. |
| `sevak-sbom-npm.cdx.json` | The npm packages whose code is inside the app's user interface (today `svelte`, `clsx` and `@tauri-apps/api`). Build tooling such as Vite and TypeScript is not shipped and is not listed. |
| `SBOM-SHA256SUMS.txt` | The SHA-256 of the two files above. It is separate from `SHA256SUMS.txt`, which belongs to the release itself. |

Use them with any CycloneDX-aware tool, for example a vulnerability scanner or
a dependency-tracking system. To list the components yourself:

```bash
jq -r '.components[] | "\(.name) \(.version) \(.licenses[0].license.id // .licenses[0].expression)"' sevak-sbom-rust.cdx.json
```

To produce the same files from a checkout (needs Rust, Node.js and
`cargo install cargo-cyclonedx`, at the version pinned in the script):

```bash
npm ci
node scripts/generate-sbom.mjs --out sbom
```

The SBOM and attestation workflows run automatically for releases that are
published by hand or with a personal token. GitHub does not start a workflow
for an event caused by the built-in `GITHUB_TOKEN`, which is what the automatic
release workflow publishes with, so that workflow has to call them
(`workflow_call`) or start them (`gh workflow run sbom.yml -f tag=vX.Y.Z`). Either
workflow can also be run for an existing release from the Actions tab. If a
release has no `sevak-sbom-*` assets, they were not run for it yet.

## Third-party notices and licences

`THIRD_PARTY_NOTICES.md` is the licence inventory. It is in the repository's top
folder and is installed with the app, in its resources folder: next to
`sevak.exe` on Windows, in `Sevak.app/Contents/Resources` on macOS, and under
`/usr/lib/Sevak/` for the Linux packages (the Apache License text, `LICENSE`, is
there too). It contains:

1. **Bundled data and design assets**: the WordNet dictionary, the Unicode emoji
   and CLDR data, the colour palettes some built-in themes follow, and a note on
   fonts, icons and artwork.
2. **Licences that need attention**: any dependency that is not purely
   permissive (today only MPL-2.0 crates, which are used unmodified), or that
   declares no recognisable licence. Nothing under a strong copyleft or
   non-commercial licence is included.
3. **Licence summary**, then a table of every npm package and Rust crate with
   its version, declared licence and platforms.
4. **Licence texts**: each distinct licence or notice text once, followed by the
   packages it comes from.
5. **Packages that ship no licence file**: for these, the standard text of the
   declared licence is used and the authors named in the package manifest are
   the copyright holders.

### Reading the licence list

A declared licence like `MIT OR Apache-2.0` means the author offers a choice:
Sevak may use the package under either. `AND` means both apply. `Apache-2.0 WITH
LLVM-exception` is Apache-2.0 with an extra permission. The Platforms column
says on which operating systems a crate is linked (`all`, or a subset such as
`Windows`); the licence obligations are the same wherever it appears.

### Regenerating the third-party notices

Regenerate the file whenever `Cargo.lock` or `package-lock.json` change, and
commit the result:

```bash
npm ci
node scripts/generate-third-party-notices.mjs          # rewrites THIRD_PARTY_NOTICES.md
node scripts/generate-third-party-notices.mjs --check  # fails if the file is out of date
```

The script needs only Node.js, `cargo` and the project's `npm ci`. It asks
`cargo metadata --filter-platform` for the dependencies of the `sevak` app on
`x86_64-pc-windows-msvc`, `x86_64-apple-darwin`, `aarch64-apple-darwin` and
`x86_64-unknown-linux-gnu` and merges them, following normal dependency edges
only, so development-only and build-only dependencies are left out. It finds the
npm packages in the UI bundle by running a throwaway production build and reading
its source map. Licence texts are read from the packages' own sources.

The hand-written sections are in `scripts/notices/data-and-assets.md`; edit that
file, not the generated output. When you add a bundled data file, font, icon set
or palette, add its notice there.

## Related

- [Installing Sevak](../install.md) and the "Verify your download" steps
- [Privacy](../privacy.md): every network request Sevak makes, including update checks
- [Releasing](../development.md#releasing): how a release is built and published
