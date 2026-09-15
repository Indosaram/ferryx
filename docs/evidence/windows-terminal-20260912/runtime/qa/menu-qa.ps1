param([string]$Label = '')
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$qa = Get-Process -Id 21620
if ($qa.Path -ne 'C:\Users\sook\ferryx-qa-rt-st01a0958a\orca-lite\src-tauri\target\debug\ferryx.exe') {
  throw 'QA process identity changed'
}
$condition = New-Object System.Windows.Automation.PropertyCondition(
  [System.Windows.Automation.AutomationElement]::ProcessIdProperty, [int]$qa.Id)
$elements = [System.Windows.Automation.AutomationElement]::RootElement.FindAll(
  [System.Windows.Automation.TreeScope]::Descendants, $condition)
$rows = @()
foreach ($element in $elements) {
  $current = $element.Current
  if ($current.ControlType -eq [System.Windows.Automation.ControlType]::MenuItem) {
    $rows += [pscustomobject]@{ Name=$current.Name; Rect=$current.BoundingRectangle.ToString(); Offscreen=$current.IsOffscreen }
    if ($Label -and $current.Name -eq $Label -and -not $current.IsOffscreen) {
      $pattern = $null
      if ($element.TryGetCurrentPattern([System.Windows.Automation.LegacyIAccessiblePattern]::Pattern, [ref]$pattern)) {
        $pattern.DoDefaultAction()
      } elseif ($element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
        $pattern.Invoke()
      } else {
        throw "Menu item has no invoke/expand pattern: $Label"
      }
    }
  }
}
$rows | ConvertTo-Json -Depth 3 | Set-Content 'C:\Users\sook\ferryx-qa-rt-st01a0958a\evidence\menu-uia.json'
