# Windows read-only isolation preflight

Observed 2026-09-13T06:40:33.1876915Z on `DESKTOP-1LAPJMP`.
This is resource/preservation evidence, not GUI or PTY acceptance.
Local HEAD at observation: `da6eec06d65551f67bbc43f09910cde470c3478d`.
Production fixes remain uncommitted; no Windows source synchronization,
checkout creation, scheduled task, desktop input, build or launch occurred.

## Executed probe

`ssh -o BatchMode=yes -o ConnectTimeout=15 maho-win powershell.exe -NoProfile -EncodedCommand <UTF-16LE base64 of script below>`

The placeholder documents encoding, not a second executed command. Exact
PowerShell input:

```powershell
$ErrorActionPreference='Stop'
$p=Get-CimInstance Win32_Process -Filter "Name='ferryx.exe'" |
  Select-Object ProcessId,ParentProcessId,SessionId,ExecutablePath,CreationDate,CommandLine
$disk=Get-CimInstance Win32_LogicalDisk -Filter "DeviceID='C:'" |
  Select-Object DeviceID,Size,FreeSpace
$ports=Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue |
  Where-Object { $_.LocalPort -in @(5173,1420) } |
  Select-Object LocalAddress,LocalPort,OwningProcess
[pscustomobject]@{
  utc=[DateTime]::UtcNow.ToString('o')
  host=$env:COMPUTERNAME
  processes=@($p)
  disk=$disk
  devPorts=@($ports)
  sessions=(qwinsta | Out-String)
} | ConvertTo-Json -Depth 5 -Compress
```

`mon_7C1QYV7KEW9ZCFP7` / `bash_47` completed with exit 0.
The complete JSON arrived in the monitor completion event. The subsequent
raw-output display abbreviated its long JSON/XML lines; no additional
facts are inferred from that abbreviated display. PowerShell emitted
first-use module preparation CLIXML, not a product error.

## Observed preservation identities

All three executables were
`C:\Users\sook\AppData\Local\Ferryx\ferryx.exe`, in session 1:

- GUI PID 17288, parent 2860, creation Unix milliseconds 1789211998895
  (`2026-09-12T11:19:58.895Z`).
- Daemon PID 1756, parent 17288, creation 1789212001837
  (`2026-09-12T11:20:01.837Z`), command line ends `--daemon`.
- Daemon PID 20196, parent 17288, creation 1789212002037
  (`2026-09-12T11:20:02.037Z`), command line ends `--daemon`.

These match lead-baseline.md to CIM's millisecond precision. Two daemon
processes exist; this probe does not establish which endpoint each owns
or authorize terminating either one.

C: size 999,127,248,896 bytes; free 182,029,660,160 bytes.
`qwinsta` reports services session 0 disconnected and console session 1
active for sook. The port query returned `[]` for 5173 and 1420. Because its
errors were silenced to handle an empty listener set, this is only the
observed query result, not a fail-closed exclusive port reservation.
Runtime setup must verify actual port ownership again before launch.

## Remaining boundary

The previous `windows-terminal-20260912/runtime/FRESH-RUN.md` was read as a
historical setup/cleanup recipe. Its `7f7ecd8e` screenshots, PIDs, task names
and CDP port are not current QA evidence and must not be reused blindly.
The current task still needs explicit branch/worktree creation approval.
No user daemon or installed application was changed by this preflight.
The SSH monitor exited; no QA runtime resource was acquired.
