param([Parameter(Mandatory = $true)][string]$EvidenceDirectory)

$ErrorActionPreference = 'Stop'
$status = 0
New-Item -ItemType Directory -Force $EvidenceDirectory | Out-Null
$EvidenceDirectory = (Resolve-Path $EvidenceDirectory).Path
Push-Location (Join-Path $PSScriptRoot '../ui')
try {
    $revision = & git rev-parse HEAD
    if ($LASTEXITCODE -ne 0) { throw 'Cannot determine checkout revision' }
    Write-Output "REX_UI_REVISION $revision"
    $gates = @(
        @{ Name = 'INSTALL'; Arguments = @('install', '--frozen-lockfile') },
        @{ Name = 'TSC'; Arguments = @('--bun', 'tsc', '--noEmit') },
        @{ Name = 'TSC_TEST'; Arguments = @('--bun', 'tsc', '--noEmit', '-p', 'tsconfig.test.json') },
        @{ Name = 'TOUCHED'; Arguments = @('--bun', 'vitest', 'run', '--maxWorkers=2', 'src/remote/RemoteTerminal', 'src/remote/RemoteApp.inventorySelection.test.tsx', 'src/remote/RemoteApp.referenceChat.test.tsx') },
        @{ Name = 'FULL'; Arguments = @('run', 'test') },
        @{ Name = 'BUILD'; Arguments = @('run', 'build') }
    )
    foreach ($gate in $gates) {
        $stdout = Join-Path $EvidenceDirectory ($gate.Name + '.stdout.log')
        $stderr = Join-Path $EvidenceDirectory ($gate.Name + '.stderr.log')
        # Native streams bypass Start-Transcript on Windows PowerShell. Capture both
        # explicitly and take the process exit code, not a pipeline's final status.
        $process = Start-Process -FilePath (Get-Command bun).Source -ArgumentList $gate.Arguments -NoNewWindow -Wait -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
        $code = $process.ExitCode
        Write-Output ($gate.Name + '_EXIT ' + $code)
        if ($code -ne 0) {
            $status = 1
            if ($gate.Name -eq 'INSTALL') { break }
        }
    }
} catch {
    Write-Output $_
    $status = 1
} finally {
    Pop-Location
}
Write-Output "REX_WINDOWS_UI_DONE $status"
exit $status
