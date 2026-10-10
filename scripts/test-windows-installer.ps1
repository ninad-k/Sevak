# Read-only integration checks against the compiled MSI. No installation,
# registry mutation, app launch, or change to the user's Sevak configuration.
# Run after `npm run tauri -- build --bundles nsis,msi` on Windows.
param([string] $MsiPath)
$ErrorActionPreference = 'Stop'
if (-not $MsiPath) {
    $candidates = @(Get-ChildItem -LiteralPath (Join-Path $PSScriptRoot '../target/release/bundle/msi') -Filter '*.msi')
    if ($candidates.Count -ne 1) { throw 'Build exactly one MSI, or supply -MsiPath.' }
    $MsiPath = $candidates[0].FullName
}
$MsiPath = (Resolve-Path -LiteralPath $MsiPath).Path
$installer = New-Object -ComObject WindowsInstaller.Installer
$installer.UILevel = 2 # no installer UI while opening a read-only test session
$database = $installer.OpenDatabase($MsiPath, 0)
# IgnoreMachineState avoids depending on which Sevak version is installed.
# Opening a package creates a session but does NOT run installation actions.
$session = $installer.OpenPackage($MsiPath, 1)
$script:checks = 0

function Assert-That([bool] $condition, [string] $message) {
    if (-not $condition) { throw $message }
    $script:checks++
}
function Read-Rows([string] $query, [int] $fields) {
    $view = $database.OpenView($query)
    [void]$view.Execute()
    try {
        while ($record = $view.Fetch()) {
            $row = @()
            for ($i = 1; $i -le $fields; $i++) { $row += $record.StringData($i) }
            ,$row
        }
    } finally { [void]$view.Close() }
}
function Condition-For([string] $action, [string] $table) {
    $rows = @(Read-Rows ('SELECT `Condition` FROM `' + $table + '` WHERE `Action` = ''' + $action + '''') 1)
    if ($rows.Count -ne 1) { throw "Missing or duplicate action $action in $table" }
    $rows[0][0]
}
function Check-Condition([string] $condition, [hashtable] $properties, [bool] $expected, [string] $description) {
    $values = @{
        Installed = ''; REMOVE = ''; WIX_UPGRADE_DETECTED = ''; SEVAK_EXISTING_INSTALL = '';
        SEVAK_STARTUP_CHECKED = ''; SEVAK_STARTUP_INITIAL = '0'; SEVAK_AUTOSTART = '';
        SEVAK_EXISTING_RUN = ''; SEVAK_STARTUP_APPROVAL = ''; UPGRADINGPRODUCTCODE = '';
        UserSID = 'S-1-5-21-1000-1000-1000-1001'
    }
    foreach ($key in $properties.Keys) { $values[$key] = $properties[$key] }
    foreach ($key in $values.Keys) { $session.Property($key) = $values[$key] }
    $actual = $session.EvaluateCondition($condition)
    Assert-That ($actual -eq [int]$expected) "$description (condition returned $actual): $condition"
}

try {
    $properties = @{}
    foreach ($row in (Read-Rows 'SELECT `Property`, `Value` FROM `Property`' 2)) { $properties[$row[0]] = $row[1] }
    Assert-That ($properties.ProductName -eq 'Sevak') 'Product must register as Sevak in Installed apps.'
    Assert-That ($properties.UpgradeCode -eq '{16B13FDE-6FA0-4BF2-836A-9AF38755BFEC}') 'MSI upgrade identity changed.'
    Assert-That (-not $properties.ContainsKey('SEVAK_STARTUP_CHECKED')) 'Fresh installs must not opt into startup by default.'
    Assert-That (-not $properties.ContainsKey('SEVAK_AUTOSTART')) 'Silent installs must preserve the preference by default.'
    $startupArgumentRule = @(Read-Rows 'SELECT `Condition`, `Description` FROM `LaunchCondition`' 2 | Where-Object { $_[1].StartsWith('SEVAK_AUTOSTART') })
    Assert-That ($startupArgumentRule.Count -eq 1) 'Missing startup argument validation.'
    foreach ($value in @('', 'on', 'off')) {
        Check-Condition $startupArgumentRule[0][0] @{SEVAK_AUTOSTART=$value} $true 'Valid installer startup argument'
    }
    Check-Condition $startupArgumentRule[0][0] @{SEVAK_AUTOSTART='invalid'} $false 'Invalid startup argument fails before installation'
    $shortcuts = @(Read-Rows 'SELECT `Shortcut`, `Directory_`, `Target` FROM `Shortcut`' 3)
    Assert-That (@($shortcuts | Where-Object { $_[0] -eq 'ApplicationStartMenuShortcut' -and $_[1] -eq 'ApplicationProgramsFolder' }).Count -eq 1) 'Start menu app registration missing.'

    $actions = @{}
    foreach ($row in (Read-Rows 'SELECT `Action`, `Type`, `Source`, `Target` FROM `CustomAction`' 4)) { $actions[$row[0]] = $row }
    foreach ($pair in @(@('SevakEnableStartup', '--set-startup on'), @('SevakDisableStartup', '--set-startup off'), @('SevakRefreshStartup', '--refresh-startup'), @('SevakRemoveStartup', '--remove-startup'))) {
        $row = $actions[$pair[0]]
        Assert-That ($null -ne $row -and $row[2] -eq 'Path' -and $row[3] -eq $pair[1]) "Wrong startup helper command: $($pair[0])"
        Assert-That (([int]$row[1] -band 0x800) -eq 0) 'Startup must not run with a no-impersonation system action.'
    }
    $sequence = @{}
    foreach ($row in (Read-Rows 'SELECT `Action`, `Sequence` FROM `InstallExecuteSequence`' 2)) { $sequence[$row[0]] = [int]$row[1] }
    Assert-That ($sequence.InstallFinalize -lt $sequence.SevakEnableStartup) 'Files must be committed before startup helpers run.'
    Assert-That ($sequence.SevakEnableStartup -lt $sequence.SevakDisableStartup -and $sequence.SevakDisableStartup -lt $sequence.SevakRefreshStartup -and $sequence.SevakRefreshStartup -lt $sequence.LaunchApplication) 'Automatic launch must follow all startup helpers so app sync cannot race installer settings.'

    $prefill = Condition-For 'SetSEVAK_STARTUP_CHECKED' 'InstallUISequence'
    Check-Condition $prefill @{} $false 'Fresh startup checkbox is unchecked'
    Check-Condition $prefill @{SEVAK_EXISTING_RUN='"C:\Sevak\sevak.exe" --background'} $true 'Existing startup is checked'
    foreach ($state in @('03', '07', '63')) {
        Check-Condition $prefill @{SEVAK_EXISTING_RUN='old'; SEVAK_STARTUP_APPROVAL="#x${state}0000000000000000000000"} $false 'Windows-disabled startup stays unchecked'
    }
    foreach ($state in @('02', '06')) {
        Check-Condition $prefill @{SEVAK_EXISTING_RUN='old'; SEVAK_STARTUP_APPROVAL="#x${state}0000000000000000000000"} $true 'Windows-enabled startup is checked'
    }
    foreach ($state in @('#x020100000000000000000000', '#x060100000000000000000000')) {
        Check-Condition $prefill @{SEVAK_EXISTING_RUN='old'; SEVAK_STARTUP_APPROVAL=$state} $false 'Unknown DWORD states must not alias enabled states by their low byte'
    }
    Check-Condition $prefill @{SEVAK_AUTOSTART='on'} $true 'Explicit on is checked'
    Check-Condition $prefill @{SEVAK_EXISTING_RUN='old'; SEVAK_AUTOSTART='off'} $false 'Explicit off overrides an existing entry'

    $finishEvents = @(Read-Rows 'SELECT `Argument`, `Condition`, `Ordering` FROM `ControlEvent` WHERE `Dialog_` = ''SevakExitDialog'' AND `Control_` = ''Finish''' 3)
    $enable = ($finishEvents | Where-Object { $_[0] -eq 'SevakEnableStartup' })[1]
    $disable = ($finishEvents | Where-Object { $_[0] -eq 'SevakDisableStartup' })[1]
    Assert-That ($enable -and $disable) 'Finish screen must apply both on and off selections.'
    Check-Condition $enable @{SEVAK_STARTUP_CHECKED='1'} $true 'Fresh selected checkbox enables startup'
    Check-Condition $disable @{} $true 'Fresh unchecked checkbox writes off even if old config remains'
    Check-Condition $enable @{Installed='1'; SEVAK_STARTUP_CHECKED='1'; SEVAK_STARTUP_INITIAL='1'} $false 'Unchanged upgrade does not reset Windows disable state'
    Check-Condition $disable @{WIX_UPGRADE_DETECTED='old'; SEVAK_EXISTING_RUN='old'} $false 'Untouched disabled upgrade stays unchanged'
    Check-Condition $disable @{WIX_UPGRADE_DETECTED='old'} $true 'Unchecked upgrade with missing Run entry persists off'
    Check-Condition $enable @{Installed='1'; SEVAK_STARTUP_CHECKED='1'} $true 'Opt-in on upgrade enables startup'
    Check-Condition $disable @{Installed='1'; SEVAK_STARTUP_INITIAL='1'} $true 'Unchecking on upgrade disables startup'
    Check-Condition $enable @{REMOVE='ALL'; SEVAK_STARTUP_CHECKED='1'} $false 'Uninstall Finish never enables startup'
    Check-Condition $disable @{REMOVE='ALL'} $false 'Uninstall Finish preserves stored config'

    $silentOn = Condition-For 'SevakEnableStartup' 'InstallExecuteSequence'
    $silentOff = Condition-For 'SevakDisableStartup' 'InstallExecuteSequence'
    Check-Condition $silentOn @{} $false 'Silent default never enables'
    Check-Condition $silentOff @{} $false 'Silent default never disables'
    Check-Condition $silentOn @{SEVAK_AUTOSTART='on'} $true 'Silent explicit on applies'
    Check-Condition $silentOff @{SEVAK_AUTOSTART='off'} $true 'Silent explicit off applies'
    foreach ($sid in @('S-1-5-18','S-1-5-19','S-1-5-20')) {
        Check-Condition $silentOn @{SEVAK_AUTOSTART='on'; UserSID=$sid} $false 'Built-in service accounts are never enrolled'
    }
    $refresh = Condition-For 'SevakRefreshStartup' 'InstallExecuteSequence'
    Check-Condition $refresh @{} $false 'Fresh installs do not refresh an unrelated entry'
    Check-Condition $refresh @{WIX_UPGRADE_DETECTED='old'} $true 'Upgrade refresh preserves startup status at a moved path'
    Check-Condition $refresh @{Installed='1'; REMOVE='ALL'} $false 'Uninstall must not refresh startup'
    $cleanup = Condition-For 'SevakRemoveStartup' 'InstallExecuteSequence'
    Check-Condition $cleanup @{REMOVE='ALL'} $true 'Uninstall removes owned startup registration'
    Check-Condition $cleanup @{REMOVE='ALL'; UPGRADINGPRODUCTCODE='old'} $false 'Upgrade removal preserves startup'
    Write-Output "PASS: $script:checks compiled MSI integration checks (no installation performed)."
} finally {
    [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($session)
    [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($database)
    [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($installer)
}
