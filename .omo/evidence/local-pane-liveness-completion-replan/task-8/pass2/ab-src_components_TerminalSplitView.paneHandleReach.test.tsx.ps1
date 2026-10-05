$ErrorActionPreference = 'Continue'
bun run --cwd ui test src/components/TerminalSplitView.paneHandleReach.test.tsx
$native = $LASTEXITCODE
Write-Output "NATIVE_EXIT=$native"
exit $native
