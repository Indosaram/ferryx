param([Parameter(Mandatory=$true)][string]$Scenario,[int]$TimeoutSec=180)
$ErrorActionPreference='Continue'
$base='C:\Users\sook\ferryx-pane-completion'
$root=Join-Path $base 'source-21dea3c0'
$t9=Join-Path $base 'task9-314251e0'
$bin=Join-Path $root 'src-tauri\target\debug\ferryx.exe'
$ev=Join-Path $t9 ("evidence\"+$Scenario)
$iso=Join-Path $t9 ("runtime\"+$Scenario)
$logDir=Join-Path $t9 'logs'
New-Item -ItemType Directory -Force -Path $logDir | Out-Null
$runLog=Join-Path $logDir ("scenario6-"+$Scenario+".log")
$out=Join-Path $logDir ("runner6-"+$Scenario+".out")
$err=Join-Path $logDir ("runner6-"+$Scenario+".err")
Remove-Item -Force $out,$err -ErrorAction SilentlyContinue
if (Test-Path $iso) { Remove-Item -Recurse -Force $iso }
New-Item -ItemType Directory -Force -Path (Split-Path $iso -Parent) | Out-Null
New-Item -ItemType Directory -Force -Path $ev | Out-Null
"SCENARIO=$Scenario STARTED=$((Get-Date).ToString('o'))" | Out-File $runLog
"BIN_SHA=$((Get-FileHash -Algorithm SHA256 $bin).Hash.ToLower()) LOAD=$((Get-CimInstance Win32_Processor|Select-Object -ExpandProperty LoadPercentage)) FREE_GB=$([math]::Round((Get-PSDrive C).Free/1GB,2))" | Out-File $runLog -Append
"ISO_EXISTS_BEFORE_LAUNCH=$(Test-Path $iso)" | Out-File $runLog -Append
$reaper=Start-Process powershell -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File',(Join-Path $base 'win-reaper.ps1'),'-EvidenceDir',$ev,'-Seconds','180') -RedirectStandardOutput (Join-Path $logDir ("reaper-"+$Scenario+".log")) -PassThru -WindowStyle Hidden
$rec=Start-Process powershell -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File',(Join-Path $base 'win-recognizer.ps1'),'-BarrierDir',(Join-Path $iso 'barriers'),'-TimeoutSec','150') -RedirectStandardOutput (Join-Path $logDir ("recognizer6-"+$Scenario+".log")) -PassThru -WindowStyle Hidden
Set-Location $root
$proc=Start-Process node -ArgumentList @('scripts/qa/pane-liveness.mjs','--scenario',$Scenario,'--binary',$bin,'--evidence-dir',$ev,'--isolation-root',$iso) -RedirectStandardOutput $out -RedirectStandardError $err -PassThru -WindowStyle Hidden -WorkingDirectory $root
"NODE_PID=$($proc.Id)" | Out-File $runLog -Append
$exited=$proc.WaitForExit($TimeoutSec*1000)
if ($exited) { $raw="$($proc.ExitCode)" } else { $raw='STILL_RUNNING' }
"NODE_EXITED=$exited RAW_EXIT=$raw" | Out-File $runLog -Append
if (-not $exited) {
  $mine=Get-CimInstance Win32_Process -Filter "Name='ferryx.exe'" | Where-Object { $_.ExecutablePath -like '*source-21dea3c0*' }
  foreach ($p in $mine) { taskkill /T /F /PID $p.ProcessId 2>&1 | Out-String | Out-File $runLog -Append }
  Start-Sleep -Seconds 3
  if ($proc.HasExited) { $proc.WaitForExit(); "RAW_EXIT_AFTER_LATE_REAP=$($proc.ExitCode)" | Out-File $runLog -Append } else { taskkill /T /F /PID $proc.Id 2>&1 | Out-String | Out-File $runLog -Append; "KILLED_OWN_NODE" | Out-File $runLog -Append }
}
foreach ($side in @($reaper,$rec)) { if (-not $side.HasExited) { Stop-Process -Id $side.Id -Force -ErrorAction SilentlyContinue } }
"=== RESULT.JSON ===" | Out-File $runLog -Append
$rj = Get-ChildItem -Recurse -File $ev -Filter result.json -ErrorAction SilentlyContinue | Sort-Object LastWriteTime | Select-Object -Last 1
if ($rj) { Get-Content $rj.FullName | Out-File $runLog -Append } else { "NO_RESULT_JSON" | Out-File $runLog -Append }
"=== RUNNER STDOUT ===" | Out-File $runLog -Append
Get-Content $out -ErrorAction SilentlyContinue | Out-File $runLog -Append
"=== REAPER LOG ===" | Out-File $runLog -Append
Get-Content (Join-Path $logDir ("reaper-"+$Scenario+".log")) -ErrorAction SilentlyContinue | Out-File $runLog -Append
"=== RECOGNIZER LOG ===" | Out-File $runLog -Append
Get-Content (Join-Path $logDir ("recognizer6-"+$Scenario+".log")) -ErrorAction SilentlyContinue | Out-File $runLog -Append
"SCENARIO6_DONE $Scenario RAW_EXIT=$raw" | Out-File $runLog -Append
