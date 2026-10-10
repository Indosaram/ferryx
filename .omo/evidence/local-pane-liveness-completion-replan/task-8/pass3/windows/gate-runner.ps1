$ErrorActionPreference = 'Stop'
& 'bun' 'run' '--cwd' 'ui' 'test' '--config' '../scripts/qa/pane-liveness-vitest.config.mjs'
$native=$LASTEXITCODE
Write-Output "NATIVE_EXIT=$native"
exit $native
