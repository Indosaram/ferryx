# Herdr reference-chat isolated acceptance runner (plan task 14).
#
# AUTHORED, NOT EXECUTED. The complete-code merge barrier owns every run of this script.
#
# This is the fixed CLI the plan's "Exact command contract" section names:
#
#   powershell -NoProfile -File scripts/qa/herdr-reference-chat.ps1
#     -Scenario QA-01 -Case happy
#     -FixtureManifest C:\ferryx-qa\herdr\fixtures.json
#     -CandidateManifest C:\ferryx-qa\herdr\candidate.json
#     -EvidenceDir C:\ferryx-evidence\herdr-reference\QA-01\happy
#
# Every QA row runs twice with its exact ID and -Case happy / -Case failure; the evidence
# directory substitutes that ID and case. -Scenario ALL -Case all is the final gate.
#
# The wrapper validates the arguments and the manifests, then hands the whole job to
# herdr-reference-chat.mjs and propagates its exit code unchanged:
#
#   0  every selected branch passed
#   2  BLOCKED: a real dependency (host, credential, device, driver, transcript) is missing
#   3  interaction failure (a surface could not be driven)
#   4  a selected branch FAILED its observable, or QA-11 detected tampering
#   5  usage error
#   130 interrupted
#
# It never invents a default for a manifest or an evidence directory, and it never converts a
# nonzero child result into success.

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("QA-01", "QA-02", "QA-03", "QA-04", "QA-05", "QA-06", "QA-07", "QA-08", "QA-09", "QA-10", "QA-11", "ALL")]
    [string]$Scenario,

    [Parameter(Mandatory = $true)]
    [ValidateSet("happy", "failure", "all")]
    [string]$Case,

    [Parameter(Mandatory = $true)]
    [string]$FixtureManifest,

    [Parameter(Mandatory = $true)]
    [string]$CandidateManifest,

    [Parameter(Mandatory = $true)]
    [string]$EvidenceDir,

    # Where the runner writes the candidate/page/target/PTY binding that the task 15 device
    # producers read through FERRYX_HERDR_REFERENCE_BINDING. The runner prints the resolved
    # absolute path on stdout; export that value verbatim. Default:
    # <EvidenceDir>/device-binding.json
    [string]$DeviceBindingOut,

    # Drives a real browser against the served candidate UI and, when a host declares
    # startLocal, launches the frozen gateway on a throwaway profile. Without it the runner
    # still performs every HTTP scenario and reports the browser-backed branches as blocked.
    [switch]$AllowHost,

    [ValidateSet("chromium-headless", "chrome", "msedge")]
    [string]$BrowserChannel = "chrome",

    [ValidateRange(1000, 600000)]
    [int]$TimeoutMs = 30000,

    [string]$NodeExecutable = "node"
)

$ErrorActionPreference = "Stop"

# The runner is the contract; this wrapper never reimplements it.
$runner = Join-Path $PSScriptRoot "herdr-reference-chat.mjs"
if (-not (Test-Path -LiteralPath $runner -PathType Leaf)) {
    Write-Error "Reference-chat runner missing: $runner"
    exit 5
}

# Every required input must exist as a real file before the child is started, so a typo in a
# manifest path is a usage error here rather than a confusing BLOCKED later.
foreach ($required in @($FixtureManifest, $CandidateManifest)) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
        Write-Error "Required manifest missing: $required"
        exit 5
    }
    if ((Get-Item -LiteralPath $required).Length -eq 0) {
        Write-Error "Required manifest is empty: $required"
        exit 5
    }
}

# A malformed manifest is reported here, not silently accepted and guessed at downstream.
foreach ($manifest in @($FixtureManifest, $CandidateManifest)) {
    try {
        $null = Get-Content -LiteralPath $manifest -Raw | ConvertFrom-Json
    }
    catch {
        Write-Error "Manifest is not readable JSON: $manifest"
        exit 5
    }
}

if (-not (Get-Command $NodeExecutable -ErrorAction SilentlyContinue)) {
    Write-Error "Node executable not found: $NodeExecutable"
    exit 5
}

# The evidence directory is created by the wrapper so the child always has somewhere to write
# its report; an unwritable directory is a usage error.
try {
    if (-not (Test-Path -LiteralPath $EvidenceDir)) {
        $null = New-Item -ItemType Directory -Path $EvidenceDir -Force
    }
}
catch {
    Write-Error "Evidence directory is not writable: $EvidenceDir"
    exit 5
}

$channel = if ($BrowserChannel -eq "chromium-headless") { "chromium" } else { $BrowserChannel }

$arguments = @(
    $runner,
    "--scenario", $Scenario,
    "--case", $Case,
    "--fixture-manifest", (Resolve-Path -LiteralPath $FixtureManifest).Path,
    "--candidate-manifest", (Resolve-Path -LiteralPath $CandidateManifest).Path,
    "--evidence-dir", (Resolve-Path -LiteralPath $EvidenceDir).Path,
    "--browser-channel", $channel,
    "--timeout-ms", $TimeoutMs
)
if ($AllowHost) {
    $arguments += @("--allow-host", "true")
}
if ($DeviceBindingOut) {
    $arguments += @("--device-binding-out", $DeviceBindingOut)
}

Write-Host "herdr-reference-chat: scenario=$Scenario case=$Case evidence=$EvidenceDir allowHost=$([bool]$AllowHost)"
& $NodeExecutable @arguments
$exitCode = $LASTEXITCODE

# The exit code is the verdict. It is propagated unchanged: a BLOCKED or FAILED run must never
# be reported as success by this wrapper.
switch ($exitCode) {
    0 { Write-Host "PASS: every selected branch passed." }
    2 { Write-Host "BLOCKED: a required dependency is missing." }
    3 { Write-Host "INTERACTION FAILURE: a surface could not be driven." }
    4 { Write-Host "FAIL: a selected branch failed, or provenance was rejected." }
    5 { Write-Host "USAGE ERROR." }
    130 { Write-Host "INTERRUPTED." }
    default { Write-Host "UNEXPECTED EXIT: $exitCode" }
}

exit $exitCode

