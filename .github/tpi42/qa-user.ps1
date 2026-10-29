# User-side half of the install.ps1 QA. Runs either as a freshly created local user (clean
# profile) or, as a fallback, as the runner user with fresh profile directories.
# usage: pwsh -File qa-user.ps1 <params.json> <mode: cleanuser|fallback>
param([string]$ParamsFile, [string]$Mode)
$ErrorActionPreference = 'Continue'
$p = Get-Content -LiteralPath $ParamsFile -Raw | ConvertFrom-Json
$log = Join-Path $p.qa ("user-" + $Mode + ".log")
function L([string]$m) { $m | Tee-Object -FilePath $log -Append | Out-Host }

if ($Mode -eq 'cleanuser') {
    # Real profile of this logon (LoadUserProfile); do not inherit the runner user's dirs.
    $env:LOCALAPPDATA = [Environment]::GetFolderPath('LocalApplicationData')
    $env:APPDATA = [Environment]::GetFolderPath('ApplicationData')
    $env:USERPROFILE = [Environment]::GetFolderPath('UserProfile')
} else {
    $fresh = Join-Path $p.qa 'fallback-profile'
    New-Item -ItemType Directory -Force -Path (Join-Path $fresh 'AppData\Local\Temp'), (Join-Path $fresh 'AppData\Roaming') | Out-Null
    $env:USERPROFILE = $fresh
    $env:LOCALAPPDATA = Join-Path $fresh 'AppData\Local'
    $env:APPDATA = Join-Path $fresh 'AppData\Roaming'
}
$env:TEMP = Join-Path $env:LOCALAPPDATA 'Temp'; $env:TMP = $env:TEMP
New-Item -ItemType Directory -Force -Path $env:TEMP | Out-Null
foreach ($v in 'FERRYX_INSTALL_DIR', 'FERRYX_DATA_DIR', 'FERRYX_RUNTIME_DIR', 'HOME', 'FERRYX_INSTALL_KEY') { Remove-Item "Env:$v" -ErrorAction SilentlyContinue }
L ("whoami=" + (whoami) + " mode=" + $Mode + " USERPROFILE=" + $env:USERPROFILE + " LOCALAPPDATA=" + $env:LOCALAPPDATA)
L ("before: Ferryx dir exists=" + (Test-Path (Join-Path $env:LOCALAPPDATA 'Ferryx')) + " ~/.ferryx exists=" + (Test-Path (Join-Path $env:USERPROFILE '.ferryx')))
L ("HKCU Path before: " + [Environment]::GetEnvironmentVariable('Path', 'User'))

# 1. install.ps1, unmodified, from the loopback origin with a pinned version.
$env:FERRYX_ORIGIN = $p.origin
$env:FERRYX_VERSION = $p.version
L ("--> pwsh -File install.ps1 -NoUp  (FERRYX_ORIGIN=" + $p.origin + " FERRYX_VERSION=" + $p.version + ")")
$out = & pwsh -NoProfile -File $p.installer -NoUp 2>&1
$rc = $LASTEXITCODE
$out | ForEach-Object { L ("    " + $_) }
L ("INSTALL_EXIT=" + $rc)
$installed = Join-Path $env:LOCALAPPDATA 'Ferryx\bin\ferryx-cli.exe'
L ("installed=" + $installed + " exists=" + (Test-Path $installed))
if (Test-Path $installed) { L ("installed sha256=" + (Get-FileHash $installed -Algorithm SHA256).Hash.ToLowerInvariant()) }
L ("HKCU Path after: " + [Environment]::GetEnvironmentVariable('Path', 'User'))
if ($rc -ne 0 -or -not (Test-Path $installed)) { L "USER_QA_RESULT=install-failed"; exit 1 }

# 2. --version
$ver = & $installed --version 2>&1
L ("--version -> exit " + $LASTEXITCODE + ": " + ($ver -join ' '))
$verRc = $LASTEXITCODE

# 3. Daemon (the headless daemon of the installed CLI), then up --no-service --json.
$env:FERRYX_ALLOW_INSECURE_RELAY = '1'
$dOut = Join-Path $p.qa ("daemon-" + $Mode + ".out"); $dErr = Join-Path $p.qa ("daemon-" + $Mode + ".err")
$daemon = Start-Process -FilePath $installed -ArgumentList '--daemon' -PassThru -NoNewWindow -RedirectStandardOutput $dOut -RedirectStandardError $dErr
L ("daemon pid " + $daemon.Id)
$deadline = (Get-Date).AddSeconds(90); $st = $null
while ((Get-Date) -lt $deadline) {
    $raw = & $installed remote status --json 2>$null
    try { $st = ($raw | Select-Object -Last 1) | ConvertFrom-Json } catch { $st = $null }
    if ($st -and $st.status -eq 'ok') { break }
    if ($daemon.HasExited) { break }
    Start-Sleep -Milliseconds 500
}
L ("status before up: " + ($st | ConvertTo-Json -Compress))

$env:FERRYX_INSTALL_KEY = $p.key
L ("--> ferryx-cli up --key-env --origin " + $p.origin + " --no-service --json")
$upOut = Join-Path $p.qa ("up-" + $Mode + ".json"); $upErr = Join-Path $p.qa ("up-" + $Mode + ".err")
& $installed up --key-env --origin $p.origin --no-service --json 1> $upOut 2> $upErr
$upRc = $LASTEXITCODE
Remove-Item Env:FERRYX_INSTALL_KEY -ErrorAction SilentlyContinue
L ("UP_EXIT=" + $upRc)
Get-Content $upErr | ForEach-Object { L ("    stderr: " + $_) }
$upLast = Get-Content $upOut | Select-Object -Last 1
L ("up --json last line: " + $upLast)
$up = $null; try { $up = $upLast | ConvertFrom-Json } catch { }

# 4. Live tunnel as the daemon reports it (bounded wait).
$deadline = (Get-Date).AddSeconds(90); $live = $null
while ((Get-Date) -lt $deadline) {
    $raw = & $installed remote status --json 2>$null
    try { $live = ($raw | Select-Object -Last 1) | ConvertFrom-Json } catch { $live = $null }
    if ($live -and $live.relayConnected -eq $true) { break }
    Start-Sleep -Milliseconds 500
}
L ("remote status --json: " + ($live | ConvertTo-Json -Compress))
$text = & $installed remote status 2>&1
L ("remote status (text): " + ($text -join ' '))

if (-not $daemon.HasExited) { Stop-Process -Id $daemon.Id -Force; L ("stopped own daemon pid " + $daemon.Id) }
Get-Content $dErr -Tail 15 -ErrorAction SilentlyContinue | ForEach-Object { L ("    daemon.err: " + $_) }

$ok = ($verRc -eq 0) -and ($upRc -eq 0) -and $up -and $up.enrolled -and ($live.relayConnected -eq $true)
L ("MACHINE_RECORD_ID=" + $up.enrolled.machineRecordId)
L ("USER_QA_RESULT=" + ($(if ($ok) { 'pass' } else { 'fail' })))
if ($ok) { exit 0 } else { exit 1 }
