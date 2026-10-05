#Requires -Version 5.1
<#
.SYNOPSIS
    Installs the BiFlow Windows 11 widget package on this machine.

.DESCRIPTION
    Does everything the widget needs, in one run:

      1. re-launches itself elevated (one UAC prompt) - the signature chain of
         an app package must terminate in a root the *machine* trusts, and
         Cert:\LocalMachine\Root cannot be written without elevation
      2. removes any previous loose (-Register) registration, which never
         indexes app extensions and so never reaches the Widgets Board
      3. trusts the development signing certificate
      4. installs the signed MSIX
      5. reports what the shell now sees

    Run it by double-clicking install-widget.cmd, or:

        powershell -ExecutionPolicy Bypass -File widgets\WindowsWidgetProvider\install-widget.ps1
#>
[CmdletBinding()]
param([switch]$Elevated)

$ErrorActionPreference = 'Stop'

$ProviderDir = $PSScriptRoot
$msix = Join-Path $ProviderDir 'dist\BiFlow.WidgetProvider.msix'
$cer  = Join-Path $ProviderDir 'build\signing.cer'
$packageName = 'IranSplit.BiFlow.WidgetProvider'

function Say($text, $colour = 'Gray') { Write-Host $text -ForegroundColor $colour }

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent())
    .IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

if (-not $isAdmin) {
    if ($Elevated) {
        throw 'This script is already elevated but still reports no admin token. Start it from a normal PowerShell window instead.'
    }
    Say 'Elevating: Windows will ask once, press Yes.' 'Yellow'
    Start-Process powershell -Verb RunAs -ArgumentList @(
        '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$PSCommandPath`"", '-Elevated'
    ) -Wait
    return
}

Say '== BiFlow widget installation ==' 'Cyan'

if (-not (Test-Path $msix)) {
    throw "Package not found: $msix`nBuild it first: .\build.ps1"
}
if (-not (Test-Path $cer)) {
    throw "Signing certificate not found: $cer`nBuild it first: .\build.ps1"
}

# 1. A loose -Register deployment leaves an entry in the AppModel repository
#    with NO subkeys, so the extension is never indexed and the Widgets Board
#    never lists the provider. Installing the real MSIX replaces it.
$existing = Get-AppxPackage -Name $packageName -ErrorAction SilentlyContinue
if ($existing) {
    Say "Removing previous registration ($($existing.SignatureKind), $($existing.InstallLocation))" 'Yellow'
    try { Remove-AppxPackage -Package $existing.PackageFullName -ErrorAction Stop } catch {
        Say "  could not remove: $($_.Exception.Message)" 'DarkYellow'
    }
}

# 2. Trust the development certificate. -f suppresses the confirmation dialog.
Say 'Trusting the development certificate...' 'Yellow'
& certutil.exe -addstore -f Root $cer | Out-Null
if ($LASTEXITCODE -ne 0) { throw "certutil failed with exit code $LASTEXITCODE" }

# 3. Install.
Say 'Installing the package...' 'Yellow'
try {
    Add-AppxPackage -Path $msix -ErrorAction Stop
} catch {
    Say "Add-AppxPackage failed: $($_.Exception.Message)" 'Red'
    throw
}

# 4. Report.
$package = Get-AppxPackage -Name $packageName -ErrorAction SilentlyContinue
if (-not $package) { throw 'The package did not appear in Get-AppxPackage after install.' }
Say "Installed: $($package.Name) $($package.Version)" 'Green'
Say "  Signature : $($package.SignatureKind)" 'Gray'
Say "  Location  : $($package.InstallLocation)" 'Gray'

$repo = 'HKCU:\Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\Repository\Packages'
$entry = Get-ChildItem -Path $repo -ErrorAction SilentlyContinue |
    Where-Object { $_.PSChildName -like "$packageName*" } | Select-Object -First 1
if ($entry) {
    $subs = @(Get-ChildItem -LiteralPath $entry.PSPath -ErrorAction SilentlyContinue | Select-Object -ExpandProperty PSChildName)
    Say "  Indexed   : $($subs -join ', ')" $(if ($subs.Count) { 'Green' } else { 'Yellow' })
}

Say ''
Say 'Press Win+W, click +, and pin "BiFlow Traffic".' 'Cyan'
Start-Process 'explorer.exe' 'ms-widgetboard:'
