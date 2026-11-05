$ErrorActionPreference='Continue'
$root='C:\Users\sook\ferryx-wt\rex-verify-win-20261104'
Set-Location C:\Users\sook\ferryx
# All evidence already copied locally. Remove only this verifier's worktrees.
git worktree remove --force C:\Users\sook\ferryx-wt\rex-verify-win-baseline-20261104
"BASELINE_REMOVE_EXIT=$LASTEXITCODE"
git worktree remove --force $root
"CANDIDATE_REMOVE_EXIT=$LASTEXITCODE"
git worktree prune
"PRUNE_EXIT=$LASTEXITCODE"
"CANDIDATE_EXISTS=$(Test-Path $root)"
"BASELINE_EXISTS=$(Test-Path C:\Users\sook\ferryx-wt\rex-verify-win-baseline-20261104)"
Get-ChildItem C:\Users\sook\ferryx-wt\.tmp-out\rex-win-* | Remove-Item -Force
Remove-Item C:\Users\sook\ferryx-wt\.tmp-out\rex-verify-win-20261104.bundle -Force
'NO_REMOTE_CI_REFS_CREATED; shared .target retained by policy; no per-worktree target created'
git worktree list
