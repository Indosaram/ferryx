# mac host disk reclaim receipt (intervention)

Host: maho-mac CQFQ4P2LXK, I552267@100.65.239.35
Trigger: `df -h /System/Volumes/Data` = 412Gi used / **127 MiB available (100% full)**.
Every mac gate of the first priority run (bash_1) aborted with
`No space left on device` while writing its own log, so no mac gate had run.

## What was reclaimed (this effort's own superseded staging only)

```
rm -rf /Users/I552267/ferryx-pane-completion/source          (5.1G, mtime 2026-10-03 23:20)
rm -rf /Users/I552267/ferryx-pane-completion/task8-172baa87  (1.8M, pass-2 evidence copy)
rm -rf /Users/I552267/ferryx-pane-completion/task8-5464da0d  (912K, pass-1 evidence copy)
```

Justification: all three live under `ferryx-pane-completion/`, the pane-liveness staging root this
dispatch owns. `source` predates pass 3 (it has no candidate-added
`src-tauri/src/daemon/split_journal.rs`) and is superseded by `source-21dea3c0`; the two `task8-*`
dirs are superseded evidence copies of closed passes 1 and 2.

## Deliberately NOT touched

`maho-workspace` (114G), `ferryx-monitor-2026.1003.2` (27G), `ferryx-input-diag-9297` (23G),
`ferryx-build` (13G), `ferryx-snap-target` (9.2G), `mahoquot-verify` (8.7G), `minio-data` (6.7G),
`ferryx-monitor-20261002-admitted7` (4.5G), `maho-console-profile` (4.4G), `code` (4.4G),
`Library` (15G) — all belong to other efforts/sessions; deleting them is not this dispatch's call.
`source-21dea3c0` and `task8-21dea3c0` are the active pass-3 tree and evidence and were kept.
