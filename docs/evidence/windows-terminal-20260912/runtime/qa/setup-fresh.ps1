$ErrorActionPreference = 'Stop'
$root = 'C:\Users\sook\ferryx-qa-fresh-0912'
$repo = "$root\orca-lite"
if (Test-Path $root) { throw 'Fresh QA root already exists; inspect before reuse' }
New-Item -ItemType Directory "$root\logs","$root\evidence","$root\runtime","$root\session","$root\appdata","$root\webview2" | Out-Null
git clone C:\Users\sook\ferryx-fresh-0912.bundle $repo
if ($LASTEXITCODE -ne 0) { throw 'Clone failed' }
git -C $repo checkout 7f7ecd8e
if ($LASTEXITCODE -ne 0) { throw 'Pinned checkout failed' }
$ghostty = 'C:\Users\sook\ferryx-winbuild\orca-lite\src-tauri\vendor\ghostty'
$sha = git -C $ghostty rev-parse HEAD
if ($sha -ne '6a508fd5e34c7e222c052a6d00bb3891ff3feace') { throw "Ghostty mismatch: $sha" }
if (Test-Path "$repo\src-tauri\vendor\ghostty") {
  Remove-Item -LiteralPath "$repo\src-tauri\vendor\ghostty"
}
cmd /c mklink /J "$repo\src-tauri\vendor\ghostty" $ghostty
if ($LASTEXITCODE -ne 0) { throw 'Junction failed' }
Set-Location $repo
bun install --frozen-lockfile
if ($LASTEXITCODE -ne 0) { throw 'Root install failed' }
bun install --cwd ui --frozen-lockfile
if ($LASTEXITCODE -ne 0) { throw 'UI install failed' }
$principal = New-ScheduledTaskPrincipal -UserId 'desktop-1lapjmp\sook' -LogonType Interactive
$action = New-ScheduledTaskAction -Execute 'C:\Windows\System32\cmd.exe' -Argument '/c C:\Users\sook\launch-fresh.cmd'
$settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Hours 4)
Register-ScheduledTask -TaskName FerryxFresh0912 -Action $action -Principal $principal -Settings $settings | Out-Null
Start-ScheduledTask -TaskName FerryxFresh0912
Write-Output 'FRESH_DEV_STARTED'
