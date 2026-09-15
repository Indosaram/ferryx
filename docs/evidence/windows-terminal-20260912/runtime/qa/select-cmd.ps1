try {
  & "$PSScriptRoot\click-qa-menu.ps1" -Label 'New Terminal Profile'
  & "$PSScriptRoot\menu-qa.ps1"
  & "$PSScriptRoot\click-qa-menu.ps1" -Label 'Command Prompt'
} catch {
  $_ | Out-String | Set-Content "$PSScriptRoot\..\evidence\menu-select-error.txt"
  exit 1
}
