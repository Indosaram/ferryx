$ErrorActionPreference = 'Continue'
& cargo 'test' '--manifest-path' 'src-tauri/Cargo.toml' '--lib' '--' '--test-threads=1'
$native = $LASTEXITCODE
Write-Output "NATIVE_EXIT=$native"
exit $native
