param([Parameter(Mandatory=$true)][string]$TestFile)
$ErrorActionPreference = 'Continue'
bun run --cwd ui test $TestFile
$native = $LASTEXITCODE
Write-Output "NATIVE_EXIT=$native"
exit $native
