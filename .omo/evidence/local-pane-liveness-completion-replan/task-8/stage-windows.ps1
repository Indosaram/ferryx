$ErrorActionPreference = 'Stop'
cmd.exe /d /c exit 7
$seven = $LASTEXITCODE
cmd.exe /d /c exit 0
$zero = $LASTEXITCODE
Write-Output "CAPTURE_VALIDATION=$seven,$zero"
if ($seven -ne 7 -or $zero -ne 0) { exit 99 }
Get-PSDrive C
$root = 'C:\Users\sook\ferryx-pane-completion\source-5464da0d'
if (Test-Path $root) { throw 'Owned new source already exists; do not overwrite' }
New-Item -ItemType Directory $root | Out-Null
tar.exe -xf C:\Users\sook\ferryx-pane-completion\task8-source.tar -C $root
$native = $LASTEXITCODE
if ($native -ne 0) { exit $native }
cmd.exe /d /c "mklink /J $root\src-tauri\vendor\ghostty C:\Users\sook\task2-ghostty-6a508fd5"
$native = $LASTEXITCODE
if ($native -ne 0) { exit $native }
Set-Location $root
bun install --cwd ui --frozen-lockfile
$native = $LASTEXITCODE
Write-Output "STAGING_DONE windows native=$native"
exit $native
