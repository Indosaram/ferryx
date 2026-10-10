$ErrorActionPreference='Continue'
$e='C:\Users\sook\ferryx-pane-completion\task9-120bc965\evidence'
Write-Output "=== any capture artifacts? ==="
$imgs = Get-ChildItem -Recurse -File $e -ErrorAction SilentlyContinue | Where-Object { $_.Extension -in @('.png','.jpg','.jpeg') }
foreach ($i in $imgs) { Write-Output ("  " + $i.FullName + " " + $i.Length) }
Write-Output ("IMAGE_COUNT=" + @($imgs).Count)
Write-Output "=== all evidence files per scenario ==="
foreach ($s in @('split-happy','split-attach-stall','split-cancel')) {
  Write-Output ("--- " + $s)
  Get-ChildItem -Recurse -File (Join-Path $e $s) -ErrorAction SilentlyContinue | ForEach-Object { Write-Output ("    " + $_.FullName.Replace((Join-Path $e $s),'') + " " + $_.Length) }
}
Write-Output "CAPS6_DONE"
