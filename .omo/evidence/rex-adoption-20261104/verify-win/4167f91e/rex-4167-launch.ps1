$ErrorActionPreference='Stop';$l='C:\Users\sook\ferryx-wt\.evidence\rex-verify-win-4167';$r='C:\Users\sook\ferryx-wt\rex-verify-win-20261104';$q="$l\qa"
New-Item -ItemType Directory -Force "$q\bin","$q\home","$q\data","$q\runtime" | Out-Null
$sid=[System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$names=@(Get-ChildItem \\.\pipe\ | Select-Object -ExpandProperty Name);$names | Out-File "$l\pipes-before.txt"
"DEBUG=\\.\pipe\ferryx-$sid-daemon-dev RELEASE=\\.\pipe\ferryx-$sid-daemon RUNTIME=$q\runtime DATA=$q\data" | Out-File "$l\isolation.txt"
if($names -contains "ferryx-$sid-daemon-dev"){throw 'debug pipe occupied; refusing launch'}
Copy-Item C:\Users\sook\ferryx-wt\.target\windows-dev\ferryx-cli.exe "$q\bin"
Copy-Item "$r\src-tauri\resources\ffmpeg\sdk\bin\*.dll" "$q\bin"
Get-FileHash "$q\bin\ferryx-cli.exe" | Format-List | Out-File "$l\qa-binary.txt"
$env:HOME="$q\home";$env:USERPROFILE="$q\home";$env:APPDATA="$q\home\AppData\Roaming";$env:LOCALAPPDATA="$q\home\AppData\Local";$env:FERRYX_RUNTIME_DIR="$q\runtime";$env:FERRYX_DATA_DIR="$q\data";$env:FERRYX_SESSION_DIR="$q\sessions";$env:FERRYX_ACCOUNT_RELAY_ORIGIN='http://127.0.0.1:1'
$p=Start-Process "$q\bin\ferryx-cli.exe" -ArgumentList '--daemon' -RedirectStandardOutput "$l\daemon.out" -RedirectStandardError "$l\daemon.err" -PassThru
$p.Id | Out-File "$l\daemon.pid"
"PID=$($p.Id)"
