# wait-webview.ps1 - event-driven decision for the st_01a0958a isolated relaunch.
# Watches (Register-ObjectEvent / Wait-Event, no sleep-polling):
#   1. logs\dev-stdout.log changes -> appended bytes scanned for 'failed to create webview'
#      => decision WEBVIEW_ERROR
#   2. webview2 profile dir changes -> first activity after start => WEBVIEW_PROFILE_ACTIVE
#      (WebView2 writes profile files only after the environment is created successfully)
# Exits with the first decision, or WATCH_TIMEOUT at the cap.
param([int]$TimeoutSeconds = 120)
$ErrorActionPreference = 'Stop'
$qaRoot = 'C:\Users\sook\ferryx-qa-rt-st01a0958a'
$log = Join-Path $qaRoot 'logs\dev-stdout.log'
$profileDir = Join-Path $qaRoot 'webview2'
$deadline = (Get-Date).AddSeconds($TimeoutSeconds)
$logOffset = (Get-Item $log).Length

$fswLog = New-Object System.IO.FileSystemWatcher
$fswLog.Path = (Split-Path $log); $fswLog.Filter = (Split-Path $log -Leaf)
$fswLog.NotifyFilter = [System.IO.NotifyFilters]::LastWrite
$fswLog.EnableRaisingEvents = $true
Register-ObjectEvent -InputObject $fswLog -EventName Changed -SourceIdentifier 'WvLog' | Out-Null

$fswProf = New-Object System.IO.FileSystemWatcher
$fswProf.Path = $profileDir; $fswProf.IncludeSubdirectories = $true
$fswProf.NotifyFilter = [System.IO.NotifyFilters]::FileName -bor [System.IO.NotifyFilters]::LastWrite
$fswProf.EnableRaisingEvents = $true
Register-ObjectEvent -InputObject $fswProf -EventName Created -SourceIdentifier 'WvProfC' | Out-Null
Register-ObjectEvent -InputObject $fswProf -EventName Changed -SourceIdentifier 'WvProfW' | Out-Null

$decision = $null
while (-not $decision) {
  if ((Get-Date) -gt $deadline) { $decision = 'WATCH_TIMEOUT'; break }
  $ev = Wait-Event -Timeout 5
  if (-not $ev) { continue }
  switch ($ev.SourceIdentifier) {
    'WvLog' {
      Remove-Event -EventIdentifier $ev.EventIdentifier -ErrorAction SilentlyContinue
      $len = (Get-Item $log).Length
      if ($len -gt $logOffset) {
        $stream = Get-Content $log -Raw -Encoding UTF8
        $bytes = [System.Text.Encoding]::UTF8.GetBytes($stream)
        $new = [System.Text.Encoding]::UTF8.GetString($bytes, [Math]::Min($logOffset, $bytes.Length), [Math]::Max(0, $bytes.Length - $logOffset))
        $logOffset = $len
        if (($new -replace '\x1b\[[0-9;]*m', '') -match 'failed to create webview') { $decision = 'WEBVIEW_ERROR'; break }
      }
    }
    ('WvProfC') { $decision = 'WEBVIEW_PROFILE_ACTIVE'; break }
    ('WvProfW') { $decision = 'WEBVIEW_PROFILE_ACTIVE'; break }
  }
}
foreach ($sid in 'WvLog','WvProfC','WvProfW') {
  Unregister-Event -SourceIdentifier $sid -ErrorAction SilentlyContinue
}
$fswLog.EnableRaisingEvents = $false; $fswLog.Dispose()
$fswProf.EnableRaisingEvents = $false; $fswProf.Dispose()
Write-Host "WAIT_WEBVIEW_DECISION $decision"
