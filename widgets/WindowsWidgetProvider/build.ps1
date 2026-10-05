#Requires -Version 5.1
<#
.SYNOPSIS
    Builds and signs the BiFlow Windows 11 Widgets Board package.

.DESCRIPTION
    Compiles the C++/WinRT widget provider and produces an MSIX the Windows 11
    Widgets Board can activate. This is deliberately a SEPARATE artifact from
    the BiFlow desktop installer: the desktop is NSIS and is unchanged by this
    script, and the widget package is optional.

    C++/WinRT rather than C# because the COM server needs a real IClassFactory
    and a real IWidgetProvider vtable. C#/WinRT can only build those by hand
    (ComExport was removed from .NET, and the Windows App SDK does not project
    its CWidgetProvider helper to managed code), whereas winrt::implements
    generates both. The projection headers are produced by cppwinrt.exe at
    build time, so there is no checked-in generated code to drift.

    Requires:
      * Visual Studio Build Tools with the C++ x64 toolset (cl.exe, link.exe)
      * Windows 10/11 SDK: cppwinrt.exe, makeappx.exe, UnionMetadata
      * The Microsoft.WindowsAppSDK NuGet package restored somewhere on disk
        (only its Microsoft.Windows.Widgets component is used)
      * A code-signing certificate for release builds. Without one, the package
        still installs on a development machine with developer mode enabled.

.PARAMETER Configuration
    Release (default) or Debug.

.PARAMETER CertificateThumbprint
    Thumbprint of the signing certificate in the user's store. Omit for a
    self-signed development certificate.

.PARAMETER Install
    Install the produced MSIX for the current user after building.

.PARAMETER Stage
    Copy the MSIX into packaging\staged\windows\widget\ so the next
    `pnpm tauri build --bundles nsis` bundles it into the installer and
    installs it automatically. This is what the release pipeline runs
    before the NSIS build.
#>
[CmdletBinding()]
param(
    [ValidateSet('Release', 'Debug')]
    [string]$Configuration = 'Release',

    [string]$CertificateThumbprint,

    [switch]$Install,

    [switch]$Stage
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# $PSScriptRoot is ...\widgets\WindowsWidgetProvider, so the repository root is
# two levels up. Stripping only one level resolved every path to
# widgets\widgets\WindowsWidgetProvider and the script failed on its first use.
$ProjectRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$ProviderDir = $PSScriptRoot
$SourceDir   = Join-Path $ProviderDir 'src'
$DistDir     = Join-Path $ProviderDir 'dist'
$compileDir      = Join-Path $ProviderDir 'build'
$GenDir      = Join-Path $compileDir 'generated'
$IncDir      = Join-Path $compileDir 'include'
$StageDir    = Join-Path $ProjectRoot 'packaging\staged\windows\widget'

$ExeName = 'BiFlow.WidgetProvider.exe'

function Fail($message) { throw $message }

# A self-signed development certificate, created on first use and reused
# afterwards.
#
# An *unsigned* package is not actually installable on current Windows:
# Add-AppxPackage -AllowUnsigned rejects any Publisher outside the unsigned
# namespace with 0x80073D2C. Signing with a self-signed certificate that is
# also present in TrustedPeople is the route that works on a developer machine
# and it exercises the same signing path a release build takes.
function Get-DevelopmentCertificate {
    $subject = 'CN=BiFlow Widget Provider'
    $cert = Get-ChildItem 'Cert:\CurrentUser\My' -ErrorAction SilentlyContinue |
        Where-Object { $_.Subject -eq $subject -and $_.NotAfter -gt (Get-Date).AddDays(30) } |
        Select-Object -First 1

    if (-not $cert) {
        Write-Host '  Creating a self-signed development certificate...'
        $cert = New-SelfSignedCertificate -Type Custom -Subject $subject `
            -KeyUsage DigitalSignature -KeyAlgorithm RSA -KeyLength 2048 `
            -FriendlyName 'BiFlow Widget Provider (development)' `
            -CertStoreLocation 'Cert:\CurrentUser\My' `
            -NotAfter (Get-Date).AddYears(5)
    }

    # Windows only accepts a self-signed app package when the certificate is
    # explicitly trusted, so mirror it into TrustedPeople for this user.
    $alreadyTrusted = Get-ChildItem 'Cert:\CurrentUser\TrustedPeople' -ErrorAction SilentlyContinue |
        Where-Object { $_.Thumbprint -eq $cert.Thumbprint }
    if (-not $alreadyTrusted) {
        $cer = Join-Path $env:TEMP 'biflow-widget-dev.cer'
        Export-Certificate -Cert $cert -FilePath $cer | Out-Null
        Import-Certificate -FilePath $cer -CertStoreLocation 'Cert:\CurrentUser\TrustedPeople' | Out-Null
        Write-Host '  Trusted the development certificate for this user.'
    }

    return $cert
}

# --- 0. Guard rails -------------------------------------------------------
if ([System.Environment]::OSVersion.Version.Major -lt 10) {
    Fail 'The Windows 11 Widgets Board requires Windows 10 version 1809 or newer.'
}

$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (-not (Test-Path $vswhere)) {
    Fail 'vswhere.exe not found. Install the Visual Studio Build Tools with the C++ x64 toolset.'
}
$vcvars = & $vswhere -latest -products * `
    -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
    -find 'VC\Auxiliary\Build\vcvars64.bat' | Select-Object -First 1
if (-not $vcvars) {
    Fail 'vcvars64.bat not found. Install the "MSVC v143 x64/x86 build tools" workload.'
}

$sdkRoot = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10'
$cppwinrt = Get-ChildItem (Join-Path $sdkRoot 'bin') -Recurse -Filter 'cppwinrt.exe' -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -match '\\x64\\' } | Sort-Object FullName -Descending | Select-Object -First 1
$makeappx = Get-ChildItem (Join-Path $sdkRoot 'bin') -Recurse -Filter 'makeappx.exe' -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -match '\\x64\\' } | Sort-Object FullName -Descending | Select-Object -First 1
$signtool = Get-ChildItem (Join-Path $sdkRoot 'bin') -Recurse -Filter 'signtool.exe' -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -match '\\x64\\' } | Sort-Object FullName -Descending | Select-Object -First 1
if (-not $cppwinrt) { Fail 'cppwinrt.exe not found. Install the "Windows SDK" component.' }
if (-not $makeappx) { Fail 'makeappx.exe not found. Install the "Windows SDK" component.' }
if (-not $signtool) { Fail 'signtool.exe not found. Install the "Windows SDK" component.' }

# cppwinrt.exe lives at <sdkRoot>\bin\<version>\x64\, so the version is one
# level above the x64 directory - not two, which resolves to plain "bin".
$sdkVersion = Split-Path -Leaf (Split-Path -Parent $cppwinrt.DirectoryName)
$unionMetadata = Join-Path $sdkRoot "UnionMetadata\$sdkVersion\Windows.winmd"
if (-not (Test-Path $unionMetadata)) {
    Fail "Windows SDK union metadata not found at $unionMetadata"
}

# The Windows App SDK ships the widget component as a NuGet package. Only the
# .winmd and the runtime DLL are needed; there is no C++ helper header for it,
# which is exactly why the projection is generated here.
$nugetRoot = Join-Path $env:USERPROFILE '.nuget\packages\microsoft.windowsappsdk.widgets'
$widgetsPkg = Get-ChildItem $nugetRoot -Directory -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -notmatch '-' } |
    Sort-Object { [version]$_.Name } -Descending | Select-Object -First 1
if (-not $widgetsPkg) {
    Fail @'
The Microsoft.WindowsAppSDK package is not in the NuGet cache.
Restore it once, for example:
  dotnet restore widgets\WindowsWidgetProvider
or run: dotnet new ... / dotnet add package Microsoft.WindowsAppSDK
'@
}
$winmd    = Join-Path $widgetsPkg.FullName 'metadata\Microsoft.Windows.Widgets.winmd'
$widgetsDll = Join-Path $widgetsPkg.FullName 'runtimes-framework\win-x64\native\Microsoft.Windows.Widgets.dll'
if (-not (Test-Path $winmd))      { Fail "Missing $winmd" }
if (-not (Test-Path $widgetsDll)) { Fail "Missing $widgetsDll" }

Write-Host '== BiFlow Windows 11 Widget provider ==' -ForegroundColor Cyan
Write-Host "Project  : $ProviderDir"
Write-Host "Config   : $Configuration"
Write-Host "SDK      : $sdkVersion"
Write-Host "Widgets  : microsoft.windowsappsdk.widgets $($widgetsPkg.Name)"

# --- 1. Generate the C++/WinRT projection ---------------------------------
Write-Host "`n[1/4] Generating the C++/WinRT projection" -ForegroundColor Yellow
New-Item -ItemType Directory -Force -Path $GenDir, $IncDir, "$IncDir\winrt\impl" | Out-Null
& $cppwinrt.FullName -input $winmd -output $GenDir -reference $unionMetadata | Out-Host
if ($LASTEXITCODE -ne 0) { Fail "cppwinrt failed with exit code $LASTEXITCODE" }

# Copy only the generated widget headers. The tool also emits its own
# winrt\base.h, which is an older snapshot than the SDK's and silently removes
# winrt::make, DataReader::from_buffer and the ReadAsync overloads. Copying it
# would shadow the SDK header through the quoted include path, so it must not
# reach the include path at all.
Get-ChildItem "$GenDir\winrt" -Filter 'Microsoft.Windows.Widgets*.h' |
    ForEach-Object { Copy-Item $_.FullName "$IncDir\winrt\" -Force }
Get-ChildItem "$GenDir\winrt\impl" -Filter 'Microsoft.Windows.Widgets*.h' |
    ForEach-Object { Copy-Item $_.FullName "$IncDir\winrt\impl\" -Force }
$generated = (Get-ChildItem $IncDir -Recurse -File | Measure-Object).Count
Write-Host "  staged $generated projection headers" -ForegroundColor Green

# --- 2. Compile and link --------------------------------------------------
Write-Host "`n[2/4] Compiling the COM server" -ForegroundColor Yellow
$sdkInc = Join-Path $sdkRoot "Include\$sdkVersion"
$includes = @(
    "/I`"$sdkInc\cppwinrt`"", "/I`"$sdkInc\shared`"", "/I`"$sdkInc\ucrt`"", "/I`"$IncDir`""
) -join ' '
$optFlag = if ($Configuration -eq 'Release') { '/O2' } else { '/Od' }
# $compileDir is the build ROOT; $outDir is the per-configuration child. These
# must be two distinct names: PowerShell variable names are case-insensitive, so
# an earlier $objDir silently *reassigned* $ObjDir and every later "build root"
# path resolved to build\<Configuration>\ instead.
$outDir = Join-Path $compileDir $Configuration
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
$exe = Join-Path $outDir $ExeName

$sources = @(Get-ChildItem $SourceDir -Filter '*.cpp' | ForEach-Object { "`"$($_.FullName)`"" })
$command = "call `"$vcvars`" >nul && cl /nologo /std:c++20 /EHsc $optFlag /W3 /WX /permissive- " +
    "/DUNICODE /D_UNICODE $includes /Fe:`"$exe`" /Fo`"$outDir\\`" " +
    ($sources -join ' ') +
    ' ole32.lib oleaut32.lib user32.lib shell32.lib windowsapp.lib'
cmd /c $command | Out-Host
if ($LASTEXITCODE -ne 0) { Fail "cl failed with exit code $LASTEXITCODE" }
Write-Host "  built $exe" -ForegroundColor Green

# --- 3. Stage the package layout and pack the MSIX -------------------------
Write-Host "`n[3/4] Packing the MSIX" -ForegroundColor Yellow
if (Test-Path $DistDir) { Remove-Item $DistDir -Recurse -Force }
New-Item -ItemType Directory -Force -Path $DistDir | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $DistDir 'Assets') | Out-Null

Copy-Item $exe                              -Destination $DistDir
# makeappx only accepts a footprint file named exactly AppxManifest.xml, so the
# manifest is named that in the source tree too rather than being renamed here.
Copy-Item (Join-Path $ProviderDir 'AppxManifest.xml') -Destination $DistDir
Copy-Item (Join-Path $ProviderDir 'Assets\*') -Destination (Join-Path $DistDir 'Assets') -ErrorAction Stop
# The widget component itself. Without it the shell cannot activate
# WidgetManager::GetDefault() and the provider never reaches the card.
Copy-Item $widgetsDll -Destination $DistDir

$msix = Join-Path $DistDir $ExeName.Replace('.exe', '.msix')
& $makeappx.FullName pack /d $DistDir /p $msix /o | Out-Host
if ($LASTEXITCODE -ne 0) { Fail "makeappx failed with exit code $LASTEXITCODE" }

# Signing is a separate step because this makeappx build has no /fd option,
# and because signtool is what can drive the MSIX subject interface. The
# certificate subject must equal the manifest's Identity/@Publisher or the SIP
# fails with a bare "SignerSign() failed" (0x8007000B).
$certificate = if ($CertificateThumbprint) {
    Get-ChildItem 'Cert:\CurrentUser\My' | Where-Object { $_.Thumbprint -eq $CertificateThumbprint } |
        Select-Object -First 1
} else {
    Write-Host '  No -CertificateThumbprint given; using the development certificate.' -ForegroundColor DarkYellow
    Get-DevelopmentCertificate
}
if (-not $certificate) { Fail "Signing certificate $($CertificateThumbprint) was not found in Cert:\CurrentUser\My." }

$pfx = Join-Path $compileDir 'signing.pfx'
$cer = Join-Path $compileDir 'signing.cer'
$pfxPassword = ConvertTo-SecureString -String 'biflow' -Force -AsPlainText
Export-PfxCertificate -Cert $certificate -FilePath $pfx -Password $pfxPassword | Out-Null
Export-Certificate -Cert $certificate -FilePath $cer | Out-Null
& $signtool.FullName sign /f $pfx /p 'biflow' /fd SHA256 $msix | Out-Host
if ($LASTEXITCODE -ne 0) { Fail "signtool failed with exit code $LASTEXITCODE" }
Write-Host "  signed $msix" -ForegroundColor Green
Write-Host "  packed $msix" -ForegroundColor Green

# --- 4. Stage for the NSIS installer, and/or install ----------------------
if ($Stage) {
    New-Item -ItemType Directory -Force -Path $StageDir | Out-Null
    # Clear any previous build so a stale MSIX is never bundled.
    Get-ChildItem $StageDir -Filter '*.msix' -ErrorAction SilentlyContinue | Remove-Item -Force
    Copy-Item $msix -Destination $StageDir
    Write-Host "`n[4/4] Staged for the NSIS installer" -ForegroundColor Yellow
    Write-Host "  $StageDir\$([IO.Path]::GetFileName($msix))" -ForegroundColor Green
    Write-Host '  Run: pnpm tauri build --bundles nsis' -ForegroundColor Green
    Write-Host '  The installer will register the widget automatically.' -ForegroundColor Green
} elseif ($Install) {
    Write-Host "`n[4/4] Installing for the current user" -ForegroundColor Yellow
    # Register the unpacked layout rather than installing the packed MSIX.
    #
    # Add-AppxPackage -Path needs a signature chain the machine trusts, and an
    # unsigned package is refused outright on current Windows (0x80073D2C).
    # -Register deploys the same package straight from the dist folder with
    # developer mode, no certificate and no interactive root-trust prompt, which
    # is what makes this a single command.
    $devMode = (Get-ItemProperty `
            'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock' `
            -ErrorAction SilentlyContinue).AllowDevelopmentWithoutDevLicense
    if ($devMode -ne 1) {
        Write-Warning 'Developer mode is off, so -Register will be refused. Enable it in Settings > Privacy & security > For developers.'
    }

    try {
        Add-AppxPackage -Register (Join-Path $DistDir 'AppxManifest.xml') `
            -DisableDevelopmentMode:$false -ForceApplicationShutdown -ErrorAction Stop
        Write-Host '  Registered.' -ForegroundColor Green
    } catch {
        Write-Host "  Add-AppxPackage failed: $($_.Exception.Message)" -ForegroundColor Red
        throw
    }

    $package = Get-AppxPackage -Name 'IranSplit.BiFlow.WidgetProvider' -ErrorAction SilentlyContinue
    if ($package) {
        Write-Host "  $($package.Name) $($package.Version) -> $($package.InstallLocation)" -ForegroundColor Green
    } else {
        Write-Warning 'The package did not appear in Get-AppxPackage.'
    }
    Write-Host '  Open the Widgets Board (Win+W), press + , pick BiFlow, then pin "BiFlow Traffic".' -ForegroundColor Green
} else {
    Write-Host "`n[4/4] Skipped install (pass -Install or -Stage)" -ForegroundColor Yellow
}

Write-Host "`nDone." -ForegroundColor Cyan
