$ErrorActionPreference = 'Stop'
& 'bun' 'run' '--cwd' 'ui' 'test' 'src/lib/localSplitLifecycle.test.ts'
$native=$LASTEXITCODE
Write-Output "NATIVE_EXIT=$native"
exit $native
