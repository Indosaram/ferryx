param([Parameter(Mandatory=$true)][string]$Scenario,[int]$TimeoutSec=180)
$ErrorActionPreference='Continue'
$base='C:\Users\sook\ferryx-pane-completion'
$t9=Join-Path $base 'task9-314251e0'
$iso=Join-Path $t9 ("runtime\"+$Scenario)
$ev=Join-Path $t9 ("evidence\"+$Scenario)
$logDir=Join-Path $t9 'logs'
New-Item -ItemType Directory -Force -Path $logDir | Out-Null
$runLog=Join-Path $logDir ("bat-log-"+$Scenario+".txt")
if (Test-Path $iso) { Remove-Item -Recurse -Force $iso }
New-Item -ItemType Directory -Force -Path (Split-Path $iso -Parent) | Out-Null
New-Item -ItemType Directory -Force -Path $ev | Out-Null
"BAT_START=$((Get-Date).ToString('o')) ISO_EXISTS=$(Test-Path $iso)" | Out-File $runLog
$reaper=Start-Process powershell -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File',(Join-Path $base 'win-reaper.ps1'),'-EvidenceDir',$ev,'-Seconds','180') -RedirectStandardOutput (Join-Path $logDir ("bat-reaper-"+$Scenario+".log")) -PassThru -WindowStyle Hidden
$p=Start-Process cmd -ArgumentList @('/d','/c',(Join-Path $base 'run-scenario.bat'),$Scenario) -PassThru -WindowStyle Hidden -WorkingDirectory $base
"BAT_WRAPPER_PID=$($p.Id)" | Out-File $runLog -Append
$exited=$p.WaitForExit($TimeoutSec*1000)
"WRAPPER_EXITED=$exited" | Out-File $runLog -Append
if (-not $exited) { taskkill /T /F /PID $p.Id 2>&1 | Out-String | Out-File $runLog -Append }
if (-not $reaper.HasExited) { Stop-Process -Id $reaper.Id -Force -ErrorAction SilentlyContinue }
$exitFile=Join-Path $logDir ("bat-"+$Scenario+".exit")
"BAT_EXIT_FILE=[$((Get-Content $exitFile -ErrorAction SilentlyContinue) -join '')]" | Out-File $runLog -Append
"=== RESULT VERDICT ===" | Out-File $runLog -Append
$rj=Get-ChildItem -Recurse -File $ev -Filter result.json -ErrorAction SilentlyContinue | Sort-Object LastWriteTime | Select-Object -Last 1
if ($rj) { Get-Content $rj.FullName | Out-File $runLog -Append } else { "NO_RESULT_JSON" | Out-File $runLog -Append }
"BAT_LOG_DONE" | Out-File $runLog -Append
