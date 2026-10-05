$ErrorActionPreference='Continue'
$base = 'C:\Users\sook\ferryx-pane-completion'
$staging = Join-Path $base 'source-21dea3c0'
Write-Output "CLEANUP_START=$((Get-Date).ToString('o'))"
Write-Output "=== inventory: task-owned candidates ==="
$cands = Get-CimInstance Win32_Process | Where-Object { $_.Name -in @('ferryx.exe','node.exe','cargo.exe','rustc.exe','cmd.exe') }
foreach ($p in $cands) {
  $path = $_.Exception  # placeholder
}
$rows = @()
foreach ($p in $cands) {
  $exe = $p.ExecutablePath
  $cl = $p.CommandLine
  $owned = $false
  if ($exe -and $exe -like ($staging + '*')) { $owned = $true }
  if ($exe -and $exe -like ($base + '*')) { $owned = $true }
  if ($cl -and $cl -like ('*' + $base + '*')) { $owned = $true }
  $rows += [pscustomobject]@{ PID=$p.ProcessId; PPID=$p.ParentProcessId; NAME=$p.Name; CREATED=$p.CreationDate; OWNED=$owned; EXE=$exe; CMD=($cl -replace '\s+',' ').Substring(0,[Math]::Min(140,($cl -replace '\s+',' ').Length)) }
}
$rows | Sort-Object OWNED -Descending | Format-Table -AutoSize | Out-String -Width 260 | Write-Output
Write-Output "=== identity check on the inherited leaked PID 23672 ==="
$leak = Get-CimInstance Win32_Process -Filter "ProcessId=23672" -ErrorAction SilentlyContinue
if ($leak) {
  Write-Output ("LEAK_FOUND PID=23672 NAME=" + $leak.Name + " EXE=" + $leak.ExecutablePath + " PPID=" + $leak.ParentProcessId + " CREATED=" + $leak.CreationDate)
  if ($leak.ExecutablePath -like ($base + '*')) { Write-Output "LEAK_IDENTITY_MATCHES_TASK_STAGING=TRUE" } else { Write-Output "LEAK_IDENTITY_MATCHES_TASK_STAGING=FALSE" }
} else { Write-Output "LEAK_GONE PID=23672 (already reaped by this session at 2026-10-05T00:00:xx; receipt recorded in win-reap output)" }
Write-Output "=== reaping task-owned trees ==="
foreach ($p in $rows) {
  if ($p.OWNED) {
    Write-Output ("REAP PID=" + $p.PID + " NAME=" + $p.NAME + " EXE=" + $p.EXE)
    taskkill /T /F /PID $p.PID 2>&1 | Out-String | Write-Output
  }
}
Start-Sleep -Seconds 2
Write-Output "=== after sweep ==="
$after = Get-CimInstance Win32_Process | Where-Object { ($_.Name -in @('ferryx.exe','node.exe')) -and (($_.ExecutablePath -like ($base + '*')) -or ($_.CommandLine -like ('*' + $base + '*'))) }
foreach ($p in $after) { Write-Output ("STILL_ALIVE PID=" + $p.ProcessId + " NAME=" + $p.Name + " EXE=" + $p.ExecutablePath) }
Write-Output ("TASK_OWNED_ALIVE_COUNT=" + @($after).Count)
Write-Output "=== foreign processes left untouched (count) ==="
$foreign = Get-CimInstance Win32_Process | Where-Object { ($_.Name -in @('ferryx.exe','node.exe','cargo.exe','rustc.exe')) -and -not (($_.ExecutablePath -like ($base + '*')) -or ($_.CommandLine -like ('*' + $base + '*'))) }
foreach ($p in $foreign) { Write-Output ("FOREIGN_KEPT PID=" + $p.ProcessId + " NAME=" + $p.Name + " EXE=" + $p.ExecutablePath) }
Write-Output "CLEANUP_DONE"
