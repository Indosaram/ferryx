$ErrorActionPreference = 'Continue'
$base = 'C:\Users\sook\ferryx-pane-completion'
$root = Join-Path $base 'source-21dea3c0'
$logdir = Join-Path $base 'task9-1df40271\logs'
Set-Location $root
$log = Join-Path $logdir '04-default-build-reclaimed.log'
"DEFAULT_REBUILD_START $((Get-Date).ToString('o'))" | Out-File -FilePath $log -Encoding utf8
"FREE_GB_BEFORE=$([math]::Round((Get-PSDrive C).Free/1GB,2)) LOAD=$((Get-CimInstance Win32_Processor | Select-Object -ExpandProperty LoadPercentage))" | Out-File -FilePath $log -Append
& cargo build --manifest-path src-tauri/Cargo.toml *>&1 | Tee-Object -FilePath $log -Append
$c = $LASTEXITCODE
"DEFAULT_REBUILD_EXIT=$c at $((Get-Date).ToString('o'))" | Out-File -FilePath $log -Append -Encoding utf8
Write-Output "DEFAULT_REBUILD_EXIT=$c"
$bin = 'src-tauri\target\debug\ferryx.exe'
if (Test-Path $bin) {
  Write-Output ("BIN_BYTES=" + (Get-Item $bin).Length)
  Write-Output ("BIN_SHA=" + (Get-FileHash -Algorithm SHA256 $bin).Hash.ToLower())
  # default-binary QA marker scan (the negative control)
  $bytes = [System.IO.File]::ReadAllBytes((Resolve-Path $bin))
  $ascii = [System.Text.Encoding]::ASCII.GetString($bytes)
  foreach ($m in @('FERRYX_QA_BARRIER_DIR','FERRYX_QA_OPERATION_ID','FERRYX_QA_FIXTURE_KINDS','FERRYX_QA_FIXTURE_KIND_UNSUPPORTED','FERRYX_QA_FIXTURE_CREATE_FAILED','FERRYX_QA_FIXTURE_CLAIM_REFUSED','FERRYX_QA_FIXTURE_SETUP_UNSETTLED','collect_gui_fixture_sessions','start_gui_boot_channel','qa_producers','qa_liveness','fixture-setup','split-create','attach-handshake','cancel-ack','held-rpc','marker-output')) {
    if ($ascii.Contains($m)) { Write-Output "DEFAULT_MARKER_PRESENT $m" } else { Write-Output "DEFAULT_MARKER_ABSENT $m" }
  }
  foreach ($m in @('Attach requires the persisted seven-field pane binding','Attach binding incarnation cannot be proven')) {
    if ($ascii.Contains($m)) { Write-Output "DEFAULT_LINEAGE_PRESENT $m" } else { Write-Output "DEFAULT_LINEAGE_ABSENT $m" }
  }
} else { Write-Output "BIN_MISSING" }
Write-Output ("FREE_GB_AFTER=" + [math]::Round((Get-PSDrive C).Free/1GB,2))
Write-Output "WIN_DEFAULT3_DONE"
