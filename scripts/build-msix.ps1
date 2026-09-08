<#
.SYNOPSIS
    Packages Ferryx Windows release executable into an MSIX package suitable for Microsoft Store submission or local sideloading.

.PARAMETER ExePath
    Mandatory path to the compiled ferryx.exe binary to package. Candidate guessing is prohibited.

.PARAMETER Version
    The application release version (e.g. 2026.908.1, v2026.09.08.1, or 2026.908.1.0).
    For Store submission, MSIX requires the 4th quad component to be 0 (e.g. Major.Minor.Patch.0).

.PARAMETER Publisher
    The Publisher identity DN (defaults to CN=68073D7F-44F8-47BF-8B3E-B17FBDC44F36).

.PARAMETER PackageName
    The MSIX package name (defaults to ProjectMaho.Ferryx).

.PARAMETER OutputDir
    The output directory where generated .msix package is saved (defaults to dist/msix).

.PARAMETER SkipSigning
    Switch to skip package signing (Store ingestion mode).

.PARAMETER CertThumbprint
    Optional certificate thumbprint in Windows certificate store (Cert:\CurrentUser\My or Cert:\LocalMachine\My)
    used for local sideload signing. Required when -SkipSigning is not specified.

.PARAMETER ManifestTemplate
    Optional explicit path to AppxManifest.xml template.

.PARAMETER IconsDir
    Optional explicit path to icons directory containing required PNG assets.
#>
param (
    [Parameter(Mandatory = $true)]
    [string]$ExePath,

    [Parameter(Mandatory = $true)]
    [string]$Version,

    [string]$Publisher = "CN=68073D7F-44F8-47BF-8B3E-B17FBDC44F36",
    [string]$PackageName = "ProjectMaho.Ferryx",
    [string]$OutputDir = "dist/msix",
    [switch]$SkipSigning = $false,
    [string]$CertThumbprint = $null,
    [string]$ManifestTemplate = $null,
    [string]$IconsDir = $null
)

$ErrorActionPreference = "Stop"

Write-Host "======================================================="
Write-Host " Ferryx MSIX Packaging Tool"
Write-Host "======================================================="
Write-Host "Exe Path       : $ExePath"
Write-Host "Target Version : $Version"
Write-Host "Publisher      : $Publisher"
Write-Host "Package Name   : $PackageName"
Write-Host "Output Dir     : $OutputDir"
Write-Host "Skip Signing   : $SkipSigning"
Write-Host "======================================================="

# -------------------------------------------------------------
# 1. Validate ExePath existence
# -------------------------------------------------------------
if ([string]::IsNullOrWhiteSpace($ExePath)) {
    throw "ERROR: -ExePath parameter is required."
}
if (-not (Test-Path -Path $ExePath -PathType Leaf)) {
    throw "ERROR: Executable not found at path: $ExePath"
}
$resolvedExePath = (Resolve-Path $ExePath).Path

# -------------------------------------------------------------
# 2. Canonicalize Version & enforce Store Quad Contract (4th=0)
# -------------------------------------------------------------
function ConvertTo-CanonicalVersion {
    param([string]$RawVersion)

    if ([string]::IsNullOrWhiteSpace($RawVersion)) {
        throw "Version string cannot be empty."
    }
    $v = $RawVersion.Trim()
    if ($v.StartsWith("v", [System.StringComparison]::OrdinalIgnoreCase)) {
        $v = $v.Substring(1)
    }

    # Format 1: Calendar Date tag: YYYY.MM.DD[.R]
    if ($v -match '^(?<year>\d{4})\.(?<month>\d{1,2})\.(?<day>\d{1,2})(?:\.(?<rev>\d+))?$') {
        $year = [int]$Matches['year']
        $month = [int]$Matches['month']
        $day = [int]$Matches['day']
        $revStr = $Matches['rev']
        $rev = if ($revStr) { [int]$revStr } else { 0 }

        if ($year -lt 2026) {
            throw "Invalid release year: $year. Year must be >= 2026."
        }
        if ($month -lt 1 -or $month -gt 12) {
            throw "Invalid release month: $month."
        }
        $daysInMonth = [DateTime]::DaysInMonth($year, $month)
        if ($day -lt 1 -or $day -gt $daysInMonth) {
            throw "Invalid release day: $day for month $month in year $year."
        }
        if ($rev -lt 0 -or $rev -gt 65535) {
            throw "Revision $rev out of range (0..65535)."
        }

        $appMinor = ($month * 100) + $day
        if ($appMinor -gt 65535) {
            throw "Calculated minor version $appMinor exceeds 65535."
        }
        return [PSCustomObject]@{
            AppVersion  = "$year.$appMinor.$rev"
            MsixVersion = "$year.$appMinor.$rev.0"
        }
    }

    # Format 2: Quad version: Major.Minor.Patch.Revision
    $parts = $v.Split('.')
    if ($parts.Length -eq 4) {
        foreach ($p in $parts) {
            if ($p -notmatch '^\d+$') {
                throw "Version part '$p' is not numeric in '$v'."
            }
            $num = [int64]$p
            if ($num -lt 0 -or $num -gt 65535) {
                throw "Version part $num out of range (0..65535) in '$v'."
            }
        }
        $p0 = [int]$parts[0]
        $p1 = [int]$parts[1]
        $p2 = [int]$parts[2]
        $p3 = [int]$parts[3]

        # Windows Store requirement: Store packages require the 4th quad component to be 0
        if ($p3 -ne 0) {
            throw "MSIX Store packages require the 4th quad component to be 0 for Store ingestion (got revision $p3 in '$v'). Use 3-part SemVer or quad with .0."
        }
        return [PSCustomObject]@{
            AppVersion  = "$p0.$p1.$p2"
            MsixVersion = "$p0.$p1.$p2.0"
        }
    }

    # Format 3: 3-part SemVer: Major.Minor.Patch
    if ($parts.Length -eq 3) {
        foreach ($p in $parts) {
            if ($p -notmatch '^\d+$') {
                throw "Version part '$p' is not numeric in '$v'."
            }
            $num = [int64]$p
            if ($num -lt 0 -or $num -gt 65535) {
                throw "Version part $num out of range (0..65535) in '$v'."
            }
        }
        $p0 = [int]$parts[0]
        $p1 = [int]$parts[1]
        $p2 = [int]$parts[2]
        return [PSCustomObject]@{
            AppVersion  = "$p0.$p1.$p2"
            MsixVersion = "$p0.$p1.$p2.0"
        }
    }

    throw "Invalid version format '$RawVersion'. Expected vYYYY.MM.DD[.R], Major.Minor.Patch, or Major.Minor.Patch.0."
}

$canon = ConvertTo-CanonicalVersion $Version
$appVersion = $canon.AppVersion
$msixVersion = $canon.MsixVersion
Write-Host "[1/6] App Version: $appVersion, MSIX Quad Version: $msixVersion"

# -------------------------------------------------------------
# 3. Verify binary version matches expected app version
# -------------------------------------------------------------
$vi = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($resolvedExePath)
$pv = if ($vi.ProductVersion) { $vi.ProductVersion.Trim() } else { "" }
$fv = if ($vi.FileVersion) { $vi.FileVersion.Trim() } else { "" }
$productParts = "$($vi.ProductMajorPart).$($vi.ProductMinorPart).$($vi.ProductBuildPart)"
$fileParts = "$($vi.FileMajorPart).$($vi.FileMinorPart).$($vi.FileBuildPart)"

$matched = ($pv -eq $appVersion) -or
           ($pv -eq $msixVersion) -or
           ($fv -eq $appVersion) -or
           ($fv -eq $msixVersion) -or
           ($productParts -eq $appVersion) -or
           ($fileParts -eq $appVersion)

if (-not $matched) {
    throw "Binary version verification failed: binary version does not match expected app version '$appVersion' (or '$msixVersion'). Binary reported ProductVersion='$pv', FileVersion='$fv', Parts='$productParts'."
}
Write-Host "[2/6] Binary version verified successfully against $appVersion"

# -------------------------------------------------------------
# 4. Locate Windows SDK Binaries (MakeAppx.exe, SignTool.exe)
# -------------------------------------------------------------
$sdkRoots = @(
    "C:\Program Files (x86)\Windows Kits\10\bin",
    "C:\Program Files\Windows Kits\10\bin"
)
$makeAppx = $null
$signTool = $null

foreach ($root in $sdkRoots) {
    if (Test-Path $root) {
        if (-not $makeAppx) {
            $makeAppx = (Get-ChildItem -Path $root -Filter "MakeAppx.exe" -Recurse -ErrorAction SilentlyContinue | Where-Object { $_.FullName -like "*x64*" } | Select-Object -First 1).FullName
        }
        if (-not $signTool) {
            $signTool = (Get-ChildItem -Path $root -Filter "SignTool.exe" -Recurse -ErrorAction SilentlyContinue | Where-Object { $_.FullName -like "*x64*" } | Select-Object -First 1).FullName
        }
    }
}
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
Write-Host "[3/6] Found Windows SDK MakeAppx: $makeAppx"

# -------------------------------------------------------------
# 5. Upfront signing parameters validation
# -------------------------------------------------------------
$cleanThumbprint = $null
if ($SkipSigning) {
    Write-Host "Store packaging mode (-SkipSigning): package created unsigned for Microsoft Store ingestion."
} else {
    if ([string]::IsNullOrWhiteSpace($CertThumbprint)) {
        throw "ERROR: Signing requested but no -CertThumbprint provided. Specify -CertThumbprint <thumbprint> for sideload signing, or -SkipSigning for Store submission."
    }
    $cleanThumbprint = ($CertThumbprint -replace '\s', '').ToUpperInvariant()
    $cert = Get-Item "Cert:\CurrentUser\My\$cleanThumbprint" -ErrorAction SilentlyContinue
    if (-not $cert) {
        $cert = Get-Item "Cert:\LocalMachine\My\$cleanThumbprint" -ErrorAction SilentlyContinue
    }
    if (-not $cert) {
        throw "ERROR: Signing certificate with thumbprint '$CertThumbprint' not found in Cert:\CurrentUser\My or Cert:\LocalMachine\My."
    }
    if (-not $signTool) {
        throw "ERROR: SignTool.exe not found. Cannot sign MSIX package."
    }
    Write-Host "Found signing certificate with thumbprint: $cleanThumbprint"
}

# -------------------------------------------------------------
# 6. Prepare isolated fresh staging directory & fail if output exists
# -------------------------------------------------------------
$resolvedOutputDir = [System.IO.Path]::GetFullPath($OutputDir)
New-Item -ItemType Directory -Force -Path $resolvedOutputDir | Out-Null

$msixOutputFile = Join-Path $resolvedOutputDir "Ferryx_${appVersion}_x64.msix"
if (Test-Path -Path $msixOutputFile -PathType Leaf) {
    throw "ERROR: Output package already exists at '$msixOutputFile'. Refusing to overwrite existing package."
}

$stagingDir = Join-Path ([System.IO.Path]::GetTempPath()) ("ferryx-msix-staging-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path (Join-Path $stagingDir "Assets") | Out-Null

try {
    # Copy executable
    Copy-Item $resolvedExePath -Destination (Join-Path $stagingDir "ferryx.exe") -Force

    # Locate and copy icon assets
    $iconSource = $null
    if ($IconsDir -and (Test-Path $IconsDir)) {
        $iconSource = $IconsDir
    } else {
        $repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
        $candidates = @(
            (Join-Path $repoRoot "src-tauri/icons"),
            (Join-Path $repoRoot "icons"),
            "src-tauri/icons",
            "icons"
        )
        foreach ($c in $candidates) {
            if (Test-Path $c) { $iconSource = $c; break }
        }
    }
    if ($iconSource) {
        Write-Host "Copying assets from: $iconSource"
        Get-ChildItem -Path $iconSource -Filter "*.png" | ForEach-Object {
            Copy-Item $_.FullName -Destination (Join-Path $stagingDir "Assets/$($_.Name)") -Force
        }
    } else {
        Write-Warning "Icon folder not found. MakeAppx may fail if icons are missing."
    }

    # Locate manifest template
    $templateSource = $null
    if ($ManifestTemplate -and (Test-Path $ManifestTemplate)) {
        $templateSource = $ManifestTemplate
    } else {
        $repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
        $candidates = @(
            (Join-Path $repoRoot "src-tauri/windows/msix/AppxManifest.xml"),
            "src-tauri/windows/msix/AppxManifest.xml",
            "windows/msix/AppxManifest.xml"
        )
        foreach ($c in $candidates) {
            if (Test-Path $c) { $templateSource = $c; break }
        }
    }
    if (-not $templateSource -or -not (Test-Path $templateSource)) {
        throw "ERROR: Manifest template not found at $templateSource"
    }

    $manifestContent = Get-Content $templateSource -Raw
    $identityRegex = [regex]'<Identity[^>]*>'
    $identityMatch = $identityRegex.Match($manifestContent)
    if (-not $identityMatch.Success) {
        throw "ERROR: <Identity> element not found in manifest template"
    }
    $newIdentity = $identityMatch.Value `
        -replace 'Version="[0-9.]*"', "Version=""$msixVersion"""
    $manifestContent = $manifestContent.Substring(0, $identityMatch.Index) + $newIdentity + $manifestContent.Substring($identityMatch.Index + $identityMatch.Length)

    $manifestPath = Join-Path $stagingDir "AppxManifest.xml"
    $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($manifestPath, $manifestContent, $utf8NoBom)
    Write-Host "[4/6] Generated AppxManifest.xml with version $msixVersion"

    # -------------------------------------------------------------
    # 7. Pack MSIX & check native MakeAppx exit code
    # -------------------------------------------------------------
    Write-Host "[5/6] Packing MSIX package to: $msixOutputFile"
    & $makeAppx pack /d $stagingDir /p $msixOutputFile /nv /o
    if ($LASTEXITCODE -ne 0) {
        throw "ERROR: MakeAppx.exe pack failed with exit code $LASTEXITCODE"
    }
    if (-not (Test-Path -Path $msixOutputFile -PathType Leaf)) {
        throw "ERROR: Failed to create MSIX package: $msixOutputFile"
    }

    # -------------------------------------------------------------
    # 8. Validate packaged manifest from archive
    # -------------------------------------------------------------
    function Validate-PackagedManifest {
        param(
            [Parameter(Mandatory = $true)]
            [string]$PackagePath,

            [Parameter(Mandatory = $true)]
            [string]$ExpectedName,

            [Parameter(Mandatory = $true)]
            [string]$ExpectedVersion,

            [Parameter(Mandatory = $true)]
            [string]$ExpectedPublisher,

            [string]$ExpectedProcessorArchitecture = "x64"
        )

        if (-not (Test-Path -Path $PackagePath -PathType Leaf)) {
            throw "ERROR: Package file not found: $PackagePath"
        }

        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $zip = [System.IO.Compression.ZipFile]::OpenRead($PackagePath)
        try {
            $manifestEntry = $zip.GetEntry("AppxManifest.xml")
            if (-not $manifestEntry) {
                throw "ERROR: Packaged MSIX does not contain AppxManifest.xml: $PackagePath"
            }
            $stream = $manifestEntry.Open()
            try {
                $reader = New-Object System.IO.StreamReader($stream)
                $xmlText = $reader.ReadToEnd()
            } finally {
                $stream.Dispose()
            }
        } finally {
            $zip.Dispose()
        }

        $manifestXml = [xml]$xmlText
        $idNode = $manifestXml.Package.Identity
        if (-not $idNode) {
            throw "ERROR: Packaged AppxManifest.xml missing <Identity> element"
        }
        if ($idNode.Name -ne $ExpectedName) {
            throw "ERROR: Packaged manifest Identity Name '$($idNode.Name)' does not match expected '$ExpectedName'"
        }
        if ($idNode.Version -ne $ExpectedVersion) {
            throw "ERROR: Packaged manifest Identity Version '$($idNode.Version)' does not match expected '$ExpectedVersion'"
        }
        if ($idNode.Publisher -ne $ExpectedPublisher) {
            throw "ERROR: Packaged manifest Identity Publisher '$($idNode.Publisher)' does not match expected '$ExpectedPublisher'"
        }
        if ($idNode.ProcessorArchitecture -ne $ExpectedProcessorArchitecture) {
            throw "ERROR: Packaged manifest Identity ProcessorArchitecture '$($idNode.ProcessorArchitecture)' does not match expected '$ExpectedProcessorArchitecture'"
        }
        Write-Host "Packaged manifest verified: Name='$($idNode.Name)', Version='$($idNode.Version)', Publisher='$($idNode.Publisher)', ProcessorArchitecture='$($idNode.ProcessorArchitecture)'"
    }

    Write-Host "[6/6] Validating packaged MSIX manifest from archive..."
    Validate-PackagedManifest `
        -PackagePath $msixOutputFile `
        -ExpectedName $PackageName `
        -ExpectedVersion $msixVersion `
        -ExpectedPublisher $Publisher `
        -ExpectedProcessorArchitecture "x64"

    # -------------------------------------------------------------
    # 9. Execute Sideload signing if enabled
    # -------------------------------------------------------------
    if (-not $SkipSigning) {
        Write-Host "Signing package with SignTool using certificate $cleanThumbprint..."
        & $signTool sign /fd SHA256 /sha1 $cleanThumbprint $msixOutputFile
        if ($LASTEXITCODE -ne 0) {
            throw "ERROR: SignTool.exe failed with exit code $LASTEXITCODE signing '$msixOutputFile'."
        }
        Write-Host "MSIX signed successfully!"
    }

    Write-Host "======================================================="
    Write-Host " MSIX BUILD SUCCESSFUL: $msixOutputFile"
    Write-Host "======================================================="
} finally {
    if ($stagingDir -and (Test-Path $stagingDir)) {
        Remove-Item -Recurse -Force $stagingDir -ErrorAction SilentlyContinue
    }
}
