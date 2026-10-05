$ErrorActionPreference='Continue'
Write-Output ("FREE_GB=" + [math]::Round((Get-PSDrive C).Free/1GB,2))
Write-Output "=== biggest dirs under the staging root ==="
foreach ($d in @('source-21dea3c0\src-tauri\target','source-21dea3c0\node_modules','source-21dea3c0\ui\node_modules','source-base\src-tauri\target','task8-21dea3c0')) {
  $p = Join-Path 'C:\Users\sook\ferryx-pane-completion' $d
  if (Test-Path $p) {
    $sz = (Get-ChildItem -Recurse -File $p -ErrorAction SilentlyContinue | Measure-Object -Property Length -Sum).Sum
    Write-Output ("  " + $d + " = " + [math]::Round($sz/1GB,2) + " GB")
  }
}
Write-Output "=== my OWN staging tree target subdirs ==="
Get-ChildItem 'C:\Users\sook\ferryx-pane-completion\source-21dea3c0\src-tauri\target\debug\deps' -ErrorAction SilentlyContinue | Sort-Object Length -Descending | Select-Object -First 6 | ForEach-Object { Write-Output ("  " + $_.Name + " " + [math]::Round($_.Length/1MB,1) + " MB") }
Write-Output "DISK3_DONE"
