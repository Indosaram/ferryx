$ErrorActionPreference='Continue'
$t9='C:\Users\sook\ferryx-pane-completion\task9-120bc965'
Write-Output ("NOW=" + (Get-Date).ToString('o'))
foreach ($s in @('split-happy','split-attach-stall','split-cancel')) {
  Write-Output ("########## " + $s)
  $e = Join-Path $t9 ('evidence\' + $s)
  $rj = Get-ChildItem -Recurse -File $e -Filter result.json -ErrorAction SilentlyContinue | Sort-Object LastWriteTime | Select-Object -Last 1
  if ($rj) { $j = Get-Content $rj.FullName -Raw | ConvertFrom-Json
    Write-Output ("  verdict=" + $j.verdict + " code=" + $j.error.code)
    Write-Output ("  gate.ok=" + $j.cleanupGate.ok + " dirsRemoved=" + $j.cleanupGate.directoriesRemoved)
    Write-Output ("  message=" + $j.error.message)
  } else { Write-Output '  NO_RESULT' }
  $a = Get-ChildItem -Recurse -File $e -Filter actions.jsonl -ErrorAction SilentlyContinue | Sort-Object LastWriteTime | Select-Object -Last 1
  if ($a) { Write-Output ("  actions: " + ((Get-Content $a.FullName | ForEach-Object { ($_ | ConvertFrom-Json).action }) -join ' | ')) }
  Write-Output "  --- click action verbatim ---"
  if ($a) { Get-Content $a.FullName | Where-Object { $_ -match 'click-split-affordance' } }
}
Write-Output "=== ISOLATION-ROOT HOLDERS (the lead's ask) ==="
$iso = Join-Path $t9 'runtime'
Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -like ('*' + $iso + '*') -or ($_.Name -eq 'ferryx.exe' -and $_.ExecutablePath -like '*source-21dea3c0*') } | ForEach-Object { Write-Output ("  HOLDER PID=" + $_.ProcessId + " PPID=" + $_.ParentProcessId + " S" + $_.SessionId + " " + $_.Name + " :: " + ($_.CommandLine -replace '\s+',' ').Substring(0,[Math]::Min(100,($_.CommandLine -replace '\s+',' ').Length))) }
Write-Output "=== do the isolation roots still exist? ==="
Get-ChildItem $iso -ErrorAction SilentlyContinue | ForEach-Object { Write-Output ("  " + $_.Name) }
Write-Output "READ7_DONE"
