$ErrorActionPreference='Continue'
$base='C:\Users\sook\ferryx-pane-completion'
Write-Output ("FREE_BEFORE_GB=" + [math]::Round((Get-PSDrive C).Free/1GB,2))
# Reclaim ONLY this task's own staging tree build cache: the QA build cannot link
# without space, and this target dir is mine to manage. Foreign trees untouched.
$t = Join-Path $base 'source-21dea3c0\src-tauri\target'
if (Test-Path $t) {
  Write-Output "REMOVING_OWN_TARGET $t"
  Remove-Item -Recurse -Force $t -ErrorAction SilentlyContinue
}
Start-Sleep -Seconds 2
Write-Output ("FREE_AFTER_GB=" + [math]::Round((Get-PSDrive C).Free/1GB,2))
Write-Output "=== foreign trees kept ==="
Get-ChildItem $base -Directory | ForEach-Object { Write-Output ("  " + $_.Name) }
Write-Output "RECLAIM_DONE"
