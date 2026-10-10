$ErrorActionPreference = 'Stop'
$env:CARGO_TARGET_DIR = 'C:\Users\sook\ferryx-pane-completion\source-172baa87\target'
node C:/Users/sook/ferryx-pane-completion/task8-172baa87/runner.mjs C:/Users/sook/ferryx-pane-completion/source-172baa87 C:/Users/sook/ferryx-pane-completion/task8-172baa87 windows
$native=$LASTEXITCODE
exit $native
