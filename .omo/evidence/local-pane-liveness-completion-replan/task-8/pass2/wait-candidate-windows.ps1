$ErrorActionPreference = 'Stop'
do {
  $owned = @(Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'node.exe' -and $_.CommandLine -and $_.CommandLine.Contains('task8-172baa87/runner.mjs') })
  if ($owned.Count) { Start-Sleep -Seconds 5 }
} while ($owned.Count)
Write-Output 'CANDIDATE_EXITED windows'
