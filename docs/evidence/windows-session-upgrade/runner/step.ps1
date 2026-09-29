# One phase1 build/test step: records launcher + cmd identity, bounds the run, checks artifacts/test counts, prints the sentinel.
param(
  [Parameter(Mandatory = $true)][string]$Run,
  [Parameter(Mandatory = $true)][string]$Step
)
. (Join-Path $PSScriptRoot 'common.ps1')

function New-StepDef([string]$Cwd, [string]$Cmd, [int]$TimeoutSec, [string[]]$Needs, [bool]$NeedsDist, [bool]$CountsTests) {
  return @{ cwd = $Cwd; cmd = $Cmd; timeout = $TimeoutSec; needs = $Needs; needsDist = $NeedsDist; tests = $CountsTests }
}
$contract = 'windows_session_host_contract'
$cargo = 'cargo --config "{QA}"'
$manifest = '--locked --manifest-path src-tauri\Cargo.toml'
$steps = @{
  'ui-install'       = (New-StepDef 'ui' 'bun install --frozen-lockfile' 1800 @('checkout') $false $false)
  'ui-build'         = (New-StepDef 'ui' 'bun run build' 1800 @('ui-install') $false $false)
  'build-bin'        = (New-StepDef '.' "$cargo build $manifest --bin ferryx" 5400 @('ui-build') $true $false)
  'lib-session-host' = (New-StepDef '.' "$cargo test $manifest --lib -- terminal::session_host::" 5400 @('ui-build') $true $true)
  'lib-output-hub'   = (New-StepDef '.' "$cargo test $manifest --lib -- terminal::output_hub::tests::" 5400 @('ui-build') $true $true)
  'contract'         = (New-StepDef '.' "$cargo test $manifest --test $contract" 5400 @('ui-build') $true $true)
  'lib-full'         = (New-StepDef '.' "$cargo test $manifest --lib" 7200 @('ui-build') $true $true)
}
if (-not $steps.ContainsKey($Step)) {
  [Console]::Error.WriteLine("UNKNOWN_STEP: $Step")
  Write-Output "WSU $Run $Step EXIT=2"
  exit 2
}

$root = Get-WsuRoot $Run
$src = Join-Path $root 'src'
New-Item -ItemType Directory -Force -Path (Join-Path $root 'logs'), (Join-Path $root 'evidence'), (Join-Path $root 'cleanup') | Out-Null
Exit-IfStepRan $root $Run $Step
$log = Join-Path $root "logs\$Step.log"
$cmdLog = Join-Path $root "logs\$Step-command.log"
$def = $steps[$Step]

function Invoke-WsuStep {
  foreach ($n in $def.needs) { Assert-StepOk $root $n }
  $qa = Set-WsuEnv $root
  $dist = Join-Path $src 'ui\dist\index.html'
  if ($def.needsDist -and -not (Test-Path -LiteralPath $dist)) { throw 'UI_DIST_MISSING: real ui build required before cargo' }
  if ($Step -eq 'contract' -and -not (Test-Path -LiteralPath (Join-Path $src "src-tauri\tests\$contract.rs"))) {
    Write-Text $log "MISSING_UPSTREAM: src-tauri\tests\$contract.rs is not in the tested tree; contract not run`n"
    return 3
  }
  $cwd = $src
  if ($def.cwd -ne '.') { $cwd = Join-Path $src $def.cwd }
  $cmd = $def.cmd.Replace('{QA}', $qa)
  Write-Text $cmdLog ('utc=' + [DateTime]::UtcNow.ToString('o') + "`ncwd=$cwd`ncmd=$cmd`ntimeoutSec=$($def.timeout)`n")

  $null = Add-ProcRecord $root (Get-ProcIdentity $PID) "launcher:$Step"
  $psi = New-Object System.Diagnostics.ProcessStartInfo
  $psi.FileName = Join-Path $env:SystemRoot 'System32\cmd.exe'
  $psi.Arguments = '/d /s /c "' + $cmd + ' > "' + $log + '" 2>&1"'
  $psi.WorkingDirectory = $cwd
  $psi.UseShellExecute = $false
  $psi.CreateNoWindow = $true
  $proc = [System.Diagnostics.Process]::Start($psi)
  $ident = Get-ProcIdentity $proc.Id
  if ($null -eq $ident) {
    $ident = [pscustomobject]@{ pid = $proc.Id; creationUtc = $proc.StartTime.ToUniversalTime().ToString('o'); exe = $psi.FileName; parentPid = $PID }
  }
  $rec = Add-ProcRecord $root $ident "step:$Step"

  if (-not $proc.WaitForExit([int]($def.timeout * 1000))) {
    Add-Line $cmdLog "TIMEOUT after $($def.timeout)s"
    $failed = Stop-OwnedProcesses $root @($rec) (Join-Path $root "cleanup\timeout-$Step.jsonl")
    if ($failed -gt 0) { Add-Line $cmdLog "TIMEOUT_KILL_INCOMPLETE failed=$failed" }
    return 124
  }
  $proc.WaitForExit()
  $rc = $proc.ExitCode
  Add-Line $cmdLog "processExit=$rc"
  if ($rc -ne 0) { return $rc }

  if ($Step -eq 'ui-build' -and -not (Test-Path -LiteralPath $dist)) { Add-Line $cmdLog 'UI_DIST_MISSING after build'; return 4 }
  if ($Step -eq 'build-bin') {
    $exe = Join-Path $root 'target\debug\ferryx.exe'
    if (-not (Test-Path -LiteralPath $exe)) { Add-Line $cmdLog "ARTIFACT_MISSING: $exe"; return 4 }
    $item = Get-Item -LiteralPath $exe
    Write-Text (Join-Path $root 'evidence\ferryx-exe.txt') ("path=$exe`nsha256=" + (Get-Sha256 $exe) + "`nbytes=$($item.Length)`nmtimeUtc=" + $item.LastWriteTimeUtc.ToString('o') + "`n")
  }
  if ($def.tests) {
    $passed = 0; $failedTests = 0; $ignored = 0; $binaries = 0
    foreach ($l in [IO.File]::ReadAllLines($log)) {
      if ($l -match 'test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored') {
        $binaries++; $passed += [int]$Matches[1]; $failedTests += [int]$Matches[2]; $ignored += [int]$Matches[3]
      }
    }
    Write-Text (Join-Path $root "evidence\$Step-tests.txt") "binaries=$binaries`npassed=$passed`nfailed=$failedTests`nignored=$ignored`n"
    if ($failedTests -gt 0) { return 1 }
    if ($passed -eq 0) { Add-Line $cmdLog 'NO_TESTS_EXECUTED'; return 5 }
  }
  return 0
}

$code = 1
try { $code = [int](Invoke-WsuStep) }
catch {
  Add-Line $cmdLog ('ERROR: ' + $_.Exception.Message)
  $code = 1
}
Complete-Step $root $Run $Step $code
exit $code
