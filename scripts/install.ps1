# Ferryx CLI One-Line Installer (Windows)
#
# Usage:
#   irm https://relay.ferryx.dev/install.ps1 | iex
#   irm https://ferryx.dev/install.ps1 | iex
#   & ([scriptblock]::Create((irm https://relay.ferryx.dev/install.ps1))) -Key <install key>
#
# Parameters:
#   -Key K       install key; handed to ferryx-cli in FERRYX_INSTALL_KEY, never as an argument
#   -KeyStdin    read the install key as exactly one line from stdin
#   -Email E     account email hint for the approval request
#   -NoUp        install the binary only; do not run "ferryx-cli up"
#   -Json        pass --json to "ferryx-cli up"; its JSON summary is the last stdout line
#
# Custom options via environment variables:
#   FERRYX_INSTALL_DIR : directory where ferryx-cli.exe is placed
#                        (default: %LOCALAPPDATA%\Ferryx\bin, falling back to %USERPROFILE%\.local\bin)
#   FERRYX_ORIGIN      : custom relay/download origin (default: https://relay.ferryx.dev,
#                        must be https or http on loopback)
#   FERRYX_VERSION     : specific tag version or 'latest'
#                        (default: latest, must match ^[A-Za-z0-9._-]+$)
#   FERRYX_INSTALL_KEY : install key (same as -Key)
#
# Every downloaded binary is checked against the SHA256SUMS of the same GitHub release
# (FERRYX_VERSION, or the latest release); a source whose bytes do not match is skipped, and
# the install fails when no source matches.
#
# Exit codes: 0 installed, 1 install failure, 2 usage error or unsupported platform
# (only windows/amd64 is supported; Windows ARM64 is not; a host whose Windows loader rejects
# the published ferryx-cli, for example a missing DLL), otherwise the exit code of
# "ferryx-cli up".
#
# Windows PowerShell 5.1 compatible; ASCII only.

param(
    [string]$Key = "",
    [switch]$KeyStdin,
    [string]$Email = "",
    [switch]$NoUp,
    [switch]$Json
)

$FerryxOrigin = $env:FERRYX_ORIGIN
if ([string]::IsNullOrEmpty($FerryxOrigin)) {
    $FerryxOrigin = "https://relay.ferryx.dev"
}

$FerryxVersion = $env:FERRYX_VERSION
if ([string]::IsNullOrEmpty($FerryxVersion)) {
    $FerryxVersion = "latest"
}

function Get-FerryxReleaseBaseUrl {
    if ($FerryxVersion -eq "latest") {
        return "https://github.com/Indosaram/ferryx/releases/latest/download"
    }
    return "https://github.com/Indosaram/ferryx/releases/download/" + $FerryxVersion
}

# Every failure names the download sources in use and the variables that override them.
function Write-FerryxSourceOverrides {
    [Console]::Error.WriteLine("    Download origin: " + $FerryxOrigin + " (override with FERRYX_ORIGIN); release: " + (Get-FerryxReleaseBaseUrl) + " (pin a tag with FERRYX_VERSION).")
}

$FerryxInstallKey = $Key
if ([string]::IsNullOrEmpty($FerryxInstallKey)) {
    $FerryxInstallKey = $env:FERRYX_INSTALL_KEY
}
if ($KeyStdin) {
    $FerryxInstallKey = [Console]::In.ReadLine()
    if ([string]::IsNullOrWhiteSpace($FerryxInstallKey)) {
        [Console]::Error.WriteLine("Error: -KeyStdin found no install key on stdin.")
        Write-FerryxSourceOverrides
        exit 2
    }
}
if ($null -eq $FerryxInstallKey) {
    $FerryxInstallKey = ""
}
$FerryxInstallKey = $FerryxInstallKey.Trim()
# Nothing launched below may inherit a key it was not explicitly handed.
Remove-Item Env:\FERRYX_INSTALL_KEY -ErrorAction SilentlyContinue

# Windows PowerShell 5.1 still negotiates TLS 1.0 on older builds; the relay and GitHub
# only accept TLS 1.2 or newer.
try {
    [System.Net.ServicePointManager]::SecurityProtocol = [System.Net.ServicePointManager]::SecurityProtocol -bor [System.Net.SecurityProtocolType]::Tls12
} catch {
}

function Write-FerryxError {
    param([Parameter(Mandatory = $true)][string]$Message)
    [Console]::Error.WriteLine("Error: " + $Message)
}

# FERRYX_ORIGIN must be https with a host, or http on a loopback host (localhost, 127.0.0.1, [::1]).
function Test-FerryxOrigin {
    param([Parameter(Mandatory = $true)][string]$Origin)

    $parsed = $null
    if (-not [System.Uri]::TryCreate($Origin, [System.UriKind]::Absolute, [ref]$parsed)) {
        return $false
    }
    if ($parsed.Scheme -eq "https") {
        return -not [string]::IsNullOrEmpty($parsed.Host)
    }
    if ($parsed.Scheme -eq "http") {
        $hostName = [string]$parsed.Host
        if ($hostName.StartsWith("[") -and $hostName.EndsWith("]")) {
            $hostName = $hostName.Substring(1, $hostName.Length - 2)
        }
        return @("localhost", "127.0.0.1", "::1") -contains $hostName.ToLowerInvariant()
    }
    return $false
}

# Reads at most the first two bytes of a file and returns them, or $null when the file is
# missing, shorter than two bytes, or unreadable. The artifact is never fully loaded.
function Read-FerryxHeader {
    param([Parameter(Mandatory = $true)][string]$Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        return $null
    }
    $stream = $null
    try {
        $stream = [System.IO.File]::OpenRead($Path)
        if ($stream.Length -lt 2) {
            return $null
        }
        $header = New-Object byte[] 2
        if ($stream.Read($header, 0, 2) -ne 2) {
            return $null
        }
        return , $header
    } catch {
        return $null
    } finally {
        if ($null -ne $stream) {
            $stream.Dispose()
        }
    }
}

# A genuine Windows executable starts with the DOS signature 'MZ' (0x4D 0x5A).
function Test-FerryxWindowsExecutable {
    param([Parameter(Mandatory = $true)][string]$Path)

    $header = Read-FerryxHeader -Path $Path
    if ($null -eq $header) {
        return $false
    }
    return (($header[0] -eq 0x4D) -and ($header[1] -eq 0x5A))
}

# Hex form of the two leading bytes, used to name what was actually downloaded.
function Get-FerryxHeaderHex {
    param([Parameter(Mandatory = $true)][string]$Path)

    $header = Read-FerryxHeader -Path $Path
    if ($null -eq $header) {
        return ""
    }
    $hex = ""
    foreach ($value in $header) {
        $hex = $hex + $value.ToString("X2")
    }
    return $hex
}

# Downloads one URL to one file. Throws on HTTP errors so the caller can try the next source.
function Invoke-FerryxDownload {
    param(
        [Parameter(Mandatory = $true)][string]$Url,
        [Parameter(Mandatory = $true)][string]$Destination
    )

    $client = New-Object System.Net.WebClient
    try {
        $client.DownloadFile($Url, $Destination)
    } finally {
        $client.Dispose()
    }
}

# Runs '<Executable> --help' and returns an object carrying the process exit code together
# with the captured stdout+stderr. Returns $null when the image could not be started at all
# (for example an architecture mismatch or a truncated download).
function Invoke-FerryxSelfCheckRun {
    param(
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $true)][string]$WorkDir
    )

    $stdoutPath = Join-Path $WorkDir "selfcheck.out"
    $stderrPath = Join-Path $WorkDir "selfcheck.err"
    try {
        $process = Start-Process -FilePath $Executable -ArgumentList "--help" -WorkingDirectory $WorkDir -NoNewWindow -Wait -PassThru -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath
    } catch {
        return $null
    }
    if ($null -eq $process) {
        return $null
    }
    $output = ""
    foreach ($capturePath in @($stdoutPath, $stderrPath)) {
        try {
            if (Test-Path -LiteralPath $capturePath -PathType Leaf) {
                $output = $output + [System.IO.File]::ReadAllText($capturePath)
            }
        } catch {
            # An unreadable capture file just leaves the banner check without that text.
        }
    }
    return [pscustomobject]@{
        ExitCode = $process.ExitCode
        Output   = $output
    }
}

# Exit codes that mean the platform never handed control to the CLI, rather than the CLI
# answering: 126/127 mirror install.sh (cannot execute / not found); the rest are Windows
# loader failures reported as NTSTATUS codes, which arrive here as negative Int32 values --
# 0xC0000135 STATUS_DLL_NOT_FOUND, 0xC0000139 STATUS_ENTRYPOINT_NOT_FOUND,
# 0xC000007B STATUS_INVALID_IMAGE_FORMAT, 0xC0000142 STATUS_DLL_INIT_FAILED.
function Test-FerryxUnrunnableExitCode {
    param([Parameter(Mandatory = $true)][int]$ExitCode)

    return @(126, 127, -1073741515, -1073741511, -1073741701, -1073741502) -contains $ExitCode
}

# Judges one '<Executable> --help' run. Measured contract: '--help exits 1 by design;
# install.sh accepts that, so a non-zero exit alone is not a failure'. A run passes when the
# process started, the platform loader did not reject the image, and the captured output
# carries the Ferryx CLI version or usage banner at the start of a line ("Usage: ferryx-cli",
# "Ferryx CLI is running" or "ferryx-cli <version>"; a crash message that merely contains the
# exe path does not count). A crash (any other negative NTSTATUS exit, for example 0xC0000005)
# fails even after the banner was printed, like a signal death in install.sh. Returns a result object
# with Passed, the run's ExitCode, and Failure - a short phrase for the caller's error line.
function Invoke-FerryxSelfCheck {
    param(
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $true)][string]$WorkDir
    )

    $run = Invoke-FerryxSelfCheckRun -Executable $Executable -WorkDir $WorkDir
    if ($null -eq $run) {
        return [pscustomobject]@{ Passed = $false; Unsupported = $true; ExitCode = $null; Failure = "cannot start on this machine" }
    }
    if (Test-FerryxUnrunnableExitCode -ExitCode $run.ExitCode) {
        $ntstatus = "0x{0:X8}" -f $run.ExitCode
        $reason = "--help exit " + $run.ExitCode
        if ($run.ExitCode -eq -1073741515) {
            $reason = $reason + ", " + $ntstatus + " STATUS_DLL_NOT_FOUND: a DLL it needs, such as the Microsoft Visual C++ runtime (VCRUNTIME140.dll), is missing"
        } elseif ($run.ExitCode -lt 0) {
            $reason = $reason + ", Windows loader status " + $ntstatus
        }
        return [pscustomobject]@{ Passed = $false; Unsupported = $true; ExitCode = $run.ExitCode; Failure = ("cannot run on this machine (" + $reason + ")") }
    }
    if ($run.ExitCode -lt 0) {
        return [pscustomobject]@{ Passed = $false; Unsupported = $false; ExitCode = $run.ExitCode; Failure = ("crashed during its self-check (--help exit " + $run.ExitCode + ", status " + ("0x{0:X8}" -f $run.ExitCode) + ")") }
    }
    if ($run.Output -notmatch '(?m)^(Usage: ferryx-cli|Ferryx CLI is running|ferryx-cli \d)') {
        return [pscustomobject]@{ Passed = $false; Unsupported = $false; ExitCode = $run.ExitCode; Failure = ("failed its self-check (--help exit " + $run.ExitCode + ", output did not look like the Ferryx CLI usage)") }
    }
    return [pscustomobject]@{ Passed = $true; Unsupported = $false; ExitCode = $run.ExitCode; Failure = $null }
}

# Removes the directories one install created: <InstallDir> and its parents up to $CreatedTop
# (the topmost one that did not exist before). Only empty directories go; the first one that
# cannot be removed (not empty, or in use) ends the walk, which is the intended stop.
function Remove-FerryxCreatedDirectories {
    param(
        [Parameter(Mandatory = $true)][string]$InstallDir,
        [string]$CreatedTop
    )

    if ([string]::IsNullOrEmpty($CreatedTop)) {
        return
    }
    $dir = $InstallDir
    while (-not [string]::IsNullOrEmpty($dir)) {
        try {
            [System.IO.Directory]::Delete($dir)
        } catch {
            return
        }
        if ($dir -eq $CreatedTop) {
            return
        }
        $dir = Split-Path -Parent $dir
    }
}

# Installs the verified download $TempExe as <InstallDir>\ferryx-cli.exe and returns the exit
# status for the caller: 0 installed, 2 the Windows loader rejected the candidate (unsupported),
# 1 any other failure. The candidate must pass its self-check before the install directory is
# created or anything is replaced, so a refusal leaves no new directory. The candidate is staged
# and the previous exe kept under names unique to this run (.ferryx-cli.exe.new.<guid>,
# .ferryx-cli.exe.old.<guid>), so a leftover or concurrent ferryx-cli.exe.new/.old - file or
# directory - is never used or removed. The previous exe is kept until the installed copy passes
# its own check; when that check fails, the previous exe is restored (or, on a fresh install, the
# new one and the directories this call created are removed).
function Install-FerryxCandidate {
    param(
        [Parameter(Mandatory = $true)][string]$TempExe,
        [Parameter(Mandatory = $true)][string]$InstallDir,
        [Parameter(Mandatory = $true)][string]$WorkDir
    )

    $targetExe = Join-Path $InstallDir "ferryx-cli.exe"
    $runId = [System.Guid]::NewGuid().ToString("N")
    $stagedExe = Join-Path $InstallDir (".ferryx-cli.exe.new." + $runId)
    $backupExe = Join-Path $InstallDir (".ferryx-cli.exe.old." + $runId)

    $candidateCheck = Invoke-FerryxSelfCheck -Executable $TempExe -WorkDir $WorkDir
    if (-not $candidateCheck.Passed) {
        Write-FerryxError ("downloaded executable " + $candidateCheck.Failure + ".")
        Write-FerryxError "    Existing installation, if any, was left untouched."
        Write-FerryxSourceOverrides
        if ($candidateCheck.Unsupported) {
            return 2
        }
        return 1
    }

    # The topmost missing ancestor of the install dir: what New-Item creates and a failure removes.
    $createdTop = $null
    $probe = $InstallDir
    while ((-not [string]::IsNullOrEmpty($probe)) -and (-not (Test-Path -LiteralPath $probe))) {
        $createdTop = $probe
        $probe = Split-Path -Parent $probe
    }
    try {
        New-Item -ItemType Directory -Force -Path $InstallDir -ErrorAction Stop | Out-Null
    } catch {
        Remove-FerryxCreatedDirectories -InstallDir $InstallDir -CreatedTop $createdTop
        Write-FerryxError ('Cannot create the install directory ''{0}'': {1}' -f $InstallDir, $_.Exception.Message)
        Write-FerryxSourceOverrides
        return 1
    }

    # Stage beside the target, set the previous exe aside, then move the staged copy in.
    $hadPrevious = Test-Path -LiteralPath $targetExe -PathType Leaf
    try {
        Copy-Item -LiteralPath $TempExe -Destination $stagedExe -Force -ErrorAction Stop
        if ($hadPrevious) {
            Move-Item -LiteralPath $targetExe -Destination $backupExe -Force -ErrorAction Stop
            # A rename keeps the old timestamp; mark the backup as fresh so a concurrent run's
            # age-guarded cleanup below leaves it alone. Best effort: the guard is only a safety margin.
            try { (Get-Item -LiteralPath $backupExe -Force).LastWriteTime = Get-Date } catch { }
        }
        Move-Item -LiteralPath $stagedExe -Destination $targetExe -Force -ErrorAction Stop
    } catch {
        $installError = $_.Exception.Message
        Remove-Item -LiteralPath $stagedExe -Force -ErrorAction SilentlyContinue
        if ($hadPrevious -and -not (Test-Path -LiteralPath $targetExe -PathType Leaf)) {
            Move-Item -LiteralPath $backupExe -Destination $targetExe -Force -ErrorAction SilentlyContinue
        }
        Remove-FerryxCreatedDirectories -InstallDir $InstallDir -CreatedTop $createdTop
        Write-FerryxError ('Cannot install to ''{0}'': {1}' -f $targetExe, $installError)
        Write-FerryxError "    Existing installation, if any, was left untouched."
        Write-FerryxSourceOverrides
        return 1
    }

    # Confirm the installed path itself runs; otherwise put the previous state back.
    $installedCheck = Invoke-FerryxSelfCheck -Executable $targetExe -WorkDir $WorkDir
    if (-not $installedCheck.Passed) {
        if ($hadPrevious -and -not (Test-Path -LiteralPath $backupExe -PathType Leaf)) {
            # The backup is gone (a concurrent install replaced ferryx-cli.exe and cleaned up), so
            # ferryx-cli.exe may now be that run's install: leave it rather than delete it.
            $restoreNote = ('    The previous ferryx-cli.exe backup ''{0}'' is gone (another install may have replaced it); ''{1}'' was left as it is.' -f $backupExe, $targetExe)
        } elseif ($hadPrevious) {
            Remove-Item -LiteralPath $targetExe -Force -ErrorAction SilentlyContinue
            Move-Item -LiteralPath $backupExe -Destination $targetExe -Force -ErrorAction SilentlyContinue
            if ((Test-Path -LiteralPath $targetExe -PathType Leaf) -and -not (Test-Path -LiteralPath $backupExe)) {
                $restoreNote = "    The previous ferryx-cli.exe was restored."
            } else {
                $restoreNote = ('    Restoring the previous ferryx-cli.exe failed; it was kept as ''{0}''.' -f $backupExe)
            }
        } else {
            Remove-Item -LiteralPath $targetExe -Force -ErrorAction SilentlyContinue
            Remove-FerryxCreatedDirectories -InstallDir $InstallDir -CreatedTop $createdTop
            $restoreNote = "    Nothing was left installed."
        }
        Write-FerryxError ('installed ''{0}'' {1}.' -f $targetExe, $installedCheck.Failure)
        Write-FerryxError $restoreNote
        Write-FerryxSourceOverrides
        if ($installedCheck.Unsupported) {
            return 2
        }
        return 1
    }
    # A running previous exe cannot be deleted on Windows; it then stays as .ferryx-cli.exe.old.<guid>
    # until a later successful install removes it here. Other runs' backups are removed only once they
    # are over an hour old, so a concurrent run's live backup is kept.
    Remove-Item -LiteralPath $backupExe -Force -ErrorAction SilentlyContinue
    $staleBefore = (Get-Date).AddMinutes(-60)
    Get-ChildItem -LiteralPath $InstallDir -Filter ".ferryx-cli.exe.old.*" -File -Force -ErrorAction SilentlyContinue |
        Where-Object { $_.LastWriteTime -lt $staleBefore } |
        Remove-Item -Force -ErrorAction SilentlyContinue

    Write-Host ("==> Successfully installed ferryx-cli to " + $targetExe)
    Write-Host ("    Verified: " + $targetExe + " executes (--help exit " + $installedCheck.ExitCode + ").")
    return 0
}

# Validate FERRYX_ORIGIN before anything is downloaded.
if (-not (Test-FerryxOrigin -Origin $FerryxOrigin)) {
    Write-FerryxError ('Invalid FERRYX_ORIGIN ''{0}''. Must be https:// or http:// on a loopback host (localhost, 127.0.0.1, [::1]).' -f $FerryxOrigin)
    Write-FerryxSourceOverrides
    exit 1
}

# Validate FERRYX_VERSION: must match ^[A-Za-z0-9._-]+$.
if ($FerryxVersion -notmatch '^[A-Za-z0-9._-]+$') {
    Write-FerryxError ('Invalid FERRYX_VERSION ''{0}''. Must match ^[A-Za-z0-9._-]+$.' -f $FerryxVersion)
    Write-FerryxSourceOverrides
    exit 1
}

$releaseBaseUrl = Get-FerryxReleaseBaseUrl

Write-Host "==> Ferryx CLI Installer"

# 1. Detect the CPU architecture. A 32-bit host process reports x86 in
#    PROCESSOR_ARCHITECTURE while PROCESSOR_ARCHITEW6432 carries the real machine
#    architecture, so the latter wins when it is present.
$rawArch = $env:PROCESSOR_ARCHITEW6432
if ([string]::IsNullOrEmpty($rawArch)) {
    $rawArch = $env:PROCESSOR_ARCHITECTURE
}
if ([string]::IsNullOrEmpty($rawArch)) {
    Write-FerryxError "Cannot detect the CPU architecture: neither PROCESSOR_ARCHITEW6432 nor PROCESSOR_ARCHITECTURE is set."
    Write-FerryxSourceOverrides
    exit 1
}
if ($rawArch.ToUpperInvariant() -eq "ARM64") {
    Write-FerryxError ('Windows ARM64 ({0}) is not supported; ferryx-cli ships for Windows x64 (AMD64) only.' -f $rawArch)
    Write-FerryxSourceOverrides
    exit 2
}
if ($rawArch.ToUpperInvariant() -ne "AMD64") {
    Write-FerryxError ('Windows {0} is not supported; ferryx-cli ships for Windows x64 (AMD64) only.' -f $rawArch)
    Write-FerryxSourceOverrides
    exit 2
}
$platformOs = "windows"
$platformArch = "amd64"

# 2. Determine the download artifact name.
$artifactName = "ferryx-cli-windows-amd64.exe"

# 3. Resolve the download URLs.
$primaryUrl = $FerryxOrigin + "/download/" + $artifactName
$secondaryUrl = $FerryxOrigin + "/download/ferryx-cli"
$githubUrl = $releaseBaseUrl + "/" + $artifactName

# 4. Temporary download location.
$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("ferryx-install-" + [System.Guid]::NewGuid().ToString("N"))
$tempExe = Join-Path $tempDir "ferryx-cli.exe"
$tempSums = Join-Path $tempDir "SHA256SUMS"

try {
    try {
        New-Item -ItemType Directory -Force -Path $tempDir -ErrorAction Stop | Out-Null
    } catch {
        Write-FerryxError ('Cannot create a temporary directory ''{0}'': {1}' -f $tempDir, $_.Exception.Message)
        Write-FerryxSourceOverrides
        exit 1
    }

    # 4a. Fetch the release checksums. Releases publish SHA256SUMS; older ones SHA256SUMS.txt.
    Write-Host ("--> Downloading SHA256SUMS from " + $releaseBaseUrl + " ...")
    $sumsFound = $false
    foreach ($sumsName in @("SHA256SUMS", "SHA256SUMS.txt")) {
        try {
            Invoke-FerryxDownload -Url ($releaseBaseUrl + "/" + $sumsName) -Destination $tempSums
        } catch {
            Remove-Item -LiteralPath $tempSums -Force -ErrorAction SilentlyContinue
            continue
        }
        if ((Test-Path -LiteralPath $tempSums -PathType Leaf) -and ((Get-Item -LiteralPath $tempSums).Length -gt 0)) {
            $sumsFound = $true
            break
        }
    }
    if (-not $sumsFound) {
        Write-FerryxError ("could not download SHA256SUMS from " + $releaseBaseUrl + "; refusing to install an unverified binary.")
        Write-FerryxSourceOverrides
        exit 1
    }
    $expectedSha256 = $null
    foreach ($line in (Get-Content -LiteralPath $tempSums)) {
        $fields = $line.Trim() -split '\s+'
        if ($fields.Count -ge 2 -and ($fields[1] -eq $artifactName -or $fields[1] -eq ("*" + $artifactName))) {
            $expectedSha256 = $fields[0].ToLowerInvariant()
            break
        }
    }
    if ($null -eq $expectedSha256 -or $expectedSha256 -notmatch '^[0-9a-f]{64}$') {
        Write-FerryxError ("SHA256SUMS from " + $releaseBaseUrl + " has no valid entry for " + $artifactName + ".")
        Write-FerryxSourceOverrides
        exit 1
    }

    # 5. Download, rejecting any source whose payload is not a Windows executable.
    Write-Host ("--> Downloading ferryx-cli for " + $platformOs + "/" + $platformArch + "...")
    $downloadSucceeded = $false
    $checksumMismatch = $false
    foreach ($url in @($primaryUrl, $secondaryUrl, $githubUrl)) {
        Write-Host ("    Trying " + $url + " ...")
        try {
            Invoke-FerryxDownload -Url $url -Destination $tempExe
        } catch {
            Write-Host ("    Skipping: " + $_.Exception.Message)
            Remove-Item -LiteralPath $tempExe -Force -ErrorAction SilentlyContinue
            continue
        }
        if (-not (Test-Path -LiteralPath $tempExe -PathType Leaf)) {
            Write-Host "    Skipping: no file was written"
            continue
        }
        if ((Get-Item -LiteralPath $tempExe).Length -le 0) {
            Write-Host "    Skipping: empty download"
            continue
        }
        if (Test-FerryxWindowsExecutable -Path $tempExe) {
            $actualSha256 = (Get-FileHash -LiteralPath $tempExe -Algorithm SHA256).Hash.ToLowerInvariant()
            if ($actualSha256 -ne $expectedSha256) {
                Write-Host ("    Skipping: SHA256 mismatch (expected " + $expectedSha256 + ", got " + $actualSha256 + ")")
                $checksumMismatch = $true
                continue
            }
            Write-Host ("    SHA256 verified: " + $actualSha256)
            $downloadSucceeded = $true
            break
        }
        Write-Host ("    Skipping: artifact is not a " + $platformOs + "/" + $platformArch + " executable (header " + (Get-FerryxHeaderHex -Path $tempExe) + ")")
    }

    if (-not $downloadSucceeded) {
        if ($checksumMismatch) {
            Write-FerryxError ("no downloaded ferryx-cli matched the SHA256SUMS of the release (" + $artifactName + ").")
            Write-FerryxSourceOverrides
            exit 1
        }
        Write-FerryxError "Failed to download ferryx-cli from available endpoints."
        Write-FerryxSourceOverrides
        exit 1
    }

    # 6. Confirm the accepted artifact is a Windows executable (the loop above already
    #    filtered; this is the authoritative gate and names the detected magic on failure).
    if (-not (Test-FerryxWindowsExecutable -Path $tempExe)) {
        Write-FerryxError ("no downloadable artifact matched " + $platformOs + "/" + $platformArch + ".")
        Write-FerryxError ("Detected header bytes: " + (Get-FerryxHeaderHex -Path $tempExe))
        Write-FerryxSourceOverrides
        exit 1
    }

    # 7. Determine the installation directory.
    $installDir = $env:FERRYX_INSTALL_DIR
    if ([string]::IsNullOrEmpty($installDir)) {
        if (-not [string]::IsNullOrEmpty($env:LOCALAPPDATA)) {
            $installDir = Join-Path $env:LOCALAPPDATA "Ferryx\bin"
        } elseif (-not [string]::IsNullOrEmpty($env:USERPROFILE)) {
            $installDir = Join-Path $env:USERPROFILE ".local\bin"
        } else {
            Write-FerryxError "Cannot determine an install directory: set FERRYX_INSTALL_DIR, or make sure LOCALAPPDATA or USERPROFILE is set."
            Write-FerryxSourceOverrides
            exit 1
        }
    }

    $targetExe = Join-Path $installDir "ferryx-cli.exe"

    # 8. Self-check the candidate, then create the directory and install it (see
    #    Install-FerryxCandidate). A loader rejection is an unsupported platform: exit 2, like
    #    install.sh; a refusal leaves no new directory and any existing installation untouched.
    $installStatus = Install-FerryxCandidate -TempExe $tempExe -InstallDir $installDir -WorkDir $tempDir
    if ($installStatus -ne 0) {
        exit $installStatus
    }

    # 9. Check PATH. Adding the directory to the user PATH must never fail the install.
    $environmentKey = $null
    try {
        $environmentKey = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey("Environment", $true)

        $userPath = $null
        $userPathKind = [Microsoft.Win32.RegistryValueKind]::ExpandString
        if ($null -ne $environmentKey) {
            $rawPath = $environmentKey.GetValue("Path", $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
            if ($null -ne $rawPath) {
                $userPath = [string]$rawPath
                $userPathKind = $environmentKey.GetValueKind("Path")
            }
        }

        $normalizedInstallDir = $installDir.TrimEnd("\")
        $pathAlreadyConfigured = $false
        if (-not [string]::IsNullOrEmpty($userPath)) {
            foreach ($entry in $userPath.Split(";")) {
                if ($entry.Trim().TrimEnd("\") -ieq $normalizedInstallDir) {
                    $pathAlreadyConfigured = $true
                    break
                }
            }
        }

        if ((-not $pathAlreadyConfigured) -and ($null -ne $environmentKey)) {
            $newUserPath = $installDir
            if (-not [string]::IsNullOrEmpty($userPath)) {
                $newUserPath = $userPath.TrimEnd(";") + ";" + $installDir
            }
            $environmentKey.SetValue("Path", $newUserPath, $userPathKind)
            Write-Host ""
            Write-Host ("Notice: added " + $installDir + " to your user PATH (HKCU:\Environment).")
            Write-Host "        Open a new terminal for the change to take effect."
        }
    } catch {
        Write-Host ""
        Write-Host ("Notice: could not add " + $installDir + " to your user PATH automatically: " + $_.Exception.Message)
        Write-Host "        Add that directory to PATH manually to run ferryx-cli from any shell."
    } finally {
        if ($null -ne $environmentKey) {
            $environmentKey.Close()
        }
    }

    $nextStep = @(
        "",
        "Next step: link this machine with your account and start its daemon:",
        ("  & '" + $targetExe + "' up --origin " + $FerryxOrigin),
        ("  (with an install key: set FERRYX_INSTALL_KEY, then & '" + $targetExe + "' up --key-env --origin " + $FerryxOrigin + ")"),
        ""
    )

    # 10. Link this machine and keep its daemon running. Progress uses Write-Host, so with -Json
    #     the only stdout output is the JSON summary line of "ferryx-cli up --json".
    if ($NoUp) {
        foreach ($line in $nextStep) { Write-Host $line }
        exit 0
    }
    if ([string]::IsNullOrEmpty($FerryxInstallKey) -and [string]::IsNullOrEmpty($Email) -and [Console]::IsInputRedirected) {
        # No key, no email hint and nobody at a terminal: an approval prompt would go unseen.
        Write-Host "No install key was given and stdin is not a terminal; skipping 'ferryx-cli up'."
        foreach ($line in $nextStep) { Write-Host $line }
        exit 0
    }

    $upArgs = @("up")
    if (-not [string]::IsNullOrEmpty($FerryxInstallKey)) {
        $upArgs += "--key-env"
    }
    $upArgs += @("--origin", $FerryxOrigin)
    if (-not [string]::IsNullOrEmpty($Email)) {
        $upArgs += @("--email", $Email)
    }
    if ($Json) {
        $upArgs += "--json"
    }

    Write-Host ("--> Running: ferryx-cli " + ($upArgs -join " "))
    $upStatus = 1
    try {
        if (-not [string]::IsNullOrEmpty($FerryxInstallKey)) {
            # The key travels in the environment of this one call only, never in argv.
            $env:FERRYX_INSTALL_KEY = $FerryxInstallKey
        }
        & $targetExe @upArgs
        $upStatus = $LASTEXITCODE
    } finally {
        Remove-Item Env:\FERRYX_INSTALL_KEY -ErrorAction SilentlyContinue
    }

    if ($upStatus -ne 0) {
        switch ($upStatus) {
            2 { $upReason = "usage error" }
            3 { $upReason = "the install key was already used, expired, or is invalid" }
            4 { $upReason = "the account owner denied this machine" }
            5 { $upReason = "the approval request expired" }
            6 { $upReason = "the account's plan limit was reached" }
            default { $upReason = "see the error above" }
        }
        Write-FerryxError ("ferryx-cli up failed (exit " + $upStatus + "): " + $upReason + ".")
        Write-FerryxError ("    ferryx-cli stays installed at " + $targetExe + "; rerun it with 'up --origin " + $FerryxOrigin + "' when ready.")
        Write-FerryxSourceOverrides
        exit $upStatus
    }
} finally {
    if ((-not [string]::IsNullOrEmpty($tempDir)) -and (Test-Path -LiteralPath $tempDir)) {
        Remove-Item -LiteralPath $tempDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}
