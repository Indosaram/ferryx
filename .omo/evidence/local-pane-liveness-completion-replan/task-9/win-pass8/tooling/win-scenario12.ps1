param([Parameter(Mandatory=$true)][string]$Scenario,[int]$TimeoutSec=480)
$ErrorActionPreference='Continue'
$base='C:\Users\sook\ferryx-pane-completion'
$root=Join-Path $base 'source-21dea3c0'
$t9=Join-Path $base 'task9-48b4ed93'
$bin=Join-Path $root 'src-tauri\target\debug\ferryx.exe'
$ev=Join-Path $t9 ("evidence\"+$Scenario)
$iso=Join-Path $t9 ("runtime\"+$Scenario)
$logDir=Join-Path $t9 'logs'
New-Item -ItemType Directory -Force -Path $logDir | Out-Null
$runLog=Join-Path $logDir ("scenario12-"+$Scenario+".log")
if (Test-Path $iso) { Remove-Item -Recurse -Force $iso }
New-Item -ItemType Directory -Force -Path (Split-Path $iso -Parent) | Out-Null
New-Item -ItemType Directory -Force -Path $ev | Out-Null
"SCENARIO=$Scenario STARTED=$((Get-Date).ToString('o'))" | Out-File $runLog
"MY_SSH_SESSION=$((Get-Process -Id $PID).SessionId)" | Out-File $runLog -Append
"BIN_SHA=$((Get-FileHash -Algorithm SHA256 $bin).Hash.ToLower())" | Out-File $runLog -Append
"HOST_LOAD=$((Get-CimInstance Win32_Processor | Select-Object -ExpandProperty LoadPercentage)) FREE_GB=$([math]::Round((Get-PSDrive C).Free/1GB,2))" | Out-File $runLog -Append
$c = Get-NetTCPConnection -LocalPort 5173 -State Listen -ErrorAction SilentlyContinue
"PORT_5173_BEFORE=$(if ($c) { 'OCCUPIED' } else { 'FREE' })" | Out-File $runLog -Append
$p=Start-Process cmd -ArgumentList @('/d','/c',(Join-Path $base 'run-scenario8.bat'),$Scenario) -PassThru -WindowStyle Hidden -WorkingDirectory $base
"WRAPPER_PID=$($p.Id)" | Out-File $runLog -Append
$started=Get-Date
$exitF=Join-Path $logDir ("bat-"+$Scenario+".exit")
$deadline=(Get-Date).AddSeconds($TimeoutSec)
$why='TIMEOUT'
while ((Get-Date) -lt $deadline) {
  if (Test-Path $exitF) { $why='EXIT_FILE'; break }
  if ($p.HasExited) { $why='WRAPPER_EXITED'; break }
  Start-Sleep -Milliseconds 500
}
"NODE_HANG_OBSERVED=$(if ($why -eq 'TIMEOUT') { 'YES_STILL_HUNG' } else { 'NO_TERMINATED' }) WAIT_REASON=$why" | Out-File $runLog -Append
"ELAPSED_S=$([math]::Round(((Get-Date)-$started).TotalSeconds,1))" | Out-File $runLog -Append
if ($why -eq 'TIMEOUT') { taskkill /T /F /PID $p.Id 2>&1 | Out-String | Out-File $runLog -Append }
"RAW_EXIT_CONTENT=[$((Get-Content $exitF -ErrorAction SilentlyContinue) -join '')]" | Out-File $runLog -Append
"RAW_EXIT_BYTES=$((Get-Item $exitF -ErrorAction SilentlyContinue).Length)" | Out-File $runLog -Append
$rj=Get-ChildItem -Recurse -File $ev -Filter result.json -ErrorAction SilentlyContinue | Sort-Object LastWriteTime | Select-Object -Last 1
if ($rj) { $j = Get-Content $rj.FullName -Raw | ConvertFrom-Json
  "VERDICT=$($j.verdict) CODE=$($j.error.code)" | Out-File $runLog -Append
  "GATE_OK=$($j.cleanupGate.ok) DIRS_REMOVED=$($j.cleanupGate.directoriesRemoved) PROCS_REAPED=$($j.cleanupGate.processesReaped)" | Out-File $runLog -Append
  "HOLDERS=$($j.cleanupGate.holders | ConvertTo-Json -Compress)" | Out-File $runLog -Append
  "ERROR_MESSAGE=$($j.error.message)" | Out-File $runLog -Append
}
"=== ACTION NAMES IN ORDER ===" | Out-File $runLog -Append
$a=Get-ChildItem -Recurse -File $ev -Filter actions.jsonl -ErrorAction SilentlyContinue | Sort-Object LastWriteTime | Select-Object -Last 1
if ($a) { Get-Content $a.FullName | ForEach-Object { ($_ | ConvertFrom-Json).action } | Out-File $runLog -Append } else { "NO_ACTIONS" | Out-File $runLog -Append }
"=== CLICK ACTIONS (verbatim) ===" | Out-File $runLog -Append
if ($a) { Get-Content $a.FullName | Where-Object { $_ -match 'click-split-affordance|click-pane-affordance|pane-session-bound|frontend' } | Out-File $runLog -Append }
"PORT_5173_AFTER=$($(if (Get-NetTCPConnection -LocalPort 5173 -State Listen -ErrorAction SilentlyContinue) { 'OCCUPIED' } else { 'FREE' }))" | Out-File $runLog -Append
"ISO_EXISTS_AFTER=$((Test-Path $iso))" | Out-File $runLog -Append
"SCENARIO12_DONE $Scenario" | Out-File $runLog -Append
