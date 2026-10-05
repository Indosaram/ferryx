$ErrorActionPreference = 'Stop'
& bun 'run' '--cwd' 'ui' 'test'
$native=$LASTEXITCODE
Write-Output "NATIVE_EXIT=$native"
exit $native
