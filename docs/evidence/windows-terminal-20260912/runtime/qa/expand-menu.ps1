try {
  & "$PSScriptRoot\click-qa-menu.ps1" -Label 'New Terminal Profile'
  & "$PSScriptRoot\menu-qa.ps1"
  & "$PSScriptRoot\capture-screen.ps1" -Tag 'shell-profiles'
} catch {
  $_ | Out-String | Set-Content "$PSScriptRoot\..\evidence\menu-expand-error.txt"
  exit 1
}
