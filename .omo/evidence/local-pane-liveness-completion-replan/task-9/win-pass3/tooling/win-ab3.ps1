$ErrorActionPreference = 'Continue'
$base = 'C:\Users\sook\ferryx-pane-completion'
$root = Join-Path $base 'source-21dea3c0'
$logdir = Join-Path $base 'task9-1df40271\logs'
New-Item -ItemType Directory -Force -Path $logdir | Out-Null
Set-Location $root
Write-Output ("DISK_FREE_GB=" + [math]::Round((Get-PSDrive C).Free/1GB,2))
Write-Output ("LOAD=" + (Get-CimInstance Win32_Processor | Select-Object -ExpandProperty LoadPercentage))

# Experiment 1: the EXACT command that passed on linux at 1.98 (A/B across hosts).
$l1 = Join-Path $logdir '02-qa-check-ab.log'
"AB_QA_CHECK_START $((Get-Date).ToString('o'))" | Out-File -FilePath $l1 -Encoding utf8
& cargo check --manifest-path src-tauri/Cargo.toml --all-targets --features local-split-qa *>&1 | Tee-Object -FilePath $l1 -Append
$c1 = $LASTEXITCODE
"AB_QA_CHECK_EXIT=$c1 at $((Get-Date).ToString('o'))" | Out-File -FilePath $l1 -Append -Encoding utf8
Write-Output "AB_QA_CHECK_EXIT=$c1"

# Experiment 2: does the SHIPPED (default) configuration still build here?
$l2 = Join-Path $logdir '03-default-build.log'
"DEFAULT_BUILD_START $((Get-Date).ToString('o'))" | Out-File -FilePath $l2 -Encoding utf8
& cargo build --manifest-path src-tauri/Cargo.toml *>&1 | Tee-Object -FilePath $l2 -Append
$c2 = $LASTEXITCODE
"DEFAULT_BUILD_EXIT=$c2 at $((Get-Date).ToString('o'))" | Out-File -FilePath $l2 -Append -Encoding utf8
Write-Output "DEFAULT_BUILD_EXIT=$c2"
Write-Output ("DISK_FREE_GB_AFTER=" + [math]::Round((Get-PSDrive C).Free/1GB,2))
Write-Output "WIN_AB_DONE"
