$ErrorActionPreference = 'Stop'
& cargo 'test' '--manifest-path' 'src-tauri/Cargo.toml' '--test' 'daemon_handover_transfer_contract' '--' '--nocapture' '--test-threads=1'
$native=$LASTEXITCODE
Write-Output "NATIVE_EXIT=$native"
exit $native
