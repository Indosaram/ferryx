$ErrorActionPreference = 'Stop'
& cargo 'test' '--manifest-path' 'src-tauri/Cargo.toml' '--lib' 'pane_liveness_' '--' '--nocapture' '--test-threads=1'
$native=$LASTEXITCODE
Write-Output "NATIVE_EXIT=$native"
exit $native
