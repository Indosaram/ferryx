# Preflight: fresh root, uploaded inputs match in-manifest, disk, toolchain, ghostty pin. Leaves no processes running.
param([Parameter(Mandatory = $true)][string]$Run)
. (Join-Path $PSScriptRoot 'common.ps1')

$Step = 'preflight'
$root = Get-WsuRoot $Run
foreach ($d in 'logs', 'evidence', 'cleanup') { New-Item -ItemType Directory -Force -Path (Join-Path $root $d) | Out-Null }
Exit-IfStepRan $root $Run $Step
$log = Join-Path $root 'logs\preflight.log'
$code = 1
try {
  $in = Join-Path $root 'in'
  if ($PSScriptRoot -ine (Join-Path $in 'runner')) { throw "RUNNER_LOCATION: $PSScriptRoot" }
  if (Test-Path -LiteralPath (Join-Path $root 'run-start.txt')) { throw 'RUN_NOT_FRESH: run-start.txt exists' }
  Write-Text (Join-Path $root 'run-start.txt') ("run=$Run`nutc=" + [DateTime]::UtcNow.ToString('o') + "`nhost=$env:COMPUTERNAME`nuser=$env:USERNAME`n")

  $lines = @([IO.File]::ReadAllLines((Join-Path $in 'in-manifest.tsv')) | Where-Object { $_.Trim() })
  foreach ($line in $lines) {
    $parts = $line.Split("`t")
    $rel = $parts[1] -replace '/', '\'
    $actual = Get-Sha256 (Join-Path $in $rel)
    if ($actual -ne $parts[0]) { throw "INPUT_HASH_MISMATCH: $rel" }
    Add-Line $log "input ok $actual $rel"
  }
  $present = @(Get-ChildItem -LiteralPath $in -Recurse -File | Where-Object { $_.Name -ne 'in-manifest.tsv' })
  if ($present.Count -ne $lines.Count) { throw "INPUT_SET_MISMATCH: listed=$($lines.Count) present=$($present.Count)" }

  $free = (Get-PSDrive -Name C).Free
  Add-Line $log "c-free-bytes=$free"
  if ($free -lt $MinFreeBytes) { throw "DISK_LOW: $free < $MinFreeBytes" }

  $null = Set-WsuEnv $root
  $tool = Join-Path $root 'evidence\toolchain.txt'
  $tar = Join-Path $env:SystemRoot 'System32\tar.exe'
  foreach ($t in @(@('git', '--version'), @($tar, '--version'), @('cargo', '--version'), @('rustc', '-vV'), @('bun', '--version'), @('zig', 'version'))) {
    $out = Invoke-Native $log $t[0] @($t[1])
    Add-Line $tool ("[" + (Get-Command $t[0]).Source + "]")
    foreach ($l in $out) { Add-Line $tool $l }
  }
  $gh = (Invoke-Native $log 'git' @('-C', $GhosttySrc, 'rev-parse', 'HEAD'))[0].Trim()
  Add-Line $tool "ghostty=$gh"
  if ($gh -ne $GhosttyPin) { throw "GHOSTTY_PIN_MISMATCH: $gh" }
  $code = 0
} catch {
  Add-Line $log ('ERROR: ' + $_.Exception.Message)
  $code = 1
}
Complete-Step $root $Run $Step $code
exit $code
