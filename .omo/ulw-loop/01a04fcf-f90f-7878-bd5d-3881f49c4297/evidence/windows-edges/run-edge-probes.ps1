$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$repo = "C:\Users\sook\ferryx-ulw-01a04fcf"
$exe = Join-Path $repo "src-tauri\target\debug\ferryx.exe"
$edgeRoot = Join-Path $repo ".edge-runtime"
$edgeLocalAppData = Join-Path $edgeRoot "local"
$edgeAppData = Join-Path $edgeRoot "roaming"
$runtime = Join-Path $edgeLocalAppData "Ferryx\runtime"
$portFile = Join-Path $runtime "daemon.port"
$lockFile = Join-Path $runtime "daemon.lock"
$driver = Join-Path $repo "probe-daemon-edges.mjs"

Remove-Item $edgeRoot -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $runtime, $edgeAppData | Out-Null

# Stale runtime files must not prevent a clean start or survive as a stale port.
Set-Content -Path $portFile -Value "9" -NoNewline
Set-Content -Path $lockFile -Value "stale-unlocked-lock" -NoNewline

$daemon = $null
try {
  $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
  $startInfo.FileName = $exe
  $startInfo.Arguments = "--daemon"
  $startInfo.WorkingDirectory = $repo
  $startInfo.UseShellExecute = $false
  $startInfo.CreateNoWindow = $true
  $startInfo.EnvironmentVariables["LOCALAPPDATA"] = $edgeLocalAppData
  $startInfo.EnvironmentVariables["APPDATA"] = $edgeAppData
  $daemon = [System.Diagnostics.Process]::new()
  $daemon.StartInfo = $startInfo
  if (-not $daemon.Start()) { throw "failed to start isolated daemon" }
  $deadline = [DateTime]::UtcNow.AddSeconds(20)
  while ([DateTime]::UtcNow -lt $deadline -and (
    -not (Test-Path $portFile) -or (Get-Content $portFile -Raw).Trim() -eq "9"
  )) { Start-Sleep -Milliseconds 100 }
  if (-not (Test-Path $portFile) -or (Get-Content $portFile -Raw).Trim() -eq "9") {
    throw "isolated daemon startup failed; exited=$($daemon.HasExited)"
  }
  $port = [int](Get-Content $portFile -Raw)
  if ($port -eq 9 -or $port -le 0) { throw "stale daemon.port was not replaced: $port" }
  Write-Output "STALE_RUNTIME_RECOVERY_PASS port=$port lockExists=$(Test-Path $lockFile)"

  & bun $driver $port $repo --skip-wsl
  if ($LASTEXITCODE -ne 0) { throw "protocol edge driver failed: $LASTEXITCODE" }

  # Running a linked Rust test executable proves the Common-Controls v6 manifest startup boundary.
  & cargo test --manifest-path (Join-Path $repo "src-tauri\Cargo.toml") --lib ipc::cli_install -- --nocapture
  if ($LASTEXITCODE -ne 0) { throw "manifest/startup test executable failed: $LASTEXITCODE" }
  Write-Output "MANIFEST_TEST_STARTUP_PASS"
} finally {
  if ($daemon -and -not $daemon.HasExited) {
    Stop-Process -Id $daemon.Id -Force
    $daemon.WaitForExit(5000) | Out-Null
  }
  Remove-Item $driver -Force -ErrorAction SilentlyContinue
  Remove-Item $edgeRoot -Recurse -Force -ErrorAction SilentlyContinue
  $remaining = @(
    Get-CimInstance Win32_Process -Filter "Name = 'ferryx.exe'" |
      Where-Object { $_.ExecutablePath -eq $exe -and $_.CommandLine -match '--daemon' }
  )
  Write-Output "EDGE_AUDIT_CLEANUP daemonProcesses=$($remaining.Count) portExists=$(Test-Path $portFile)"
}

Write-Output "WINDOWS_UNCOVERED_EDGES_PASS"
