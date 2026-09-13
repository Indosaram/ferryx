# Windows PowerShell 5.1, same interactive user/session as the owned Ferryx app.
# Read-only WinRT history access; never clears history or submits a toast.
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$AppUserModelId,
    [Parameter(Mandatory = $true)][Guid]$RunId,
    [Parameter(Mandatory = $true)]
    [ValidateSet('targeted-system', 'targeted-silent', 'idless-system', 'idless-silent')]
    [string]$Case,
    [Parameter(Mandatory = $true)][string]$OutputDirectory,
    [Parameter(Mandatory = $true)][switch]$OwnedInteractiveAllocation
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (!$OwnedInteractiveAllocation) { throw 'Explicit owned allocation acknowledgment required' }
if ($AppUserModelId -match 'PowerShell|powershell.exe') {
    throw 'Shared PowerShell fallback identity is not an owned Ferryx identity'
}
if (!(Test-Path -LiteralPath $OutputDirectory -PathType Container)) {
    throw 'OutputDirectory must be an existing owned evidence directory'
}
$marker = "P14-$($RunId.ToString())-$Case"
$xmlPath = Join-Path $OutputDirectory "$marker.xml"
$receiptPath = Join-Path $OutputDirectory "$marker.json"
if ((Test-Path -LiteralPath $xmlPath) -or (Test-Path -LiteralPath $receiptPath)) {
    throw 'Refusing to overwrite an existing observation'
}
$null = [Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime]
$history = [Windows.UI.Notifications.ToastNotificationManager]::History
$matches = @()
foreach ($toast in $history.GetHistory($AppUserModelId)) {
    $raw = $toast.Content.GetXml()
    $doc = New-Object System.Xml.XmlDocument
    $doc.XmlResolver = $null
    $doc.LoadXml($raw)
    # Match the complete marker as text, not a substring of unrelated history.
    $texts = @($doc.SelectNodes('/toast/visual/binding/text') | ForEach-Object { $_.InnerText })
    if ($texts -contains $marker) { $matches += @{ Raw = $raw; Document = $doc } }
}
if ($matches.Count -ne 1) {
    throw "INCONCLUSIVE: expected one native history toast for $marker, found $($matches.Count). No sound verdict."
}
$observed = $matches[0]
$audio = @($observed.Document.SelectNodes('/toast/audio'))
$silent = @($audio | Where-Object { $_.GetAttribute('silent') -eq 'true' })
$loop = @($audio | Where-Object { $_.GetAttribute('loop') -eq 'true' })
$actions = @($observed.Document.SelectNodes('/toast/actions/action'))
$defaults = @($actions | Where-Object { $_.GetAttribute('arguments') -eq 'default' })
$targeted = $Case.StartsWith('targeted-')
$system = $Case.EndsWith('-system')
$soundPass = if ($system) {
    $silent.Count -eq 0 -and $audio.Count -le 1 -and $loop.Count -eq 0
} else {
    $audio.Count -eq 1 -and $silent.Count -eq 1 -and
    !$audio[0].HasAttribute('src') -and $loop.Count -eq 0
}
$actionPass = if ($targeted) { $defaults.Count -eq 1 } else { $actions.Count -eq 0 }
$receipt = [ordered]@{
    runId = $RunId.ToString(); case = $Case; appUserModelId = $AppUserModelId
    marker = $marker; historyMatches = $matches.Count
    soundPass = $soundPass; actionRegistrationPass = $actionPass
    clickRouting = 'requires separate real click/drain receipt'
    verdict = $(if ($soundPass -and $actionPass) { 'PASS' } else { 'RED' })
}
# CreateNew prevents racing writers from replacing an existing artifact.
foreach ($artifact in @(@{ Path = $xmlPath; Text = $observed.Raw },
                        @{ Path = $receiptPath; Text = ($receipt | ConvertTo-Json -Depth 5) })) {
    $stream = [System.IO.File]::Open($artifact.Path, [System.IO.FileMode]::CreateNew)
    try {
        $bytes = [System.Text.Encoding]::UTF8.GetBytes($artifact.Text)
        $stream.Write($bytes, 0, $bytes.Length)
    } finally { $stream.Dispose() }
}
$receipt | ConvertTo-Json -Depth 5
if (!$soundPass -or !$actionPass) { exit 1 }
