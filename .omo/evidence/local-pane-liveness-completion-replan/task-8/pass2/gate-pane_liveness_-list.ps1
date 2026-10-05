$ErrorActionPreference = 'Stop'
& cargo 'test' '--manifest-path' 'src-tauri/Cargo.toml' '--lib' 'pane_liveness_' '--' '--list'
$native=$LASTEXITCODE
Write-Output "NATIVE_EXIT=$native"
exit $native
