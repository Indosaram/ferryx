# Ferryx CLI One-Line Installer (Windows)
#
# Usage:
#   irm https://relay.ferryx.dev/install.ps1 | iex
#   irm https://ferryx.dev/install.ps1 | iex
#
# Custom options via environment variables:
#   FERRYX_INSTALL_DIR : directory where ferryx-cli.exe is placed
#                        (default: %LOCALAPPDATA%\Ferryx\bin, falling back to %USERPROFILE%\.local\bin)
#   FERRYX_ORIGIN      : custom relay/download origin (default: https://relay.ferryx.dev,
#                        must be https or http on loopback)
#   FERRYX_VERSION     : specific tag version or 'latest'
#                        (default: latest, must match ^[A-Za-z0-9._-]+$)
#
# Windows PowerShell 5.1 compatible; ASCII only.

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
# carries the Ferryx CLI usage banner ("ferryx-cli" or "Usage:"). Returns a result object
# with Passed, the run's ExitCode, and Failure - a short phrase for the caller's error line.
function Invoke-FerryxSelfCheck {
    param(
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $true)][string]$WorkDir
    )

    $run = Invoke-FerryxSelfCheckRun -Executable $Executable -WorkDir $WorkDir
    if ($null -eq $run) {
        return [pscustomobject]@{ Passed = $false; ExitCode = $null; Failure = "cannot start on this machine" }
    }
    if (Test-FerryxUnrunnableExitCode -ExitCode $run.ExitCode) {
        return [pscustomobject]@{ Passed = $false; ExitCode = $run.ExitCode; Failure = ("cannot run on this machine (--help exit " + $run.ExitCode + ")") }
    }
    if (($run.Output -notmatch "ferryx-cli") -and ($run.Output -notmatch "Usage:")) {
        return [pscustomobject]@{ Passed = $false; ExitCode = $run.ExitCode; Failure = ("failed its self-check (--help exit " + $run.ExitCode + ", output did not look like the Ferryx CLI usage)") }
    }
    return [pscustomobject]@{ Passed = $true; ExitCode = $run.ExitCode; Failure = $null }
}

$FerryxOrigin = $env:FERRYX_ORIGIN
if ([string]::IsNullOrEmpty($FerryxOrigin)) {
    $FerryxOrigin = "https://relay.ferryx.dev"
}

$FerryxVersion = $env:FERRYX_VERSION
if ([string]::IsNullOrEmpty($FerryxVersion)) {
    $FerryxVersion = "latest"
}

# Validate FERRYX_ORIGIN before anything is downloaded.
if (-not (Test-FerryxOrigin -Origin $FerryxOrigin)) {
    Write-FerryxError ('Invalid FERRYX_ORIGIN ''{0}''. Must be https:// or http:// on a loopback host (localhost, 127.0.0.1, [::1]).' -f $FerryxOrigin)
    exit 1
}

# Validate FERRYX_VERSION: must match ^[A-Za-z0-9._-]+$.
if ($FerryxVersion -notmatch '^[A-Za-z0-9._-]+$') {
    Write-FerryxError ('Invalid FERRYX_VERSION ''{0}''. Must match ^[A-Za-z0-9._-]+$.' -f $FerryxVersion)
    exit 1
}

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
    exit 1
}
if ($rawArch.ToUpperInvariant() -ne "AMD64") {
    Write-FerryxError ('Unsupported CPU architecture ''{0}''. The Ferryx CLI is published for windows/amd64 only.' -f $rawArch)
    exit 1
}
$platformOs = "windows"
$platformArch = "amd64"

# 2. Determine the download artifact name.
$artifactName = "ferryx-cli-windows-amd64.exe"

# 3. Resolve the download URLs.
$primaryUrl = $FerryxOrigin + "/download/" + $artifactName
$secondaryUrl = $FerryxOrigin + "/download/ferryx-cli"
if ($FerryxVersion -eq "latest") {
    $githubUrl = "https://github.com/Indosaram/ferryx/releases/latest/download/" + $artifactName
} else {
    $githubUrl = "https://github.com/Indosaram/ferryx/releases/download/" + $FerryxVersion + "/" + $artifactName
}

# 4. Temporary download location.
$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("ferryx-install-" + [System.Guid]::NewGuid().ToString("N"))
$tempExe = Join-Path $tempDir "ferryx-cli.exe"

try {
    try {
        New-Item -ItemType Directory -Force -Path $tempDir -ErrorAction Stop | Out-Null
    } catch {
        Write-FerryxError ('Cannot create a temporary directory ''{0}'': {1}' -f $tempDir, $_.Exception.Message)
        exit 1
    }

    # 5. Download, rejecting any source whose payload is not a Windows executable.
    Write-Host ("--> Downloading ferryx-cli for " + $platformOs + "/" + $platformArch + "...")
    $downloadSucceeded = $false
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
            $downloadSucceeded = $true
            break
        }
        Write-Host ("    Skipping: artifact is not a " + $platformOs + "/" + $platformArch + " executable (header " + (Get-FerryxHeaderHex -Path $tempExe) + ")")
    }

    if (-not $downloadSucceeded) {
        Write-FerryxError "Failed to download ferryx-cli from available endpoints."
        exit 1
    }

    # 6. Confirm the accepted artifact is a Windows executable (the loop above already
    #    filtered; this is the authoritative gate and names the detected magic on failure).
    if (-not (Test-FerryxWindowsExecutable -Path $tempExe)) {
        Write-FerryxError ("no downloadable artifact matched " + $platformOs + "/" + $platformArch + ".")
        Write-FerryxError ("Detected header bytes: " + (Get-FerryxHeaderHex -Path $tempExe))
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
            exit 1
        }
    }

    $targetExe = Join-Path $installDir "ferryx-cli.exe"
    $stagedExe = $targetExe + ".new"

    try {
        New-Item -ItemType Directory -Force -Path $installDir -ErrorAction Stop | Out-Null
    } catch {
        Write-FerryxError ('Cannot create the install directory ''{0}'': {1}' -f $installDir, $_.Exception.Message)
        exit 1
    }

    # 8. Verify the candidate can execute BEFORE it replaces anything. Measured contract:
    #    '--help exits 1 by design; install.sh accepts that, so a non-zero exit alone is not a
    #    failure' - what must hold is that the process starts and prints the Ferryx CLI usage.
    #    A payload that cannot run here must leave any existing installation byte-for-byte
    #    untouched.
    $candidateCheck = Invoke-FerryxSelfCheck -Executable $tempExe -WorkDir $tempDir
    if (-not $candidateCheck.Passed) {
        Write-FerryxError ("downloaded executable " + $candidateCheck.Failure + ".")
        Write-FerryxError "    Existing installation, if any, was left untouched."
        exit 1
    }

    # 9. Install: stage beside the target, then move over it.
    Remove-Item -LiteralPath $stagedExe -Force -ErrorAction SilentlyContinue
    try {
        Copy-Item -LiteralPath $tempExe -Destination $stagedExe -Force -ErrorAction Stop
        Move-Item -LiteralPath $stagedExe -Destination $targetExe -Force -ErrorAction Stop
    } catch {
        Remove-Item -LiteralPath $stagedExe -Force -ErrorAction SilentlyContinue
        Write-FerryxError ('Cannot install to ''{0}'': {1}' -f $targetExe, $_.Exception.Message)
        exit 1
    }

    # 10. Confirm the installed path itself runs.
    $installedCheck = Invoke-FerryxSelfCheck -Executable $targetExe -WorkDir $tempDir
    if (-not $installedCheck.Passed) {
        Write-FerryxError ('installed ''{0}'' {1}.' -f $targetExe, $installedCheck.Failure)
        exit 1
    }

    Write-Host ("==> Successfully installed ferryx-cli to " + $targetExe)
    Write-Host ("    Verified: " + $targetExe + " executes (--help exit " + $installedCheck.ExitCode + ").")

    # 11. Check PATH. Adding the directory to the user PATH must never fail the install.
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

    Write-Host ""
    Write-Host "Next steps:"
    Write-Host "  1. Start the headless daemon (from a new terminal):"
    Write-Host "     Start-Process ferryx-cli -ArgumentList '--daemon'"
    Write-Host ""
    Write-Host "  2. Link your machine with your account:"
    Write-Host ("     ferryx-cli account login --email <your-email> --origin " + $FerryxOrigin)
    Write-Host ""
} finally {
    if ((-not [string]::IsNullOrEmpty($tempDir)) -and (Test-Path -LiteralPath $tempDir)) {
        Remove-Item -LiteralPath $tempDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}
