param([Parameter(Mandatory=$true)][string]$Scenario,[int]$TimeoutSec=240)
$ErrorActionPreference='Continue'
$base='C:\Users\sook\ferryx-pane-completion'
$root=Join-Path $base 'source-21dea3c0'
$t9=Join-Path $base 'task9-91d447e1'
$logDir=Join-Path $t9 'logs'
$ev=Join-Path $t9 ("evidence\"+$Scenario)
$iso=Join-Path $t9 ("runtime\"+$Scenario)
$keep=Join-Path $t9 ("preserved\"+$Scenario)
$runLog=Join-Path $logDir ("s1-"+$Scenario+".log")
$out=Join-Path $logDir ("s1-"+$Scenario+".out")
$err=Join-Path $logDir ("s1-"+$Scenario+".err")
$exitF=Join-Path $logDir ("s1-"+$Scenario+".exit")
Remove-Item -Force $out,$err,$exitF -ErrorAction SilentlyContinue
if (Test-Path $iso) { Remove-Item -Recurse -Force $iso }
New-Item -ItemType Directory -Force -Path (Split-Path $iso -Parent) | Out-Null
New-Item -ItemType Directory -Force -Path $ev | Out-Null
if (Test-Path $keep) { Remove-Item -Recurse -Force $keep }
New-Item -ItemType Directory -Force -Path $keep | Out-Null

"SCENARIO=$Scenario STARTED=$((Get-Date).ToString('o'))" | Out-File $runLog
"MY_SESSION=$((Get-Process -Id $PID).SessionId)" | Out-File $runLog -Append
"BIN_SHA=$((Get-FileHash -Algorithm SHA256 (Join-Path $root 'src-tauri\target\debug\ferryx.exe')).Hash.ToLower())" | Out-File $runLog -Append
"HOST_LOAD=$((Get-CimInstance Win32_Processor | Select-Object -ExpandProperty LoadPercentage)) FREE_GB=$([math]::Round((Get-PSDrive C).Free/1GB,2))" | Out-File $runLog -Append

# Launch the runner INTO the interactive console session (session 1) so the app can
# own a VISIBLE window; SSH lands in session 0 where no window is ever visible.
$taskName = 'ferryx-t9p4-' + $Scenario
$bat = Join-Path $base ('s1-run-' + $Scenario + '.bat')
$body = @(
  '@echo off'
  ('cd /d ' + $root)
  ('node scripts\qa\pane-liveness.mjs --scenario ' + $Scenario + ' --binary "' + (Join-Path $root 'src-tauri\target\debug\ferryx.exe') + '" --evidence-dir "' + $ev + '" --isolation-root "' + $iso + '" > "' + $out + '" 2> "' + $err + '"')
  ('echo %ERRORLEVEL% > "' + $exitF + '"')
) -join "`r`n"
Set-Content -Path $bat -Value $body -Encoding ascii
& schtasks /delete /tn $taskName /f 2>&1 | Out-Null
& schtasks /create /tn $taskName /tr $bat /sc once /st 00:00 /f /it 2>&1 | Out-String | Out-File $runLog -Append
$started = Get-Date
& schtasks /run /tn $taskName 2>&1 | Out-String | Out-File $runLog -Append
# Wait for the exit file the task writes.
$deadline = (Get-Date).AddSeconds($TimeoutSec)
while ((Get-Date) -lt $deadline) { if (Test-Path $exitF) { break }; Start-Sleep -Milliseconds 500 }
"ELAPSED_S=$([math]::Round(((Get-Date)-$started).TotalSeconds,1))" | Out-File $runLog -Append
& schtasks /delete /tn $taskName /f 2>&1 | Out-String | Out-File $runLog -Append
"RAW_EXIT=[$((Get-Content $exitF -ErrorAction SilentlyContinue) -join '').Trim()]" | Out-File $runLog -Append
"=== RESULT.JSON ===" | Out-File $runLog -Append
$rj=Get-ChildItem -Recurse -File $ev -Filter result.json -ErrorAction SilentlyContinue | Sort-Object LastWriteTime | Select-Object -Last 1
if ($rj) { Get-Content $rj.FullName | Out-File $runLog -Append } else { "NO_RESULT_JSON" | Out-File $runLog -Append }
"=== ACTIONS ===" | Out-File $runLog -Append
$a=Get-ChildItem -Recurse -File $ev -Filter actions.jsonl -ErrorAction SilentlyContinue | Sort-Object LastWriteTime | Select-Object -Last 1
if ($a) { Get-Content $a.FullName | Out-File $runLog -Append } else { "NO_ACTIONS" | Out-File $runLog -Append }
"=== STDOUT ===" | Out-File $runLog -Append
Get-Content $out -ErrorAction SilentlyContinue | Out-File $runLog -Append
"=== STDERR ===" | Out-File $runLog -Append
Get-Content $err -ErrorAction SilentlyContinue | Out-File $runLog -Append
"=== PRESERVED ===" | Out-File $runLog -Append
Get-ChildItem -Recurse -File $keep -ErrorAction SilentlyContinue | ForEach-Object { Write-Output ("P " + $_.FullName.Replace($keep,'') + " " + $_.Length) } | Out-File $runLog -Append
"S1_DONE $Scenario" | Out-File $runLog -Append
Write-Output "S1_DONE $Scenario"
