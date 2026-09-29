# Cleanup: identity-checked kill of recorded task-owned processes. Writes receipts; never deletes anything.
param([Parameter(Mandatory = $true)][string]$Run)
. (Join-Path $PSScriptRoot 'common.ps1')

$root = Get-WsuRoot $Run
$cdir = Join-Path $root 'cleanup'
New-Item -ItemType Directory -Force -Path $cdir | Out-Null
$stamp = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ')
$receipt = Join-Path $cdir "cleanup-$stamp.jsonl"
$code = 1
$detail = ''
try {
  $records = @(Read-ProcRecords $root)
  if ($records.Count -eq 0) {
    Write-Receipt $receipt 'NO_RECORDS' 0 '' '' '' 'procs.jsonl absent or empty'
    $code = 0
  } else {
    $failed = [int](Stop-OwnedProcesses $root $records $receipt)
    $detail = "records=$($records.Count) failed=$failed"
    if ($failed -gt 0) { $code = 2 } else { $code = 0 }
  }
} catch {
  $detail = 'ERROR: ' + $_.Exception.Message
  Write-Receipt $receipt 'ERROR' 0 '' '' '' $_.Exception.Message
  $code = 1
}
Write-Text (Join-Path $cdir 'cleanup-exit.log') "EXIT=$code`nreceipt=cleanup-$stamp.jsonl`n$detail`n"
Add-Line (Join-Path $cdir 'cleanup-history.log') "$stamp EXIT=$code $detail"
Write-Output "WSU $Run cleanup EXIT=$code"
exit $code
