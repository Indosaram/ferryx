$ErrorActionPreference = 'Stop'
& cargo 'test' '--manifest-path' 'src-tauri/Cargo.toml' '--lib' '--features' 'local-split-qa' 'qa_barrier' '--' '--nocapture' '--test-threads=1'
$native=$LASTEXITCODE
Write-Output "NATIVE_EXIT=$native"
exit $native
