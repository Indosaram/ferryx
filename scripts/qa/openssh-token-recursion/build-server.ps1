# PowerShell script to build minimal sshd.exe from OpenSSH-portable source
# Statically scoped, process-local environment only - no global system/service changes
[CmdletBinding()]
param (
    [Parameter(Mandatory = $true)]
    [string]$HomeDir,

    [ValidateSet('Release', 'Debug')]
    [string]$Configuration = 'Release',

    [ValidateSet('x64', 'Win32', 'ARM64')]
    [string]$Platform = 'x64'
)

$ErrorActionPreference = 'Stop'

Write-Host "=========================================================="
Write-Host "OpenSSH sshd.exe Build Recipe"
Write-Host "HomeDir:       $HomeDir"
Write-Host "Configuration: $Configuration"
Write-Host "Platform:      $Platform"
Write-Host "=========================================================="

if (-not (Test-Path $HomeDir)) {
    throw "Target HomeDir does not exist: $HomeDir"
}

$resolvedHome = (Resolve-Path $HomeDir).Path
$opensshDir = Join-Path $resolvedHome "contrib\win32\openssh"
$solutionFile = Join-Path $opensshDir "Win32-OpenSSH.sln"
$pathsTargets = Join-Path $opensshDir "paths.targets"

if (-not (Test-Path $solutionFile)) {
    throw "OpenSSH solution not found at $solutionFile. Ensure repository is unpacked into $resolvedHome."
}

# 1. Locate MSBuild 2022 BuildTools (Process-Local)
$msBuildExe = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\MSBuild\Current\Bin\MSBuild.exe"
if (-not (Test-Path $msBuildExe)) {
    # Check VS install location via vswhere if not in default BuildTools path
    $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path $vswhere) {
        $vsPath = & $vswhere -latest -products * -requires Microsoft.Component.MSBuild -property installationPath
        if ($vsPath) {
            $msBuildExe = Join-Path $vsPath "MSBuild\Current\Bin\MSBuild.exe"
        }
    }
}
if (-not (Test-Path $msBuildExe)) {
    throw "MSBuild.exe not found at $msBuildExe"
}
Write-Host "[OK] Using MSBuild: $msBuildExe"

# 2. Locate Windows 10/11 SDK Version (Prefer 10.0.26100.0 or 10.0.28000.0)
$sdkLibRoot = "C:\Program Files (x86)\Windows Kits\10\Lib"
$sdkVersion = $null
if (Test-Path $sdkLibRoot) {
    $availableSdks = Get-ChildItem -Path $sdkLibRoot | Where-Object { $_.PSIsContainer -and ($_.Name -like "10.*") } | Select-Object -ExpandProperty Name | Sort-Object -Descending
    foreach ($ver in $availableSdks) {
        $umLib = Join-Path $sdkLibRoot "$ver\um\x64\kernel32.lib"
        if (Test-Path $umLib) {
            $sdkVersion = $ver
            break
        }
    }
}
if (-not $sdkVersion) {
    throw "No usable Windows SDK found under $sdkLibRoot"
}
Write-Host "[OK] Using Windows SDK Version: $sdkVersion"

# 3. Locate vcpkg (Process-Local)
# Visual Studio 2022 BuildTools installs embedded vcpkg at:
$vsVcpkg = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\vcpkg\vcpkg.exe"
$vcpkgExe = $null

if (Test-Path $vsVcpkg) {
    $vcpkgExe = $vsVcpkg
} elseif (Get-Command vcpkg -ErrorAction SilentlyContinue) {
    $vcpkgExe = (Get-Command vcpkg).Source
} else {
    $candidate = "C:\vcpkg\vcpkg.exe"
    if (Test-Path $candidate) {
        $vcpkgExe = $candidate
    }
}

if (-not $vcpkgExe) {
    Write-Warning "vcpkg.exe not found. If dependencies are not pre-installed, vcpkg bootstrap will be needed."
} else {
    Write-Host "[OK] Using vcpkg: $vcpkgExe"
    $env:VCPKG_ROOT = Split-Path -Parent $vcpkgExe
}

# 4. Process-Local Environment Configuration
# Strictly do not alter machine/user PATH or system environment
$vsInstallPath = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools"
$env:vctargetspath = "$vsInstallPath\MSBuild\Microsoft\VC\v170\"
$env:VS170COMNTOOLS = "$vsInstallPath\Common7\Tools\"

$gitCmd = "C:\Program Files\Git\cmd"
if ((Test-Path $gitCmd) -and ($env:Path -notlike "*$gitCmd*")) {
    $env:Path = "$gitCmd;$env:Path"
}

# 5. Build Core Dependencies & sshd Project
# OpenSSH build uses vcpkg manifest mode defined in contrib\win32\openssh\vcpkg.json
# Dependencies: zlib, libressl, libfido2, libcbor
# Targets required for sshd: config.vcxproj, win32iocompat.vcxproj, openbsd_compat.vcxproj, libssh.vcxproj, sshd.vcxproj
Write-Host "Starting MSBuild for sshd target..."

$msBuildArgs = @(
    $solutionFile,
    "/t:Build",
    "/p:Configuration=$Configuration",
    "/p:Platform=$Platform",
    "/p:WindowsSDKVersion=$sdkVersion",
    "/p:WindowsTargetPlatformVersion=$sdkVersion",
    "/m:2",
    "/nologo",
    "/v:m"
)

# If embedded vcpkg is present, supply vcpkg props/targets to msbuild
$vcpkgTargets = "$vsInstallPath\VC\vcpkg\scripts\buildsystems\msbuild\vcpkg.targets"
if (Test-Path $vcpkgTargets) {
    $msBuildArgs += "/p:VcpkgEnableManifest=true"
    $msBuildArgs += "/p:ForceImportBeforeCppTargets=$vcpkgTargets"
    $msBuildArgs += "/p:VcpkgAdditionalInstallOptions=--overlay-triplets=$opensshDir\vcpkg_triplets --overlay-ports=$opensshDir\vcpkg_overlay_ports --x-buildtrees-root=$resolvedHome\blds --x-packages-root=$resolvedHome\pkgs"
}

Write-Host "Executing: & `"$msBuildExe`" $($msBuildArgs -join ' ')"
& $msBuildExe $msBuildArgs

if ($LASTEXITCODE -ne 0) {
    throw "MSBuild failed with exit code $LASTEXITCODE"
}

# 6. Verify Output Binary
$binDir = Join-Path $resolvedHome "bin\$Platform\$Configuration"
$sshdBin = Join-Path $binDir "sshd.exe"

if (-not (Test-Path $sshdBin)) {
    # Check alternate output path (contrib\win32\openssh\bin)
    $altBin = Join-Path $opensshDir "bin\$Platform\$Configuration\sshd.exe"
    if (Test-Path $altBin) {
        $sshdBin = $altBin
    }
}

if (Test-Path $sshdBin) {
    Write-Host "=========================================================="
    Write-Host "[SUCCESS] sshd.exe built successfully at: $sshdBin"
    Write-Host "Binary size: $((Get-Item $sshdBin).Length) bytes"
    Write-Host "=========================================================="
} else {
    throw "sshd.exe was not produced at expected location: $binDir\sshd.exe"
}
