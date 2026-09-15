# Event-driven build monitor for st_01a0958a QA launch (no sleep-polling).
# FileSystemWatcher -> Register-ObjectEvent (string source id) -> Wait-Event.
# Streams appended log lines. Exits on: failure markers, cargo Finished (BUILD_FINISHED),
# task-process exit, or bounded cap.
param(
  [int]$TimeoutMinutes = 60
)
$ErrorActionPreference = 'Stop'
$qaRoot = 'C:\Users\sook\ferryx-qa-rt-st01a0958a'
$log = Join-Path $qaRoot 'logs\dev-stdout.log'
$deadline = (Get-Date).AddMinutes($TimeoutMinutes)

if (-not (Test-Path $log)) { Write-Host "MONITOR_ABORT log-missing $log"; exit 2 }
$offset = (Get-Item $log).Length

$fsw = New-Object System.IO.FileSystemWatcher
$fsw.Path = (Split-Path $log)
$fsw.Filter = (Split-Path $log -Leaf)
$fsw.NotifyFilter = [System.IO.NotifyFilters]::LastWrite
$fsw.EnableRaisingEvents = $true
Register-ObjectEvent -InputObject $fsw -EventName Changed -SourceIdentifier 'QaLogChanged' | Out-Null

function Test-QaAlive {
  return [bool](Get-CimInstance Win32_Process -Filter "Name='cmd.exe'" -ErrorAction SilentlyContinue |
    Where-Object { $_.CommandLine -like '*ferryx-qa-rt-st01a0958a\qa\launch*.cmd*' })
}

$decision = $null
while (-not $decision) {
  if ((Get-Date) -gt $deadline) { $decision = 'MONITOR_TIMEOUT'; break }
  if (-not (Test-QaAlive)) {
    # Drain final log state before deciding; the launcher may have just exited.
    Start-Sleep -Milliseconds 1500
    if (-not (Test-QaAlive)) { $decision = 'LAUNCHER_EXITED'; break }
  }
  $ev = Wait-Event -SourceIdentifier 'QaLogChanged' -Timeout 15
  if ($ev) { Remove-Event -EventIdentifier $ev.EventIdentifier -ErrorAction SilentlyContinue }
  $len = (Get-Item $log).Length
  if ($len -gt $offset) {
    $stream = Get-Content $log -Raw -Encoding UTF8
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($stream)
    $new = [System.Text.Encoding]::UTF8.GetString($bytes, [Math]::Min($offset, $bytes.Length), [Math]::Max(0, $bytes.Length - $offset))
    $offset = $len
    $clean = $new -replace '\x1b\[[0-9;]*m', ''
    foreach ($line in ($clean -split "`r?`n")) {
      if ($line.Trim()) { Write-Host ("L| " + $line) }
    }
    if ($clean -match 'error\[|error: could not compile|panicked at') { $decision = 'BUILD_ERROR'; break }
    if ($clean -match 'failed to create webview') { $decision = 'WEBVIEW_ERROR'; break }
    if ($clean -match '^\s*Finished|Finished\s+`dev`') { $decision = 'BUILD_FINISHED'; break }
  }
}
Unregister-Event -SourceIdentifier 'QaLogChanged' -ErrorAction SilentlyContinue
$fsw.EnableRaisingEvents = $false
$fsw.Dispose()
Write-Host "MONITOR_DECISION $decision"
if ($decision -eq 'BUILD_ERROR') {
  Get-Content $log -Tail 40 | ForEach-Object { Write-Host ("TAIL| " + ($_ -replace '\x1b\[[0-9;]*m', '')) }
}
