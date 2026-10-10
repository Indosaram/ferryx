$ErrorActionPreference = 'Continue'
bun run --cwd ui test src/lib/pairedDaemonRollout.test.ts
$native = $LASTEXITCODE
Write-Output "NATIVE_EXIT=$native"
exit $native
