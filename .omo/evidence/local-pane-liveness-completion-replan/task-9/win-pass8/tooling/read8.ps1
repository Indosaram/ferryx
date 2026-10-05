$ErrorActionPreference='Continue'
$t9='C:\Users\sook\ferryx-pane-completion\task9-48b4ed93'
foreach ($s in @('split-happy','split-attach-stall','split-cancel')) {
  Write-Output ("########## " + $s)
  $e = Join-Path $t9 ('evidence\' + $s)
  $a = Get-ChildItem -Recurse -File $e -Filter actions.jsonl -ErrorAction SilentlyContinue | Sort-Object LastWriteTime | Select-Object -Last 1
  if ($a) {
    Write-Output "=== ACTION NAMES IN ORDER ==="
    Get-Content $a.FullName | ForEach-Object { ($_ | ConvertFrom-Json).action }
    Write-Output "=== FRONTEND ACTIONS (verbatim) ==="
    Get-Content $a.FullName | Where-Object { $_ -match 'frontend' }
  } else { Write-Output "NO_ACTIONS" }
}
Write-Output "READ8_DONE"
