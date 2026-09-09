param(
    [Parameter(Mandatory = $true)]
    [string]$Archive
)

$ErrorActionPreference = "Stop"
$fixture = Join-Path $env:TEMP ("ferryx-helper-check-" + [guid]::NewGuid().ToString())
$result = 1

try {
    New-Item -ItemType Directory -Path $fixture | Out-Null
    & tar -xf $Archive -C $fixture
    if ($LASTEXITCODE -ne 0) { throw "Cannot extract QA source archive" }
    Write-Output "WINDOWS_HELPER_CHECK $fixture"
    & cargo check --locked --manifest-path (Join-Path $fixture "remote-helper\Cargo.toml")
    $result = $LASTEXITCODE
}
finally {
    if (Test-Path -LiteralPath $fixture) {
        Remove-Item -LiteralPath $fixture -Recurse -Force
    }
    if (Test-Path -LiteralPath $Archive) {
        Remove-Item -LiteralPath $Archive -Force
    }
    Write-Output "cleanup: removed Windows helper source and build fixture $fixture"
}

exit $result
