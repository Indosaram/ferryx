param([Parameter(Mandatory=$true)][string]$Scenario,[int]$TimeoutSec=420)
$ErrorActionPreference='Continue'
$base='C:\Users\sook\ferryx-pane-completion'
$root=Join-Path $base 'source-21dea3c0'
$t9=Join-Path $base 'task9-120bc965'
$bin=Join-Path $root 'src-tauri\target\debug\ferryx.exe'
$ev=Join-Path $t9 ("evidence\"+$Scenario)
$iso=Join-Path $t9 ("runtime\"+$Scenario)
$logDir=Join-Path $t9 'logs'
New-Item -ItemType Directory -Force -Path $logDir | Out-Null
$runLog=Join-Path $logDir ("scenario10-"+$Scenario+".log")
if (Test-Path $iso) { Remove-Item -Recurse -Force $iso }
New-Item -ItemType Directory -Force -Path (Split-Path $iso -Parent) | Out-Null
New-Item -ItemType Directory -Force -Path $ev | Out-Null
"SCENARIO=$Scenario STARTED=$((Get-Date).ToString('o'))" | Out-File $runLog
"MY_SSH_SESSION=$((Get-Process -Id $PID).SessionId)" | Out-File $runLog -Append
"BIN_SHA=$((Get-FileHash -Algorithm SHA256 $bin).Hash.ToLower())" | Out-File $runLog -Append
"HOST_LOAD=$((Get-CimInstance Win32_Processor | Select-Object -ExpandProperty LoadPercentage)) FREE_GB=$([math]::Round((Get-PSDrive C).Free/1GB,2))" | Out-File $runLog -Append
$p=Start-Process cmd -ArgumentList @('/d','/c',(Join-Path $base 'run-scenario6.bat'),$Scenario) -PassThru -WindowStyle Hidden -WorkingDirectory $base
"WRAPPER_PID=$($p.Id)" | Out-File $runLog -Append
$started=Get-Date
$exited=$p.WaitForExit($TimeoutSec*1000)
"WRAPPER_EXITED=$exited ELAPSED_S=$([math]::Round(((Get-Date)-$started).TotalSeconds,1))" | Out-File $runLog -Append
if (-not $exited) { taskkill /T /F /PID $p.Id 2>&1 | Out-String | Out-File $runLog -Append }
# Snapshot what the delegated run left, then read everything.
$rj=Get-ChildItem -Recurse -File $ev -Filter result.json -ErrorAction SilentlyContinue | Sort-Object LastWriteTime | Select-Object -Last 1
if ($rj) { $j = Get-Content $rj.FullName -Raw | ConvertFrom-Json
  "VERDICT=$($j.verdict) CODE=$($j.error.code) GATE=$($j.cleanupGate.ok) DIRS_REMOVED=$($j.cleanupGate.directoriesRemoved)" | Out-File $runLog -Append
  "ERROR_MESSAGE=$($j.error.message)" | Out-File $runLog -Append
}
"=== ACTIONS ===" | Out-File $runLog -Append
$a=Get-ChildItem -Recurse -File $ev -Filter actions.jsonl -ErrorAction SilentlyContinue | Sort-Object LastWriteTime | Select-Object -Last 1
if ($a) { Get-Content $a.FullName | Out-File $runLog -Append } else { "NO_ACTIONS" | Out-File $runLog -Append }
"=== FILES PRODUCED ===" | Out-File $runLog -Append
Get-ChildItem -Recurse -File $ev -ErrorAction SilentlyContinue | ForEach-Object { Write-Output ("  " + $_.FullName.Replace($ev,'') + " " + $_.Length) } | Out-File $runLog -Append
"=== ISOLATION ROOT HOLDER (if cleanup failed) ===" | Out-File $runLog -Append
Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -like ('*'+$iso+'*') } | ForEach-Object { Write-Output ("  HOLDER PID=" + $_.ProcessId + " " + $_.Name) } | Out-File $runLog -Append
"SCENARIO10_DONE $Scenario" | Out-File $runLog -Append
