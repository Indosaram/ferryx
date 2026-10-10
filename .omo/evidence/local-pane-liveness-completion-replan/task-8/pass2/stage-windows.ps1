$ErrorActionPreference = 'Stop'
cmd.exe /d /c exit 7
$seven = $LASTEXITCODE
cmd.exe /d /c exit 0
$zero = $LASTEXITCODE
Write-Output "CAPTURE_VALIDATION=$seven,$zero"
if ($seven -ne 7 -or $zero -ne 0) { exit 99 }
Get-PSDrive C
$root = 'C:\Users\sook\ferryx-pane-completion\source-172baa87'
if (Test-Path $root) { throw 'Owned new source already exists; do not overwrite' }
New-Item -ItemType Directory $root | Out-Null
tar.exe -xf C:\Users\sook\ferryx-pane-completion\pass2-source.tar -C $root
$native = $LASTEXITCODE
if ($native -ne 0) { exit $native }
if ((Get-ChildItem "$root\src-tauri\vendor\ghostty" -Force).Count -ne 0) { throw 'Nonempty submodule placeholder' }
[System.IO.Directory]::Delete("$root\src-tauri\vendor\ghostty")
cmd.exe /d /c "mklink /J $root\src-tauri\vendor\ghostty C:\Users\sook\task2-ghostty-6a508fd5"
$native = $LASTEXITCODE
if ($native -ne 0) { exit $native }
Set-Location $root
bun install --cwd ui --frozen-lockfile
$native = $LASTEXITCODE
Write-Output "STAGING_DONE windows native=$native"
exit $native
