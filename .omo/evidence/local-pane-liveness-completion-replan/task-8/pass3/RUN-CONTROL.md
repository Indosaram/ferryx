# Pass 3 run control (durable)

Candidate: `21dea3c01d1ec423498bc48eb2d29107be75eddf` (tree `02488435`, base `d82b35e4`).
Notepad: `/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/ulw-20261004-151839.XXXXXX.md.mrlsHnwHCA`
Evidence root: `/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation/.omo/evidence/local-pane-liveness-completion-replan/task-8/pass3`

## Live runs (do NOT duplicate)

| host | ssh | runner | current gate | logs |
| --- | --- | --- | --- | --- |
| mac CQFQ4P2LXK | I552267@100.65.239.35 | pid 75800 `runner3.mjs` | full-ui | `/Users/I552267/ferryx-pane-completion/task8-21dea3c0/{stage.log,commands.jsonl,logs/}` |
| linux indo | indo@100.91.254.71 | pids 1512984/1512991/1512992 `runner3-resume.mjs` | rust configs then full-ui | `/home/indo/ferryx-pane-completion/task8-21dea3c0/{resume.log,commands.jsonl,logs/}` |
| windows DESKTOP-1LAPJMP | sook@100.126.171.58 (key maho_win_builder_ed25519) | `runner3.mjs` | full-ui | `C:/Users/sook/ferryx-pane-completion/task8-21dea3c0/{commands.jsonl,logs/}` |

Local sessions: `bash_10` (windows runner), `bash_11` (linux resume).

## Monitors

mac stream `mon_8PXN6Y2C9X3ASS40`; mac disk watchdog `mon_2QKENQN7E460HC07`;
linux resume `mon_YCNNE5ATW2RX64S2`; windows commands `mon_3X14E0A4H3D1J7GY`;
completion: mac `mon_PV54H919Y9ZZNJEY`, linux `mon_JVAWWCY7NB56BJNQ`, windows `mon_1CBKKFTZ7ZH5BY6H`.

## To finish from cold

1. `sh /Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation/.omo/evidence/local-pane-liveness-completion-replan/task-8/pass3/fetch-evidence.sh` (archives host logs into pass3/{mac,linux,windows}/).
2. `sh /Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation/.omo/evidence/local-pane-liveness-completion-replan/task-8/pass3/cleanup-unix.sh mac`, `sh /Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation/.omo/evidence/local-pane-liveness-completion-replan/task-8/pass3/cleanup-unix.sh linux`, and the windows
   `cleanup-windows.ps1` (preserves shared Ghostty + foreign roots).
3. Assemble verdicts from `pass3/*/commands.jsonl`.

## Current verdict

UI: PASS x3 (build 0; split 19/19; lifecycle 112/112; runner 28/28). Rust: NOT_RUN_BLOCKED x3 by the single
E0505 (`blocking-e0505-repair-brief.md`). Full-UI: running. Cleanup: pending. **Task 8 NOT complete.**
