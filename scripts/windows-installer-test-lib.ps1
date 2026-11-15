# Pure/shared helpers. Dot-sourcing this file never installs, launches, or changes an OS setting.
function New-SevakValidationReport([string] $Suite, [string] $Kind) {
    [ordered]@{
        schemaVersion = 1; suite = $Suite; kind = $Kind
        startedAt = [DateTime]::UtcNow.ToString('o'); finishedAt = $null
        automaticStatus = 'not_run'; installationExecuted = $false
        checks = [Collections.Generic.List[object]]::new()
        commands = [Collections.Generic.List[object]]::new()
        artifacts = @(); errors = @()
        manualChecks = @(
            @{id='restart-sign-in'; status='not_run'; reason='Requires a real restart/sign-in and tray/hotkey observation.'},
            @{id='installer-ui-checkbox'; status='not_run'; reason='Silent installation does not exercise wizard controls.'},
            @{id='alternate-admin-uac'; status='not_run'; reason='Requires a second account and interactive UAC.'}
        )
    }
}

function Save-SevakValidationReport($Report, [string] $Path) {
    $Report.finishedAt = [DateTime]::UtcNow.ToString('o')
    $Report['summary'] = @{
        passed = @($Report.checks | Where-Object status -eq 'passed').Count
        failed = @($Report.checks | Where-Object status -eq 'failed').Count
        skipped = @($Report.checks | Where-Object status -eq 'skipped').Count
        notRun = @($Report.checks | Where-Object status -eq 'not_run').Count
        manualNotRun = @($Report.manualChecks | Where-Object status -eq 'not_run').Count
    }
    $resolved = [IO.Path]::GetFullPath($Path)
    [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($resolved))
    $Report | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $resolved -Encoding utf8
}

# Callers pass globs such as target/release/bundle/nsis/*.exe. An existing
# literal path wins (names may contain [ ]); otherwise the pattern must match
# exactly one file, so a stale second installer can never be picked silently.
function Resolve-SevakSinglePath([string] $Path, [string] $Name) {
    if (-not $Path) { throw "$Name is required." }
    if (Test-Path -LiteralPath $Path -PathType Leaf) { return (Resolve-Path -LiteralPath $Path).Path }
    if (-not [Management.Automation.WildcardPattern]::ContainsWildcardCharacters($Path)) {
        throw "$Name does not exist: $Path"
    }
    $found = @(Resolve-Path -Path $Path -ErrorAction SilentlyContinue | Where-Object { Test-Path -LiteralPath $_.Path -PathType Leaf })
    if ($found.Count -ne 1) {
        $list = if ($found.Count) { ': ' + (($found | ForEach-Object { $_.Path }) -join '; ') } else { '.' }
        throw "$Name pattern '$Path' must match exactly one file; it matched $($found.Count)$list"
    }
    $found[0].Path
}

# Stable report identifiers: every check in every report has an `id`.
function ConvertTo-SevakCheckId([string] $Text, [hashtable] $Used) {
    $id = ($Text.ToLowerInvariant() -replace '[^a-z0-9]+', '-').Trim('-')
    if ($id.Length -gt 80) { $id = $id.Substring(0, 80).TrimEnd('-') }
    if (-not $id) { $id = 'check' }
    if ($null -ne $Used) {
        $base = $id; $n = 1
        while ($Used.ContainsKey($id)) { $n++; $id = "$base-$n" }
        $Used[$id] = $true
    }
    $id
}

function Get-SevakVmGateFailures([hashtable] $Facts, [bool] $DisposableVm, [string] $Confirmation, [string] $Format) {
    if (-not $Facts.Windows) { 'Windows is required.' }
    if (-not $DisposableVm -or $Confirmation -cne 'I_ACKNOWLEDGE_THIS_IS_A_DISPOSABLE_VM' -or $Facts.EnvironmentOptIn -ne '1') {
        'Explicit disposable-VM switch, confirmation, and SEVAK_DISPOSABLE_VM=1 are all required.'
    }
    # HypervisorPresent alone also matches physical Windows hosts running Hyper-V/WSL.
    # The hardware check still matches persistent VMs (Dev Box, Windows 365 Cloud
    # PCs, long-lived Hyper-V or Parallels guests); only the explicit opt-ins
    # protect those, so it narrows the gate but never replaces them.
    if ("$($Facts.Manufacturer) $($Facts.Model)" -notmatch '(?i)Virtual Machine|VMware|VirtualBox|innotek|KVM|QEMU|HVM domU|Amazon EC2|Google Compute Engine|Parallels') {
        'Hardware identity does not identify a disposable virtual machine.'
    }
    if ($Facts.UserSid -in @('S-1-5-18', 'S-1-5-19', 'S-1-5-20')) { 'Run as a test user, never SYSTEM or a built-in service account.' }
    if ($Format -eq 'Nsis' -and ($Facts.Elevated -or -not $Facts.DesktopShell)) {
        'NSIS acceptance requires an unelevated signed-in desktop user with Explorer running.'
    }
    if ($Format -eq 'Msi' -and -not $Facts.Elevated) { 'MSI acceptance requires an elevated test-user session.' }
}

function Get-SevakCoreVersion([string] $Version) {
    if ($Version -notmatch '^(\d+)\.(\d+)\.(\d+)(?:[.-]|$)') { throw "Cannot compare installer version: $Version" }
    [Version]::new([int]$Matches[1], [int]$Matches[2], [int]$Matches[3])
}

function Get-SevakSavedStartup([string] $Text) {
    $general = [regex]::Match($Text, '(?ms)^\s*\[general\]\s*\r?\n(?<body>.*?)(?=^\s*\[|\z)')
    $value = [regex]::Match($general.Groups['body'].Value, '(?m)^\s*launch_at_login\s*=\s*(true|false)\s*(?:#.*)?$')
    if (-not $value.Success) { throw 'Missing general.launch_at_login in the generated settings.' }
    $value.Groups[1].Value -eq 'true'
}

function Get-SevakRunParts([string] $Command) {
    if ($Command -notmatch '^"(?<exe>[^"]+)"(?<arguments>.*)$') { throw 'Startup command must contain a quoted executable path.' }
    @{executable=$Matches.exe; arguments=$Matches.arguments}
}

function Get-SevakArtifact([string] $Path) {
    $resolved = (Resolve-Path -LiteralPath $Path).Path
    $item = Get-Item -LiteralPath $resolved
    if ($item.PSIsContainer) { throw 'Expected an installer file.' }
    $result = [ordered]@{path=$resolved; sha256=(Get-FileHash -LiteralPath $resolved -Algorithm SHA256).Hash.ToLowerInvariant(); bytes=$item.Length}
    if ($item.Extension -eq '.msi') {
        $engine = New-Object -ComObject WindowsInstaller.Installer
        $db = $null; $view = $null
        try {
            $db = $engine.OpenDatabase($resolved, 0)
            foreach ($property in @('ProductName', 'ProductVersion', 'ProductCode', 'UpgradeCode')) {
                $view = $db.OpenView("SELECT ``Value`` FROM ``Property`` WHERE ``Property`` = '$property'")
                [void]$view.Execute()
                $row = $view.Fetch()
                if (-not $row) { throw "MSI is missing $property." }
                $result[$property] = $row.StringData(1)
                [void]$view.Close()
                [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($view)
                $view = $null
            }
        } finally {
            foreach ($obj in @($view, $db, $engine)) { if ($obj) { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($obj) } }
        }
    } else {
        $result['ProductName'] = $item.VersionInfo.ProductName
        $result['ProductVersion'] = $item.VersionInfo.ProductVersion
        $result['FileVersion'] = $item.VersionInfo.FileVersion
    }
    $result
}
