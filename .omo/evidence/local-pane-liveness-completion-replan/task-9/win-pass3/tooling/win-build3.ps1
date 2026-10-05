$ErrorActionPreference = 'Continue'
$base = 'C:\Users\sook\ferryx-pane-completion'
$root = Join-Path $base 'source-21dea3c0'
$logdir = Join-Path $base 'task9-1df40271\logs'
New-Item -ItemType Directory -Force -Path $logdir | Out-Null
Set-Location $root
$log = Join-Path $logdir '01-qa-build.log'
"QA_BUILD_START $((Get-Date).ToString('o'))" | Out-File -FilePath $log -Encoding utf8
"LOAD=$((Get-CimInstance Win32_Processor | Select-Object -ExpandProperty LoadPercentage)) FREE_GB=$([math]::Round((Get-PSDrive C).Free/1GB,2))" | Out-File -FilePath $log -Append
& cargo build --manifest-path src-tauri/Cargo.toml --features local-split-qa *>&1 | Tee-Object -FilePath $log -Append
$code = $LASTEXITCODE
"QA_BUILD_EXIT=$code at $((Get-Date).ToString('o'))" | Out-File -FilePath $log -Append -Encoding utf8
Write-Output "QA_BUILD_EXIT=$code"
$bin = 'src-tauri\target\debug\ferryx.exe'
if (Test-Path $bin) {
  $fi = Get-Item $bin
  Write-Output ("BIN_PATH=" + $fi.FullName)
  Write-Output ("BIN_BYTES=" + $fi.Length)
  Write-Output ("BIN_SHA=" + (Get-FileHash -Algorithm SHA256 $bin).Hash.ToLower())
} else { Write-Output "BIN_MISSING" }
Write-Output "QA_BUILD_DONE"
