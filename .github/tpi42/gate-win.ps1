# Windows gate for todo 42 (runs on windows-latest from the repo root of the lane snapshot).
$ErrorActionPreference = 'Continue'
$root = (Get-Location).Path
$ci = Join-Path $root 'ci-logs'; New-Item -ItemType Directory -Force -Path $ci | Out-Null
$h = Join-Path $root '.github/tpi42'
function Step([string]$n, [scriptblock]$b) {
    $f = Join-Path $ci "$n.log"
    "+ $n $(Get-Date -Format o)" | Tee-Object -FilePath $f | Out-Host
    $global:LASTEXITCODE = 0
    try { & $b 2>&1 | Tee-Object -FilePath $f -Append | Out-Host; $rc = $global:LASTEXITCODE } catch { "EXCEPTION: $_" | Tee-Object -FilePath $f -Append | Out-Host; $rc = 99 }
    if ($null -eq $rc) { $rc = 0 }
    "EXIT_CODE=$rc" | Tee-Object -FilePath $f -Append | Out-Host
    "$n exit=$rc" | Add-Content (Join-Path $ci 'exits.txt')
    return $rc
}

$target = Join-Path $root 'src-tauri\target'
$out = 'C:\tpi42\out'
New-Item -ItemType Directory -Force -Path $out | Out-Null

Step 'env' {
    "os: " + [Environment]::OSVersion.VersionString
    rustc --version; cargo --version; zig version; bun --version; node --version; pwsh --version
    "CARGO_ENCODED_RUSTFLAGS present: " + (Test-Path Env:CARGO_ENCODED_RUSTFLAGS)
    "RUSTFLAGS=[" + $env:RUSTFLAGS + "]"
    git -C src-tauri/vendor/ghostty rev-parse HEAD
    git rev-parse HEAD; git rev-parse 'HEAD^{tree}'
} | Out-Null

# 1. The generated cli-windows-amd64 build step, cut verbatim from createWindowsBuildScript.
Step 'gen-cli-step' { bun (Join-Path $h 'gen-cli-step.mjs') $root $target $out | Set-Content -Encoding utf8 (Join-Path $ci 'cli-step.ps1'); Get-Content (Join-Path $ci 'cli-step.ps1') } | Out-Null
$buildRc = Step 'cli-build' { pwsh -NoProfile -File (Join-Path $ci 'cli-step.ps1') }
$cli = Join-Path $out 'ferryx-cli-windows-amd64.exe'

Step 'dumpbin-cli' {
    $vswhere = Join-Path ([Environment]::GetFolderPath('ProgramFilesX86')) 'Microsoft Visual Studio\Installer\vswhere.exe'
    $dumpbin = & $vswhere -latest -products '*' -find 'VC\Tools\MSVC\**\bin\Hostx64\x64\dumpbin.exe' | Select-Object -First 1
    "dumpbin: $dumpbin"
    & $dumpbin /nologo /dependents $cli
    $deps = @(& $dumpbin /nologo /dependents $cli | ForEach-Object { $_.Trim() } | Where-Object { $_ -match '^[A-Za-z0-9_.+-]+\.dll$' })
    ConvertTo-Json -Compress -InputObject $deps | Set-Content -Encoding utf8 (Join-Path $ci 'cli-dependents.json')
    "CLI_DEPENDENTS=" + (ConvertTo-Json -Compress -InputObject $deps)
    $crt = @($deps | Where-Object { $_ -match '^(vcruntime|msvcp|vccorlib|concrt|vcomp|mfc)[0-9a-z_]*\.dll$' -or $_ -match '^api-ms-win-crt-' -or $_ -match '^ucrtbase' })
    "CRT_DLLS=" + ($crt -join ',')
    "sha256=" + (Get-FileHash $cli -Algorithm SHA256).Hash.ToLowerInvariant() + " bytes=" + (Get-Item $cli).Length
    & $cli --version; "version exit=$LASTEXITCODE"
    if ($crt.Count -ne 0) { $global:LASTEXITCODE = 1 }
} | Out-Null

# 2. Failure fixture: the same toolchain WITHOUT +crt-static (a dynamically linked exe), measured with dumpbin.
Step 'dyn-fixture' {
    $d = 'C:\tpi42\dynfix'; Remove-Item -Recurse -Force $d -ErrorAction SilentlyContinue
    cargo new --quiet --vcs none --name ferryx-cli $d
    Remove-Item Env:RUSTFLAGS -ErrorAction SilentlyContinue
    cargo build --quiet --release --manifest-path "$d\Cargo.toml"
    $vswhere = Join-Path ([Environment]::GetFolderPath('ProgramFilesX86')) 'Microsoft Visual Studio\Installer\vswhere.exe'
    $dumpbin = & $vswhere -latest -products '*' -find 'VC\Tools\MSVC\**\bin\Hostx64\x64\dumpbin.exe' | Select-Object -First 1
    & $dumpbin /nologo /dependents "$d\target\release\ferryx-cli.exe"
    $deps = @(& $dumpbin /nologo /dependents "$d\target\release\ferryx-cli.exe" | ForEach-Object { $_.Trim() } | Where-Object { $_ -match '^[A-Za-z0-9_.+-]+\.dll$' })
    ConvertTo-Json -Compress -InputObject $deps | Set-Content -Encoding utf8 (Join-Path $ci 'dyn-dependents.json')
    "DYN_DEPENDENTS=" + (ConvertTo-Json -Compress -InputObject $deps)
} | Out-Null

Step 'floor-gate' { bun (Join-Path $h 'floor-gate.mjs') (Join-Path $ci 'cli-dependents.json') (Join-Path $ci 'dyn-dependents.json') } | Out-Null
Step 'test-release-platforms' { bun test scripts/release-platforms.test.mjs } | Out-Null

# 3. Self-hosted relay on this runner (same static-CRT flags, so the shared deps are reused).
Step 'relay-build' {
    $env:CARGO_TARGET_DIR = $target
    $env:RUSTFLAGS = '-C target-feature=+crt-static'
    cargo build --manifest-path src-tauri/Cargo.toml --release --no-default-features --bin ferryx-relay
    $rc = $LASTEXITCODE
    Remove-Item Env:RUSTFLAGS
    $global:LASTEXITCODE = $rc
} | Out-Null

if ($buildRc -ne 0 -or -not (Test-Path $cli)) { 'CLI build failed; QA skipped' | Add-Content (Join-Path $ci 'exits.txt'); exit 0 }
$relayExe = Join-Path $target 'release\ferryx-relay.exe'
if (-not (Test-Path $relayExe)) { 'relay build failed; QA skipped' | Add-Content (Join-Path $ci 'exits.txt'); exit 0 }

$qa = 'C:\tpi42\qa'; New-Item -ItemType Directory -Force -Path $qa | Out-Null
$version = 'v2026.10.27.5-tpi42'
$port = 18987
$origin = "http://127.0.0.1:$port"
$params = $null

Step 'qa-setup' {
    # Origin: the relay's own /download route serves ferryx-cli-windows-amd64.exe from FERRYX_DOWNLOADS_DIR.
    $dl = Join-Path $qa 'downloads'; New-Item -ItemType Directory -Force -Path $dl | Out-Null
    Copy-Item $cli (Join-Path $dl 'ferryx-cli-windows-amd64.exe')
    $sha = (Get-FileHash $cli -Algorithm SHA256).Hash.ToLowerInvariant()
    # Release side: SHA256SUMS at the exact URL install.ps1 builds (github.com/.../releases/download/<tag>/SHA256SUMS),
    # served from loopback: hosts file -> 127.0.0.1, a self-signed github.com cert trusted machine-wide.
    $rel = Join-Path $qa "gh\Indosaram\ferryx\releases\download\$version"; New-Item -ItemType Directory -Force -Path $rel | Out-Null
    [IO.File]::WriteAllText((Join-Path $rel 'SHA256SUMS'), "$sha  ferryx-cli-windows-amd64.exe`n")
    "SHA256SUMS: $sha  ferryx-cli-windows-amd64.exe"
    $openssl = 'C:\Program Files\Git\usr\bin\openssl.exe'
    & $openssl req -x509 -newkey rsa:2048 -nodes -days 2 -subj '/CN=github.com' -addext 'subjectAltName=DNS:github.com' -keyout "$qa\gh.key" -out "$qa\gh.crt" 2>&1
    Import-Certificate -FilePath "$qa\gh.crt" -CertStoreLocation Cert:\LocalMachine\Root | Select-Object Thumbprint, Subject
    Copy-Item C:\Windows\System32\drivers\etc\hosts "$qa\hosts.bak"
    Add-Content C:\Windows\System32\drivers\etc\hosts "`n127.0.0.1 github.com"
    ipconfig /flushdns | Out-Null
    $gh = Start-Process python -ArgumentList @((Join-Path $h 'ghstub.py'), "$qa\gh", "$qa\gh.crt", "$qa\gh.key", '443') -PassThru -NoNewWindow -RedirectStandardOutput "$qa\ghstub.out" -RedirectStandardError "$qa\ghstub.err"
    "ghstub pid $($gh.Id)"; $gh.Id | Set-Content "$qa\ghstub.pid"
    # Relay (self-host mode, file mailer, loopback only).
    $env:FERRYX_DEPLOYMENT_MODE = 'selfhost'; $env:FERRYX_RELAY_PORT = "$port"; $env:FERRYX_ACCOUNT_ORIGIN = $origin
    $env:FERRYX_ACCOUNT_DATA_DIR = "$qa\relay\account"; $env:FERRYX_RELAY_DATA_DIR = "$qa\relay\data"; $env:FERRYX_DOWNLOADS_DIR = $dl
    $relay = Start-Process $relayExe -PassThru -NoNewWindow -RedirectStandardOutput "$qa\relay.out" -RedirectStandardError "$qa\relay.err"
    "relay pid $($relay.Id)"; $relay.Id | Set-Content "$qa\relay.pid"
    foreach ($v in 'FERRYX_DEPLOYMENT_MODE', 'FERRYX_RELAY_PORT', 'FERRYX_ACCOUNT_ORIGIN', 'FERRYX_ACCOUNT_DATA_DIR', 'FERRYX_RELAY_DATA_DIR', 'FERRYX_DOWNLOADS_DIR') { Remove-Item "Env:$v" }
    $up = $false
    for ($i = 0; $i -lt 300 -and -not $up; $i++) { try { Invoke-RestMethod "$origin/api/account/v1/health" | Out-Null; $up = $true } catch { Start-Sleep -Milliseconds 200 } }
    "relay health: $up"
    $sumsProbe = Invoke-WebRequest "https://github.com/Indosaram/ferryx/releases/download/$version/SHA256SUMS" -UseBasicParsing
    "loopback github SHA256SUMS probe: HTTP $($sumsProbe.StatusCode) $([Text.Encoding]::ASCII.GetString($sumsProbe.Content).Trim())"
    $dlProbe = Invoke-WebRequest "$origin/download/ferryx-cli-windows-amd64.exe" -UseBasicParsing -OutFile "$qa\probe.exe" -PassThru
    "relay /download probe: HTTP $($dlProbe.StatusCode) sha256=" + (Get-FileHash "$qa\probe.exe" -Algorithm SHA256).Hash.ToLowerInvariant()
    # Account session through the file mailer, then one install key.
    Invoke-RestMethod -Method Post -ContentType 'application/json' -Body '{"email":"qa@example.test"}' "$origin/api/account/v1/login/request" | Out-Null
    Start-Sleep -Milliseconds 500
    $mail = Get-ChildItem -Recurse -File "$qa\relay\account\mail" | Get-Content -Raw
    $code = ([regex]::Match(($mail -join "`n"), 'code=([A-Za-z0-9_-]+)')).Groups[1].Value
    $token = (Invoke-RestMethod -Method Post -ContentType 'application/json' -Body (@{ code = $code } | ConvertTo-Json) "$origin/api/account/v1/login/consume").token
    $key = (Invoke-RestMethod -Method Post -ContentType 'application/json' -Headers @{ authorization = "Bearer $token" } -Body '{}' "$origin/api/account/v1/install-keys").key
    "session token: " + ($(if ($token) { 'yes' } else { 'NO' })) + "; install key: " + ($(if ($key) { 'yes' } else { 'NO' }))
    Copy-Item (Join-Path $root 'scripts\install.ps1') "$qa\install.ps1"; Copy-Item (Join-Path $h 'qa-user.ps1') "$qa\qa-user.ps1"
    "install.ps1 copy sha256=" + (Get-FileHash "$qa\install.ps1" -Algorithm SHA256).Hash.ToLowerInvariant() + " (same bytes as scripts/install.ps1: " + ((Get-FileHash "$qa\install.ps1").Hash -eq (Get-FileHash (Join-Path $root 'scripts\install.ps1')).Hash) + ")"
    @{ qa = $qa; origin = $origin; version = $version; installer = "$qa\install.ps1"; key = $key; token = $token } | ConvertTo-Json | Set-Content -Encoding utf8 "$qa\params.json"
    if (-not ($up -and $token -and $key)) { $global:LASTEXITCODE = 1 }
} | Out-Null

# 4. install.ps1 + --version + up as a freshly created local user (clean profile).
Step 'qa-install-cleanuser' {
    icacls $qa /grant 'Everyone:(OI)(CI)F' /T /Q | Out-Null
    $user = 'tpi42qa'; $pw = 'Q' + [Guid]::NewGuid().ToString('N').Substring(0, 20) + '!a1'
    net user $user $pw /add /y | Out-Null
    # Batch logon right for the scheduled-task fallback (Performance Log Users holds it by default); not an admin.
    net localgroup 'Performance Log Users' $user /add | Out-Null
    "groups: " + ((Get-LocalGroup | Where-Object { (Get-LocalGroupMember $_ -ErrorAction SilentlyContinue).Name -contains "$env:COMPUTERNAME\$user" }).Name -join ',')
    "created local user $user (exists: " + [bool](Get-LocalUser $user -ErrorAction SilentlyContinue) + ")"
    $cred = New-Object System.Management.Automation.PSCredential("$env:COMPUTERNAME\$user", (ConvertTo-SecureString $pw -AsPlainText -Force))
    $pwsh = (Get-Command pwsh).Source
    $how = 'none'
    try {
        $p = Start-Process -FilePath $pwsh -ArgumentList @('-NoProfile', '-File', "$qa\qa-user.ps1", "$qa\params.json", 'cleanuser') -Credential $cred -LoadUserProfile -WorkingDirectory $qa -PassThru -RedirectStandardOutput "$qa\cu.out" -RedirectStandardError "$qa\cu.err"
        $done = $p.WaitForExit(600000)
        $how = 'Start-Process -Credential'; "Start-Process -Credential exited=$done exit=$($p.ExitCode)"
    } catch { "Start-Process -Credential failed: $_" }
    if (-not (Select-String -Path "$qa\user-cleanuser.log" -Pattern 'USER_QA_RESULT=' -Quiet -ErrorAction SilentlyContinue)) {
        $how = 'scheduled task'
        $act = New-ScheduledTaskAction -Execute $pwsh -Argument "-NoProfile -File `"$qa\qa-user.ps1`" `"$qa\params.json`" cleanuser" -WorkingDirectory $qa
        Register-ScheduledTask -TaskName tpi42qa -Action $act -User "$env:COMPUTERNAME\$user" -Password $pw -RunLevel Limited -Force | Out-Null
        Start-ScheduledTask -TaskName tpi42qa
        $deadline = (Get-Date).AddMinutes(8)
        while ((Get-Date) -lt $deadline -and (Get-ScheduledTask -TaskName tpi42qa).State -eq 'Running') { Start-Sleep -Seconds 2 }
        "scheduled task last result: " + (Get-ScheduledTaskInfo -TaskName tpi42qa).LastTaskResult
        Unregister-ScheduledTask -TaskName tpi42qa -Confirm:$false
    }
    "clean-user launch: $how"
    Get-Content "$qa\user-cleanuser.log" -ErrorAction SilentlyContinue
    Get-Content "$qa\cu.err" -ErrorAction SilentlyContinue
    if (-not (Select-String -Path "$qa\user-cleanuser.log" -Pattern 'USER_QA_RESULT=pass' -Quiet -ErrorAction SilentlyContinue)) { $global:LASTEXITCODE = 1 }
} | Out-Null

if (-not (Select-String -Path "$qa\user-cleanuser.log" -Pattern 'USER_QA_RESULT=' -Quiet -ErrorAction SilentlyContinue)) {
    Step 'qa-install-fallback' { pwsh -NoProfile -File (Join-Path $h 'qa-user.ps1') "$qa\params.json" fallback } | Out-Null
}

Step 'qa-account' {
    $p = Get-Content "$qa\params.json" -Raw | ConvertFrom-Json
    "machines on the account:"
    Invoke-RestMethod -Headers @{ authorization = "Bearer $($p.token)" } "$($p.origin)/api/account/v1/machines" | ConvertTo-Json -Depth 6 -Compress
    "relay.err tail:"; Get-Content "$qa\relay.err" -Tail 20
    "ghstub.err tail (request log):"; Get-Content "$qa\ghstub.err" -Tail 10
} | Out-Null

Step 'qa-cleanup' {
    foreach ($f in 'relay.pid', 'ghstub.pid') { $id = Get-Content "$qa\$f" -ErrorAction SilentlyContinue; if ($id) { Stop-Process -Id $id -Force -ErrorAction SilentlyContinue; "stopped $f $id" } }
    Copy-Item "$qa\hosts.bak" C:\Windows\System32\drivers\etc\hosts -Force; "hosts restored"
    Get-ChildItem Cert:\LocalMachine\Root | Where-Object Subject -eq 'CN=github.com' | Remove-Item; "test cert removed"
    net user tpi42qa /delete | Out-Null; "user removed"
    Get-Process ferryx-cli -ErrorAction SilentlyContinue | ForEach-Object { "left-over ferryx-cli pid $($_.Id) $($_.Path)"; Stop-Process -Id $_.Id -Force }
} | Out-Null
Copy-Item "$qa\*.log", "$qa\*.json", "$qa\*.err", "$qa\*.out" $ci -ErrorAction SilentlyContinue
exit 0
