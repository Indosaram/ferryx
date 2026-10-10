$ErrorActionPreference = 'Continue'
$base = 'C:\Users\sook\ferryx-pane-completion'
$root = Join-Path $base 'source-21dea3c0'
Set-Location $root
Write-Output "STAGE4_START $((Get-Date).ToString('o'))"
$j = Get-Item 'src-tauri\vendor\ghostty' -Force
Write-Output ("GHOSTTY=" + $j.LinkType + " -> " + $j.Target)
$expected = @{
 'scripts/lib/qa-scenarios/common-harness.mjs' = '62634b72c65d03869bc0132277fee34947b2116c7696b58c90470a0f520ae999'
 'scripts/lib/qa-scenarios/diagnostic-classifier.mjs' = 'b9dc896ac6935a8bac59100dd0f7cfd362738b3581b7d8f2608c4c76c8a6d5d3'
 'scripts/qa/pane-liveness.mjs' = 'b5cb88c26bf368eff178969b0e7ec3a1b087c76bd7fd1fe24310df2f70946a83'
 'scripts/qa/pane-liveness.test.mjs' = '0dedd762d69c3491070bc92457f5c4d335bfe39c529c88a8dad61ae863e7f23a'
 'src-tauri/src/daemon/qa_producers.rs' = '9d5f4b6bf0b4de90f072a312f2da7ba9d8c6f40406742ac34c5af3420e823a37'
 'src-tauri/src/daemon/server.rs' = 'd3e905f170791db762efce730cbb794e643c60ae72444bd8cf250d78121007ad'
 'src-tauri/src/ipc/qa_barrier.rs' = '48064101ca4b785bd69dc74177b434c53aac57cb2d79a0978b85076419bab84e'
 'src-tauri/src/ipc/terminal.rs' = '42744ef8a86fa5dcaa7f67df68be9310fb213166d0309da899d88c9dc896206f'
}
Write-Output "=== PRE-STATE ==="
foreach ($k in $expected.Keys) {
  if (-not (Test-Path $k)) { Write-Output "PRE_MISSING $k"; continue }
  $h = (Get-FileHash -Algorithm SHA256 $k).Hash.ToLower()
  if ($h -eq $expected[$k]) { Write-Output "PRE_ALREADY $k" } else { Write-Output ("PRE_DIFFERS " + $k + " got=" + $h.Substring(0,16)) }
}
& tar.exe -xzf (Join-Path $base 'delta-91d447e1.tar.gz') -C $root
Write-Output "TAR_EXIT=$LASTEXITCODE"
Write-Output "=== POST-EXTRACT ==="
$bad = 0
foreach ($k in $expected.Keys) {
  $h = (Get-FileHash -Algorithm SHA256 $k).Hash.ToLower()
  if ($h -eq $expected[$k]) { Write-Output "STAGED_OK $k" } else { Write-Output ("STAGED_MISMATCH " + $k + " want=" + $expected[$k] + " got=" + $h); $bad++ }
}
Write-Output "STAGED_BAD_COUNT=$bad"
Get-ChildItem -Path (Join-Path $root 'scripts') -Recurse -File | ForEach-Object { $_.LastWriteTime = Get-Date }
$rs = Get-ChildItem -Path (Join-Path $root 'src-tauri') -Recurse -Include *.rs,*.toml -File
foreach ($f in $rs) { $f.LastWriteTime = Get-Date }
Write-Output "TOUCHED_RS_TOML=$($rs.Count) AT $((Get-Date).ToString('o'))"
foreach ($s in @('src-tauri\target\debug\ferryx.exe','src-tauri\target\debug\ferryx.pdb')) {
  if (Test-Path $s) { Remove-Item -Force $s; Write-Output "DELETED_STALE $s" } else { Write-Output "NO_STALE $s" }
}
Write-Output ("FREE_GB=" + [math]::Round((Get-PSDrive C).Free/1GB,2))
Write-Output ("LOAD=" + (Get-CimInstance Win32_Processor | Select-Object -ExpandProperty LoadPercentage))
Write-Output "STAGE4_DONE"
