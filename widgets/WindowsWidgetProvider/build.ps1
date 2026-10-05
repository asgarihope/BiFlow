#Requires -Version 5.1
<#
.SYNOPSIS
    Builds and signs the BiFlow Windows 11 Widgets Board package.

.DESCRIPTION
    Compiles the C# widget provider and produces an MSIX that the Windows 11
    Widgets Board can activate. This is deliberately a SEPARATE artifact from
    the BiFlow desktop installer: the desktop is NSIS and is unchanged by this
    script, and the widget package is optional.

    Requires:
      * .NET SDK 8.0 or newer  (dotnet --version)
      * Windows 10 SDK build tools for MakeAppx / MakePri
      * A code-signing certificate for release builds. Without one, the
        package still builds and installs on a development machine with
        developer mode enabled.

.PARAMETER Configuration
    Release (default) or Debug.

.PARAMETER CertificateThumbprint
    Thumbprint of the signing certificate in the user's store. Omit for a
    self-signed development certificate.

.PARAMETER Install
    Install the produced MSIX for the current user after building.
#>
[CmdletBinding()]
param(
    [ValidateSet('Release', 'Debug')]
    [string]$Configuration = 'Release',

    [string]$CertificateThumbprint,

    [switch]$Install
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$ProjectRoot  = Split-Path -Parent $PSScriptRoot
$ProviderDir  = Join-Path $ProjectRoot 'widgets\WindowsWidgetProvider'
$ProviderRoot = $ProviderDir
$DistDir      = Join-Path $ProviderRoot 'dist'

# --- 0. Guard rails -------------------------------------------------------
$osVersion = [System.Environment]::OSVersion.Version
if ($osVersion.Major -lt 10) {
    throw 'The Windows 11 Widgets Board requires Windows 10 version 1809 or newer.'
}

$dotnet = Get-Command dotnet -ErrorAction SilentlyContinue
if (-not $dotnet) {
    throw 'dotnet not found. Install the .NET SDK 8.0+ from https://dotnet.microsoft.com/download'
}

# The runtime alone is not enough: we need `dotnet build`.
$sdkList = & dotnet --list-sdks 2>&1
if (-not $sdkList) {
    throw @'
No .NET SDK is installed (only the runtime was found).
Install the .NET SDK 8.0 or newer, then re-run this script.
  winget install Microsoft.DotNet.SDK.8
'@
}

Write-Host "== BiFlow Windows 11 Widget provider ==" -ForegroundColor Cyan
Write-Host "Project : $ProviderDir"
Write-Host "Config  : $Configuration"

# --- 1. Compile the provider ---------------------------------------------
Push-Location $ProviderDir
try {
    Write-Host "`n[1/4] Restoring + building the COM server" -ForegroundColor Yellow
    & dotnet build `
        -c $Configuration `
        -r win-x64 `
        --self-contained false `
        -p:Platform=x64 `
        --nologo `
        -v minimal
    if ($LASTEXITCODE -ne 0) {
        throw "dotnet build failed with exit code $LASTEXITCODE"
    }
}
finally {
    Pop-Location
}

$BuildDir = Join-Path $ProviderDir "bin\x64\$Configuration\net8.0-windows10.0.19041.0\win-x64"
$ExePath = Join-Path $BuildDir 'BiFlow.WidgetProvider.exe'
if (-not (Test-Path $ExePath)) {
    # Newer SDKs append a RID-specific folder name; fall back to a search.
    $found = Get-ChildItem -Path (Join-Path $ProviderDir 'bin') -Recurse -Filter 'BiFlow.WidgetProvider.exe' -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($found) {
        $BuildDir = $found.DirectoryName
        $ExePath = $found.FullName
    }
    else {
        throw "Build reported success but $ExePath was not found."
    }
}
Write-Host "Built: $ExePath" -ForegroundColor Green

# --- 2. Stage the package layout -----------------------------------------
Write-Host "`n[2/4] Staging the MSIX layout" -ForegroundColor Yellow
if (Test-Path $DistDir) {
    Remove-Item $DistDir -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $DistDir | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $DistDir 'Assets') | Out-Null

Copy-Item $ExePath                        -Destination $DistDir
Copy-Item (Join-Path $ProviderDir 'Package.appxmanifest') -Destination $DistDir
Copy-Item (Join-Path $ProviderDir 'Assets\*')             -Destination (Join-Path $DistDir 'Assets')

# The Windows App SDK runtime ships as framework-dependent DLLs next to the
# exe; copy them so the package is self-contained on a clean machine.
Get-ChildItem -Path $BuildDir -Filter '*.dll' -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -notlike 'System.*' -and $_.Name -notlike 'Microsoft.CSharp*' } |
    Copy-Item -Destination $DistDir

Write-Host "Staged $((Get-ChildItem $DistDir -Recurse).Count) files into $DistDir" -ForegroundColor Green

# --- 3. MakeAppx ----------------------------------------------------------
$makeAppx = Get-ChildItem `
    -Path "${env:ProgramFiles(x86)}\Windows Kits\10\bin" `
    -Recurse -Filter 'makeappx.exe' -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -match 'x64' } |
    Sort-Object FullName -Descending |
    Select-Object -First 1

if (-not $makeAppx) {
    throw 'makeappx.exe not found. Install the "Windows SDK" from the Visual Studio Installer.'
}

$msixPath = Join-Path $DistDir 'BiFlow.WidgetProvider.msix'
Write-Host "`n[3/4] Packing MSIX with $($makeAppx.Name)" -ForegroundColor Yellow

$makeAppxArgs = @('pack', '/d', $DistDir, '/p', $msixPath, '/o')
if ($CertificateThumbprint) {
    $makeAppxArgs += '/fd'
    $makeAppxArgs += $CertificateThumbprint
}
else {
    Write-Host 'No certificate specified - building an unsigned package.' -ForegroundColor DarkYellow
    Write-Host 'Windows will require developer mode (or a sideload policy) to install it.' -ForegroundColor DarkYellow
}

& $makeAppx.FullName @makeAppxArgs | Out-Host
if ($LASTEXITCODE -ne 0) {
    throw "makeappx failed with exit code $LASTEXITCODE"
}
Write-Host "Packed: $msixPath" -ForegroundColor Green

# --- 4. Optional install --------------------------------------------------
if ($Install) {
    Write-Host "`n[4/4] Installing for the current user" -ForegroundColor Yellow
    if ($CertificateThumbprint) {
        Add-AppxPackage -Path $msixPath
    }
    else {
        Add-AppxPackage -Path $msixPath -AllowUnsigned
    }
    Write-Host 'Installed. Open the Widgets Board and pin "BiFlow Traffic" from the + menu.' -ForegroundColor Green
}
else {
    Write-Host "`n[4/4] Skipped install (pass -Install to install it)" -ForegroundColor Yellow
}

Write-Host "`nDone." -ForegroundColor Cyan
