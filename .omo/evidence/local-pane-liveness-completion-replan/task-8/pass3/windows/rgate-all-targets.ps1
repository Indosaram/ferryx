$ErrorActionPreference = 'Stop'
& 'cargo' 'check' '--manifest-path' 'src-tauri/Cargo.toml' '--all-targets'
$native=$LASTEXITCODE
Write-Output "NATIVE_EXIT=$native"
exit $native
