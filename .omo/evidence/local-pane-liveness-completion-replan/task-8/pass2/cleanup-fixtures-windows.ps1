$ErrorActionPreference = 'Stop'
$names=@('pane-liveness-test-ceW8rt','pane-liveness-test-FApyRi','pane-liveness-test-LWD279','pane-liveness-test-XnLqAs')
foreach($name in $names){$p=Join-Path $env:TEMP $name; Remove-Item -LiteralPath $p -Recurse -Force; if(Test-Path $p){throw 'Fixture remains'}}
Write-Output 'FIXTURE_CLEANUP_OK ownedRoots=4 foreignRootsPreserved=2'
