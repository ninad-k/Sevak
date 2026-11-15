import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const available = spawnSync("pwsh", ["-NoProfile", "-Command", "$PSVersionTable.PSVersion.Major"], { windowsHide: true }).status === 0;
const quote = (text) => `'${text.replaceAll("'", "''")}'`;
const run = (args) => spawnSync("pwsh", ["-NoProfile", "-File", ...args], { encoding: "utf8", windowsHide: true, timeout: 30_000 });

test("VM gate rejects physical hosts, incomplete opt-in, wrong tokens and service identities", { skip: !available && "PowerShell 7 unavailable" }, () => {
  const dir = mkdtempSync(join(tmpdir(), "sevak-vm-gate-"));
  try {
    const script = join(dir, "gate.ps1");
    writeFileSync(script, String.raw`
      $ErrorActionPreference = 'Stop'
      . ${quote(join(root, "scripts/windows-installer-test-lib.ps1"))}
      function Check($condition, $message) { if (-not $condition) { throw $message } }
      $facts = @{Windows=$true; Manufacturer='Microsoft Corporation'; Model='Virtual Machine'; EnvironmentOptIn='1'; UserSid='S-1-5-21-1000'; Elevated=$true; DesktopShell=$false}
      $ack = 'I_ACKNOWLEDGE_THIS_IS_A_DISPOSABLE_VM'
      Check (@(Get-SevakVmGateFailures $facts $true $ack 'Msi').Count -eq 0) 'Hosted Microsoft VM should support MSI without a desktop shell'
      Check (@(Get-SevakVmGateFailures $facts $false $ack 'Msi').Count -gt 0) 'Explicit switch is required'
      Check (@(Get-SevakVmGateFailures $facts $true 'yes' 'Msi').Count -gt 0) 'Exact acknowledgement is required'
      $facts.EnvironmentOptIn=''; Check (@(Get-SevakVmGateFailures $facts $true $ack 'Msi').Count -gt 0) 'Environment opt-in is required'; $facts.EnvironmentOptIn='1'
      $facts.Model='Precision'; $facts.Manufacturer='Dell'; $facts.HypervisorPresent=$true
      Check (@(Get-SevakVmGateFailures $facts $true $ack 'Msi').Count -gt 0) 'Physical Hyper-V host must fail'
      $facts.Model='Virtual Machine'; $facts.Manufacturer='Microsoft Corporation'
      foreach ($sid in @('S-1-5-18','S-1-5-19','S-1-5-20')) { $facts.UserSid=$sid; Check (@(Get-SevakVmGateFailures $facts $true $ack 'Msi').Count -gt 0) 'Service account must fail' }
      $facts.UserSid='S-1-5-21-1000'
      Check (@(Get-SevakVmGateFailures $facts $true $ack 'Nsis').Count -gt 0) 'Elevated NSIS must fail'
      $facts.Elevated=$false
      Check (@(Get-SevakVmGateFailures $facts $true $ack 'Nsis').Count -gt 0) 'NSIS needs desktop shell'
      $facts.DesktopShell=$true
      Check (@(Get-SevakVmGateFailures $facts $true $ack 'Nsis').Count -eq 0) 'Normal desktop NSIS should pass'
      Check (@(Get-SevakVmGateFailures $facts $true $ack 'Msi').Count -gt 0) 'Unelevated MSI must fail'
      Check ((Get-SevakCoreVersion '1.2.3.9') -eq (Get-SevakCoreVersion '1.2.3.1')) 'Fourth MSI field cannot claim an upgrade'
      $crlf = [string][char]13 + [char]10
      $config = '[general]' + $crlf + 'launch_at_login = true # yes' + $crlf + '[plugins]' + $crlf + 'launch_at_login = false' + $crlf
      Check (Get-SevakSavedStartup $config) 'CRLF config and comments should parse'
      $parts=Get-SevakRunParts '"C:\Program Files\Sevak\sevak.exe" "--background" --config "C:\Users\VM User\custom.toml"'
      Check ($parts.executable -eq 'C:\Program Files\Sevak\sevak.exe') 'Quoted path parsing'
      Check ($parts.arguments -eq ' "--background" --config "C:\Users\VM User\custom.toml"') 'Arguments preserved verbatim'
      $report=New-SevakValidationReport 'fixture' 'read-only'
      $report.checks.Add(@{id='not executed';status='not_run'})
      Save-SevakValidationReport $report ${quote(join(dir, "report.json"))}
    `);
    const result = run([script]);
    assert.equal(result.status, 0, result.stdout + result.stderr);
    const report = JSON.parse(readFileSync(join(dir, "report.json"), "utf8"));
    assert.equal(report.installationExecuted, false);
    assert.equal(report.summary.passed, 0);
    assert.equal(report.summary.notRun, 1);
    assert.equal(report.summary.manualNotRun, 3);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test("VM runner without authorization refuses before installing and reports unperformed tests honestly", { skip: !available && "PowerShell 7 unavailable" }, () => {
  const dir = mkdtempSync(join(tmpdir(), "sevak-vm-refusal-"));
  try {
    const path = join(dir, "refused.json");
    // Deliberately omit ALL opt-ins and installer paths. The gate cannot pass,
    // even when these tests themselves happen to run on an ephemeral VM.
    const result = run([join(root, "scripts/test-windows-installer-vm.ps1"), "-ReportPath", path]);
    assert.equal(result.status, 1, result.stdout + result.stderr);
    const report = JSON.parse(readFileSync(path, "utf8"));
    assert.equal(report.automaticStatus, "failed");
    assert.equal(report.installationExecuted, false);
    assert.equal(report.commands.length, 0);
    assert.equal(report.summary.passed, 0);
    assert.ok(report.summary.notRun >= 10);
    assert.equal(report.manualChecks.every((check) => check.status === "not_run"), true);
    assert.ok(report.checks.every((check) => typeof check.id === "string" && !("description" in check)));
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test("artifact checks use the shared report schema: stable unique ids, errors only on failure", { skip: !available && "PowerShell 7 unavailable" }, () => {
  const dir = mkdtempSync(join(tmpdir(), "sevak-report-schema-"));
  try {
    const script = join(dir, "schema.ps1");
    writeFileSync(script, String.raw`
      $ErrorActionPreference = 'Stop'
      . ${quote(join(root, "scripts/windows-installer-test-lib.ps1"))}
      $source = ${quote(join(root, "scripts/test-windows-installer.ps1"))}
      $tokens=$null; $errors=$null
      $ast=[Management.Automation.Language.Parser]::ParseFile($source,[ref]$tokens,[ref]$errors)
      if ($errors.Count) { throw ($errors -join '; ') }
      $fn=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Assert-That'},$true)
      . ([scriptblock]::Create($fn.Extent.Text))
      $report=New-SevakValidationReport 'fixture' 'read-only'; $script:checkIds=@{}; $script:checks=0
      Assert-That $true 'Valid startup argument'
      Assert-That $true 'Valid startup argument'
      try { Assert-That $false 'Startup rule (condition returned 0): X' 'Startup rule' } catch { }
      Save-SevakValidationReport $report ${quote(join(dir, "report.json"))}
    `);
    const result = run([script]);
    assert.equal(result.status, 0, result.stdout + result.stderr);
    const report = JSON.parse(readFileSync(join(dir, "report.json"), "utf8"));
    assert.deepEqual(report.checks.map((check) => check.id), ["valid-startup-argument", "valid-startup-argument-2", "startup-rule"]);
    assert.ok(report.checks.every((check) => !("description" in check)));
    assert.equal(report.checks[2].status, "failed");
    assert.match(report.checks[2].error, /condition returned 0/);
    assert.ok(!("error" in report.checks[0]));
    assert.equal(report.summary.failed, 1);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test("installer path patterns must match exactly one file, and globs cannot spill into other parameters", { skip: !available && "PowerShell 7 unavailable" }, () => {
  const dir = mkdtempSync(join(tmpdir(), "sevak-path-patterns-"));
  try {
    for (const name of ["a-setup.exe", "b-setup.exe", "one.msi", "[x].msi"]) writeFileSync(join(dir, name), "");
    const script = join(dir, "paths.ps1");
    writeFileSync(script, String.raw`
      $ErrorActionPreference = 'Stop'
      . ${quote(join(root, "scripts/windows-installer-test-lib.ps1"))}
      $dir = ${quote(dir)}
      function Fails($pattern, $expected) {
        try { Resolve-SevakSinglePath $pattern 'NsisPath' | Out-Null } catch {
          if ($_.Exception.Message -notmatch $expected) { throw "Unexpected error for ${"$"}{pattern}: $($_.Exception.Message)" }
          return
        }
        throw "Accepted $pattern"
      }
      Fails (Join-Path $dir '*.exe') 'matched 2: .*a-setup\.exe.*b-setup\.exe'
      Fails (Join-Path $dir '*.dmg') 'matched 0'
      Fails (Join-Path $dir 'missing.exe') 'does not exist'
      Fails '' 'required'
      if ((Resolve-SevakSinglePath (Join-Path $dir 'o*.msi') 'MsiPath') -ne (Join-Path $dir 'one.msi')) { throw 'single match' }
      if ((Resolve-SevakSinglePath (Join-Path $dir '[x].msi') 'MsiPath') -ne (Join-Path $dir '[x].msi')) { throw 'literal brackets' }
      Write-Output 'paths-ok'
    `);
    const result = run([script]);
    assert.equal(result.status, 0, result.stdout + result.stderr);
    assert.match(result.stdout, /paths-ok/);
    // An unquoted glob the shell expanded to two files must not bind the second
    // file to another parameter; PositionalBinding=$false rejects it.
    for (const [file, flag] of [["test-windows-installer.ps1", "-NsisPath"], ["test-windows-installer-vm.ps1", "-InstallerPath"]]) {
      const spill = run([join(root, "scripts", file), flag, join(dir, "a-setup.exe"), join(dir, "b-setup.exe"), "-ReportPath", join(dir, `${file}.json`)]);
      assert.notEqual(spill.status, 0, file);
      assert.match(spill.stdout + spill.stderr, /positional parameter/i, file);
    }
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

// Real Authenticode signatures from a throwaway self-signed certificate that
// exists only for this test (CurrentUser\My, removed in finally). It is never
// added to a trust store: an untrusted chain must fail on its own, and a shim
// that trusts only this test's certificates exercises the thumbprint and
// timestamp checks. A timestamped success case needs a network timestamp
// authority, so only failure cases run here.
const windowsSigning = process.platform === "win32" && available;
test("Windows publisher-signature gate rejects unsigned, untrusted, wrong-publisher and untimestamped files", { skip: !windowsSigning && "Windows with PowerShell 7 required", timeout: 120_000 }, () => {
  const dir = mkdtempSync(join(tmpdir(), "sevak-signature-gate-"));
  try {
    const script = join(dir, "gate.ps1");
    writeFileSync(script, String.raw`
      $ErrorActionPreference = 'Stop'
      $verify = ${quote(join(root, "scripts/verify-windows-signatures.ps1"))}
      $dir = ${quote(dir)}
      $certs = @()
      try {
        # A freshly compiled, unsigned PE image (a library is enough for
        # Authenticode; the extension does not matter to the signature check).
        $exe = Join-Path $dir 'plain.dll'
        Add-Type -TypeDefinition 'public static class SevakSignatureFixture { public static int Value() { return 1; } }' -OutputAssembly $exe
        foreach ($name in @('expected', 'other')) {
          $certs += New-SelfSignedCertificate -Type CodeSigningCert -Subject "CN=Sevak signature gate test ($name)" -CertStoreLocation Cert:\CurrentUser\My -KeyExportPolicy NonExportable -NotAfter (Get-Date).AddDays(1)
        }
        $expected, $other = $certs
        function New-BuildRoot([string] $name, $cert) {
          $root = Join-Path $dir $name
          foreach ($sub in @('bundle/nsis', 'bundle/msi')) { [void][IO.Directory]::CreateDirectory((Join-Path $root $sub)) }
          $app = Join-Path $root 'sevak.exe'
          Copy-Item -LiteralPath $exe -Destination $app
          if ($cert) {
            $signed = Set-AuthenticodeSignature -FilePath $app -Certificate $cert -HashAlgorithm SHA256
            if (-not $signed.SignerCertificate) { throw "Could not sign the $name fixture: $($signed.StatusMessage)" }
          }
          # sevak.exe is checked first, so these only need to exist.
          Copy-Item -LiteralPath $exe -Destination (Join-Path $root 'bundle/nsis/Sevak_x64-setup.exe')
          Set-Content -LiteralPath (Join-Path $root 'bundle/msi/Sevak_x64_en-US.msi') -Value ''
          $root
        }
        function Rejects([string] $root, [string] $pattern) {
          try { & $verify -BuildRoot $root -ExpectedThumbprint $expected.Thumbprint | Out-Null }
          catch {
            if ($_.Exception.Message -notmatch $pattern) { throw "$root failed for the wrong reason: $($_.Exception.Message)" }
            Write-Output "rejected: $(Split-Path -Leaf $root)"
            return
          }
          throw "$root was accepted"
        }
        Rejects (New-BuildRoot 'unsigned' $null) 'Invalid or missing Authenticode signature: sevak\.exe \(NotSigned\)'
        Rejects (New-BuildRoot 'untrusted' $expected) 'Invalid or missing Authenticode signature: sevak\.exe'
        $extra = New-BuildRoot 'two-installers' $expected
        Copy-Item -LiteralPath $exe -Destination (Join-Path $extra 'bundle/nsis/Stale_x64-setup.exe')
        Rejects $extra 'Expected exactly one nsis installer'

        # Trust only this test's certificates, so the remaining checks run on
        # genuine signatures instead of stopping at the untrusted chain.
        $testThumbprints = @($certs | ForEach-Object Thumbprint)
        function Get-AuthenticodeSignature([string] $LiteralPath) {
          $real = Microsoft.PowerShell.Security\Get-AuthenticodeSignature -LiteralPath $LiteralPath
          $status = [string]$real.Status
          if ($status -in @('UnknownError', 'NotTrusted') -and $real.SignerCertificate -and $real.SignerCertificate.Thumbprint -in $testThumbprints) { $status = 'Valid' }
          [pscustomobject]@{Status=$status; SignerCertificate=$real.SignerCertificate; TimeStamperCertificate=$real.TimeStamperCertificate}
        }
        Rejects (New-BuildRoot 'unsigned-shimmed' $null) '\(NotSigned\)'
        Rejects (New-BuildRoot 'wrong-publisher' $other) 'Unexpected publisher certificate: sevak\.exe'
        Rejects (New-BuildRoot 'no-timestamp' $expected) 'Missing trusted timestamp: sevak\.exe'
        Write-Output 'signature-gate-ok'
      } finally {
        foreach ($cert in $certs) { Remove-Item -LiteralPath "Cert:\CurrentUser\My\$($cert.Thumbprint)" -DeleteKey -ErrorAction SilentlyContinue }
      }
    `);
    const result = spawnSync("pwsh", ["-NoProfile", "-File", script], { encoding: "utf8", windowsHide: true, timeout: 110_000 });
    assert.equal(result.status, 0, result.stdout + result.stderr);
    assert.match(result.stdout, /signature-gate-ok/);
    for (const name of ["unsigned", "untrusted", "two-installers", "wrong-publisher", "no-timestamp"]) assert.match(result.stdout, new RegExp(`rejected: ${name}\\b`));
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test("VM maintenance uses explicit repair flags and cleanup attempts every installed product identity", { skip: !available && "PowerShell 7 unavailable" }, () => {
  const dir = mkdtempSync(join(tmpdir(), "sevak-vm-command-model-"));
  try {
    const script = join(dir, "commands.ps1");
    writeFileSync(script, String.raw`
      $ErrorActionPreference = 'Stop'
      $source = ${quote(join(root, "scripts/test-windows-installer-vm.ps1"))}
      $tokens=$null; $errors=$null
      $ast=[Management.Automation.Language.Parser]::ParseFile($source,[ref]$tokens,[ref]$errors)
      if ($errors.Count) { throw ($errors -join '; ') }
      # Extract only command-building functions. Never execute the suite body,
      # process runner, registry operations, or installation preflight here.
      foreach ($name in @('Invoke-SevakInstaller','Invoke-SevakUninstall')) {
        $fn=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true)
        . ([scriptblock]::Create($fn.Extent.Text))
      }
      $script:Calls=[Collections.Generic.List[object]]::new()
      function Invoke-SmokeProcess($File,$Arguments,$Expected,$Installation) { $script:Calls.Add(@{file=$File;arguments=$Arguments;expected=@($Expected)}) }
      function Assert-Smoke($Condition,$Message) { if (-not $Condition) { throw $Message } }
      function Get-SevakInstalledApps { }
      function Get-SevakStartLinks { }
      function Read-StartupValue { }
      $Format='Msi'; $script:InstallDir=${quote(join(dir, "install path"))}
      $script:LogDir=${quote(dir)}; $script:CommandNumber=0
      $env:WINDIR=${quote(dir)}
      $script:AttemptedArtifacts=[Collections.Generic.List[object]]::new()
      $script:InstallDirs=[Collections.Generic.List[string]]::new()
      $report=[ordered]@{}
      $first=@{path='first.msi';ProductCode='{FIRST}'}; $second=@{path='second.msi';ProductCode='{SECOND}'}
      Invoke-SevakInstaller $first
      Invoke-SevakInstaller $first '' $true
      Invoke-SevakInstaller $second
      Invoke-SevakUninstall $true
      $script:Calls | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath ${quote(join(dir, "calls.json"))}
      $report.cleanup | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath ${quote(join(dir, "msi-cleanup.json"))}
      # NSIS: a failed upgrade into a new folder must uninstall the first folder too.
      $script:Calls.Clear(); $Format='Nsis'; $report=[ordered]@{}
      $script:AttemptedArtifacts.Clear(); $script:InstallDirs.Clear()
      $firstDir=${quote(join(dir, "nsis first"))}; $secondDir=$firstDir + '-upgrade'
      foreach ($d in @($firstDir, $secondDir)) { [void][IO.Directory]::CreateDirectory($d); Set-Content -LiteralPath (Join-Path $d 'uninstall.exe') -Value 'stub' }
      $script:InstallDir=$firstDir; Invoke-SevakInstaller $first
      $script:InstallDir=$secondDir; Invoke-SevakInstaller $second
      Invoke-SevakUninstall $true
      $script:Calls | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath ${quote(join(dir, "nsis-calls.json"))}
      $report.cleanup | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath ${quote(join(dir, "nsis-cleanup.json"))}
    `);
    const result = run([script]);
    assert.equal(result.status, 0, result.stdout + result.stderr);
    const calls = JSON.parse(readFileSync(join(dir, "calls.json"), "utf8"));
    assert.equal(calls.length, 5);
    assert.doesNotMatch(calls[0].arguments, /REINSTALL=/);
    assert.match(calls[1].arguments, /REINSTALL=ALL REINSTALLMODE=amus/);
    assert.doesNotMatch(calls[2].arguments, /REINSTALL=/);
    assert.match(calls[3].arguments, /^\/x \{FIRST\}/);
    assert.match(calls[4].arguments, /^\/x \{SECOND\}/);
    assert.ok(calls[3].expected.includes(1605));
    const msiCleanup = JSON.parse(readFileSync(join(dir, "msi-cleanup.json"), "utf8"));
    assert.deepEqual(msiCleanup.productCodes, ["{FIRST}", "{SECOND}"]);
    assert.equal(msiCleanup.uninstallers.length, 2);
    const nsis = JSON.parse(readFileSync(join(dir, "nsis-calls.json"), "utf8")).filter((call) => /uninstall\.exe$/.test(call.file));
    assert.equal(nsis.length, 2, "both NSIS installation folders are uninstalled");
    assert.match(nsis[0].arguments, /_\?=.*nsis first$/);
    assert.match(nsis[1].arguments, /_\?=.*nsis first-upgrade$/);
    const nsisCleanup = JSON.parse(readFileSync(join(dir, "nsis-cleanup.json"), "utf8"));
    assert.equal(nsisCleanup.installDirectories.length, 2);
    assert.ok(nsisCleanup.uninstallers.every((entry) => entry.uninstallerRan === true));
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test("artifact inspection emits a failure report even when input resolution fails", { skip: !available && "PowerShell 7 unavailable" }, () => {
  const dir = mkdtempSync(join(tmpdir(), "sevak-inspector-failure-"));
  try {
    const path = join(dir, "failure.json");
    const result = run([join(root, "scripts/test-windows-installer.ps1"), "-MsiPath", join(dir, "missing.msi"), "-ReportPath", path]);
    assert.notEqual(result.status, 0);
    const report = JSON.parse(readFileSync(path, "utf8"));
    assert.equal(report.automaticStatus, "failed");
    assert.equal(report.installationExecuted, false);
    assert.equal(report.summary.passed, 0);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
