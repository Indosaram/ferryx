$ErrorActionPreference='Stop'
$root='C:\Users\sook\ferryx-wt\rex-verify-win-20261104'
$logs="$root\.omo\evidence\rex-adoption-20261104\verify-win"
$qa="$root\.qa\pipe"
New-Item -ItemType Directory -Force "$qa\bin","$qa\home","$qa\data","$qa\rt" | Out-Null
Copy-Item C:\Users\sook\ferryx-wt\.target\debug\ferryx-cli.exe "$qa\bin\ferryx-cli.exe"
Copy-Item "$root\src-tauri\resources\ffmpeg\sdk\bin\*.dll" "$qa\bin"
$sid=[System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$release="\\.\pipe\ferryx-$sid-daemon"
$debug="$release-dev"
"RELEASE_PIPE=$release`nDEBUG_PIPE=$debug`nDIFFERENT=$($release -ne $debug)" | Out-File "$logs\pipe-safety.log"
$names=@(Get-ChildItem \\.\pipe\ | Select-Object -ExpandProperty Name)
$names | Out-File "$logs\pipes-before.log"
if ($names -contains "ferryx-$sid-daemon-dev") { throw 'Same-flavor daemon pipe exists: refusing launch' }
Get-CimInstance Win32_Process | Where-Object Name -match '^ferryx' | Select-Object ProcessId,ExecutablePath,CommandLine | Format-List | Out-File "$logs\processes-before.log"
$env:USERPROFILE="$qa\home";$env:HOME="$qa\home";$env:APPDATA="$qa\home\AppData\Roaming";$env:LOCALAPPDATA="$qa\home\AppData\Local"
$env:FERRYX_RUNTIME_DIR="$qa\rt";$env:FERRYX_DATA_DIR="$qa\data";$env:XDG_CONFIG_HOME="$qa\home\.config"
$env:FERRYX_ACCOUNT_RELAY_ORIGIN='http://127.0.0.1:1'
$p=Start-Process "$qa\bin\ferryx-cli.exe" -ArgumentList @('--daemon') -RedirectStandardOutput "$logs\qa-daemon.out.log" -RedirectStandardError "$logs\qa-daemon.err.log" -PassThru
$p.Id | Out-File "$logs\qa-daemon.pid"
& python C:\Users\sook\ferryx-wt\.tmp-out\rex-win-live-env.py $p.Id | Out-File "$logs\qa-daemon-live-env.json"
if($LASTEXITCODE -ne 0){throw 'Cannot inspect live environment'}
$live=Get-Content "$logs\qa-daemon-live-env.json" -Raw | ConvertFrom-Json
if($live.FERRYX_RUNTIME_DIR -ne "$qa\rt" -or $live.USERPROFILE -ne "$qa\home"){throw 'Live environment not isolated'}
$ErrorActionPreference='Continue'
& "$qa\bin\ferryx-cli.exe" agents list --json *> "$logs\pipe-agents-list.log"
"EXIT=$LASTEXITCODE" | Add-Content "$logs\pipe-agents-list.log"
Get-ChildItem \\.\pipe\ | Where-Object Name -match "ferryx-$sid" | Format-Table | Out-File "$logs\pipes-after.log"
