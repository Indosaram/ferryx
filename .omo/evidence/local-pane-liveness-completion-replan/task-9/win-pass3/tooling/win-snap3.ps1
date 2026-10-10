param([Parameter(Mandatory=$true)][string]$Source,[Parameter(Mandatory=$true)][string]$Dest,[int]$Seconds=240)
$ErrorActionPreference='Continue'
$deadline=(Get-Date).AddSeconds($Seconds)
$ticks=0
while ((Get-Date) -lt $deadline) {
  if (Test-Path $Source) { try { Copy-Item -Path (Join-Path $Source '*') -Destination $Dest -Recurse -Force -ErrorAction SilentlyContinue } catch { } }
  $ticks++
  Start-Sleep -Milliseconds 150
}
Write-Output ("SNAP3_DONE ticks=" + $ticks)
