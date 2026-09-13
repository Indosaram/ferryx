$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$env:GIT_OPTIONAL_LOCKS = '0'
function GitRead([string]$Repo, [string[]]$GitArgs) {
  $result = & git -C $Repo @GitArgs 2>&1
  if ($LASTEXITCODE -ne 0) { throw "git $GitArgs failed at ${Repo}: $result" }
  return ($result | Out-String).Trim()
}
$roots = @('C:\Users\sook\ferryx-qa-fresh-0912', 'C:\Users\sook\ferryx-qa-rt-st01a0958a')
$checkouts = foreach ($root in $roots) {
  $repo = "$root\orca-lite"
  if (Test-Path "$repo\.git") {
    [pscustomobject]@{
      root=$root; head=(GitRead $repo @('rev-parse','HEAD'))
      status=(GitRead $repo @('status','--porcelain=v1','--untracked-files=no'))
      remotes=(GitRead $repo @('remote','-v'))
      worktrees=(GitRead $repo @('worktree','list','--porcelain'))
      isolationDirs=@('runtime','session','appdata','appdata-roaming','webview2','logs','evidence' | ForEach-Object {
        [pscustomobject]@{name=$_;exists=(Test-Path "$root\$_")}
      })
      debugBinary=if(Test-Path "$repo\target\debug\ferryx.exe") {
        Get-Item "$repo\target\debug\ferryx.exe" | Select-Object FullName,Length,LastWriteTimeUtc
      } else { $null }
    }
  } else { [pscustomobject]@{root=$root;missing=$true} }
}
$allProcesses = @(Get-CimInstance Win32_Process)
$listeners = @(Get-NetTCPConnection -State Listen -ErrorAction Stop)
$tools = foreach($name in @('git','bun','cargo','rustc','zig','node')) {
  $cmd = Get-Command $name -ErrorAction Ignore
  [pscustomobject]@{name=$name;path=if($cmd){$cmd.Source}else{$null}}
}
$sessions = qwinsta 2>&1 | Out-String
if($LASTEXITCODE -ne 0){throw "qwinsta failed: $sessions"}
[pscustomobject]@{
  utc=[DateTime]::UtcNow.ToString('o');host=$env:COMPUTERNAME
  identity=[Security.Principal.WindowsIdentity]::GetCurrent().Name
  sshSession=(Get-Process -Id $PID).SessionId;sessions=$sessions
  disk=(Get-CimInstance Win32_LogicalDisk -Filter "DeviceID='C:'" | Select-Object DeviceID,Size,FreeSpace)
  processes=@($allProcesses | Where-Object { $_.Name -eq 'ferryx.exe' -or $_.CommandLine -match 'ferryx-qa-' } | Select-Object ProcessId,ParentProcessId,SessionId,ExecutablePath,CreationDate,CommandLine)
  ports=@($listeners | Where-Object {$_.LocalPort -in @(5173,1420,9223,9224,53986)} | Select-Object LocalAddress,LocalPort,OwningProcess)
  tasks=@(Get-ScheduledTask | Where-Object {$_.TaskName -match 'Ferryx.*(QA|Fresh|0912)'} | Select-Object TaskName,State,@{n='UserId';e={$_.Principal.UserId}},@{n='LogonType';e={$_.Principal.LogonType.ToString()}},Actions)
  checkouts=@($checkouts);tools=@($tools)
} | ConvertTo-Json -Depth 8
