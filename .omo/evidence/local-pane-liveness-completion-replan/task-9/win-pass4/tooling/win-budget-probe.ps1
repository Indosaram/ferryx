param([string]$Scenario = 'split-happy', [string]$Kinds = 'source', [int]$TimeoutSec = 60)
$ErrorActionPreference = 'Continue'
$base = 'C:\Users\sook\ferryx-pane-completion'
$root = Join-Path $base 'source-21dea3c0'
$bin = Join-Path $root 'src-tauri\target\debug\ferryx.exe'
$t9 = Join-Path $base 'task9-91d447e1'
$probe = Join-Path $t9 ('budget-probe\' + $Scenario)
if (Test-Path $probe) { Remove-Item -Recurse -Force $probe }
$bd = Join-Path $probe 'barriers'
$data = Join-Path $probe 'data'
$rt = Join-Path $probe 'runtime'
$home = Join-Path $probe 'home'
foreach ($d in @($bd,$data,$rt,$home)) { New-Item -ItemType Directory -Force -Path $d | Out-Null }
$runId = 'qa-run-budgetprobe-' + [guid]::NewGuid().ToString().Substring(0,8)
$opId = 'qa-op-budgetprobe-' + [guid]::NewGuid().ToString().Substring(0,8)

# The runner's own isolated env (buildIsolatedEnv) + the fixture-kind declaration.
$env:FERRYX_DATA_DIR = $data
$env:FERRYX_RUNTIME_DIR = $rt
$env:FERRYX_QA_BARRIER_DIR = $bd
$env:FERRYX_QA_RUN_ID = $runId
$env:FERRYX_QA_OPERATION_ID = $opId
$env:FERRYX_QA_FIXTURE_KINDS = $Kinds

Write-Output "PROBE scenario=$Scenario kinds=$Kinds runId=$runId opId=$opId"
Write-Output "BIN_SHA=$((Get-FileHash -Algorithm SHA256 $bin).Hash.ToLower()) BYTES=$((Get-Item $bin).Length)"
Write-Output ("HOST_LOAD=" + (Get-CimInstance Win32_Processor | Select-Object -ExpandProperty LoadPercentage) + " FREE_GB=" + [math]::Round((Get-PSDrive C).Free/1GB,2))

$receipt = Join-Path $bd 'fixture-setup.receipt.jsonl'
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$p = Start-Process -FilePath $bin -PassThru -WorkingDirectory $root
Write-Output "PROBE_PID=$($p.Id)"
$deadline = (Get-Date).AddSeconds($TimeoutSec)
$firstSeen = $null
while ((Get-Date) -lt $deadline) {
  if (Test-Path $receipt) { $firstSeen = $sw.ElapsedMilliseconds; break }
  Start-Sleep -Milliseconds 25
}
Write-Output ("RECEIPT_FIRST_SEEN_MS=" + $(if ($null -eq $firstSeen) { 'NEVER_WITHIN_' + ($TimeoutSec*1000) } else { $firstSeen }))
if (Test-Path $receipt) {
  Write-Output "=== RECEIPT LINE 0 (verbatim) ==="
  Get-Content $receipt | Select-Object -First 1
  $j = (Get-Content $receipt | Select-Object -First 1) | ConvertFrom-Json
  Write-Output ("fixtureCreationElapsedMs=" + $j.fixtureCreationElapsedMs)
  Write-Output ("fixtureKindsRequested=" + ($j.fixtureKindsRequested -join ','))
  Write-Output ("fixtureKindsClaimed=" + ($j.fixtureKindsClaimed -join ','))
  Write-Output ("fixtureKindsUnsupported=" + ($j.fixtureKindsUnsupported | ConvertTo-Json -Compress))
  Write-Output ("fixtureCreationFailures=" + ($j.fixtureCreationFailures -join ' | '))
  Write-Output ("fixtureClaimsRefused=" + ($j.fixtureClaimsRefused -join ' | '))
  Write-Output ("sessionsNotRunning=" + ($j.sessionsNotRunning -join ','))
  Write-Output ("SESSION_COUNT=" + @($j.sessions).Count)
  foreach ($s in @($j.sessions)) { Write-Output ("  SESSION kind=" + $s.kind + " backendSessionId=" + $s.backendSessionId + " running=" + $s.ownershipReceipt.running) }
  $totalMs = $sw.ElapsedMilliseconds
  Write-Output ("TOTAL_MS_TO_RECEIPT_READ=" + $totalMs)
  Write-Output ("INSIDE_9000MS_AWAIT=" + ($totalMs -lt 9000))
}
# Reap only this probe's own processes.
$mine = Get-CimInstance Win32_Process -Filter "Name='ferryx.exe'" | Where-Object { $_.ExecutablePath -like '*source-21dea3c0*' }
foreach ($q in $mine) { taskkill /T /F /PID $q.ProcessId 2>&1 | Out-String | Write-Output }
Write-Output "BUDGET_PROBE_DONE"
