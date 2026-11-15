#Requires -Version 7.0
<#
Actual installer acceptance tests. NEVER run on a developer's normal machine.
Requires an explicitly opted-in disposable Windows VM and a clean test account.
NSIS: normal desktop-user PowerShell. MSI: elevated test-user PowerShell.
No reboot or UAC interaction is automated; those checks remain not_run in JSON.
#>
# Path parameters accept a quoted wildcard pattern that must match exactly one
# file. Positional binding is off so an expanded multi-file glob cannot bind to
# other parameters.
[CmdletBinding(PositionalBinding=$false)]
param(
    [ValidateSet('Nsis','Msi')][string] $Format = 'Msi',
    [string] $InstallerPath,
    [string] $UpgradeInstallerPath,
    [string] $ExpectedVersion,
    [string] $UpgradeExpectedVersion,
    [switch] $DisposableVm,
    [string] $Confirmation,
    [string] $ReportPath = (Join-Path $PSScriptRoot '../target/installer-validation/vm.json'),
    [int] $TimeoutSeconds = 180
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-installer-test-lib.ps1')
$report = New-SevakValidationReport 'windows-installer-vm' 'actual-installation'
$report['format'] = $Format
$script:GatePassed = $false
$script:ActiveInstall = $false
$script:CurrentArtifact = $null
$script:AttemptedArtifacts = [Collections.Generic.List[object]]::new()
$script:InstallDir = $null
# Every installation folder used, in order. NSIS upgrades move to a new folder,
# so cleanup after a failed upgrade must cover the first one too.
$script:InstallDirs = [Collections.Generic.List[string]]::new()
$script:CommandNumber = 0
$script:LogDir = $null
$configFile = $null
$runKey = 'Software\Microsoft\Windows\CurrentVersion\Run'
$approvedKey = 'Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run'
$caseIds = @('preflight', 'clean-install', 'installed-apps-and-start-menu', 'default-startup-off',
    'installer-startup-on', 'installer-startup-off', 'same-version-reinstall',
    'os-disable-preserved', 'upgrade', 'owned-uninstall', 'cleanup')
foreach ($id in $caseIds) { $report.checks.Add(@{id=$id; status='not_run'}) }

function Assert-Smoke([bool] $Condition, [string] $Message) { if (-not $Condition) { throw $Message } }
function Invoke-SmokeCase([string] $Id, [scriptblock] $Action) {
    $case = $report.checks | Where-Object id -eq $Id
    $case.startedAt = [DateTime]::UtcNow.ToString('o')
    try { & $Action; $case.status = 'passed' }
    catch { $case.status = 'failed'; $case.error = $_.Exception.Message; throw }
    finally { $case.finishedAt = [DateTime]::UtcNow.ToString('o') }
}
function Skip-SmokeCase([string] $Id, [string] $Reason) {
    $case = $report.checks | Where-Object id -eq $Id
    $case.status = 'skipped'; $case.reason = $Reason
}
function Read-StartupValue([bool] $Machine = $false, [bool] $Approval = $false) {
    $hive = if ($Machine) { [Microsoft.Win32.Registry]::LocalMachine } else { [Microsoft.Win32.Registry]::CurrentUser }
    $key = $hive.OpenSubKey($(if ($Approval) {$approvedKey} else {$runKey}))
    try { if ($key) { return $key.GetValue('Sevak', $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames) } }
    finally { if ($key) { $key.Dispose() } }
}
function Get-SevakInstalledApps {
    foreach ($root in @('HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall',
        'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall',
        'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall')) {
        if (Test-Path -LiteralPath $root) {
            Get-ChildItem -LiteralPath $root | ForEach-Object { Get-ItemProperty -LiteralPath $_.PSPath } |
                Where-Object DisplayName -eq 'Sevak'
        }
    }
}
function Get-SevakStartLinks {
    $shell = New-Object -ComObject WScript.Shell
    try {
        foreach ($folder in @([Environment]::GetFolderPath('Programs'), [Environment]::GetFolderPath('CommonPrograms'))) {
            foreach ($relative in @('Sevak/Sevak.lnk', 'Sevak.lnk')) {
                $link = Join-Path $folder $relative
                if (Test-Path -LiteralPath $link) {
                    $shortcut = $shell.CreateShortcut($link)
                    [pscustomobject]@{path=$link; target=$shortcut.TargetPath}
                    [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($shortcut)
                }
            }
        }
    } finally { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($shell) }
}
function Invoke-SmokeProcess([string] $File, [string] $Arguments, [int[]] $Expected = @(0), [bool] $Installation = $false) {
    if (-not $script:GatePassed) { throw 'Installation gate has not passed.' }
    $script:CommandNumber++
    $command = @{file=$File; arguments=$Arguments; startedAt=[DateTime]::UtcNow.ToString('o'); exitCode=$null}
    $report.commands.Add($command)
    if ($Installation) { $report.installationExecuted = $true }
    $process = Start-Process -FilePath $File -ArgumentList $Arguments -WindowStyle Hidden -PassThru
    # Caching the handle now keeps ExitCode available after exit; without it
    # Start-Process -PassThru can report a null exit code.
    $null = $process.Handle
    if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
        $process.Kill($true)
        throw "Process timed out: $File (VM should be discarded)."
    }
    $process.Refresh()
    $command.exitCode = $process.ExitCode
    $command.finishedAt = [DateTime]::UtcNow.ToString('o')
    if ($process.ExitCode -eq 3010) { $report['rebootRequestedByInstaller'] = $true }
    Assert-Smoke ($process.ExitCode -in $Expected) "Unexpected exit $($process.ExitCode) from $File; expected $($Expected -join ',')."
}
function Invoke-SevakInstaller($Artifact, [string] $Startup = '', [bool] $Reinstall = $false) {
    $script:CurrentArtifact = $Artifact
    if ($Artifact -notin $script:AttemptedArtifacts) { $script:AttemptedArtifacts.Add($Artifact) }
    if ($script:InstallDir -notin $script:InstallDirs) { $script:InstallDirs.Add($script:InstallDir) }
    # Track possible partial installs too; cleanup is attempted if setup fails.
    $script:ActiveInstall = $true
    if ($Format -eq 'Nsis') {
        $arguments = '/S /CURRENTUSER'
        if ($Startup) { $arguments += " /AUTOSTART=$Startup" }
        # NSIS requires /D to be last and its path unquoted, including spaces.
        $arguments += " /D=$script:InstallDir"
        Invoke-SmokeProcess $Artifact.path $arguments @(0) $true
    } else {
        $log = Join-Path $script:LogDir "msi-$script:CommandNumber.log"
        $arguments = "/i `"$($Artifact.path)`" /qn /norestart /L*v `"$log`" INSTALLDIR=`"$script:InstallDir`""
        if ($Startup) { $arguments += " SEVAK_AUTOSTART=$Startup" }
        if ($Reinstall) { $arguments += ' REINSTALL=ALL REINSTALLMODE=amus' }
        Invoke-SmokeProcess (Join-Path $env:WINDIR 'System32/msiexec.exe') $arguments @(0,3010) $true
    }
}
function Invoke-SevakUninstall([bool] $Cleanup = $false) {
    # Cleanup covers every folder and product identity this run installed;
    # a normal uninstall covers only the current one.
    $directories = if ($Cleanup -and $script:InstallDirs.Count) { @($script:InstallDirs) } else { @($script:InstallDir) }
    if ($Cleanup) {
        $report['cleanup'] = [ordered]@{
            installDirectories = @($directories)
            productCodes = @(if ($Format -eq 'Msi') { $script:AttemptedArtifacts | ForEach-Object { $_.ProductCode } })
            uninstallers = [Collections.Generic.List[object]]::new()
        }
    }
    if ($Format -eq 'Nsis') {
        foreach ($directory in $directories) {
            $uninstall = Join-Path $directory 'uninstall.exe'
            $ran = Test-Path -LiteralPath $uninstall
            if ($ran) { Invoke-SmokeProcess $uninstall "/S /CURRENTUSER _?=$directory" @(0) $true }
            if ($Cleanup) { $report.cleanup.uninstallers.Add([ordered]@{installDirectory=$directory; uninstallerRan=$ran}) }
        }
    } else {
        $artifacts = if ($Cleanup) { @($script:AttemptedArtifacts) } else { @($script:CurrentArtifact) }
        foreach ($artifact in $artifacts) {
            $log = Join-Path $script:LogDir "uninstall-$script:CommandNumber.log"
            Invoke-SmokeProcess (Join-Path $env:WINDIR 'System32/msiexec.exe') "/x $($artifact.ProductCode) /qn /norestart /L*v `"$log`"" @($(if ($Cleanup) {0,1605,3010} else {0,3010})) $true
            if ($Cleanup) { $report.cleanup.uninstallers.Add([ordered]@{productCode=$artifact.ProductCode; uninstallerRan=$true}) }
        }
    }
    Assert-Smoke (@(Get-SevakInstalledApps).Count -eq 0) 'Cleanup left an Installed apps registration; restore the VM snapshot.'
    Assert-Smoke (@(Get-SevakStartLinks).Count -eq 0) 'Cleanup left a Start menu entry; restore the VM snapshot.'
    Assert-Smoke (-not (Read-StartupValue)) 'Cleanup left startup registration; restore the VM snapshot.'
    foreach ($directory in $directories) {
        Assert-Smoke (-not (Test-Path -LiteralPath (Join-Path $directory 'sevak.exe'))) "Cleanup left the test executable in $directory; restore the VM snapshot."
    }
    $script:ActiveInstall = $false
}
function Assert-Installed([string] $Expected = '') {
    $binary = Join-Path $script:InstallDir 'sevak.exe'
    Assert-Smoke (Test-Path -LiteralPath $binary) 'Installed executable is missing.'
    $apps = @(Get-SevakInstalledApps)
    Assert-Smoke ($apps.Count -eq 1) "Expected exactly one Installed apps entry, found $($apps.Count)."
    Assert-Smoke ($apps[0].InstallLocation.Trim('"').TrimEnd('\') -eq $script:InstallDir.TrimEnd('\')) 'Installed apps points at the wrong installation folder.'
    $links = @(Get-SevakStartLinks)
    Assert-Smoke ($links.Count -eq 1 -and $links[0].target -eq $binary) 'Expected one Start menu entry pointing at the installed executable.'
    Assert-Smoke (-not (Read-StartupValue -Machine $true)) 'Installer unexpectedly created machine-wide startup.'
    if ($Expected) { Assert-Smoke ((Get-Item -LiteralPath $binary).VersionInfo.ProductVersion -eq $Expected) 'Installed binary embeds an unexpected version.' }
}
function Assert-Startup([bool] $Enabled, [bool] $Saved) {
    $binary = Join-Path $script:InstallDir 'sevak.exe'
    Invoke-SmokeProcess $binary '--startup-status' @($(if ($Enabled) {0} else {1}))
    if ($Enabled) {
        $parts = Get-SevakRunParts (Read-StartupValue)
        Assert-Smoke ($parts.executable -eq $binary -and $parts.arguments -match '--background') 'Startup must target the installed executable in background mode.'
    } else { Assert-Smoke (-not (Read-StartupValue)) 'Opt-out must remove the Run value.' }
    Assert-Smoke ((Get-SevakSavedStartup (Get-Content -LiteralPath $configFile -Raw)) -eq $Saved) 'Persisted startup preference disagrees with the installer.'
    Assert-Smoke (-not (Read-StartupValue -Machine $true)) 'Startup must remain per-user.'
}

try {
    Invoke-SmokeCase 'preflight' {
        Assert-Smoke $IsWindows 'Windows is required.'
        $script:configFile = Join-Path $env:APPDATA 'sevak/config.toml'
        $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
        $principal = [Security.Principal.WindowsPrincipal]::new($identity)
        $computer = Get-CimInstance Win32_ComputerSystem
        $facts = @{
            Windows=$IsWindows; Manufacturer=$computer.Manufacturer; Model=$computer.Model
            EnvironmentOptIn=$env:SEVAK_DISPOSABLE_VM; UserSid=$identity.User.Value
            Elevated=$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
            DesktopShell=@(Get-Process explorer -ErrorAction SilentlyContinue | Where-Object SessionId -eq (Get-Process -Id $PID).SessionId).Count -gt 0
        }
        $report['environment'] = $facts
        $failures = @(Get-SevakVmGateFailures $facts $DisposableVm.IsPresent $Confirmation $Format)
        Assert-Smoke ($failures.Count -eq 0) ($failures -join ' ')
        Assert-Smoke ($TimeoutSeconds -ge 30 -and $TimeoutSeconds -le 900) 'TimeoutSeconds must be between 30 and 900.'
        Assert-Smoke ($InstallerPath -ne '') 'InstallerPath is required.'
        $InstallerPath = Resolve-SevakSinglePath $InstallerPath 'InstallerPath'
        $script:InitialArtifact = Get-SevakArtifact $InstallerPath
        $report.artifacts += $script:InitialArtifact
        Assert-Smoke ($script:InitialArtifact.ProductName -eq 'Sevak') 'Installer does not identify itself as Sevak.'
        Assert-Smoke ([IO.Path]::GetExtension($InstallerPath) -eq $(if ($Format -eq 'Msi') {'.msi'} else {'.exe'})) 'Installer extension does not match Format.'
        if ($Format -eq 'Msi') { Assert-Smoke ($script:InitialArtifact.UpgradeCode -eq '{16B13FDE-6FA0-4BF2-836A-9AF38755BFEC}') 'Unexpected MSI upgrade identity.' }
        if ($UpgradeInstallerPath) {
            $UpgradeInstallerPath = Resolve-SevakSinglePath $UpgradeInstallerPath 'UpgradeInstallerPath'
            $script:UpgradeArtifact = Get-SevakArtifact $UpgradeInstallerPath
            $report.artifacts += $script:UpgradeArtifact
            Assert-Smoke ($script:UpgradeArtifact.ProductName -eq 'Sevak') 'Upgrade installer is not Sevak.'
            Assert-Smoke ([IO.Path]::GetExtension($UpgradeInstallerPath) -eq [IO.Path]::GetExtension($InstallerPath)) 'Upgrade installer format must match the baseline.'
            if ($Format -eq 'Msi') { Assert-Smoke ($script:UpgradeArtifact.UpgradeCode -eq $script:InitialArtifact.UpgradeCode) 'Upgrade MSI has a different upgrade identity.' }
            Assert-Smoke ((Get-SevakCoreVersion $script:UpgradeArtifact.ProductVersion) -gt (Get-SevakCoreVersion $script:InitialArtifact.ProductVersion)) 'A real upgrade requires a higher major.minor.patch version, not only a new filename or fourth MSI field.'
        }
        Assert-Smoke (@(Get-SevakInstalledApps).Count -eq 0) 'VM account is not clean: Sevak is already installed.'
        Assert-Smoke (@(Get-SevakStartLinks).Count -eq 0) 'VM account is not clean: Sevak Start menu entries exist.'
        Assert-Smoke (@(Get-Process sevak -ErrorAction SilentlyContinue).Count -eq 0) 'VM account is not clean: Sevak is running.'
        Assert-Smoke (-not (Test-Path -LiteralPath (Join-Path $env:APPDATA 'sevak'))) 'VM account is not clean: Sevak settings/data exist. Restore a snapshot.'
        Assert-Smoke (-not (Read-StartupValue) -and -not (Read-StartupValue -Machine $true) -and -not (Read-StartupValue -Approval $true)) 'VM account is not clean: a Sevak startup registration or approval exists.'
        $token = [Guid]::NewGuid().ToString('N')
        $base = if ($Format -eq 'Msi') { $env:ProgramFiles } else { $env:LOCALAPPDATA }
        $script:InstallDir = Join-Path $base "Sevak smoke $token"
        $script:LogDir = Join-Path ([IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($ReportPath))) "logs-$token"
        [void][IO.Directory]::CreateDirectory($script:LogDir)
        $report['installDirectory'] = $script:InstallDir
        $report['logsDirectory'] = $script:LogDir
        $script:GatePassed = $true
    }
    Invoke-SmokeCase 'clean-install' { Invoke-SevakInstaller $script:InitialArtifact }
    Invoke-SmokeCase 'installed-apps-and-start-menu' { Assert-Installed $ExpectedVersion }
    Invoke-SmokeCase 'default-startup-off' {
        Assert-Smoke (-not (Read-StartupValue)) 'Fresh silent install unexpectedly enabled startup.'
        Invoke-SmokeProcess (Join-Path $script:InstallDir 'sevak.exe') '--startup-status' @(1)
    }
    Invoke-SmokeCase 'installer-startup-on' { Invoke-SevakInstaller $script:InitialArtifact 'on'; Assert-Startup $true $true }
    Invoke-SmokeCase 'installer-startup-off' { Invoke-SevakInstaller $script:InitialArtifact 'off'; Assert-Startup $false $false }
    Invoke-SmokeCase 'same-version-reinstall' {
        $before = (Get-FileHash -LiteralPath $configFile).Hash
        # Remove only the executable just installed into this suite's unique VM
        # folder, then require the installer to restore it (MSI REINSTALL=ALL).
        Remove-Item -LiteralPath (Join-Path $script:InstallDir 'sevak.exe') -Force
        Invoke-SevakInstaller $script:InitialArtifact '' $true
        Assert-Installed $ExpectedVersion
        Assert-Startup $false $false
        Assert-Smoke ((Get-FileHash -LiteralPath $configFile).Hash -eq $before) 'Reinstall rewrote unrelated settings.'
    }
    Invoke-SmokeCase 'os-disable-preserved' {
        Invoke-SmokeProcess (Join-Path $script:InstallDir 'sevak.exe') '--set-startup on'
        $script:BeforeRun = Read-StartupValue
        $script:DisabledApproval = [byte[]](3,0,0,0,0,0,0,0,0,0,0,0)
        # This mutation is deliberately behind the VM gate and applies only to
        # the entry created by the suite in the initially empty test account.
        $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($approvedKey)
        try { $key.SetValue('Sevak', $script:DisabledApproval, [Microsoft.Win32.RegistryValueKind]::Binary) } finally { $key.Dispose() }
        Add-Content -LiteralPath $configFile -Value "`n# Sevak VM smoke preserved-settings sentinel"
        $script:BeforeConfigHash = (Get-FileHash -LiteralPath $configFile).Hash
        Invoke-SevakInstaller $script:InitialArtifact
        Invoke-SmokeProcess (Join-Path $script:InstallDir 'sevak.exe') '--startup-status' @(1)
        Assert-Smoke ((Read-StartupValue) -eq $script:BeforeRun) 'Unchanged reinstall rewrote the startup command.'
        Assert-Smoke ([Convert]::ToHexString([byte[]](Read-StartupValue -Approval $true)) -eq [Convert]::ToHexString($script:DisabledApproval)) 'Reinstall changed Windows disabled state.'
        Assert-Smoke ((Get-FileHash -LiteralPath $configFile).Hash -eq $script:BeforeConfigHash) 'Reinstall rewrote the saved preference.'
    }
    if ($UpgradeInstallerPath) {
        Invoke-SmokeCase 'upgrade' {
            # MSI remembers its installation folder via AppSearch. A moved MSI
            # upgrade needs a separate manual scenario; NSIS supports /D here.
            if ($Format -eq 'Nsis') { $script:InstallDir += '-upgrade' }
            Invoke-SevakInstaller $script:UpgradeArtifact
            Assert-Installed $UpgradeExpectedVersion
            $before = Get-SevakRunParts $script:BeforeRun
            $after = Get-SevakRunParts (Read-StartupValue)
            Assert-Smoke ($after.executable -eq (Join-Path $script:InstallDir 'sevak.exe') -and $after.arguments -eq $before.arguments) 'Upgrade did not refresh the target while preserving startup arguments.'
            Invoke-SmokeProcess (Join-Path $script:InstallDir 'sevak.exe') '--startup-status' @(1)
            Assert-Smoke ([Convert]::ToHexString([byte[]](Read-StartupValue -Approval $true)) -eq [Convert]::ToHexString($script:DisabledApproval)) 'Upgrade reenables Windows-disabled startup.'
            Assert-Smoke ((Get-FileHash -LiteralPath $configFile).Hash -eq $script:BeforeConfigHash) 'Upgrade rewrote settings.'
        }
    } else { Skip-SmokeCase 'upgrade' 'A second installer with a higher major.minor.patch version was not supplied; same-version reinstall is reported separately.' }
    Invoke-SmokeCase 'owned-uninstall' {
        Invoke-SmokeProcess (Join-Path $script:InstallDir 'sevak.exe') '--set-startup on'
        $before = (Get-FileHash -LiteralPath $configFile).Hash
        Invoke-SevakUninstall
        Assert-Smoke (-not (Test-Path -LiteralPath (Join-Path $script:InstallDir 'sevak.exe'))) 'Uninstall left the main executable.'
        Assert-Smoke (@(Get-SevakInstalledApps).Count -eq 0) 'Uninstall left an Installed apps entry.'
        Assert-Smoke (@(Get-SevakStartLinks).Count -eq 0) 'Uninstall left a Start menu entry.'
        Assert-Smoke (-not (Read-StartupValue)) 'Uninstall left this executable registered for startup.'
        Assert-Smoke ((Get-FileHash -LiteralPath $configFile).Hash -eq $before) 'Uninstall changed retained settings.'
    }
    Skip-SmokeCase 'cleanup' 'Normal uninstall completed; the disposable VM retains test settings for inspection and must be discarded.'
    $report.automaticStatus = 'passed'
} catch {
    $original = $_
    $report.automaticStatus = 'failed'
    $report.errors += $original.Exception.Message
    if ($script:GatePassed -and $script:ActiveInstall) {
        try { Invoke-SmokeCase 'cleanup' { Invoke-SevakUninstall $true } }
        catch { $report.errors += "Cleanup: $($_.Exception.Message)" }
    } elseif ($script:GatePassed) {
        Skip-SmokeCase 'cleanup' 'No test installation was active when the failure occurred.'
    }
    # The test failure, not a cleanup failure, is the error this run reports.
    Write-Error $original -ErrorAction Continue
} finally {
    # Never let a report-writing problem hide the test result or its error.
    try { Save-SevakValidationReport $report $ReportPath }
    catch {
        $report.automaticStatus = 'failed'
        Write-Error "Could not write the validation report ${ReportPath}: $($_.Exception.Message)" -ErrorAction Continue
    }
    Write-Output "Automated $Format VM result: $($report.automaticStatus). Real restart/sign-in, wizard UI and alternate-user UAC remain NOT RUN. Report: $ReportPath"
}
if ($report.automaticStatus -ne 'passed') { exit 1 }
