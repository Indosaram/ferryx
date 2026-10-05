param(
  [Parameter(Mandatory=$true)][string]$Route,
  [int]$TimeoutSec = 180
)
$ErrorActionPreference = 'Continue'
$base = 'C:\Users\sook\ferryx-pane-completion'
$out  = Join-Path $base ('exp-result-' + $Route + '.json')
$log  = Join-Path $base ('exp-log-' + $Route + '.txt')
Remove-Item -Force $out -ErrorAction SilentlyContinue

# The measurement must run INSIDE the interactive console session (session 1):
# SSH lands in session 0 where no window can be visible. Same mechanism the
# runner itself uses and that this verifier proved in pass 4.
$taskName = 'ferryx-exp-' + $Route
$inner = Join-Path $base ('exp-inner-' + $Route + '.ps1')
Copy-Item (Join-Path $base 'exp-session1.ps1') $inner -Force
$bat = Join-Path $base ('exp-run-' + $Route + '.bat')
$body = @(
  '@echo off'
  ('powershell -NoProfile -ExecutionPolicy Bypass -File "' + $inner + '" -Route ' + $Route + ' -Out "' + $out + '" > "' + $log + '" 2>&1')
) -join "`r`n"
Set-Content -Path $bat -Value $body -Encoding ascii

& schtasks /delete /tn $taskName /f 2>&1 | Out-Null
& schtasks /create /tn $taskName /tr $bat /sc once /st 00:00 /f /it 2>&1 | Out-String | Write-Output
& schtasks /run /tn $taskName 2>&1 | Out-String | Write-Output

$deadline = (Get-Date).AddSeconds($TimeoutSec)
while ((Get-Date) -lt $deadline) {
  if (Test-Path $out) { break }
  Start-Sleep -Milliseconds 500
}
Start-Sleep -Seconds 2
& schtasks /delete /tn $taskName /f 2>&1 | Out-Null
Write-Output ("OUT_EXISTS=" + (Test-Path $out))
if (Test-Path $out) { Get-Content $out -Raw }
else { Write-Output "--- inner log ---"; Get-Content $log -Raw -ErrorAction SilentlyContinue }
