# Inactive worktree cleanup

The user redirected this session from DAG native QA to stopping app builds
and deleting inactive worktrees. No app build was started after that request.
The owned failed debug supervisor bash_278 and watchers bash_279, bash_280,
and bash_262 were stopped. The DAG Boulder entry is paused, not completed.
The separate Herdr entry was preserved.

## Removed

All paths below were under `/Users/indo/code/project/orca-lite-wt/`:

- `dag-viewport-wave1`
- `dag-viewport-wave2`
- `herdr-a03-baseline`
- `integration-4track`

The directories are absent and `git worktree prune` removed their registrations.
Their branch refs remain intact. The detached baseline commit is still referenced
by `herdr-cloud-wave0`. No branch deletion, commit, merge or history rewrite occurred.

DAG evidence and execution records plus local ignored Cargo lockfiles were moved
before deletion into `.omo/evidence/worktree-cleanup-20260914/`, grouped by original
worktree name. The archive occupies 56,732 KiB. Original evidence links into removed
worktrees must now resolve through that archive.

## Retained

Seventeen linked worktrees remain, in addition to the main checkout:

- `orca-lite-remote`
- `dag-export-health`, `dag-viewport-integration`, `dag-viewport-wave3`
- `herdr-batch1-a06`, `herdr-batch1-a07`, `herdr-batch1-a11`
- `herdr-batch1-http-repair`, `herdr-resume-01a097f8`
- `herdr-wave0`, `herdr-wave0-clean`, `herdr-wave1`, `herdr-wave2`
- `sa-docs`, `sa-tests`, `sa-watchdog`, `sa-worktree-disk`

Every retained checkout has uncommitted or untracked work on the final successful
status inspection. `herdr-resume-01a097f8` also had live Cargo/compiler/frontend
processes and open files; this session did not launch or terminate them.
Shared node_modules and Ghostty symlink targets were preserved.

## Verification and corrected inventory

The initial survey incorrectly interpreted empty stdout from a failed status
command as clean for several Herdr checkouts. A fail-closed pre-deletion check
stopped at the first such checkout. Git reported:

`expected submodule path 'src-tauri/vendor/ghostty' not to be a symbolic link`

Successful `git status --porcelain --ignore-submodules=all` then exposed their
uncommitted code. None of those five directories was removed. Their temporarily
relocated ignored Cargo.lock files were restored without overwriting destinations.
The initial nine-candidate announcement was corrected to four actual removals.

The first removal attempt hit Bun shell's `rm` option incompatibility and deleted
nothing; explicit `/bin/rm -rf` succeeded. No force operation bypassed a dirty
worktree check.

The four removed directories measured 230,300 KiB before evidence preservation;
subtracting the retained archive gives approximately 170 MiB of allocated content
removed, not an exact APFS physical-space accounting. Filesystem availability
changed from 1.7 GiB to 8.7 GiB while a separate `mo clean` process was also running.
That entire increase must not be attributed to this cleanup.

All final retained-checkout status commands exited 0. Deleted paths were checked
absent, surviving branch hashes matched their initial values, and the final Git
worktree inventory contains 18 entries. No tests or builds were needed or run for
this filesystem cleanup. This report is uncommitted.
