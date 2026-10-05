$ErrorActionPreference = 'Stop'
$root='C:\Users\sook\ferryx-pane-completion\base-pass2-d82b35e4'
if(Test-Path $root){throw 'Base owned source already exists'}
New-Item -ItemType Directory $root | Out-Null
tar.exe -xf C:/Users/sook/ferryx-pane-completion/base-pass2.tar -C $root
$native=$LASTEXITCODE
if($native -ne 0){exit $native}
Set-Location $root
bun install --cwd ui --frozen-lockfile
$native=$LASTEXITCODE
exit $native
