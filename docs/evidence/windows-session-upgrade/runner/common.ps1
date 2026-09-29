# Shared helpers for the Windows session-upgrade phase1 runner. Dot-sourced by every runner script.
Set-StrictMode -Version 2
$ErrorActionPreference = 'Stop'

$WsuBase = 'C:\Users\sook\ferryx-wsu'
$GhosttySrc = 'C:\Users\sook\ferryx-ghostty'
$GhosttyPin = '6a508fd5e34c7e222c052a6d00bb3891ff3feace'
$MinFreeBytes = 20GB
$Utf8 = New-Object System.Text.UTF8Encoding($false)

function Assert-RunId([string]$Run) {
  if ($Run -notmatch '^wsu-[0-9a-f]{8}-[0-9a-f]{8}-r[0-9]{2}$') { throw "BAD_RUN_ID: $Run" }
}
function Get-WsuRoot([string]$Run) { Assert-RunId $Run; return (Join-Path $WsuBase $Run) }
function Write-Text([string]$Path, [string]$Text) { [IO.File]::WriteAllText($Path, $Text, $Utf8) }
function Add-Line([string]$Path, [string]$Text) { [IO.File]::AppendAllText($Path, $Text + "`n", $Utf8) }
function Get-Sha256([string]$Path) { return (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant() }
function ConvertTo-Utc([string]$Text) {
  return [DateTime]::Parse($Text, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::RoundtripKind).ToUniversalTime()
}
function Test-SameInstant([DateTime]$A, [DateTime]$B) { return ([Math]::Abs(($A - $B).TotalMilliseconds) -lt 1) }

function Complete-Step([string]$Root, [string]$Run, [string]$Step, [int]$Code) {
  Write-Text (Join-Path $Root "logs\$Step-exit.log") "EXIT=$Code`n"
  Write-Output "WSU $Run $Step EXIT=$Code"
}
# A step runs once per RUN; refusing here keeps an earlier exit log from being overwritten.
function Exit-IfStepRan([string]$Root, [string]$Run, [string]$Step) {
  if (Test-Path -LiteralPath (Join-Path $Root "logs\$Step-exit.log")) {
    [Console]::Error.WriteLine("STEP_ALREADY_RAN: $Step (use a new rNN)")
    Write-Output "WSU $Run $Step EXIT=9"
    exit 9
  }
}
function Assert-StepOk([string]$Root, [string]$Step) {
  $p = Join-Path $Root "logs\$Step-exit.log"
  if (-not (Test-Path -LiteralPath $p)) { throw "PREREQ_MISSING: $Step has not run" }
  $t = ([IO.File]::ReadAllText($p)).Trim()
  if ($t -ne 'EXIT=0') { throw "PREREQ_FAILED: $Step $t" }
}

# Isolated environment for everything the runner launches. Returns the cargo config path that clears rustc-wrapper.
function Set-WsuEnv([string]$Root) {
  $dirs = [ordered]@{
    HOME = 'env\home'; APPDATA = 'env\appdata'; LOCALAPPDATA = 'env\localappdata'; TMP = 'env\tmp'
    XDG_CONFIG_HOME = 'env\xdg\config'; XDG_DATA_HOME = 'env\xdg\data'; XDG_CACHE_HOME = 'env\xdg\cache'; XDG_STATE_HOME = 'env\xdg\state'
    FERRYX_RUNTIME_DIR = 'ferryx\runtime'; FERRYX_DATA_DIR = 'ferryx\data'; FERRYX_SESSION_DIR = 'ferryx\session'
    CARGO_TARGET_DIR = 'target'
  }
  foreach ($k in $dirs.Keys) {
    $p = Join-Path $Root $dirs[$k]
    New-Item -ItemType Directory -Force -Path $p | Out-Null
    Set-Item -Path "Env:$k" -Value $p
  }
  $env:USERPROFILE = $env:HOME; $env:TEMP = $env:TMP; $env:TMPDIR = $env:TMP
  $gitcfg = Join-Path $Root 'env\gitconfig'
  if (-not (Test-Path -LiteralPath $gitcfg)) { Write-Text $gitcfg '' }
  $env:GIT_CONFIG_GLOBAL = $gitcfg; $env:GIT_CONFIG_NOSYSTEM = '1'
  $env:CARGO_HOME = 'C:\Users\sook\.cargo'; $env:RUSTUP_HOME = 'C:\Users\sook\.rustup'
  $env:CARGO_BUILD_JOBS = '3'; $env:CARGO_INCREMENTAL = '0'
  $env:CARGO_PROFILE_DEV_DEBUG = '0'; $env:CARGO_PROFILE_TEST_DEBUG = '0'
  foreach ($k in 'RUSTC_WRAPPER', 'CARGO_BUILD_RUSTC_WRAPPER') { Remove-Item -Path "Env:$k" -ErrorAction SilentlyContinue }
  $env:PATH = 'C:\Users\sook\.cargo\bin;C:\Users\sook\.bun\bin;' + $env:PATH
  $qa = Join-Path $Root 'env\cargo-qa.toml'
  if (-not (Test-Path -LiteralPath $qa)) { Write-Text $qa "[build]`nrustc-wrapper = `"`"`n" }
  return $qa
}

# Run a native command, log output and exit code, throw on a missing tool or nonzero LASTEXITCODE.
function Invoke-Native([string]$Log, [string]$Exe, [string[]]$ArgList) {
  if (-not (Get-Command $Exe -ErrorAction SilentlyContinue)) { Add-Line $Log "> $Exe (NOT FOUND)"; throw "TOOL_MISSING: $Exe" }
  $global:LASTEXITCODE = -999
  $prev = $ErrorActionPreference
  $ErrorActionPreference = 'Continue'
  try { $out = @(& $Exe @ArgList 2>&1 | ForEach-Object { "$_" }) } finally { $ErrorActionPreference = $prev }
  $code = $global:LASTEXITCODE
  Add-Line $Log ("> $Exe " + ($ArgList -join ' '))
  foreach ($l in $out) { Add-Line $Log $l }
  Add-Line $Log "EXIT=$code"
  if ($code -ne 0) { throw "NATIVE_FAILED: $Exe exited $code" }
  return , $out
}

function Get-ProcIdentity([int]$ProcId) {
  $p = Get-CimInstance -ClassName Win32_Process -Filter "ProcessId = $ProcId" -ErrorAction SilentlyContinue
  if ($null -eq $p) { return $null }
  return [pscustomobject]@{
    pid = [int]$p.ProcessId; creationUtc = $p.CreationDate.ToUniversalTime().ToString('o')
    exe = [string]$p.ExecutablePath; parentPid = [int]$p.ParentProcessId
  }
}
# Append one procs.jsonl row and return it.
function Add-ProcRecord([string]$Root, $Identity, [string]$Role) {
  $row = [pscustomobject][ordered]@{
    pid = $Identity.pid; creationUtc = $Identity.creationUtc; exe = $Identity.exe
    parentPid = $Identity.parentPid; role = $Role
  }
  Add-Line (Join-Path $Root 'procs.jsonl') ($row | ConvertTo-Json -Compress)
  return $row
}
function Read-ProcRecords([string]$Root) {
  $p = Join-Path $Root 'procs.jsonl'
  if (-not (Test-Path -LiteralPath $p)) { return @() }
  return @([IO.File]::ReadAllLines($p) | Where-Object { $_.Trim() } | ForEach-Object { $_ | ConvertFrom-Json })
}
function Write-Receipt([string]$Path, [string]$Action, [int]$ProcId, [string]$Creation, [string]$Exe, [string]$Role, [string]$Detail) {
  $row = [pscustomobject][ordered]@{
    utc = [DateTime]::UtcNow.ToString('o'); action = $Action; pid = $ProcId
    creationUtc = $Creation; exe = $Exe; role = $Role; detail = $Detail
  }
  Add-Line $Path ($row | ConvertTo-Json -Compress)
}

# Kill recorded processes (pid + creation + exe match) and their verified descendants, leaves first, no /T.
# Children of an exited recorded pid are claimed only when their exe or command line is under the run root.
# Returns the number of SKIP_MISMATCH / SKIP_UNVERIFIED / KILL_FAILED outcomes. Never deletes files.
function Stop-OwnedProcesses([string]$Root, [object[]]$Records, [string]$ReceiptPath) {
  $snap = @(Get-CimInstance -ClassName Win32_Process)
  $byPid = @{}
  $children = @{}
  foreach ($p in $snap) {
    $byPid[[int]$p.ProcessId] = $p
    $pp = [int]$p.ParentProcessId
    if (-not $children.ContainsKey($pp)) { $children[$pp] = New-Object System.Collections.ArrayList }
    [void]$children[$pp].Add($p)
  }
  $rootPrefix = $Root + '\'
  $failed = 0
  $owned = New-Object System.Collections.ArrayList
  $queue = New-Object System.Collections.Queue
  $seen = @{}

  foreach ($r in $Records) {
    $recPid = [int]$r.pid
    if ($recPid -eq $PID) { continue }
    $recCreation = ConvertTo-Utc $r.creationUtc
    $cur = $byPid[$recPid]
    if ($null -eq $cur) {
      Write-Receipt $ReceiptPath 'ALREADY_EXITED' $recPid $r.creationUtc $r.exe $r.role 'recorded process not running'
      $queue.Enqueue(@{ pid = $recPid; creation = $recCreation; depth = 0; verified = $false })
      continue
    }
    $curCreation = $cur.CreationDate.ToUniversalTime()
    if ((Test-SameInstant $curCreation $recCreation) -and ([string]$cur.ExecutablePath -ieq [string]$r.exe)) {
      $seen[$recPid] = $true
      [void]$owned.Add(@{ proc = $cur; creation = $curCreation; depth = 0; role = $r.role })
      $queue.Enqueue(@{ pid = $recPid; creation = $curCreation; depth = 0; verified = $true })
    } else {
      Write-Receipt $ReceiptPath 'SKIP_MISMATCH' $recPid $curCreation.ToString('o') ([string]$cur.ExecutablePath) $r.role 'pid alive with different creation or exe'
      $failed++
    }
  }

  while ($queue.Count -gt 0) {
    $n = $queue.Dequeue()
    if (-not $children.ContainsKey($n.pid)) { continue }
    foreach ($c in $children[$n.pid]) {
      $cPid = [int]$c.ProcessId
      if ($cPid -eq $PID -or $cPid -eq $n.pid -or $seen.ContainsKey($cPid)) { continue }
      $cCreation = $c.CreationDate.ToUniversalTime()
      $exe = [string]$c.ExecutablePath
      # Older than the parent means the parent pid was reused; not ours.
      if (($cCreation - $n.creation).TotalMilliseconds -lt -1) { continue }
      if (-not $n.verified) {
        $cmdLine = [string]$c.CommandLine
        $underRoot = $exe.StartsWith($rootPrefix, [StringComparison]::OrdinalIgnoreCase) -or
          ($cmdLine.IndexOf($rootPrefix, [StringComparison]::OrdinalIgnoreCase) -ge 0)
        if (-not $underRoot) {
          Write-Receipt $ReceiptPath 'SKIP_UNVERIFIED' $cPid $cCreation.ToString('o') $exe 'descendant' 'child of exited recorded pid not tied to run root; left running'
          $failed++
          continue
        }
      }
      $seen[$cPid] = $true
      [void]$owned.Add(@{ proc = $c; creation = $cCreation; depth = $n.depth + 1; role = 'descendant' })
      $queue.Enqueue(@{ pid = $cPid; creation = $cCreation; depth = $n.depth + 1; verified = $true })
    }
  }

  foreach ($o in @($owned | Sort-Object { $_.depth } -Descending)) {
    $procId = [int]$o.proc.ProcessId
    $exe = [string]$o.proc.ExecutablePath
    $created = $o.creation.ToString('o')
    try { $h = [System.Diagnostics.Process]::GetProcessById($procId) }
    catch { Write-Receipt $ReceiptPath 'ALREADY_EXITED' $procId $created $exe $o.role 'gone before kill'; continue }
    try {
      if (-not (Test-SameInstant $h.StartTime.ToUniversalTime() $o.creation)) {
        Write-Receipt $ReceiptPath 'SKIP_MISMATCH' $procId $created $exe $o.role 'start time changed before kill'
        $failed++
        continue
      }
      $h.Kill()
      if ($h.WaitForExit(15000)) { Write-Receipt $ReceiptPath 'KILLED' $procId $created $exe $o.role '' }
      else { Write-Receipt $ReceiptPath 'KILL_FAILED' $procId $created $exe $o.role 'still running 15s after kill'; $failed++ }
    } catch {
      if ($h.HasExited) { Write-Receipt $ReceiptPath 'ALREADY_EXITED' $procId $created $exe $o.role 'exited during kill' }
      else { Write-Receipt $ReceiptPath 'KILL_FAILED' $procId $created $exe $o.role $_.Exception.Message; $failed++ }
    } finally { $h.Dispose() }
  }
  return $failed
}
