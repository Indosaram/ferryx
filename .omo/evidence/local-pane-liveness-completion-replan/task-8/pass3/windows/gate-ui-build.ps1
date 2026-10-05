$ErrorActionPreference = 'Stop'
& 'bun' 'run' '--cwd' 'ui' 'build'
$native=$LASTEXITCODE
Write-Output "NATIVE_EXIT=$native"
exit $native
