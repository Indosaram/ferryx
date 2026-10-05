$ErrorActionPreference = 'Continue'
& cargo 'test' '--manifest-path' 'src-tauri/Cargo.toml' '--lib' 'local_split_reliability_' '--' '--nocapture' '--test-threads=1'
$native = $LASTEXITCODE
Write-Output "NATIVE_EXIT=$native"
exit $native
