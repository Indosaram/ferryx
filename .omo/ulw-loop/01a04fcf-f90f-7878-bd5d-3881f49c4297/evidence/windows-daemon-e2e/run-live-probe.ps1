$ErrorActionPreference = "Stop"

$repo = "C:\Users\sook\ferryx-ulw-01a04fcf"
$exe = Join-Path $repo "src-tauri\target\debug\ferryx.exe"
$runtime = Join-Path $env:LOCALAPPDATA "Ferryx\runtime"
$out1 = Join-Path $repo "daemon-e2e-1.out.log"
$err1 = Join-Path $repo "daemon-e2e-1.err.log"
$out2 = Join-Path $repo "daemon-e2e-2.out.log"
$err2 = Join-Path $repo "daemon-e2e-2.err.log"

New-Item -ItemType Directory -Force $runtime | Out-Null
Get-CimInstance Win32_Process -Filter "Name = 'ferryx.exe'" |
  Where-Object { $_.ExecutablePath -eq $exe -and $_.CommandLine -match '--daemon' } |
  ForEach-Object { Stop-Process -Id $_.ProcessId -Force }
Remove-Item (Join-Path $runtime "daemon.port") -Force -ErrorAction SilentlyContinue
Remove-Item $out1, $err1, $out2, $err2 -Force -ErrorAction SilentlyContinue

$daemon1 = $null
try {
  $daemon1 = Start-Process -FilePath $exe -ArgumentList "--daemon" -WorkingDirectory $repo `
    -RedirectStandardOutput $out1 -RedirectStandardError $err1 -PassThru

  $deadline = [DateTime]::UtcNow.AddSeconds(20)
  while (
    [DateTime]::UtcNow -lt $deadline -and
    -not ((Test-Path $out1) -and (Select-String -Path $out1 -Pattern "FERRYX_DAEMON_READY" -Quiet))
  ) {
    Start-Sleep -Milliseconds 100
  }
  if (-not ((Test-Path $out1) -and (Select-String -Path $out1 -Pattern "FERRYX_DAEMON_READY" -Quiet))) {
    throw "daemon 1 readiness timeout: $(Get-Content $err1 -Raw -ErrorAction SilentlyContinue)"
  }

  $portFile = Join-Path $runtime "daemon.port"
  $port1 = (Get-Content $portFile -Raw).Trim()
  Write-Output "DAEMON1_READY pid=$($daemon1.Id) port=$port1"

  Set-Location $repo
  & bun script/qa/win-daemon-e2e.mjs $repo
  if ($LASTEXITCODE -ne 0) {
    throw "first E2E failed exit=$LASTEXITCODE"
  }
  Write-Output "E2E1_PASS"

  $daemon2 = Start-Process -FilePath $exe -ArgumentList "--daemon" -WorkingDirectory $repo `
    -RedirectStandardOutput $out2 -RedirectStandardError $err2 -PassThru
  if (-not $daemon2.WaitForExit(10000)) {
    Stop-Process -Id $daemon2.Id -Force
    throw "duplicate daemon did not exit"
  }
  $daemon2.Refresh()

  $port2 = (Get-Content $portFile -Raw).Trim()
  $duplicateText =
    (Get-Content $out2 -Raw -ErrorAction SilentlyContinue) + "`n" +
    (Get-Content $err2 -Raw -ErrorAction SilentlyContinue)
  if ($daemon2.ExitCode -eq 0) {
    throw "duplicate daemon exited zero"
  }
  if ($duplicateText -notmatch "Another daemon instance is already holding the lock") {
    throw "duplicate lock message missing: $duplicateText"
  }
  if ($port2 -ne $port1) {
    throw "daemon port clobbered $port1 -> $port2"
  }
  if ($daemon1.HasExited) {
    throw "daemon 1 exited during duplicate probe"
  }
  Write-Output "DUPLICATE_REJECT_PASS exit=$($daemon2.ExitCode) port=$port2"

  & bun script/qa/win-daemon-e2e.mjs $repo
  if ($LASTEXITCODE -ne 0) {
    throw "second E2E failed exit=$LASTEXITCODE"
  }
  Write-Output "E2E2_PASS"
} finally {
  if ($daemon1 -and -not $daemon1.HasExited) {
    Stop-Process -Id $daemon1.Id -Force
    $daemon1.WaitForExit(5000) | Out-Null
  }
  Remove-Item (Join-Path $runtime "daemon.port") -Force -ErrorAction SilentlyContinue
  $remaining = @(
    Get-CimInstance Win32_Process -Filter "Name = 'ferryx.exe'" |
      Where-Object { $_.ExecutablePath -eq $exe -and $_.CommandLine -match '--daemon' }
  )
  Write-Output (
    "CLEANUP daemonProcesses=$($remaining.Count) " +
    "portExists=$(Test-Path (Join-Path $runtime 'daemon.port')) " +
    "lockExists=$(Test-Path (Join-Path $runtime 'daemon.lock'))"
  )
  Get-PSDrive C | ForEach-Object {
    Write-Output ("FREE_GB=" + [math]::Round($_.Free / 1GB, 2))
  }
}

Write-Output "WINDOWS_DAEMON_E2E_PASS"
