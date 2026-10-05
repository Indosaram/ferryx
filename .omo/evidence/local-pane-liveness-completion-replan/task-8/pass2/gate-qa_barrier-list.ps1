$ErrorActionPreference = 'Stop'
& cargo 'test' '--manifest-path' 'src-tauri/Cargo.toml' '--lib' '--features' 'local-split-qa' 'qa_barrier' '--' '--list'
$native=$LASTEXITCODE
Write-Output "NATIVE_EXIT=$native"
exit $native
