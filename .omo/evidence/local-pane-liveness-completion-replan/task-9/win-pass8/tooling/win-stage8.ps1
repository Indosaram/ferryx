$ErrorActionPreference = 'Continue'
$base = 'C:\Users\sook\ferryx-pane-completion'
$root = Join-Path $base 'source-21dea3c0'
Set-Location $root
Write-Output "STAGE8_START $((Get-Date).ToString('o'))"
Write-Output ("FREE_GB=" + [math]::Round((Get-PSDrive C).Free/1GB,2))
$expected = @{}
Get-Content (Join-Path $base 'manifest-48b4ed93.txt') | ForEach-Object { $p = $_ -split ' ', 2; if ($p.Count -eq 2) { $expected[$p[0].Trim()] = $p[1].Trim() } }
Write-Output ("MANIFEST_ROWS=" + $expected.Count)
foreach ($k in $expected.Keys) { if (-not (Test-Path $k)) { Write-Output "PRE_MISSING $k" } else { $h = (Get-FileHash -Algorithm SHA256 $k).Hash.ToLower(); if ($h -eq $expected[$k]) { Write-Output "PRE_ALREADY $k" } else { Write-Output ("PRE_DIFFERS " + $k) } } }
& tar.exe -xzf (Join-Path $base 'delta-48b4ed93.tar.gz') -C $root
Write-Output "TAR_EXIT=$LASTEXITCODE"
$bad = 0
foreach ($k in $expected.Keys) { $h = (Get-FileHash -Algorithm SHA256 $k).Hash.ToLower(); if ($h -eq $expected[$k]) { Write-Output "STAGED_OK $k" } else { Write-Output ("STAGED_MISMATCH " + $k); $bad++ } }
Write-Output "STAGED_BAD_COUNT=$bad"
Get-ChildItem -Path (Join-Path $root 'scripts') -Recurse -File | ForEach-Object { $_.LastWriteTime = Get-Date }
$rs = Get-ChildItem -Path (Join-Path $root 'src-tauri') -Recurse -Include *.rs,*.toml -File
foreach ($f in $rs) { $f.LastWriteTime = Get-Date }
Write-Output ("TESTFILE_SHA=" + (Get-FileHash -Algorithm SHA256 'scripts\qa\pane-liveness.test.mjs').Hash.ToLower())
Write-Output ("BIN_EXISTS=" + (Test-Path 'src-tauri\target\debug\ferryx.exe'))
Write-Output ("BIN_SHA=" + $(if (Test-Path 'src-tauri\target\debug\ferryx.exe') { (Get-FileHash -Algorithm SHA256 'src-tauri\target\debug\ferryx.exe').Hash.ToLower() } else { 'NONE' }))
Write-Output ("UI_DIST=" + (Test-Path 'ui\dist\index.html'))
Write-Output "STAGE8_DONE"
