$ErrorActionPreference = 'Continue'
$base = 'C:\Users\sook\ferryx-pane-completion'
$root = Join-Path $base 'source-21dea3c0'
$logdir = Join-Path $base 'task9-91d447e1\logs'
New-Item -ItemType Directory -Force -Path $logdir | Out-Null
Set-Location $root
Write-Output ("FREE_BEFORE_GB=" + [math]::Round((Get-PSDrive C).Free/1GB,2))
# Reclaim ONLY this task's own build cache: the QA build must link a fresh lib+bin
# and the host is down to a few GB from foreign sessions' trees.
$t = 'src-tauri\target'
if (Test-Path $t) { Remove-Item -Recurse -Force $t -ErrorAction SilentlyContinue; Write-Output "RECLAIMED_OWN_TARGET" }
Start-Sleep -Seconds 2
Write-Output ("FREE_AFTER_GB=" + [math]::Round((Get-PSDrive C).Free/1GB,2))

$log = Join-Path $logdir '01-qa-build.log'
"QA_BUILD_START $((Get-Date).ToString('o'))" | Out-File -FilePath $log -Encoding utf8
"LOAD=$((Get-CimInstance Win32_Processor | Select-Object -ExpandProperty LoadPercentage))" | Out-File -FilePath $log -Append
& cargo build --manifest-path src-tauri/Cargo.toml --features local-split-qa *>&1 | Tee-Object -FilePath $log -Append
$c = $LASTEXITCODE
"QA_BUILD_EXIT=$c at $((Get-Date).ToString('o'))" | Out-File -FilePath $log -Append -Encoding utf8
Write-Output "QA_BUILD_EXIT=$c"
$bin = 'src-tauri\target\debug\ferryx.exe'
if (Test-Path $bin) {
  $fi = Get-Item $bin
  Write-Output ("BIN_PATH=" + $fi.FullName)
  Write-Output ("BIN_BYTES=" + $fi.Length)
  Write-Output ("BIN_SHA=" + (Get-FileHash -Algorithm SHA256 $bin).Hash.ToLower())
} else { Write-Output "BIN_MISSING" }
Write-Output ("FREE_AFTER_BUILD_GB=" + [math]::Round((Get-PSDrive C).Free/1GB,2))
Write-Output "WIN_QA_BUILD4_DONE"
