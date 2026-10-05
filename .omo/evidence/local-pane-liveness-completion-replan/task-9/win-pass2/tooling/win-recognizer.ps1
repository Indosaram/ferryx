param(
  [Parameter(Mandatory=$true)][string]$BarrierDir,
  [int]$TimeoutSec = 420,
  [switch]$SelfTest
)
$ErrorActionPreference = 'Continue'
# The runner creates <isolationRoot>/barriers itself; wait for it, then log.
$bdDeadline = (Get-Date).AddSeconds($TimeoutSec)
while (-not (Test-Path $BarrierDir)) {
  if ((Get-Date) -gt $bdDeadline) { Write-Output 'RECOG_TIMEOUT_NO_BARRIER_DIR'; exit 2 }
  Start-Sleep -Milliseconds 50
}
$logPath = Join-Path $BarrierDir 'recognizer.log'
function Log($m) { $line = "$((Get-Date).ToString('o')) $m"; Write-Output $line; Add-Content -Path $logPath -Value $line -ErrorAction SilentlyContinue }

Add-Type -AssemblyName System.Runtime.WindowsRuntime
$null = [Windows.Media.Ocr.OcrEngine, Windows.Foundation, ContentType=WindowsRuntime]
$null = [Windows.Storage.StorageFile, Windows.Storage, ContentType=WindowsRuntime]
$null = [Windows.Storage.FileAccessMode, Windows.Storage, ContentType=WindowsRuntime]
$null = [Windows.Graphics.Imaging.BitmapDecoder, Windows.Graphics.Imaging, ContentType=WindowsRuntime]
$null = [Windows.Globalization.Language, Windows.Globalization, ContentType=WindowsRuntime]

$asTaskGeneric = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' })[0]
function Await($op, $resultType) {
  $t = $asTaskGeneric.MakeGenericMethod($resultType).Invoke($null, @($op))
  $t.Wait(-1) | Out-Null
  return $t.Result
}
# Declared normalization: OCR renders the '_' separator as whitespace. Fixed rule:
# uppercase, collapse runs of [_ whitespace] to one space, trim.
function Norm([string]$s) {
  if ($null -eq $s) { return '' }
  $u = $s.ToUpperInvariant()
  $u = [regex]::Replace($u, '[_\s]+', ' ')
  return $u.Trim()
}
$MARKER = 'FERRYX_SPLIT_READY'
$MARKER_NORM = 'FERRYX SPLIT READY'

$engine = $null
try { $engine = [Windows.Media.Ocr.OcrEngine]::TryCreateFromLanguage([Windows.Globalization.Language]::new('ko')) } catch { Log "ENGINE_KO_FAIL $($_.Exception.Message)" }
if (-not $engine) { try { $engine = [Windows.Media.Ocr.OcrEngine]::TryCreateFromUserProfileLanguages() } catch { Log "ENGINE_USER_FAIL $($_.Exception.Message)" } }
if (-not $engine) { Log 'NO_OCR_ENGINE'; exit 4 }
Log "OCR_ENGINE=$($engine.RecognizerLanguage.LanguageTag)"

function OcrFile([string]$path) {
  $file = Await ([Windows.Storage.StorageFile]::GetFileFromPathAsync($path)) ([Windows.Storage.StorageFile])
  $stream = Await ($file.OpenAsync([Windows.Storage.FileAccessMode]::Read)) ([Windows.Storage.Streams.IRandomAccessStream])
  $decoder = Await ([Windows.Graphics.Imaging.BitmapDecoder]::CreateAsync($stream)) ([Windows.Graphics.Imaging.BitmapDecoder])
  $bmp = Await ($decoder.GetSoftwareBitmapAsync([Windows.Graphics.Imaging.BitmapPixelFormat]::Bgra8, [Windows.Graphics.Imaging.BitmapAlphaMode]::Premultiplied)) ([Windows.Graphics.Imaging.SoftwareBitmap])
  $res = Await ($engine.RecognizeAsync($bmp)) ([Windows.Media.Ocr.OcrResult])
  $stream.Dispose()
  return $res
}

if ($SelfTest) {
  Add-Type -AssemblyName System.Drawing
  $bmp = New-Object System.Drawing.Bitmap(900, 300)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.Clear([System.Drawing.Color]::Black)
  $font = New-Object System.Drawing.Font('Consolas', 28)
  $g.DrawString($MARKER, $font, [System.Drawing.Brushes]::White, 20, 120)
  $g.Dispose()
  $tmp = Join-Path $env:TEMP 'recognizer-selftest.png'
  $bmp.Save($tmp, [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
  $r = OcrFile $tmp
  Log "SELFTEST_RAW=[$($r.Text)]"
  Log "SELFTEST_NORM=[$(Norm $r.Text)]"
  Log "SELFTEST_MATCH=$((Norm $r.Text).Contains($MARKER_NORM))"
  Log 'SELFTEST_DONE'
  exit 0
}

$cr = Join-Path $BarrierDir 'capture-ready.json'
$deadline = (Get-Date).AddSeconds($TimeoutSec)
while (-not (Test-Path $cr)) {
  if ((Get-Date) -gt $deadline) { Log 'RECOG_TIMEOUT_NO_CAPTURE_READY'; exit 3 }
  Start-Sleep -Milliseconds 40
}
Log 'CAPTURE_READY_SEEN'
try { $ready = Get-Content $cr -Raw | ConvertFrom-Json } catch { Log "CAPTURE_READY_UNPARSEABLE $($_.Exception.Message)"; exit 5 }
$shot = $ready.screenshotPath
$tp = $ready.targetPaneBounds
$wb = $ready.windowBounds
Log "SCREENSHOT=$shot SHA=$($ready.screenshotSha256) WINDOW=$($wb.x),$($wb.y),$($wb.width),$($wb.height) TARGETPANE=$($tp.x),$($tp.y),$($tp.width),$($tp.height)"
if (-not (Test-Path $shot)) { Log 'SCREENSHOT_MISSING'; exit 6 }
$ocr = OcrFile $shot
$norm = Norm $ocr.Text
Log "OCR_RAW=[$($ocr.Text)]"
Log "OCR_NORM=[$norm]"
if (-not $norm.Contains($MARKER_NORM)) { Log 'MARKER_NOT_RECOGNIZED'; exit 7 }
$tokens = @('FERRYX', 'SPLIT', 'READY')
$mBox = $null
foreach ($line in $ocr.Lines) {
  if (-not (Norm $line.Text).Contains($MARKER_NORM)) { continue }
  foreach ($word in $line.Words) {
    $wn = Norm $word.Text
    if ($tokens -notcontains $wn) { continue }
    $r = $word.BoundingRect
    $b = @{ x = [int]($r.X + $wb.x); y = [int]($r.Y + $wb.y); w = [int]$r.Width; h = [int]$r.Height }
    if ($mBox -eq $null) { $mBox = $b } else {
      $x1 = [Math]::Min($mBox.x, $b.x); $y1 = [Math]::Min($mBox.y, $b.y)
      $x2 = [Math]::Max($mBox.x + $mBox.w, $b.x + $b.w); $y2 = [Math]::Max($mBox.y + $mBox.h, $b.y + $b.h)
      $mBox = @{ x = $x1; y = $y1; w = $x2 - $x1; h = $y2 - $y1 }
    }
  }
}
if ($mBox -eq $null) { Log 'MARKER_BBOX_UNRESOLVED'; exit 8 }
Log "MARKER_BBOX_SCREEN=$($mBox.x),$($mBox.y),$($mBox.w),$($mBox.h)"
$cx = $mBox.x + ($mBox.w / 2.0); $cy = $mBox.y + ($mBox.h / 2.0)
$inside = ($cx -ge $tp.x) -and ($cx -le ($tp.x + $tp.width)) -and ($cy -ge $tp.y) -and ($cy -le ($tp.y + $tp.height))
Log "MARKER_CENTER=$cx,$cy INSIDE_TARGET_PANE=$inside"
if (-not $inside) { Log 'MARKER_OUTSIDE_TARGET_PANE'; exit 9 }
$artifact = [ordered]@{
  runId = $ready.runId
  operationId = $ready.operationId
  recognizer = 'ferryx-verifier-ocr/Windows.Media.Ocr+ko'
  text = $MARKER
  paneBounds = @{ x = $tp.x; y = $tp.y; w = $tp.width; h = $tp.height }
  screenshotSha256 = $ready.screenshotSha256
}
$artifact | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $BarrierDir 'marker-recognition.json') -Encoding ascii
$prov = [ordered]@{
  recognizer = 'ferryx-verifier-ocr/Windows.Media.Ocr+ko'
  engineLanguage = $engine.RecognizerLanguage.LanguageTag
  rawOcrText = $ocr.Text
  normalizedOcrText = $norm
  normalizationRule = 'uppercase; runs of [_ whitespace] collapsed to one space; trim'
  markerBBoxScreen = $mBox
  windowBounds = $wb
  targetPaneBounds = $tp
  markerCenterInsideTargetPane = $inside
  screenshotSha256 = $ready.screenshotSha256
  note = 'text is the declared marker literal; OCR raw text differs only in the _ separator. paneBounds is the runner-claimed target pane rectangle and the marker bbox center was REQUIRED to fall inside it (exit 9 otherwise); the pane rectangle is not independently detected.'
}
$prov | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $BarrierDir 'ocr-provenance.json') -Encoding ascii
Log 'RECOGNITION_ARTIFACT_WRITTEN'
