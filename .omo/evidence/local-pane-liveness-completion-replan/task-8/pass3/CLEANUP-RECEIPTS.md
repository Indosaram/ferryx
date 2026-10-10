# Cleanup receipts — pass 4 (resumed verifier)

Every resource this session spawned, with its teardown receipt. The pass-3 candidate staging
(`source-21dea3c0` / `task8-21dea3c0`) and the local `pass3/` evidence root are **preserved** — they
hold the evidence this dispatch exists to produce.

## Remote base-A/B trees — TORN DOWN

| Host | Command | Receipt |
| --- | --- | --- |
| mac | `sh /Users/I552267/ferryx-pane-completion/cleanup-pass4-unix.sh mac` | `CLEANUP_OK mac sourceBaseAbsent=true task8BaseAbsent=true freeBefore=12Gi freeAfter=18Gi` |
| linux | `sh /home/indo/ferryx-pane-completion/cleanup-pass4-unix.sh linux` | `CLEANUP_OK linux sourceBaseAbsent=true task8BaseAbsent=true freeBefore=450G freeAfter=457G` |

Removed on both hosts: `source-base` (the reconstructed base d82b35e4 tree), `task8-base`
(base A/B evidence, all of it already archived here), `base-overlay.tar.gz`, `delta-abd9e890.tar.gz`,
and on linux additionally `task8-21dea3c0-base`. Verified absent after removal by the script itself
(`sourceBaseAbsent=true task8BaseAbsent=true`), which is the receipt.

## Remote disk reclaims — DONE (interventions, receipts on file)

| Host | Command | Receipt |
| --- | --- | --- |
| mac | `rm -rf .../ferryx-pane-completion/{source,task8-172baa87,task8-5464da0d}` | `mac/DISK-RECLAIM-RECEIPT.md` — df 127 MiB → 5.2 GiB free |
| mac | `rm -rf .../source-21dea3c0/target` (regenerable build cache) | recorded in `mac/DISK-RECLAIM-RECEIPT.md` — → 18 GiB free |
| windows | `Remove-Item -Recurse -Force ...\ferryx-pane-completion\source` | `windows/DISK-RECLAIM-RECEIPT.md` — 17.6 → 40.4 GB free |

## Local temp files — TORN DOWN

```
rm -f /tmp/ulw.delta.abd9e890.tar.gz /tmp/ulw.base-overlay.tar.gz
rm -f /tmp/ulw.base-modified.txt /tmp/ulw.base-added.txt
rm -f /tmp/ulw.mac-lib-fails.txt /tmp/ulw.linux-lib-fails.txt /tmp/ulw.win-lib-fails.txt
rm -f /tmp/rgateA-abd9e890.sh /tmp/rgateA-abd9e890.ps1 /tmp/rgateB-base.sh /tmp/rgateB2-base.sh
rm -f /tmp/rgateB2-base-mac.sh /tmp/rgateB3-base-mac.sh /tmp/rgateB4-base-mac.sh
rm -f /tmp/rgateC-targets.sh /tmp/rgateD-gates.sh /tmp/rgateAB-classify.sh /tmp/rgateAB2.sh
rm -f /tmp/classify-lib-failures.mjs /tmp/win-*.ps1 /tmp/mac-only.txt /tmp/mac-linux-shared.txt
```

## Remote base-A/B trees — windows

| Host | Command | Receipt |
| --- | --- | --- |
| windows | `powershell -File win-cleanup-pass4.ps1`, then `cmd rd /s /q` × 5 retries over ~10 min | `task8-base absent: True`; `source-base` **partially removed** — the whole tree went except `source-base\src-tauri`, which the OS keeps returning "being used by another process" for even though **no process command line references `source-base`** (verified by enumerating every `cargo.exe`/`rustc.exe`/`ferryx_lib*` with its command line: all remaining ones belong to `task3-product-gate-01a100ad` and `ferryx-baseline-herdr-50c5ca99`, i.e. other sessions). Free space on `C:` went 40.4 → **47.3 GB** with the removal. A lingering released handle is the likely cause; the residual is reported rather than force-fixed, and no foreign process was killed to remove it.
**Final state:** `source-base` = an **empty directory skeleton** (0 bytes of files, measured) holding one un-removable `src-tauri` directory; `task8-base` = **absent**; `delta-abd9e890.tar.gz` / `base-overlay.tar.gz` / `overlay-x` = **absent**; my 47 helper `.ps1` files = **removed** (`remaining win ps1: 0`); `source-21dea3c0` / `task8-21dea3c0` (the candidate staging the evidence is bound to) = **intact**. |

## Preserved on purpose

| Resource | Why |
| --- | --- |
| `pass3/` (local evidence root, incl. `linux/logs/ab/` × 32) | the deliverable |
| `source-21dea3c0` / `task8-21dea3c0` on all three hosts | candidate staging that the evidence is bound to |
| `mac/logs/full-lib-base-HUNG*.log`, `linux/logs/full-lib-base-HUNG.log`, `windows/logs/full-lib-HUNG.log` | evidence for the hangs reported as findings |
| the eight foreign `cargo.exe` processes on maho-win | not descendants of this dispatch's runner; another session's work |
| shared Ghostty roots and foreign worktrees on every host | not this dispatch's to remove |

## Pass 5 additions (verification at `39e722ce`)

| Resource | Teardown | Receipt |
| --- | --- | --- |
| mac `source-21dea3c0/target` (purged to clear os-error-28 / disk-full) | `rm -rf` | regenerable; mac free went 161 MiB → 14 GiB, later fell to 3.3 GiB from **foreign** trees only |
| mac hung full-lib run (pid 70455 + cargo 70321) | `kill` | only this dispatch's pids; partial log preserved as `mac/logs/I-full-lib-HUNG-at-p11-reaper.log` (2285 lines) |
| mac `rgateH` run | `pkill -f rgateH-gates.sh` | partial log preserved as `mac/logs/rustH-gates.PARTIAL-DISK.log`; superseded by the lib-only run |
| linux `rgateG` run (stale, mixed bytes) | `kill` | log kept as `linux/rustG-gates.STALE.log` |
| linux A/B launcher (died on SIGHUP) | none needed | re-run with `setsid`; the base-side result landed |
| remote helper scripts on all three hosts | retained under `task8-21dea3c0/` | they are the reproduction commands for Task 9 |

**Not touched** (foreign, other sessions'): `maho-workspace` 114G,
`ferryx-monitor-2026.1003.2` 27G, `ferryx-input-diag-9297` 23G on mac; the eight foreign
`cargo.exe` processes on maho-win.
