$ErrorActionPreference = 'Stop'
& cargo 'test' '--manifest-path' 'src-tauri/Cargo.toml' '--test' 'daemon_handover_transfer_contract' '--' '--list'
$native=$LASTEXITCODE
Write-Output "NATIVE_EXIT=$native"
exit $native
