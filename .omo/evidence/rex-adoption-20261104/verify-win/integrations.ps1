$ErrorActionPreference='Continue'
$root='C:\Users\sook\ferryx-wt\rex-verify-win-20261104'
$logs="$root\.omo\evidence\rex-adoption-20261104\verify-win"
$qa="$root\.qa\integrations"
New-Item -ItemType Directory -Force "$qa\home\.claude","$qa\home\.codex","$qa\home\.omp","$qa\data" | Out-Null
$env:USERPROFILE="$qa\home"
$env:HOME="$qa\home"
$env:APPDATA="$qa\home\AppData\Roaming"
$env:FERRYX_DATA_DIR="$qa\data"
$env:FERRYX_RUNTIME_DIR="$qa\runtime"
$cli='C:\Users\sook\ferryx-wt\.target\debug\ferryx-cli.exe'
Get-ChildItem Env: | Where-Object Name -match 'USERPROFILE|^HOME$|APPDATA|FERRYX' | Format-Table | Out-File "$logs\integrations-env.log"
& $cli integrations skill install --all *> "$logs\integrations-install.log"
"EXIT=$LASTEXITCODE" | Add-Content "$logs\integrations-install.log"
& $cli integrations status --json *> "$logs\integrations-status.log"
"EXIT=$LASTEXITCODE" | Add-Content "$logs\integrations-status.log"
$before=@{};Get-ChildItem "$qa\home" -Recurse -File | ForEach-Object { $before[$_.FullName]=$_.LastWriteTimeUtc.Ticks }
& $cli integrations skill install --all *> "$logs\integrations-idempotent.log"
"EXIT=$LASTEXITCODE" | Add-Content "$logs\integrations-idempotent.log"
$changes=@(Get-ChildItem "$qa\home" -Recurse -File | Where-Object { $before[$_.FullName] -ne $_.LastWriteTimeUtc.Ticks })
"CHANGED_FILES=$($changes.Count)" | Add-Content "$logs\integrations-idempotent.log"
$skill="$qa\home\.claude\skills\ferryx\SKILL.md"
(Get-Content $skill | Where-Object {$_ -notmatch 'ferryx-managed:'}) | Set-Content "$skill.tmp"
Move-Item "$skill.tmp" $skill -Force
Add-Content $skill 'owner takeover line'
& $cli integrations skill uninstall --all *> "$logs\integrations-uninstall.log"
"EXIT=$LASTEXITCODE" | Add-Content "$logs\integrations-uninstall.log"
"USER_EDIT_PRESERVED=$((Get-Content $skill -Raw).Contains('owner takeover line'))" | Add-Content "$logs\integrations-uninstall.log"
& $cli integrations skill install nosuchagent *> "$logs\integrations-unknown.log"
"EXIT=$LASTEXITCODE" | Add-Content "$logs\integrations-unknown.log"
