param(
    [Parameter(Mandatory = $true)]
    [string]$Archive,
    [string]$TestFilter = "",
    [switch]$ContractsOnly
)

$ErrorActionPreference = "Stop"
$fixture = Join-Path $env:TEMP ("ferryx-helper-verify-" + [guid]::NewGuid().ToString())
$result = 1
$previousBinary = $env:FERRYX_QA_HELPER_BINARY

try {
    New-Item -ItemType Directory -Path $fixture | Out-Null
    & tar -xf $Archive -C $fixture
    if ($LASTEXITCODE -ne 0) { throw "Cannot extract QA source archive" }
    Write-Output "WINDOWS_HELPER_VERIFY $fixture"
    $manifest = Join-Path $fixture "remote-helper\Cargo.toml"
    & cargo test --locked --manifest-path $manifest $TestFilter -- --nocapture
    if ($LASTEXITCODE -ne 0) { throw "Helper contracts failed: $LASTEXITCODE" }
    if ($ContractsOnly) {
        $result = 0
    }
    else {
        & cargo build --locked --manifest-path $manifest
        if ($LASTEXITCODE -ne 0) { throw "Debug helper build failed: $LASTEXITCODE" }
        $env:FERRYX_QA_HELPER_BINARY = Join-Path $fixture "remote-helper\target\debug\ferryx-remote-helper.exe"
        & bun (Join-Path $fixture "scripts\qa\ssh-helper-survival.mjs")
        $result = $LASTEXITCODE
    }
}
finally {
    $env:FERRYX_QA_HELPER_BINARY = $previousBinary
    if (Test-Path -LiteralPath $fixture) {
        Remove-Item -LiteralPath $fixture -Recurse -Force
    }
    if (Test-Path -LiteralPath $Archive) {
        Remove-Item -LiteralPath $Archive -Force
    }
    Write-Output "cleanup: removed Windows helper verification fixture $fixture"
}

exit $result
