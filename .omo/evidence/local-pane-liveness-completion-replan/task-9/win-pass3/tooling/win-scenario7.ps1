param([Parameter(Mandatory=$true)][string]$Scenario,[int]$TimeoutSec=240)
$ErrorActionPreference='Continue'
$base='C:\Users\sook\ferryx-pane-completion'
$root=Join-Path $base 'source-21dea3c0'
$t9=Join-Path $base 'task9-1df40271'
$bin=Join-Path $root 'src-tauri\target\debug\ferryx.exe'
$ev=Join-Path $t9 ("evidence\"+$Scenario)
$iso=Join-Path $t9 ("runtime\"+$Scenario)
$keep=Join-Path $t9 ("preserved\"+$Scenario)
$logDir=Join-Path $t9 'logs'
New-Item -ItemType Directory -Force -Path $logDir | Out-Null
$runLog=Join-Path $logDir ("scenario7-"+$Scenario+".log")
$exitFile=Join-Path $logDir ("bat-"+$Scenario+".exit")
Remove-Item -Force $exitFile -ErrorAction SilentlyContinue
if (Test-Path $iso) { Remove-Item -Recurse -Force $iso }
New-Item -ItemType Directory -Force -Path (Split-Path $iso -Parent) | Out-Null
New-Item -ItemType Directory -Force -Path $ev | Out-Null
if (Test-Path $keep) { Remove-Item -Recurse -Force $keep }
New-Item -ItemType Directory -Force -Path $keep | Out-Null

"SCENARIO=$Scenario STARTED=$((Get-Date).ToString('o'))" | Out-File $runLog
"BIN_SHA=$((Get-FileHash -Algorithm SHA256 $bin).Hash.ToLower()) BYTES=$((Get-Item $bin).Length)" | Out-File $runLog -Append
"HOST_LOAD=$((Get-CimInstance Win32_Processor | Select-Object -ExpandProperty LoadPercentage)) FREE_GB=$([math]::Round((Get-PSDrive C).Free/1GB,2)) BOOT=$((Get-CimInstance Win32_OperatingSystem).LastBootUpTime)" | Out-File $runLog -Append
"ISO_EXISTS_BEFORE_LAUNCH=$(Test-Path $iso)" | Out-File $runLog -Append

$reaper=Start-Process powershell -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File',(Join-Path $base 'win-reaper3.ps1'),'-EvidenceDir',$ev,'-IsolationRoot',$iso,'-Seconds','240') -RedirectStandardOutput (Join-Path $logDir ("reaper3-"+$Scenario+".log")) -PassThru -WindowStyle Hidden
$snap=Start-Process powershell -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File',(Join-Path $base 'win-snap3.ps1'),'-Source',$iso,'-Dest',$keep,'-Seconds','240') -RedirectStandardOutput (Join-Path $logDir ("snap3-"+$Scenario+".log")) -PassThru -WindowStyle Hidden

$p=Start-Process cmd -ArgumentList @('/d','/c',(Join-Path $base 'run-scenario3.bat'),$Scenario) -PassThru -WindowStyle Hidden -WorkingDirectory $base
"WRAPPER_PID=$($p.Id)" | Out-File $runLog -Append
$exited=$p.WaitForExit($TimeoutSec*1000)
"WRAPPER_EXITED=$exited" | Out-File $runLog -Append
if (-not $exited) { taskkill /T /F /PID $p.Id 2>&1 | Out-String | Out-File $runLog -Append }
foreach ($side in @($reaper,$snap)) { if (-not $side.HasExited) { Stop-Process -Id $side.Id -Force -ErrorAction SilentlyContinue } }
"RAW_EXIT=[$((Get-Content $exitFile -ErrorAction SilentlyContinue) -join '').Trim()]" | Out-File $runLog -Append

"=== RESULT.JSON ===" | Out-File $runLog -Append
$rj=Get-ChildItem -Recurse -File $ev -Filter result.json -ErrorAction SilentlyContinue | Sort-Object LastWriteTime | Select-Object -Last 1
if ($rj) { Get-Content $rj.FullName | Out-File $runLog -Append } else { "NO_RESULT_JSON" | Out-File $runLog -Append }
"=== RUNNER STDOUT ===" | Out-File $runLog -Append
Get-Content (Join-Path $logDir ("bat-"+$Scenario+".out")) -ErrorAction SilentlyContinue | Out-File $runLog -Append
"=== RUNNER STDERR ===" | Out-File $runLog -Append
Get-Content (Join-Path $logDir ("bat-"+$Scenario+".err")) -ErrorAction SilentlyContinue | Out-File $runLog -Append
"=== PRESERVED BARRIER/RECEIPT FILES ===" | Out-File $runLog -Append
Get-ChildItem -Recurse -File $keep -ErrorAction SilentlyContinue | ForEach-Object { Write-Output ("P " + $_.FullName.Replace($keep,'') + " " + $_.Length) } | Out-File $runLog -Append
"=== PRESERVED JSON/JSONL CONTENT ===" | Out-File $runLog -Append
Get-ChildItem -Recurse -File $keep -ErrorAction SilentlyContinue | Where-Object { $_.Name -like '*.json' -or $_.Name -like '*.jsonl' } | ForEach-Object { Write-Output ("--- " + $_.Name); Get-Content $_.FullName } | Out-File $runLog -Append
"SCENARIO7_DONE $Scenario" | Out-File $runLog -Append
