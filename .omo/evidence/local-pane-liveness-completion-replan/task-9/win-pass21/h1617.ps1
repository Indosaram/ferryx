param([int]$Which = 0)
$ErrorActionPreference='Continue'
$root='C:\Users\sook\ferryx-pane-completion\source-21dea3c0'
$bun='C:\Users\sook\.bun\bin\bun.exe'
$outDir='C:\Users\sook\ferryx-pane-completion\h1617'
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
Set-Location $root
$revision = 'cfb4374b + the st_01a108cd uncommitted fixes (host ui/ and src-tauri/ hashes match local)'

if ($Which -eq 16 -or $Which -eq 0) {
  Write-Output "=== H-16: workspaceStore / workspaceRestore / nativeTerminalInputQueue as their OWN recorded gate ==="
  $log = Join-Path $outDir 'H16-ui-suites.log'
  Remove-Item $log -Force -ErrorAction SilentlyContinue
  Write-Output ("revision: " + $revision) | Tee-Object -FilePath $log -Append
  Write-Output ("cmd: bun x vitest run --maxWorkers=1 src/state/workspaceStore.test.tsx src/state/workspaceRestore.test.tsx src/lib/nativeTerminalInputQueue.test.ts") | Tee-Object -FilePath $log -Append
  Push-Location (Join-Path $root 'ui')
  & $bun x vitest run --maxWorkers=1 src/state/workspaceStore.test.tsx src/state/workspaceRestore.test.tsx src/lib/nativeTerminalInputQueue.test.ts 2>&1 | Tee-Object -FilePath $log -Append
  $code = $LASTEXITCODE
  Pop-Location
  Write-Output ("H16_EXIT=" + $code) | Tee-Object -FilePath $log -Append
}

if ($Which -eq 17 -or $Which -eq 0) {
  Write-Output "=== H-17a: cargo test --lib pane_liveness_contract (the EXACT filter, under its own name) ==="
  $log2 = Join-Path $outDir 'H17a-pane-contract.log'
  Remove-Item $log2 -Force -ErrorAction SilentlyContinue
  Write-Output ("revision: " + $revision) | Tee-Object -FilePath $log2 -Append
  Write-Output ("cmd: cargo test --manifest-path src-tauri/Cargo.toml --lib pane_liveness_contract") | Tee-Object -FilePath $log2 -Append
  Push-Location $root
  & cargo test --manifest-path src-tauri/Cargo.toml --lib pane_liveness_contract 2>&1 | Tee-Object -FilePath $log2 -Append
  $code2 = $LASTEXITCODE
  Pop-Location
  Write-Output ("H17a_EXIT=" + $code2) | Tee-Object -FilePath $log2 -Append

  Write-Output "=== H-17b: localSplitContract.test.ts under its own name ==="
  $log3 = Join-Path $outDir 'H17b-localSplitContract.log'
  Remove-Item $log3 -Force -ErrorAction SilentlyContinue
  Write-Output ("revision: " + $revision) | Tee-Object -FilePath $log3 -Append
  Write-Output ("cmd: bun x vitest run --maxWorkers=1 src/lib/localSplitContract.test.ts") | Tee-Object -FilePath $log3 -Append
  Push-Location (Join-Path $root 'ui')
  & $bun x vitest run --maxWorkers=1 src/lib/localSplitContract.test.ts 2>&1 | Tee-Object -FilePath $log3 -Append
  $code3 = $LASTEXITCODE
  Pop-Location
  Write-Output ("H17b_EXIT=" + $code3) | Tee-Object -FilePath $log3 -Append
}
Write-Output 'H1617_DONE'
