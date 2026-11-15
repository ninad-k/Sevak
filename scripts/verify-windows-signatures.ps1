# Read-only verification of the application and both distributable installers.
# Tested with real self-signed signatures in scripts/windows-installer-validation.test.mjs.
[CmdletBinding(PositionalBinding=$false)]
param(
    [Parameter(Mandatory)][string] $BuildRoot,
    [Parameter(Mandatory)][ValidatePattern('^[A-Fa-f0-9]{40}$')][string] $ExpectedThumbprint
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path -LiteralPath $BuildRoot).Path
$files = @((Get-Item -LiteralPath (Join-Path $root 'sevak.exe')))
foreach ($format in @(@('nsis', '*.exe'), @('msi', '*.msi'))) {
    $installers = @(Get-ChildItem -LiteralPath (Join-Path $root "bundle/$($format[0])") -Filter $format[1] -File)
    if ($installers.Count -ne 1) { throw "Expected exactly one $($format[0]) installer." }
    $files += $installers
}
foreach ($file in $files) {
    $signature = Get-AuthenticodeSignature -LiteralPath $file.FullName
    if ($signature.Status -ne 'Valid') { throw "Invalid or missing Authenticode signature: $($file.Name) ($($signature.Status))." }
    if ($signature.SignerCertificate.Thumbprint -ne $ExpectedThumbprint) { throw "Unexpected publisher certificate: $($file.Name)." }
    if (-not $signature.TimeStamperCertificate) { throw "Missing trusted timestamp: $($file.Name)." }
    Write-Output "Verified publisher and timestamp: $($file.Name)"
}
