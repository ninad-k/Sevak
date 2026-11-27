# Windows installer validation

Sevak validates Windows packages at three levels. A successful build or static
inspection does not establish that installation or Windows sign-in works.

| Level | What runs | What it establishes |
|---|---|---|
| Artifact inspection | Reads EXE metadata and compiled MSI tables/conditions | Product identity, embedded version, app registration, startup action ordering and conditions |
| Disposable VM acceptance | Installs, changes startup, repairs, and uninstalls the actual package | Installed apps and Start menu entries, startup on/off, retained settings, Windows-disabled startup, repair and owned-entry cleanup |
| Desktop acceptance | A person uses the wizard, changes accounts, restarts and signs in | Checkbox behaviour, alternate-administrator UAC, tray/hotkey operation and actual automatic launch |

CI and Release run artifact inspection and the MSI acceptance suite on ephemeral
Windows runners. The VM suite still runs when artifact inspection fails, and
their JSON reports and MSI logs are uploaded even when a check fails (unless the
run is cancelled). Release publication depends on those automatic checks. A normal run has
one new installer, so **a genuine upgrade remains skipped**; same-version repair
is reported separately. NSIS desktop/UAC and real restart tests remain manual.

## Inspect packages without installing

Run from the repository root in PowerShell 7. All paths may contain spaces.
Supply explicit files when the output directory contains several versions.

```powershell
pwsh -NoProfile -File scripts/test-windows-installer.ps1 `
  -MsiPath 'C:\build\Sevak_VERSION_x64_en-US.msi' `
  -NsisPath 'C:\build\Sevak_VERSION_x64-setup.exe' `
  -BinaryPath 'C:\build\sevak.exe' `
  -ExpectedVersion 'VERSION' -ExpectedMsiVersion 'MSI_NUMERIC_VERSION' `
  -ReportPath 'C:\build\artifact-validation.json'
```

This script never installs, launches Sevak or changes its settings. Optional EXE
checks are marked `not_run` when their paths are omitted. Path resolution and
inspection failures still produce a failed JSON report.

Each path may also be a **quoted** wildcard pattern, such as
`'target/release/bundle/nsis/*.exe'`; it must match exactly one file, or the
script fails and lists the matches. An existing literal path is used as is,
even if it contains `[` or `]`. Both installer scripts disable positional
parameters, so an unquoted glob that a shell expands into several files is
rejected instead of binding the extra file to another parameter.

## Build a clearly identified local test package

Use the local build command instead of distributing a baseline `0.1.0` build:

```powershell
npm run build:test:windows -- --plan
npm run build:test:windows -- --prepare-only
npm run build:test:windows
```

`--plan` prints the proposed version and new-source inventory without writing
files. `--prepare-only` creates and stamps a source snapshot without compiling.
The full command runs `npm ci`, builds both NSIS and MSI packages, copies the
binary and packages, hashes them, and performs artifact inspection. It never
installs, publishes, commits or changes version fields in the working checkout.

The default output is `target/local-builds/<version>/`. `BUILD-INFO.json` records
the Git commit, working-change flag, source-content hash, build status, artifact
hashes and validation status. Completed builds also include `SHA256SUMS.txt` and
`artifact-validation.json`. A prepared snapshot reports validation as `not_run`.

A version looks like `0.13.0-dev.20261010T203045123Z.g123456789.dirty.1230`.
The next release base comes from the existing release-version rules and local
Git tags; fetch tags before planning when release ancestry matters. An already
released HEAD uses the next patch base. The timestamp, commit and dirty marker
are embedded in the app's semantic version, while MSI receives a numeric
version through the same stamping helper used by releases.

The snapshot includes tracked working changes plus nonignored new files in the
project's source, scripts, documentation, examples and gallery directories. It
excludes unrelated untracked root files, local build directories and common
untracked dotenv/private-key filenames. Files left out by that filename filter
are listed as `source.droppedSecretLikeFiles` in the `--plan` output and in
`BUILD-INFO.json`. If one lies under `crates/`, `src-tauri/src/` or `ui/src/`,
where leaving it out could change what is compiled, planning and building fail
unless you pass `--allow-dropped-source-files`. Review the `--plan` inventory
before sharing a build; this is not a general secret scanner. Output contains copied
source as well as binaries. Local builds use the normal local signing settings;
see [Publisher signing](publisher-signing.md) for release credentials.

Options are `--base X.Y.Z`, `--revision 0..65535`, and `--output PATH`. An existing
output directory is rejected. Repository-local output must stay under `target/`.
MSI compares only the first three version fields, so two different development
timestamps or fourth fields do **not** establish a genuine MSI upgrade.
See Microsoft's [ProductVersion documentation](https://learn.microsoft.com/windows/win32/msi/productversion).

For a deliberate upgrade test, build two versions from the desired source states
using distinct bases, for example `--base 0.13.0` then `--base 0.13.1`. The latter
must be newer than the former. These are local development builds, not published
releases.

## Run real installation checks in a disposable VM

These commands modify the test account and install/uninstall Sevak. Use a fresh
Windows VM snapshot with PowerShell 7, WebView2 and no existing Sevak installation,
settings, processes, startup entries or Start menu shortcuts. Copy the repository
scripts and installers into it. Restore or discard the snapshot after the run;
the harness intentionally retains test settings and logs for inspection.

The script requires all three opt-ins below and a recognized virtual-machine
hardware identity. `HypervisorPresent` alone is insufficient: a physical Windows
host with WSL/Hyper-V must not pass. Service identities such as SYSTEM are refused.
Never bypass these checks on a development machine.

The hardware check cannot tell a disposable VM from a persistent one. Microsoft
Dev Box, Windows 365 Cloud PCs, long-lived Hyper-V guests and Parallels VMs all
report a virtual-machine identity and pass it. On those machines the explicit
opt-ins (`-DisposableVm`, the confirmation string and `SEVAK_DISPOSABLE_VM=1`)
are the only protection: never supply them on a VM you use for daily work.

For MSI, open elevated PowerShell **as the same disposable test account**. Hosted
Microsoft Windows VMs are supported and do not need an interactive Explorer shell.

```powershell
$env:SEVAK_DISPOSABLE_VM = '1'
pwsh -NoProfile -File scripts/test-windows-installer-vm.ps1 `
  -Format Msi -InstallerPath 'C:\test\Sevak_VERSION_x64_en-US.msi' `
  -ExpectedVersion 'VERSION' `
  -DisposableVm -Confirmation I_ACKNOWLEDGE_THIS_IS_A_DISPOSABLE_VM `
  -ReportPath 'C:\test\reports\msi.json'
```

For NSIS, use **unelevated** PowerShell in a signed-in desktop test account with
Explorer running. It tests current-user installation; the all-users wizard and
credential-changing UAC are separate manual scenarios.

```powershell
$env:SEVAK_DISPOSABLE_VM = '1'
pwsh -NoProfile -File scripts/test-windows-installer-vm.ps1 `
  -Format Nsis -InstallerPath 'C:\test\Sevak_VERSION_x64-setup.exe' `
  -ExpectedVersion 'VERSION' `
  -DisposableVm -Confirmation I_ACKNOWLEDGE_THIS_IS_A_DISPOSABLE_VM `
  -ReportPath 'C:\test\reports\nsis.json'
```

The suite installs into a unique path containing spaces, verifies exactly one
Installed apps entry and Start menu shortcut, checks default startup off, then
uses the installer's explicit on/off options and checks saved preferences. It
deletes the test installation's executable and requires same-version repair to
restore it; MSI uses `REINSTALL=ALL`. It creates a Windows-disabled startup state
and verifies that an unspecified reinstall preserves the command, approval and
settings. Finally it uninstalls with startup enabled and checks owned cleanup.
The main UI is not launched by this suite.

To exercise a genuine upgrade, add `-UpgradeInstallerPath PATH` and optionally
`-UpgradeExpectedVersion VERSION`. The second artifact must use the same format
and have a higher major/minor/patch version. MSI preserves its remembered
installation directory; NSIS tests a changed `/D` destination. Moving an MSI
installation requires separate desktop testing because MSI's remembered
directory lookup can take precedence over an explicit path.

Failures trigger best-effort removal of the test installations. MSI cleanup tracks
both product identities if an upgrade rolls back; NSIS cleanup uninstalls both
the original and the upgrade folder. The report's `cleanup` object lists the
folders, product codes and uninstallers that ran. A cleanup failure is recorded;
it never converts the failed test into a pass. Discard the VM even if cleanup
succeeds. `-TimeoutSeconds` controls each child process (30–900, default 180).
An installer requesting reboot is recorded, but the script does not reboot.

## Read the report and finish desktop acceptance

Reports use `schemaVersion: 1`, `kind`, `automaticStatus`, `installationExecuted`,
artifact hashes, executed commands, per-case statuses and summary counts. Both
scripts use the same check shape: a stable `id` and a `status` (`passed`,
`failed`, `skipped` or `not_run`), plus `error` for a failure and `reason` for a
skip. Preflight failure leaves all
installation cases unperformed; it cannot claim a successful smoke test.

`manualChecks` always records restart/sign-in, wizard controls and
alternate-administrator UAC as `not_run`. An automatic pass means only the
executed automatic checks passed. Record the following separately using the
[testing matrix](testing.md#windows-installer-scenarios):

1. Inspect the startup checkbox on fresh install and upgrade, and the independent
   launch-now choice. Test both per-user and all-users installation.
2. Enable startup, quit Sevak, restart and sign in. Confirm one tray instance,
   hidden launcher and a working shortcut. Disable it and repeat: no automatic
   process should start.
3. Use a second account and alternate administrator credentials during UAC.
   Confirm only the account choosing startup is enrolled.
4. Test a real previous-release upgrade with existing application data, a running
   app, scope changes and a moved MSI installation. Check settings and data remain.

The script logic has safe unit tests: `npm run test:scripts`. They exercise the
VM gate, report honesty, command construction, cleanup identity tracking, build
versioning and isolated snapshot stamping. They do not install Sevak.
