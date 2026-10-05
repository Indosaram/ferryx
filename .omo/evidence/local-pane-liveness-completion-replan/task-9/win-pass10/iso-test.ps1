$ErrorActionPreference='Continue'
$base='C:\Users\sook\ferryx-pane-completion'
$rt=Join-Path $base 'pf-iso\runtime'
$node='C:\Program Files\nodejs\node.exe'
$dmon=Join-Path $base 'source-21dea3c0\scripts\qa\pf-daemon.mjs'
$invOut=Join-Path $base 'iso-test-inv.json'
Remove-Item -Force $invOut -ErrorAction SilentlyContinue
Write-Output ("PORT_FILE=" + (Test-Path (Join-Path $rt 'daemon.port')))
if (Test-Path (Join-Path $rt 'daemon.port')) { $port=[int]((Get-Content (Join-Path $rt 'daemon.port') -Raw).Trim()); Write-Output ("PORT=" + $port); $c=Get-NetTCPConnection -LocalPort $port -State Listen -ErrorAction SilentlyContinue; Write-Output ("LISTEN=" + [bool]$c) }
Write-Output "=== node daemon query, file output, 20s cap ==="
$sw=[System.Diagnostics.Stopwatch]::StartNew()
$p = Start-Process -FilePath $node -ArgumentList @($dmon, $rt, 'list', $invOut) -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $base 'iso-test.out') -RedirectStandardError (Join-Path $base 'iso-test.err')
$ok = $p.WaitForExit(20000)
Write-Output ("EXITED=" + $ok + " MS=" + $sw.ElapsedMilliseconds)
if (-not $ok) { taskkill /T /F /PID $p.Id 2>&1 | Out-Null }
if (Test-Path $invOut) { Write-Output "--- inventory ---"; Get-Content $invOut -Raw } else { Write-Output "NO_INVENTORY_FILE"; Get-Content (Join-Path $base 'iso-test.err') -Raw -ErrorAction SilentlyContinue }
Write-Output "=== PowerShell TcpClient path (the one that hangs) ==="
$sw2=[System.Diagnostics.Stopwatch]::StartNew()
try {
  $port=[int]((Get-Content (Join-Path $rt 'daemon.port') -Raw).Trim())
  $token=(Get-Content (Join-Path $rt 'daemon.token') -Raw).Trim()
  $c=New-Object System.Net.Sockets.TcpClient
  $c.Connect('127.0.0.1',$port)
  $s=$c.GetStream()
  $w=New-Object System.IO.StreamWriter($s); $w.NewLine="`n"; $w.AutoFlush=$true
  $r=New-Object System.IO.StreamReader($s)
  $w.WriteLine('{"type":"handshake","version":5,"token":"' + $token + '"}')
  Write-Output ("WROTE_HANDSHAKE_MS=" + $sw2.ElapsedMilliseconds)
  $h=$r.ReadLine()
  Write-Output ("HANDSHAKE_LINE=" + $h)
  $w.WriteLine('{"type":"listSessions"}')
  $l=$r.ReadLine()
  Write-Output ("LIST_LINE=" + $l)
  $c.Close()
} catch { Write-Output ("TCP_ERR=" + $_.Exception.Message) }
Write-Output ("TCP_MS=" + $sw2.ElapsedMilliseconds)
Write-Output "ISO_TEST_DONE"
