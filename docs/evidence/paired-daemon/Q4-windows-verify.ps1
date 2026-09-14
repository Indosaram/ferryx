$ErrorActionPreference = 'Stop'
$root = 'C:\Users\sook\ferryx-herdr-q4-windows-01a097f8'
Set-Location $root
Start-Transcript -Path "$root\preflight-second.log"
$env:PATH = 'C:\Users\sook\.cargo\bin;C:\Users\sook\.bun\bin;' + $env:PATH
$env:CARGO_HOME = 'C:\Users\sook\.cargo'
$env:RUSTUP_HOME = 'C:\Users\sook\.rustup'
foreach ($name in @('home','runtime','data','sessions','config','cache','temp','appdata','localappdata','target')) { New-Item -ItemType Directory -Force "$root\$name" | Out-Null }
$env:HOME = "$root\home"
$env:USERPROFILE = $env:HOME
$env:HOMEDRIVE = 'C:'
$env:HOMEPATH = '\Users\sook\ferryx-herdr-q4-windows-01a097f8\home'
$env:APPDATA = "$root\appdata"
$env:LOCALAPPDATA = "$root\localappdata"
$env:FERRYX_RUNTIME_DIR = "$root\runtime"
$env:FERRYX_DATA_DIR = "$root\data"
$env:FERRYX_SESSION_DIR = "$root\sessions"
$env:XDG_CONFIG_HOME = "$root\config"
$env:XDG_DATA_HOME = "$root\data"
$env:XDG_CACHE_HOME = "$root\cache"
$env:XDG_STATE_HOME = "$root\data"
$env:XDG_RUNTIME_DIR = "$root\runtime"
$env:TMP = "$root\temp"
$env:TEMP = $env:TMP
$env:TMPDIR = $env:TMP
$env:CARGO_TARGET_DIR = "$root\target"
$env:CARGO_BUILD_JOBS = '3'
$env:RUSTC_WRAPPER = ''
$env:CARGO_PROFILE_DEV_DEBUG = '0'
$env:CARGO_PROFILE_TEST_DEBUG = '0'
$env:CARGO_INCREMENTAL = '0'
$env:GIT_CONFIG_NOSYSTEM = '1'
$env:GIT_CONFIG_GLOBAL = "$root\home\.gitconfig"
[Environment]::OSVersion.VersionString
[IO.Path]::GetTempPath()
Get-PSDrive C | Select-Object Free
Get-FileHash source.tar.gz,ghostty.bundle
cmd /c 'rustc -vV > rustc.log 2>&1'
cmd /c 'cargo -V > cargo-version.log 2>&1'
cmd /c 'git --version > git-version.log 2>&1'
cmd /c 'where zig > zig-location.log 2>&1'
cmd /c 'where rust-analyzer > lsp-location.log 2>&1'

$manifest = Get-Content manifest.json -Raw | ConvertFrom-Json
$count = 0
foreach ($p in $manifest.PSObject.Properties) {
  $actual = (Get-FileHash (Join-Path "$root\source" $p.Name) -Algorithm SHA256).Hash.ToLower()
  if ($actual -ne $p.Value.sha256) { throw "Hash mismatch: $($p.Name)" }
  $count++
}
"TRANSFER_HASHES_VERIFIED=$count" | Tee-Object transfer-verification.log

cmd /c 'git -C source\src-tauri\vendor\ghostty rev-parse HEAD > ghostty-head.log 2>&1'
Get-Content rustc.log,cargo-version.log,git-version.log,zig-location.log,lsp-location.log,ghostty-head.log
Stop-Transcript
# Compiler proof first: no library/test initialization until fixture audit is complete.
Set-Location "$root\source"
$command = 'cargo --config C:\Users\sook\ferryx-herdr-q4-windows-01a097f8\cargo-qa.toml build --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib --bin ferryx-cli --bin ferryx-relay'
$command | Set-Content "$root\build-command.log"
$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = 'cmd.exe'
$psi.Arguments = "/d /s /c $command > $root\build.log 2>&1"
$psi.UseShellExecute = $false
$psi.WorkingDirectory = "$root\source"
$psi.EnvironmentVariables['RUSTC_WRAPPER'] = ''
$p = New-Object System.Diagnostics.Process
$p.StartInfo = $psi
$null = $p.Start()
"BUILD_PID=$($p.Id)" | Set-Content "$root\monitor.log"
if (-not $p.WaitForExit(1800000)) {
  cmd /c "taskkill /PID $($p.Id) /T /F > $root\timeout-cleanup.log 2>&1"
  $p.WaitForExit()
  'TIMEOUT' | Set-Content "$root\build-exit.log"
  exit 124
}
$p.Refresh()
"BUILD_EXIT=$($p.ExitCode); WAITED=True" | Set-Content "$root\build-exit.log"
exit $p.ExitCode
