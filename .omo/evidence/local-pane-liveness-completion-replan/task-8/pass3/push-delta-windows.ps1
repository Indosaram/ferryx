$ErrorActionPreference='Stop'
$root='C:\Users\sook\ferryx-pane-completion\source-21dea3c0'
Set-Location $root
tar.exe -xzf C:\Users\sook\pass3-delta.tgz
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$files = @(
 'src-tauri/src/native_terminal/surface_host.rs',
 'src-tauri/src/daemon/client.rs',
 'src-tauri/src/daemon/handover.rs',
 'src-tauri/tests/daemon_handover_contract.rs',
 'src-tauri/tests/daemon_persistence_contract.rs',
 'src-tauri/tests/zero_config_gen4_audit.rs')
foreach ($f in $files) { Write-Output ((Get-FileHash $f -Algorithm SHA256).Hash + '  ' + $f) }
Write-Output 'DELTA_PUSHED_OK host=windows'
