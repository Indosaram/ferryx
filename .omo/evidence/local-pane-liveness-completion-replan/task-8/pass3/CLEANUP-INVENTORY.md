# Cleanup inventory — resources this session spawned

Every resource below was created by this verification session and must be torn down before Task 8 is
called complete. Teardown receipts are appended as each is removed.

## Local (control Mac)

| Resource | Path | Teardown |
| --- | --- | --- |
| delta tarball | `/tmp/ulw.delta.abd9e890.tar.gz` | `rm -f` |
| base overlay tarball | `/tmp/ulw.base-overlay.tar.gz` | `rm -f` |
| changed-file lists | `/tmp/ulw.base-modified.txt`, `/tmp/ulw.base-added.txt` | `rm -f` |
| failure name sets | `/tmp/ulw.mac-lib-fails.txt`, `/tmp/ulw.linux-lib-fails.txt` | `rm -f` |
| gate/helper scripts | `/tmp/rgateA-abd9e890.{sh,ps1}`, `/tmp/rgateB-base.sh`, `/tmp/rgateB2-base.sh`, `/tmp/rgateB2-base-mac.sh`, `/tmp/rgateB3-base-mac.sh`, `/tmp/rgateB4-base-mac.sh`, `/tmp/rgateC-targets.sh`, `/tmp/rgateD-gates.sh`, `/tmp/rgateAB-classify.sh`, `/tmp/rgateAB2.sh`, `/tmp/classify-lib-failures.mjs`, `/tmp/win-*.ps1` | `rm -f` |
| temp scratch | `/tmp/mac-only.txt`, `/tmp/mac-linux-shared.txt` | `rm -f` |

Local temp files carry no state the evidence needs: every artifact that matters is archived under
`pass3/`, which is the deliverable and is NOT cleaned.

## Remote staging (created by this session)

| Host | Resource | Teardown |
| --- | --- | --- |
| mac | `/Users/I552267/ferryx-pane-completion/source-base` (base A/B tree, hardlink clone + overlay) | `rm -rf` |
| mac | `/Users/I552267/ferryx-pane-completion/task8-base` (base A/B evidence) | `rm -rf` |
| mac | `/Users/I552267/ferryx-pane-completion/delta-abd9e890.tar.gz`, `base-overlay.tar.gz` | `rm -f` |
| linux | `/home/indo/ferryx-pane-completion/source-base` | `rm -rf` |
| linux | `/home/indo/ferryx-pane-completion/task8-21dea3c0-base` | `rm -rf` |
| linux | `/home/indo/ferryx-pane-completion/base-overlay.tar.gz`, `delta-abd9e890.tar.gz` | `rm -f` |
| windows | `C:\Users\sook\ferryx-pane-completion\source-base` (junction tree) | remove junctions with `rmdir`, then `rm -rf` the src-tauri copy |
| windows | `C:\Users\sook\ferryx-pane-completion\task8-base` | `rm -rf` |
| windows | `C:\Users\sook\ferryx-pane-completion\base-overlay.tar.gz`, `delta-abd9e890.tar.gz`, `overlay-x` | `rm -f` / `rm -rf` |
| windows | `C:\Users\sook\*.ps1` (my helper scripts) | `rm -f` |

## Deliberately NOT torn down

- The pass-3 staging trees `source-21dea3c0` / `task8-21dea3c0` on all three hosts and the local
  `pass3/` evidence root: they hold the evidence this dispatch exists to produce. The prior
  verifier's own cleanup script (`pass3/cleanup-unix.sh`, `cleanup-windows.ps1`) is the intended
  teardown for the *whole* pass-3 staging, to be run once the evidence is final and archived.
- Shared Ghostty roots, foreign worktrees, the eight foreign `cargo.exe` processes on maho-win, and
  every other session's directories on any host.

## Session-owned monitors / shells (closed automatically or explicitly)

mac/linux/windows gate monitors and the mac stall watchdog are session-scoped and die with the
session; the background bash sessions are killed as each run completes.
