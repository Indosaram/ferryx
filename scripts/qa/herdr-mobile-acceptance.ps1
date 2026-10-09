[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][ValidateSet("isolated")][string]$GatewayProfile,
    [switch]$AllowHost,
    [Parameter(Mandatory=$true)][string]$GatewayBinary,
    [Parameter(Mandatory=$true)][string]$UiDist,
    [Parameter(Mandatory=$true)][string]$CandidateManifest,
    [string]$ConsumerReceipt,
    [Alias("MobileImeEvidence")][string]$AndroidDeviceEvidence,
    [string]$NativeSpeechEvidence,
    [ValidateSet("chromium-headless","chrome","msedge")][string]$Browser="chromium-headless",
    [string]$EvidenceDir="C:\ferryx-evidence\herdr-mobile-interaction-plan\task-10",
    [string]$NodeExecutable="node"
)
$ErrorActionPreference="Stop"
if ($env:OS -ne "Windows_NT" -or -not $AllowHost) {
    throw "Acceptance requires Windows and explicit -AllowHost authorization on the designated verifier."
}
foreach ($path in @($GatewayBinary,$UiDist,$CandidateManifest)) {
    if (-not (Test-Path -LiteralPath $path)) { throw "Required composed artifact missing: $path" }
}
$runner=Join-Path $PSScriptRoot "herdr-mobile-browser-runner.mjs"
$channel=if($Browser -eq "chromium-headless") {"chromium"} else {$Browser}
$arguments=@($runner,"--evidence-dir",$EvidenceDir,"--gateway-binary",(Resolve-Path $GatewayBinary).Path,
    "--ui-dist",(Resolve-Path $UiDist).Path,"--candidate-manifest",(Resolve-Path $CandidateManifest).Path,
    "--browser-channel",$channel,"--allow-host","true")
if($ConsumerReceipt) {$arguments+=@("--consumer-receipt",$ConsumerReceipt)}
if($AndroidDeviceEvidence) {$arguments+=@("--android-device-evidence",$AndroidDeviceEvidence)}
if($NativeSpeechEvidence) {$arguments+=@("--native-speech-evidence",$NativeSpeechEvidence)}
& $NodeExecutable @arguments
exit $LASTEXITCODE
