# Purge: delete the run root only after a clean cleanup; unlinks the ghostty junction first and proves the pinned source untouched.
# Output goes to stdout only (the local run.sh log is the receipt, since the root is removed).
param([Parameter(Mandatory = $true)][string]$Run)
. (Join-Path $PSScriptRoot 'common.ps1')

function Get-GhosttyState {
  $prev = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
  try {
    $head = (& git -C $GhosttySrc rev-parse HEAD 2>&1 | ForEach-Object { "$_" }) -join ''
    if ($LASTEXITCODE -ne 0) { throw "GHOSTTY_REV_PARSE_FAILED: $head" }
    $status = @(& git -C $GhosttySrc status --porcelain --ignore-submodules=all 2>&1 | ForEach-Object { "$_" })
    if ($LASTEXITCODE -ne 0) { throw 'GHOSTTY_STATUS_FAILED' }
  } finally { $ErrorActionPreference = $prev }
  return "head=$($head.Trim()) dirty=$($status.Count)"
}

$root = Get-WsuRoot $Run
$code = 1
try {
  $exitLog = Join-Path $root 'cleanup\cleanup-exit.log'
  if (-not (Test-Path -LiteralPath $exitLog) -or ([IO.File]::ReadAllLines($exitLog)[0].Trim() -ne 'EXIT=0')) {
    Write-Output 'PURGE_REFUSED: remote cleanup-exit.log missing or nonzero'
    Write-Output "WSU $Run purge EXIT=66"
    exit 66
  }
  foreach ($r in @(Read-ProcRecords $root)) {
    $cur = Get-ProcIdentity ([int]$r.pid)
    if ($null -ne $cur -and (Test-SameInstant (ConvertTo-Utc $cur.creationUtc) (ConvertTo-Utc $r.creationUtc))) { throw "RECORDED_PROCESS_ALIVE: $($r.pid) $($r.role)" }
  }
  $before = Get-GhosttyState
  Write-Output "ghostty-before $before"
  if ($before -ne "head=$GhosttyPin dirty=0") { throw "GHOSTTY_STATE_UNEXPECTED_BEFORE: $before" }

  $vg = Join-Path $root 'src\src-tauri\vendor\ghostty'
  if (Test-Path -LiteralPath $vg) {
    if (-not ((Get-Item -LiteralPath $vg -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw "GHOSTTY_PATH_NOT_JUNCTION: $vg" }
    & (Join-Path $env:SystemRoot 'System32\cmd.exe') /d /c rmdir "$vg"
    if ($LASTEXITCODE -ne 0) { throw "JUNCTION_UNLINK_FAILED: $LASTEXITCODE" }
    if (Test-Path -LiteralPath $vg) { throw 'JUNCTION_STILL_PRESENT' }
    Write-Output "junction-unlinked $vg"
  }
  $mid = Get-GhosttyState
  if ($mid -ne $before) { throw "GHOSTTY_CHANGED_BY_UNLINK: $mid" }

  # rmdir /s does not traverse reparse points, so any remaining junction is unlinked, not followed.
  & (Join-Path $env:SystemRoot 'System32\cmd.exe') /d /c rmdir /s /q "$root"
  $rmrc = $LASTEXITCODE
  $after = Get-GhosttyState
  Write-Output "rmdir-exit=$rmrc ghostty-after $after"
  if ($after -ne $before) { throw "GHOSTTY_CHANGED_BY_PURGE: $after" }
  if (Test-Path -LiteralPath $root) { throw "ROOT_STILL_PRESENT rmdir-exit=$rmrc" }
  $code = 0
} catch {
  Write-Output ('ERROR: ' + $_.Exception.Message)
  $code = 1
}
Write-Output "WSU $Run purge EXIT=$code"
exit $code
