$ErrorActionPreference = 'Stop'
& 'cargo' 'test' '--manifest-path' 'src-tauri/Cargo.toml' '--lib' 'local_split_reliability_' '--' '--list'
$native=$LASTEXITCODE
Write-Output "NATIVE_EXIT=$native"
exit $native
