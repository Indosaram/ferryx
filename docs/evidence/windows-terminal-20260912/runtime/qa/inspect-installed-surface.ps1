$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$root=[System.Windows.Automation.AutomationElement]::RootElement
$condition=New-Object System.Windows.Automation.PropertyCondition(
 [System.Windows.Automation.AutomationElement]::ProcessIdProperty,19396)
$elements=$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition)
$rows=@($elements | ForEach-Object {
 $c=$_.Current
 if(-not $c.IsOffscreen -and $c.Name) {
  $r=$c.BoundingRectangle
  [pscustomobject]@{name=$c.Name;type=$c.ControlType.ProgrammaticName;rect=@($r.X,$r.Y,$r.Width,$r.Height)}
 }
})
$rows | ConvertTo-Json -Depth 3 | Set-Content 'C:\Users\sook\ferryx-qa-fresh-0912\evidence\installed-surface.json'
