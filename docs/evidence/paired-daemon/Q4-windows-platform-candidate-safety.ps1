$ErrorActionPreference = 'Stop'
$root = 'C:\Users\sook\ferryx-herdr-q4-windows-01a097f8'
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
[IO.Path]::GetTempPath() | Set-Content candidate-safety-isolation.log
function Run-Owned($name,$argsText) {
 $command = "cargo --config $root\cargo-qa.toml $argsText"
 $command | Set-Content "$root\candidate-safety-$name-command.log"
 $psi = New-Object System.Diagnostics.ProcessStartInfo
 $psi.FileName = 'cmd.exe'
 $psi.Arguments = "/d /s /c $command > $root\candidate-safety-$name.log 2>&1"
 $psi.UseShellExecute = $false
 $psi.WorkingDirectory = "$root\source"
 $psi.EnvironmentVariables['RUSTC_WRAPPER'] = ''
 $p = New-Object System.Diagnostics.Process
 $p.StartInfo = $psi
 $null = $p.Start()
 "$name PID=$($p.Id)" | Add-Content "$root\candidate-safety-monitor.log"
 if (-not $p.WaitForExit(1800000)) {
  cmd /c "taskkill /PID $($p.Id) /T /F > $root\candidate-safety-timeout-cleanup.log 2>&1"
  $p.WaitForExit()
  throw "$name timeout; owned tree killed and waited"
 }
 "$name EXIT=$($p.ExitCode); WAITED=True" | Add-Content "$root\candidate-safety-exits.log"
 return $p.ExitCode
}

$code=Run-Owned 'suite' 'test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test machine_catalog_persistence --test machine_worktrees --test worktree_safety --test machine_worktree_legacy_bounds --test machine_worktree_transports --test relay_pairing_generation_regression -- --nocapture'
