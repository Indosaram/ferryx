$ErrorActionPreference='Continue'
$t9='C:\Users\sook\ferryx-pane-completion\task9-42fba06f'
foreach ($s in @('split-happy','split-attach-stall','split-cancel')) {
  Write-Output ("########## " + $s)
  $e = Join-Path $t9 ('evidence\' + $s)
  $rj = Get-ChildItem -Recurse -File $e -Filter result.json -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($rj) { $j = Get-Content $rj.FullName -Raw | ConvertFrom-Json
    Write-Output ("  verdict=" + $j.verdict + " code=" + $j.error.code)
    Write-Output ("  cleanupGate: ok=" + $j.cleanupGate.ok + " dirsRemoved=" + $j.cleanupGate.directoriesRemoved + " procsReaped=" + $j.cleanupGate.processesReaped)
    Write-Output ("  holders=" + ($j.cleanupGate.holders | ConvertTo-Json -Compress))
  }
  $a = Get-ChildItem -Recurse -File $e -Filter actions.jsonl -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($a) {
    $click = Get-Content $a.FullName | Where-Object { $_ -match 'click-split-affordance' } | Select-Object -First 1
    if ($click) {
      Write-Output "  === CLICK ACTION, VERBATIM ==="
      Write-Output $click
    } else { Write-Output "  NO_CLICK_ACTION" }
  }
}
Write-Output "INVENTORY_DONE"
