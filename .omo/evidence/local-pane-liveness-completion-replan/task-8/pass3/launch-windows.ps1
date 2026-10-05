$ErrorActionPreference='Stop'
$out='C:\Users\sook\ferryx-pane-completion\task8-21dea3c0\stage.log'
$err='C:\Users\sook\ferryx-pane-completion\task8-21dea3c0\stage.err.log'
$p = Start-Process -FilePath 'powershell.exe' -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File','C:/Users/sook/ferryx-pane-completion/stage-windows.ps1' -RedirectStandardOutput $out -RedirectStandardError $err -NoNewWindow -PassThru
Write-Output "STARTED_WIN $($p.Id)"
