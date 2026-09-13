# Safe wrapper boundary execution: function mocks replace tar/cargo/bun only.
# No native build, helper, SSH connection, or user profile operation is allowed.
$ErrorActionPreference = 'Stop'
$root = Join-Path $env:TEMP ('p25-wrapper-contract-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $root | Out-Null
$previousTemp = $env:TEMP
$previousTarget = $env:CARGO_TARGET_DIR
$previousBinary = $env:FERRYX_QA_HELPER_BINARY
$wrappers = @('check-remote-helper.ps1', 'verify-remote-helper.ps1')
$failures = 0
$cases = 0
try {
    $env:TEMP = $root
    $env:CARGO_TARGET_DIR = Join-Path $root 'foreign-target-do-not-use'
    $env:FERRYX_QA_HELPER_BINARY = 'previous-helper'
    function tar {
        $destination = $args[3]
        New-Item -ItemType Directory -Force -Path (Join-Path $destination 'remote-helper') | Out-Null
        Set-Content -LiteralPath (Join-Path $destination 'remote-helper\Cargo.toml') -Value '[package]'
        $global:LASTEXITCODE = 0
    }
    function cargo {
        $script:cargoCalls += ,@($args)
        $global:LASTEXITCODE = 0
        if ($script:mode -eq 'cargo-failure') { $global:LASTEXITCODE = 1; return }
        if ($args[0] -eq 'test') {
            if ($script:mode -eq 'zero-tests') { 'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out' }
            else { 'test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out' }
        }
        if ($args[0] -eq 'build') {
            $index = [array]::IndexOf($args, '--target-dir')
            if ($index -lt 0) { throw 'BUILD_OUTPUT_NOT_OWNED' }
            $script:artifact = Join-Path $args[$index + 1] 'debug\ferryx-remote-helper.exe'
            New-Item -ItemType Directory -Force -Path (Split-Path $script:artifact) | Out-Null
            Set-Content -LiteralPath $script:artifact -Value 'owned artifact'
            @{ reason='compiler-artifact'; target=@{name='ferryx-remote-helper'; kind=@('bin')}; profile=@{test=$false}; executable=$script:artifact } | ConvertTo-Json -Compress
        }
    }
    function bun {
        if ($env:FERRYX_QA_HELPER_BINARY -ne $script:artifact) { throw 'WRONG_ARTIFACT' }
        $global:LASTEXITCODE = $(if ($script:mode -eq 'survival-failure') { 1 } else { 0 })
    }
    foreach ($wrapper in $wrappers) {
        foreach ($mode in @('success', 'cargo-failure', 'zero-tests', 'survival-failure')) {
            if ($wrapper -eq 'check-remote-helper.ps1' -and $mode -in @('zero-tests', 'survival-failure')) { continue }
            $cases++
            $script:mode = $mode
            $script:cargoCalls = @()
            $archive = Join-Path $root ($cases.ToString() + '.tar')
            [IO.File]::WriteAllBytes($archive, [byte[]]@(1,2,3,4))
            $before = (Get-FileHash -LiteralPath $archive).Hash
            # Execute original wrapper body in this scope, replacing only process
            # exit with a returned status so fixtures/mocks survive each case.
            $source = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot $wrapper)
            $source = $source -replace 'exit \$result', 'return $result'
            $code = [scriptblock]::Create($source)
            $failed = $false
            try { $output = & $code -Archive $archive; if ($output[-1] -ne 0) { $failed = $true } }
            catch { $failed = $true }
            try {
                if (-not (Test-Path -LiteralPath $archive)) { throw 'INPUT_ARCHIVE_DELETED' }
                if ((Get-FileHash -LiteralPath $archive).Hash -ne $before) { throw 'INPUT_ARCHIVE_CHANGED' }
                if ($env:CARGO_TARGET_DIR -ne (Join-Path $root 'foreign-target-do-not-use')) { throw 'TARGET_ENV_CHANGED' }
                if ($env:FERRYX_QA_HELPER_BINARY -ne 'previous-helper') { throw 'BINARY_ENV_CHANGED' }
                if ($mode -eq 'success' -and $failed) { throw 'SUCCESS_REJECTED' }
                if ($mode -ne 'success' -and -not $failed) { throw 'FAILURE_ACCEPTED' }
                if ($mode -eq 'survival-failure' -and -not (Test-Path -LiteralPath $script:artifact)) { throw 'FAILED_SURVIVAL_TREE_DELETED' }
                Write-Output "PASS $wrapper $mode"
            } catch { $failures++; Write-Output "FAIL $wrapper $mode $_" }
        }
    }
} finally {
    $env:TEMP = $previousTemp
    $env:CARGO_TARGET_DIR = $previousTarget
    $env:FERRYX_QA_HELPER_BINARY = $previousBinary
    Remove-Item -LiteralPath $root -Recurse -Force
}
Write-Output "P25_WRAPPER_RECEIPT cases=$cases failures=$failures fixtureRemoved=$root"
if ($failures -ne 0) { exit 1 }
