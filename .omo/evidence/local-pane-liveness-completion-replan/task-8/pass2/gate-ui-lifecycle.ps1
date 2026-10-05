$ErrorActionPreference = 'Stop'
& bun 'run' '--cwd' 'ui' 'test' 'src/lib/sessionPersistence.test.ts' 'src/lib/sessionLifecycle.test.ts' 'src/lib/nativeTerminalLifecycle.test.ts' 'src/components/NativeTerminalPane.lifecycle.test.tsx'
$native=$LASTEXITCODE
Write-Output "NATIVE_EXIT=$native"
exit $native
