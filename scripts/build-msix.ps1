<#
.SYNOPSIS
    Packages Ferryx Windows release executable into an MSIX package suitable for Microsoft Store submission or local sideloading.

.PARAMETER Version
    The application semantic version (e.g. 0.1.0 or v0.1.0).

.PARAMETER Publisher
    The Publisher identity DN (e.g. CN=Ferryx, O=Ferryx, C=US or Microsoft Partner Center Publisher ID).

.PARAMETER PackageName
    The MSIX package name (defaults to Ferryx).

.PARAMETER OutputDir
    The output directory where generated .msix and certificates are saved (defaults to dist/msix).

.PARAMETER SkipSigning
    Switch to skip self-signed certificate generation and signing (useful when packaging unsigned for Store ingestion).
#>
param (
    [string]$Version = "0.1.0",
    [string]$Publisher = "CN=Ferryx, O=Ferryx, C=US",
    [string]$PackageName = "Ferryx",
    [string]$OutputDir = "dist/msix",
    [switch]$SkipSigning = $false
)

$ErrorActionPreference = "Stop"

Write-Host "======================================================="
Write-Host " Ferryx MSIX Packaging Tool"
Write-Host "======================================================="
Write-Host "Target Version : $Version"
Write-Host "Publisher      : $Publisher"
Write-Host "Package Name   : $PackageName"
Write-Host "Output Dir     : $OutputDir"
Write-Host "======================================================="

# Normalize version for MSIX (must be Quad: Major.Minor.Patch.Revision)
# Date versions carry zero-padded parts (2026.08.26); MSIX rejects leading zeros, so each
# part is cast to an integer. A same-day revision (2026.08.26.2) becomes the quad revision
# so two releases on one date never produce the same package version.
$cleanVersion = $Version.Trim().TrimStart('v')
$versionParts = $cleanVersion.Split('.')
$asNumber = {
    param($value, $fallback)
    if ($value -match '^\d+$') { [string][int]$value } else { $fallback }
}
$major = & $asNumber $versionParts[0] "0"
$minor = if ($versionParts.Length -ge 2) { & $asNumber $versionParts[1] "1" } else { "1" }
$patch = if ($versionParts.Length -ge 3) { & $asNumber $versionParts[2] "0" } else { "0" }
$revision = if ($versionParts.Length -ge 4) { & $asNumber $versionParts[3] "0" } else { "0" }
$msixVersion = "$major.$minor.$patch.$revision"
Write-Host "[1/6] Normalized MSIX Quad Version: $msixVersion"

# Locate Windows SDK Binaries (MakeAppx, SignTool, MakePri)
$sdkRoots = @(
    "C:\Program Files (x86)\Windows Kits\10\bin",
    "C:\Program Files\Windows Kits\10\bin"
)

$makeAppx = $null
$signTool = $null
$makePri = $null

foreach ($root in $sdkRoots) {
    if (Test-Path $root) {
        if (-not $makeAppx) {
            $makeAppx = (Get-ChildItem -Path $root -Filter "MakeAppx.exe" -Recurse -ErrorAction SilentlyContinue | Where-Object { $_.FullName -like "*x64*" } | Select-Object -First 1).FullName
        }
        if (-not $signTool) {
            $signTool = (Get-ChildItem -Path $root -Filter "SignTool.exe" -Recurse -ErrorAction SilentlyContinue | Where-Object { $_.FullName -like "*x64*" } | Select-Object -First 1).FullName
        }
        if (-not $makePri) {
            $makePri = (Get-ChildItem -Path $root -Filter "MakePri.exe" -Recurse -ErrorAction SilentlyContinue | Where-Object { $_.FullName -like "*x64*" } | Select-Object -First 1).FullName
        }
    }
}

# Fallback to PATH
if (-not $makeAppx) {
    $cmd = Get-Command "MakeAppx.exe" -ErrorAction SilentlyContinue
    if ($cmd) { $makeAppx = $cmd.Source }
}
if (-not $signTool) {
    $cmd = Get-Command "SignTool.exe" -ErrorAction SilentlyContinue
    if ($cmd) { $signTool = $cmd.Source }
}

if (-not $makeAppx) {
    throw "ERROR: MakeAppx.exe not found. Please ensure Windows 10/11 SDK is installed."
}
Write-Host "[2/6] Found Windows SDK tools: $makeAppx"

# Prepare staging / layout directory
$layoutDir = "target/msix-layout"
if (Test-Path $layoutDir) {
    Remove-Item -Recurse -Force $layoutDir
}
New-Item -ItemType Directory -Force -Path "$layoutDir\Assets" | Out-Null
New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

# Locate Ferryx binary
$binaryCandidates = @(
    "src-tauri/target/release/ferryx.exe",
    "src-tauri/target/x86_64-pc-windows-msvc/release/ferryx.exe",
    "target/release/ferryx.exe",
    "target/x86_64-pc-windows-msvc/release/ferryx.exe"
)
$exePath = $null
foreach ($candidate in $binaryCandidates) {
    if (Test-Path $candidate) {
        $exePath = (Resolve-Path $candidate).Path
        break
    }
}
if (-not $exePath) {
    throw "ERROR: ferryx.exe not found in build targets. Run cargo build --release first."
}
Write-Host "[3/6] Found executable: $exePath"
Copy-Item $exePath -Destination "$layoutDir\ferryx.exe" -Force

# Copy App Assets (Icons / Logos)
Write-Host "[4/6] Copying Appx asset icons..."
$iconCandidates = @("src-tauri/icons", "icons")
$iconFound = $false
foreach ($iconDir in $iconCandidates) {
    if (Test-Path $iconDir) {
        Get-ChildItem -Path $iconDir -Filter "*.png" | ForEach-Object {
            Copy-Item $_.FullName -Destination "$layoutDir\Assets\$($_.Name)" -Force
        }
        $iconFound = $true
        break
    }
}
if (-not $iconFound) {
    Write-Warning "Icon folder not found, checking fallback."
}

# Generate AppxManifest.xml from template
$manifestTemplate = "src-tauri/windows/msix/AppxManifest.xml"
if (-not (Test-Path $manifestTemplate)) {
    throw "ERROR: Manifest template not found at $manifestTemplate"
}

$manifestContent = Get-Content $manifestTemplate -Raw

# Scope substitutions to the <Identity /> element only. Naive global regexes would
# also corrupt TargetDeviceFamily@Name, Capability@Name, and MinVersion/MaxVersionTested.
$identityRegex = [regex]'<Identity[^>]*>'
$identityMatch = $identityRegex.Match($manifestContent)
if (-not $identityMatch.Success) {
    throw "ERROR: <Identity> element not found in manifest template"
}
$newIdentity = $identityMatch.Value `
    -replace 'Name="[^"]*"', "Name=""$PackageName""" `
    -replace 'Publisher="[^"]*"', "Publisher=""$Publisher""" `
    -replace 'Version="[0-9.]*"', "Version=""$msixVersion"""
$manifestContent = $manifestContent.Substring(0, $identityMatch.Index) + $newIdentity + $manifestContent.Substring($identityMatch.Index + $identityMatch.Length)

$manifestPath = "$layoutDir\AppxManifest.xml"
# Write UTF-8 WITHOUT BOM: MakeAppx rejects a byte-order mark preceding the xml declaration.
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
[System.IO.File]::WriteAllText($manifestPath, $manifestContent, $utf8NoBom)
Write-Host "[5/6] Generated AppxManifest.xml with version $msixVersion"

# Pack MSIX
$msixOutputFile = "$OutputDir\Ferryx_${cleanVersion}_x64.msix"
Write-Host "[6/6] Packing MSIX package to: $msixOutputFile"
& $makeAppx pack /d $layoutDir /p $msixOutputFile /nv /o

if (-not (Test-Path $msixOutputFile)) {
    throw "ERROR: Failed to create MSIX package: $msixOutputFile"
}

# Optional signing for test / sideload distribution
if (-not $SkipSigning -and $signTool) {
    Write-Host "==> Generating self-signed developer certificate for local testing..."
    try {
        $certSubject = $Publisher
        $cert = New-SelfSignedCertificate -Type Custom `
            -Subject $certSubject `
            -KeyUsage DigitalSignature `
            -FriendlyName "Ferryx MSIX Developer Certificate" `
            -CertStoreLocation "Cert:\CurrentUser\My" `
            -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3", "2.5.29.19={text}Subject Type=End Entity")

        $pfxPassword = ConvertTo-SecureString -String "FerryxMsixSignPass2026!" -Force -AsPlainText
        $pfxPath = "$OutputDir\Ferryx_DevCert.pfx"
        Export-PfxCertificate -Cert $cert -FilePath $pfxPath -Password $pfxPassword | Out-Null

        Write-Host "==> Signing package with SignTool..."
        & $signTool sign /fd SHA256 /a /f $pfxPath /p "FerryxMsixSignPass2026!" $msixOutputFile
        Write-Host "==> MSIX signed successfully!"
    } catch {
        Write-Warning "Signing failed: $_. Package remains unsigned (ready for Microsoft Partner Center)."
    }
}

Write-Host "======================================================="
Write-Host " MSIX BUILD SUCCESSFUL: $msixOutputFile"
Write-Host "======================================================="
