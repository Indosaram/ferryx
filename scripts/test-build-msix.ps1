# scripts/test-build-msix.ps1
# Comprehensive behavior test suite for build-msix.ps1 hardening

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$script:testCount = 0
$script:passedCount = 0
$script:failedCount = 0
$script:failures = @()

function Run-Test {
    param(
        [string]$Name,
        [scriptblock]$Block
    )
    $script:testCount++
    Write-Host "--- Test $script:testCount`: $Name ---"
    try {
        & $Block
        $script:passedCount++
        Write-Host "RESULT: PASS`n" -ForegroundColor Green
    } catch {
        $script:failedCount++
        $msg = $_.Exception.Message
        $script:failures += "$Name : $msg"
        Write-Host "RESULT: FAIL - $msg`n" -ForegroundColor Red
    }
}

function Assert-Throws {
    param(
        [scriptblock]$Block,
        [string]$ExpectedPattern
    )
    $threw = $false
    $caughtMsg = ""
    try {
        & $Block
    } catch {
        $threw = $true
        $caughtMsg = $_.Exception.Message
    }
    if (-not $threw) {
        throw "Expected script to throw an exception, but it succeeded."
    }
    if ($ExpectedPattern -and ($caughtMsg -notmatch $ExpectedPattern)) {
        throw "Expected error matching '$ExpectedPattern', but caught: '$caughtMsg'"
    }
}

# Locate C# compiler for generating tiny native fake executables
$cscCandidates = @(
    "C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe",
    "C:\Windows\Microsoft.NET\Framework\v4.0.30319\csc.exe"
)
$csc = $null
foreach ($c in $cscCandidates) {
    if (Test-Path $c) { $csc = $c; break }
}
if (-not $csc) {
    $cmd = Get-Command "csc.exe" -ErrorAction SilentlyContinue
    if ($cmd) { $csc = $cmd.Source }
}
if (-not $csc) {
    throw "csc.exe (C# compiler) not found. Required for native executable fixtures."
}

# Helper to compile tiny fake PE executable with specified versions
function New-FakeExecutable {
    param(
        [string]$Path,
        [string]$AssemblyVersion = "1.0.0.0",
        [string]$FileVersion = "1.0.0.0",
        [string]$ProductVersion = "1.0.0"
    )
    $srcPath = [System.IO.Path]::ChangeExtension($Path, ".cs")
    $code = @"
using System.Reflection;
[assembly: AssemblyVersion("$AssemblyVersion")]
[assembly: AssemblyFileVersion("$FileVersion")]
[assembly: AssemblyInformationalVersion("$ProductVersion")]
public class Program { public static void Main() {} }
"@
    [System.IO.File]::WriteAllText($srcPath, $code)
    & $csc /nologo /target:exe "/out:$Path" $srcPath | Out-Null
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path $Path)) {
        throw "Failed to compile fake executable at $Path"
    }
    Remove-Item $srcPath -Force -ErrorAction SilentlyContinue
}

$scriptDir = $PSScriptRoot
$buildMsixScript = Join-Path $scriptDir "build-msix.ps1"
if (-not (Test-Path $buildMsixScript)) {
    throw "build-msix.ps1 not found at $buildMsixScript"
}

# Create test scratch directory
$scratchDir = Join-Path ([System.IO.Path]::GetTempPath()) ("ferryx-test-scratch-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $scratchDir | Out-Null

try {
    # -------------------------------------------------------------
    # Test 1: Missing -ExePath parameter throws
    # -------------------------------------------------------------
    Run-Test "Missing mandatory -ExePath parameter fails" {
        Assert-Throws {
            $output = & powershell.exe -NoProfile -NonInteractive -Command "& '$buildMsixScript' -Version '2026.908.1' -SkipSigning" 2>&1
            if ($LASTEXITCODE -eq 0) { throw "Expected non-zero exit code" }
            $fullErr = ($output -join " ")
            throw "$fullErr"
        } "Cannot process command|missing mandatory parameter|MissingMandatoryParameter|ExePath"
    }

    # -------------------------------------------------------------
    # Test 2: Non-existent -ExePath throws
    # -------------------------------------------------------------
    Run-Test "Non-existent -ExePath fails" {
        $bogusPath = Join-Path $scratchDir "does_not_exist.exe"
        Assert-Throws {
            $output = & powershell.exe -NoProfile -NonInteractive -Command "& '$buildMsixScript' -ExePath '$bogusPath' -Version '2026.908.1' -SkipSigning" 2>&1
            if ($LASTEXITCODE -eq 0) { throw "Expected non-zero exit code" }
            $fullErr = ($output -join " ")
            throw "$fullErr"
        } "not found|does not exist"
    }

    # -------------------------------------------------------------
    # Test 3: Store quad 4th component != 0 rejected
    # -------------------------------------------------------------
    Run-Test "Version quad with non-zero 4th component is rejected" {
        $dummyExe = Join-Path $scratchDir "dummy1.exe"
        New-FakeExecutable -Path $dummyExe -AssemblyVersion "2026.908.1.0" -FileVersion "2026.908.1.0" -ProductVersion "2026.908.1"
        Assert-Throws {
            $output = & powershell.exe -NoProfile -NonInteractive -Command "& '$buildMsixScript' -ExePath '$dummyExe' -Version '2026.908.1.5' -SkipSigning" 2>&1
            if ($LASTEXITCODE -eq 0) { throw "Expected non-zero exit code" }
            $fullErr = ($output -join " ")
            throw "$fullErr"
        } "4th quad component to be 0"
    }

    # -------------------------------------------------------------
    # Test 4: Invalid calendar date rejected
    # -------------------------------------------------------------
    Run-Test "Invalid calendar date tag is rejected" {
        $dummyExe = Join-Path $scratchDir "dummy2.exe"
        New-FakeExecutable -Path $dummyExe -AssemblyVersion "2026.229.0.0" -FileVersion "2026.229.0.0" -ProductVersion "2026.229.0"
        Assert-Throws {
            $output = & powershell.exe -NoProfile -NonInteractive -Command "& '$buildMsixScript' -ExePath '$dummyExe' -Version 'v2026.02.29' -SkipSigning" 2>&1
            if ($LASTEXITCODE -eq 0) { throw "Expected non-zero exit code" }
            $fullErr = ($output -join " ")
            throw "$fullErr"
        } "Invalid release day"
    }

    # -------------------------------------------------------------
    # Test 5: Binary version mismatch fails
    # -------------------------------------------------------------
    Run-Test "Binary version mismatch against expected app version fails" {
        $mismatchExe = Join-Path $scratchDir "mismatch.exe"
        New-FakeExecutable -Path $mismatchExe -AssemblyVersion "1.0.0.0" -FileVersion "1.0.0.0" -ProductVersion "1.0.0"
        Assert-Throws {
            $output = & powershell.exe -NoProfile -NonInteractive -Command "& '$buildMsixScript' -ExePath '$mismatchExe' -Version '2026.908.1' -SkipSigning" 2>&1
            if ($LASTEXITCODE -eq 0) { throw "Expected non-zero exit code" }
            $fullErr = ($output -join " ")
            throw "$fullErr"
        } "Binary version.*does not match"
    }

    # -------------------------------------------------------------
    # Test 6: Existing OutputDir .msix fails closed preserving exact bytes
    # -------------------------------------------------------------
    Run-Test "Pre-existing output package fails closed and preserves existing file bytes" {
        $matchExe = Join-Path $scratchDir "match.exe"
        New-FakeExecutable -Path $matchExe -AssemblyVersion "2026.908.1.0" -FileVersion "2026.908.1.0" -ProductVersion "2026.908.1"
        $outDir = Join-Path $scratchDir "out_collision"
        New-Item -ItemType Directory -Force -Path $outDir | Out-Null
        $existingMsix = Join-Path $outDir "Ferryx_2026.908.1_x64.msix"
        $originalBytes = [System.Text.Encoding]::UTF8.GetBytes("PREEXISTING_NONZERO_MSIX_PAYLOAD_BYTES_PRESERVED_EXACTLY_1234567890")
        [System.IO.File]::WriteAllBytes($existingMsix, $originalBytes)

        $dummyTemplate = Join-Path $scratchDir "dummy_template_collision.xml"
        [System.IO.File]::WriteAllText($dummyTemplate, @"
<?xml version="1.0" encoding="utf-8"?>
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10">
  <Identity Name="ProjectMaho.Ferryx" Publisher="CN=68073D7F-44F8-47BF-8B3E-B17FBDC44F36" Version="0.1.0.0" ProcessorArchitecture="x64" />
</Package>
"@)

        $threw = $false
        $capturedError = ""
        try {
            $procOutput = & powershell.exe -NoProfile -NonInteractive -Command "& '$buildMsixScript' -ExePath '$matchExe' -Version '2026.908.1' -OutputDir '$outDir' -ManifestTemplate '$dummyTemplate' -SkipSigning" 2>&1
            if ($LASTEXITCODE -ne 0) {
                $threw = $true
                $capturedError = ($procOutput -join " ")
            }
        } catch {
            $threw = $true
            $capturedError = $_.Exception.Message
            if (-not $capturedError -and $procOutput) {
                $capturedError = ($procOutput -join " ")
            }
        }

        if (-not $threw) {
            throw "Expected script to fail on pre-existing output package, but it succeeded with exit code 0."
        }
        if ($capturedError -notmatch "Output package already exists|Refusing to overwrite") {
            throw "Expected failure class matching 'Output package already exists', but caught: '$capturedError'"
        }
        if (-not (Test-Path -Path $existingMsix -PathType Leaf)) {
            throw "Existing MSIX package was deleted! It must be preserved."
        }
        $currentBytes = [System.IO.File]::ReadAllBytes($existingMsix)
        if ($currentBytes.Length -eq 0) {
            throw "Existing MSIX package was truncated to 0 bytes!"
        }
        if ($currentBytes.Length -ne $originalBytes.Length) {
            throw "Existing MSIX package byte length changed: expected $($originalBytes.Length), got $($currentBytes.Length)"
        }
        for ($i = 0; $i -lt $originalBytes.Length; $i++) {
            if ($currentBytes[$i] -ne $originalBytes[$i]) {
                throw "Existing MSIX package content changed at byte offset $i!"
            }
        }
    }

    # -------------------------------------------------------------
    # Test 7: Omitted -SkipSigning without -CertThumbprint fails
    # -------------------------------------------------------------
    Run-Test "Omitted -SkipSigning without -CertThumbprint fails" {
        $dummyExe = Join-Path $scratchDir "dummy3.exe"
        New-FakeExecutable -Path $dummyExe -AssemblyVersion "2026.908.1.0" -FileVersion "2026.908.1.0" -ProductVersion "2026.908.1"
        Assert-Throws {
            $output = & powershell.exe -NoProfile -NonInteractive -Command "& '$buildMsixScript' -ExePath '$dummyExe' -Version '2026.908.1'" 2>&1
            if ($LASTEXITCODE -eq 0) { throw "Expected non-zero exit code" }
            $fullErr = ($output -join " ")
            throw "$fullErr"
        } "Signing requested but no -CertThumbprint provided"
    }

    # -------------------------------------------------------------
    # Test 8: Unavailable cert thumbprint fails
    # -------------------------------------------------------------
    Run-Test "Unavailable certificate thumbprint fails" {
        $dummyExe = Join-Path $scratchDir "dummy4.exe"
        New-FakeExecutable -Path $dummyExe -AssemblyVersion "2026.908.1.0" -FileVersion "2026.908.1.0" -ProductVersion "2026.908.1"
        Assert-Throws {
            $output = & powershell.exe -NoProfile -NonInteractive -Command "& '$buildMsixScript' -ExePath '$dummyExe' -Version '2026.908.1' -CertThumbprint '00112233445566778899AABBCCDDEEFF00112233'" 2>&1
            if ($LASTEXITCODE -eq 0) { throw "Expected non-zero exit code" }
            $fullErr = ($output -join " ")
            throw "$fullErr"
        } "not found in Cert:"
    }

    # -------------------------------------------------------------
    # Test 9: Native tool non-zero exit code handling
    # -------------------------------------------------------------
    Run-Test "MakeAppx failure exit code is detected and reported" {
        $validExe = Join-Path $scratchDir "tool_fail.exe"
        New-FakeExecutable -Path $validExe -AssemblyVersion "2026.908.1.0" -FileVersion "2026.908.1.0" -ProductVersion "2026.908.1"
        $badOutDir = Join-Path $scratchDir "bad_out"
        New-Item -ItemType Directory -Force -Path $badOutDir | Out-Null

        $badManifest = Join-Path $scratchDir "bad_manifest.xml"
        [System.IO.File]::WriteAllText($badManifest, @"
<?xml version="1.0" encoding="utf-8"?>
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10">
  <Identity Name="ProjectMaho.Ferryx" Publisher="CN=68073D7F-44F8-47BF-8B3E-B17FBDC44F36" Version="2026.908.1.0" ProcessorArchitecture="x64" />
  <Properties><DisplayName>Test</DisplayName><PublisherDisplayName>Test</PublisherDisplayName><Logo>Assets\StoreLogo.png</Logo></Properties>
</Package>
"@)
        Assert-Throws {
            $output = & powershell.exe -NoProfile -NonInteractive -Command "& '$buildMsixScript' -ExePath '$validExe' -Version '2026.908.1' -OutputDir '$badOutDir' -ManifestTemplate '$badManifest' -SkipSigning" 2>&1
            if ($LASTEXITCODE -eq 0) { throw "Expected non-zero exit code" }
            $fullErr = ($output -join " ")
            throw "$fullErr"
        } "MakeAppx.*failed with exit code"
    }

    # -------------------------------------------------------------
    # Test 10: Real MakeAppx pack and zip manifest validation (Name, Version, Publisher, ProcessorArchitecture)
    # -------------------------------------------------------------
    Run-Test "Small valid fixture packs MSIX and validates packaged manifest identity" {
        $validExe = Join-Path $scratchDir "ferryx.exe"
        New-FakeExecutable -Path $validExe -AssemblyVersion "2026.908.1.0" -FileVersion "2026.908.1.0" -ProductVersion "2026.908.1"

        $iconsDir = Join-Path $scratchDir "icons"
        New-Item -ItemType Directory -Force -Path $iconsDir | Out-Null

        $pngBytes = [byte[]]@(
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A,
            0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
            0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
            0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41,
            0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
            0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
            0x42, 0x60, 0x82
        )
        $iconNames = @(
            "StoreLogo.png", "Square150x150Logo.png", "Square44x44Logo.png",
            "Square310x310Logo.png", "Square71x71Logo.png"
        )
        foreach ($name in $iconNames) {
            [System.IO.File]::WriteAllBytes((Join-Path $iconsDir $name), $pngBytes)
        }

        $manifestXml = @"
<?xml version="1.0" encoding="utf-8"?>
<Package
  xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
  xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
  xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"
  IgnorableNamespaces="uap rescap">
  <Identity Name="ProjectMaho.Ferryx" Publisher="CN=68073D7F-44F8-47BF-8B3E-B17FBDC44F36" Version="0.1.0.0" ProcessorArchitecture="x64" />
  <Properties>
    <DisplayName>Ferryx</DisplayName>
    <PublisherDisplayName>Project Maho</PublisherDisplayName>
    <Logo>Assets\StoreLogo.png</Logo>
  </Properties>
  <Dependencies>
    <TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.17763.0" MaxVersionTested="10.0.22621.0" />
  </Dependencies>
  <Resources><Resource Language="en-US" /></Resources>
  <Applications>
    <Application Id="Ferryx" Executable="ferryx.exe" EntryPoint="Windows.FullTrustApplication">
      <uap:VisualElements DisplayName="Ferryx" Description="Ferryx" BackgroundColor="transparent"
        Square150x150Logo="Assets\Square150x150Logo.png" Square44x44Logo="Assets\Square44x44Logo.png">
        <uap:DefaultTile Wide310x150Logo="Assets\Square310x310Logo.png" Square310x310Logo="Assets\Square310x310Logo.png" Square71x71Logo="Assets\Square71x71Logo.png" />
      </uap:VisualElements>
    </Application>
  </Applications>
  <Capabilities><rescap:Capability Name="runFullTrust" /></Capabilities>
</Package>
"@
        $templatePath = Join-Path $scratchDir "AppxManifest.xml"
        $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
        [System.IO.File]::WriteAllText($templatePath, $manifestXml, $utf8NoBom)

        $packOutDir = Join-Path $scratchDir "pack_out"
        New-Item -ItemType Directory -Force -Path $packOutDir | Out-Null

        & powershell.exe -NoProfile -NonInteractive -Command "& '$buildMsixScript' -ExePath '$validExe' -Version 'v2026.09.08.1' -OutputDir '$packOutDir' -ManifestTemplate '$templatePath' -IconsDir '$iconsDir' -SkipSigning"
        if ($LASTEXITCODE -ne 0) {
            throw "build-msix.ps1 exited with non-zero exit code $LASTEXITCODE"
        }

        $expectedMsix = Join-Path $packOutDir "Ferryx_2026.908.1_x64.msix"
        if (-not (Test-Path $expectedMsix)) {
            throw "Generated MSIX package not found at expected path: $expectedMsix"
        }

        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $zip = [System.IO.Compression.ZipFile]::OpenRead($expectedMsix)
        try {
            $entry = $zip.GetEntry("AppxManifest.xml")
            if (-not $entry) { throw "Packaged MSIX does not contain AppxManifest.xml" }
            $stream = $entry.Open()
            try {
                $reader = New-Object System.IO.StreamReader($stream)
                $xml = [xml]$reader.ReadToEnd()
            } finally {
                $stream.Dispose()
            }
        } finally {
            $zip.Dispose()
        }

        $id = $xml.Package.Identity
        if ($id.Name -ne "ProjectMaho.Ferryx") {
            throw "Expected Identity Name='ProjectMaho.Ferryx', got '$($id.Name)'"
        }
        if ($id.Version -ne "2026.908.1.0") {
            throw "Expected Identity Version='2026.908.1.0', got '$($id.Version)'"
        }
        if ($id.Publisher -ne "CN=68073D7F-44F8-47BF-8B3E-B17FBDC44F36") {
            throw "Expected Identity Publisher='CN=68073D7F-44F8-47BF-8B3E-B17FBDC44F36', got '$($id.Publisher)'"
        }
        if ($id.ProcessorArchitecture -ne "x64") {
            throw "Expected Identity ProcessorArchitecture='x64', got '$($id.ProcessorArchitecture)'"
        }
        Write-Host "MSIX successfully validated: $($id.Name) $($id.Version) $($id.Publisher) $($id.ProcessorArchitecture)"
    }

    # -------------------------------------------------------------
    # Test 11: Packaged manifest with wrong Publisher fails closed
    # -------------------------------------------------------------
    Run-Test "Packaged manifest with wrong Publisher fails closed" {
        $validExe = Join-Path $scratchDir "ferryx_wrong_pub.exe"
        New-FakeExecutable -Path $validExe -AssemblyVersion "2026.908.1.0" -FileVersion "2026.908.1.0" -ProductVersion "2026.908.1"

        $iconsDir = Join-Path $scratchDir "icons"

        $manifestXml = @"
<?xml version="1.0" encoding="utf-8"?>
<Package
  xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
  xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
  xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"
  IgnorableNamespaces="uap rescap">
  <Identity Name="ProjectMaho.Ferryx" Publisher="CN=WRONG-UNAUTHORIZED-PUBLISHER" Version="0.1.0.0" ProcessorArchitecture="x64" />
  <Properties>
    <DisplayName>Ferryx</DisplayName>
    <PublisherDisplayName>Project Maho</PublisherDisplayName>
    <Logo>Assets\StoreLogo.png</Logo>
  </Properties>
  <Dependencies>
    <TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.17763.0" MaxVersionTested="10.0.22621.0" />
  </Dependencies>
  <Resources><Resource Language="en-US" /></Resources>
  <Applications>
    <Application Id="Ferryx" Executable="ferryx.exe" EntryPoint="Windows.FullTrustApplication">
      <uap:VisualElements DisplayName="Ferryx" Description="Ferryx" BackgroundColor="transparent"
        Square150x150Logo="Assets\Square150x150Logo.png" Square44x44Logo="Assets\Square44x44Logo.png">
        <uap:DefaultTile Wide310x150Logo="Assets\Square310x310Logo.png" Square310x310Logo="Assets\Square310x310Logo.png" Square71x71Logo="Assets\Square71x71Logo.png" />
      </uap:VisualElements>
    </Application>
  </Applications>
  <Capabilities><rescap:Capability Name="runFullTrust" /></Capabilities>
</Package>
"@
        $templatePath = Join-Path $scratchDir "AppxManifest_WrongPub.xml"
        $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
        [System.IO.File]::WriteAllText($templatePath, $manifestXml, $utf8NoBom)

        $packOutDir = Join-Path $scratchDir "pack_out_wrong_pub"
        New-Item -ItemType Directory -Force -Path $packOutDir | Out-Null

        Assert-Throws {
            $output = & powershell.exe -NoProfile -NonInteractive -Command "& '$buildMsixScript' -ExePath '$validExe' -Version 'v2026.09.08.1' -OutputDir '$packOutDir' -ManifestTemplate '$templatePath' -IconsDir '$iconsDir' -SkipSigning" 2>&1
            if ($LASTEXITCODE -eq 0) { throw "Expected non-zero exit code when packaged manifest has wrong Publisher" }
            $fullErr = ($output -join " ")
            throw "$fullErr"
        } "Packaged manifest Identity Publisher.*does not match expected"
    }

    # -------------------------------------------------------------
    # Test 12: Packaged manifest with wrong ProcessorArchitecture (arm64) fails closed
    # -------------------------------------------------------------
    Run-Test "Packaged manifest with wrong ProcessorArchitecture (arm64) fails closed" {
        $validExe = Join-Path $scratchDir "ferryx_arm64.exe"
        New-FakeExecutable -Path $validExe -AssemblyVersion "2026.908.1.0" -FileVersion "2026.908.1.0" -ProductVersion "2026.908.1"

        $iconsDir = Join-Path $scratchDir "icons"

        $manifestXml = @"
<?xml version="1.0" encoding="utf-8"?>
<Package
  xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
  xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
  xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"
  IgnorableNamespaces="uap rescap">
  <Identity Name="ProjectMaho.Ferryx" Publisher="CN=68073D7F-44F8-47BF-8B3E-B17FBDC44F36" Version="0.1.0.0" ProcessorArchitecture="arm64" />
  <Properties>
    <DisplayName>Ferryx</DisplayName>
    <PublisherDisplayName>Project Maho</PublisherDisplayName>
    <Logo>Assets\StoreLogo.png</Logo>
  </Properties>
  <Dependencies>
    <TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.17763.0" MaxVersionTested="10.0.22621.0" />
  </Dependencies>
  <Resources><Resource Language="en-US" /></Resources>
  <Applications>
    <Application Id="Ferryx" Executable="ferryx.exe" EntryPoint="Windows.FullTrustApplication">
      <uap:VisualElements DisplayName="Ferryx" Description="Ferryx" BackgroundColor="transparent"
        Square150x150Logo="Assets\Square150x150Logo.png" Square44x44Logo="Assets\Square44x44Logo.png">
        <uap:DefaultTile Wide310x150Logo="Assets\Square310x310Logo.png" Square310x310Logo="Assets\Square310x310Logo.png" Square71x71Logo="Assets\Square71x71Logo.png" />
      </uap:VisualElements>
    </Application>
  </Applications>
  <Capabilities><rescap:Capability Name="runFullTrust" /></Capabilities>
</Package>
"@
        $templatePath = Join-Path $scratchDir "AppxManifest_Arm64.xml"
        $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
        [System.IO.File]::WriteAllText($templatePath, $manifestXml, $utf8NoBom)

        $packOutDir = Join-Path $scratchDir "pack_out_arm64"
        New-Item -ItemType Directory -Force -Path $packOutDir | Out-Null

        Assert-Throws {
            $output = & powershell.exe -NoProfile -NonInteractive -Command "& '$buildMsixScript' -ExePath '$validExe' -Version 'v2026.09.08.1' -OutputDir '$packOutDir' -ManifestTemplate '$templatePath' -IconsDir '$iconsDir' -SkipSigning" 2>&1
            if ($LASTEXITCODE -eq 0) { throw "Expected non-zero exit code when packaged manifest has arm64 ProcessorArchitecture" }
            $fullErr = ($output -join " ")
            throw "$fullErr"
        } "Packaged manifest Identity ProcessorArchitecture.*does not match expected 'x64'"
    }

} finally {
    Remove-Item -Recurse -Force $scratchDir -ErrorAction SilentlyContinue
}

Write-Host "======================================================="
Write-Host "Test Run Summary: Total=$script:testCount, Passed=$script:passedCount, Failed=$script:failedCount"
Write-Host "======================================================="
if ($script:failedCount -gt 0) {
    Write-Host "Failed Tests:" -ForegroundColor Red
    $script:failures | ForEach-Object { Write-Host " - $_" -ForegroundColor Red }
    exit 1
} else {
    Write-Host "All tests passed!" -ForegroundColor Green
    exit 0
}
