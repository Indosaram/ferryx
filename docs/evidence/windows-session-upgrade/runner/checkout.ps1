# Checkout: parent from bundle + delta, byte gate (file sha256, blob, tree), then ghostty junction.
param([Parameter(Mandatory = $true)][string]$Run)
. (Join-Path $PSScriptRoot 'common.ps1')

$Step = 'checkout'
$root = Get-WsuRoot $Run
New-Item -ItemType Directory -Force -Path (Join-Path $root 'logs'), (Join-Path $root 'evidence') | Out-Null
Exit-IfStepRan $root $Run $Step
$log = Join-Path $root 'logs\checkout.log'
$code = 1
try {
  Assert-StepOk $root 'preflight'
  $null = Set-WsuEnv $root
  $in = Join-Path $root 'in'
  $src = Join-Path $root 'src'
  $meta = [IO.File]::ReadAllText((Join-Path $in 'run.json')) | ConvertFrom-Json
  if ($meta.run -ne $Run) { throw "RUN_JSON_MISMATCH: $($meta.run)" }
  foreach ($pair in @(@('parent.bundle', $meta.bundleSha256), @('delta.tar', $meta.deltaTarSha256), @('manifest.tsv', $meta.deltaManifestSha256))) {
    $h = Get-Sha256 (Join-Path $in $pair[0])
    Add-Line $log "sha256 $h $($pair[0])"
    if ($h -ne $pair[1]) { throw "INPUT_HASH_MISMATCH: $($pair[0])" }
  }
  if (Test-Path -LiteralPath $src) { throw 'SRC_EXISTS' }

  Invoke-Native $log 'git' @('init', '-q', $src) | Out-Null
  Invoke-Native $log 'git' @('-C', $src, 'config', 'core.autocrlf', 'false') | Out-Null
  Invoke-Native $log 'git' @('-C', $src, 'config', 'core.symlinks', 'false') | Out-Null
  Invoke-Native $log 'git' @('-C', $src, 'fetch', '-q', (Join-Path $in 'parent.bundle'), 'HEAD') | Out-Null
  Invoke-Native $log 'git' @('-C', $src, 'checkout', '-q', '--detach', $meta.parentSha) | Out-Null
  $head = (Invoke-Native $log 'git' @('-C', $src, 'rev-parse', 'HEAD'))[0].Trim()
  if ($head -ne $meta.parentSha) { throw "PARENT_MISMATCH: $head" }

  Invoke-Native $log (Join-Path $env:SystemRoot 'System32\tar.exe') @('-xf', (Join-Path $in 'delta.tar'), '-C', $src) | Out-Null
  $rows = @([IO.File]::ReadAllLines((Join-Path $in 'manifest.tsv')) | Where-Object { $_ } | ForEach-Object { , ($_.Split("`t")) })
  foreach ($r in $rows) {
    $status, $mode, $blob, $sha, $bytes, $rel = $r
    $path = Join-Path $src ($rel -replace '/', '\')
    if ($status -eq 'D') {
      if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Force }
      Add-Line $log "deleted $rel"
      continue
    }
    $h = Get-Sha256 $path
    if ($h -ne $sha) { throw "FILE_HASH_MISMATCH: $rel" }
    $b = (Invoke-Native $log 'git' @('-C', $src, 'hash-object', '--', $rel))[0].Trim()
    if ($b -ne $blob) { throw "BLOB_MISMATCH: $rel" }
  }
  Invoke-Native $log 'git' @('-C', $src, 'add', '-A') | Out-Null
  foreach ($r in $rows) {
    if ($r[0] -eq 'D') { continue }
    $flag = '--chmod=-x'
    if ($r[1] -eq '100755') { $flag = '--chmod=+x' }
    Invoke-Native $log 'git' @('-C', $src, 'update-index', $flag, '--', $r[5]) | Out-Null
  }
  $tree = (Invoke-Native $log 'git' @('-C', $src, 'write-tree'))[0].Trim()
  Write-Text (Join-Path $root 'evidence\tested-tree.txt') "expected=$($meta.testedTreeSha)`nactual=$tree`n"
  if ($tree -ne $meta.testedTreeSha) { throw "TREE_MISMATCH: $tree" }

  # Gitlink checkout leaves an empty dir; replace it with the pinned ghostty junction.
  $vg = Join-Path $src 'src-tauri\vendor\ghostty'
  if (-not (Test-Path -LiteralPath $vg)) { throw "GHOSTTY_GITLINK_DIR_MISSING: $vg" }
  if (@(Get-ChildItem -LiteralPath $vg -Force).Count -ne 0) { throw "GHOSTTY_GITLINK_DIR_NOT_EMPTY: $vg" }
  Remove-Item -LiteralPath $vg -Force
  Invoke-Native $log (Join-Path $env:SystemRoot 'System32\cmd.exe') @('/d', '/c', 'mklink', '/J', $vg, $GhosttySrc) | Out-Null
  if (-not ((Get-Item -LiteralPath $vg -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'GHOSTTY_JUNCTION_NOT_CREATED' }
  $gh = (Invoke-Native $log 'git' @('-C', $vg, 'rev-parse', 'HEAD'))[0].Trim()
  Write-Text (Join-Path $root 'evidence\junction.txt') "junction=$vg`ntarget=$GhosttySrc`nhead=$gh`n"
  if ($gh -ne $GhosttyPin) { throw "GHOSTTY_PIN_MISMATCH: $gh" }
  $code = 0
} catch {
  Add-Line $log ('ERROR: ' + $_.Exception.Message)
  $code = 1
}
Complete-Step $root $Run $Step $code
exit $code
