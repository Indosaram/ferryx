$ErrorActionPreference = 'Stop'
cmd.exe /d /c exit 7
$seven = $LASTEXITCODE
cmd.exe /d /c exit 0
$zero = $LASTEXITCODE
Write-Output "CAPTURE_VALIDATION=$seven,$zero"
if ($seven -ne 7 -or $zero -ne 0) { exit 99 }
Get-PSDrive C
