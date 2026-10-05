$ErrorActionPreference = 'Stop'
Get-FileHash -Algorithm SHA256 -LiteralPath 'C:\Users\sook\ferryx-pane-completion\base-pass2.tar' | Format-List
