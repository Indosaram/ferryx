$ErrorActionPreference='Continue'
$t9='C:\Users\sook\ferryx-pane-completion\task9-120bc965'
Write-Output "=== per-scenario detail ==="
foreach ($s in @('split-happy','split-attach-stall','split-cancel')) {
  Write-Output ("########## " + $s)
  $e = Join-Path $t9 ('evidence\' + $s)
  $rj = Get-ChildItem -Recurse -File $e -Filter result.json -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($rj) { $j = Get-Content $rj.FullName -Raw | ConvertFrom-Json
    Write-Output ("  code=" + $j.error.code)
    Write-Output ("  gate.ok=" + $j.cleanupGate.ok + " procsReaped=" + $j.cleanupGate.processesReaped + " dirsRemoved=" + $j.cleanupGate.directoriesRemoved)
  }
  $a = Get-ChildItem -Recurse -File $e -Filter actions.jsonl -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($a) {
    $click = Get-Content $a.FullName | Where-Object { $_ -match 'click-split-affordance' } | Select-Object -First 1
    if ($click) { $c = $click | ConvertFrom-Json
      Write-Output ("  CLICK code=" + $c.code + " candidateCount=" + $c.candidateCount + " actionableCount=" + $c.actionableCount + " candidates=" + ($c.candidates | ConvertTo-Json -Compress))
      Write-Output ("  scope: focusedFound=" + $c.scope.focusedFound + " focusSource=" + $c.scope.focusSource + " depth=" + $c.scope.depth + " isWindowRoot=" + $c.scope.isWindowRoot)
      Write-Output ("  window: hwnd=" + $c.window.mainWindowHandle + " visible=" + $c.window.windowVisible + " interactive=" + $c.window.interactive + " session=" + $c.window.sessionId)
    } else { Write-Output "  NO_CLICK_ACTION" }
    Write-Output ("  actions: " + ((Get-Content $a.FullName | ForEach-Object { ($_ | ConvertFrom-Json).action }) -join ' | '))
  }
  Write-Output "  --- relogin: the split-right powershell probe stdout ---"
  if ($a) { Get-Content $a.FullName | Where-Object { $_ -match 'split-right' } | Select-Object -First 1 }
}
Write-Output "=== the split-right probe failure text per scenario ==="
foreach ($s in @('split-happy','split-attach-stall','split-cancel')) {
  $e = Join-Path $t9 ('evidence\' + $s)
  $a = Get-ChildItem -Recurse -File $e -Filter actions.jsonl -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($a) { $line = Get-Content $a.FullName | Where-Object { $_ -match '"probe":"split-right"' } | Select-Object -First 1
    if ($line) { $o = $line | ConvertFrom-Json; $inner = $o.stdout | ConvertFrom-Json; Write-Output ("  " + $s + " failure=" + $inner.failure + " detail=" + $inner.detail + " scopeDepth=" + $inner.scopeDepth) }
  }
}
Write-Output "FINAL6_DONE"
