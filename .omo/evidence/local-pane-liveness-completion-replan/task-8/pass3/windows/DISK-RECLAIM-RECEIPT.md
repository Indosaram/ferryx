# windows host disk reclaim receipt (intervention)

Host: DESKTOP-1LAPJMP, sook@100.126.171.58 (key maho_win_builder_ed25519)
Trigger: `C:` free space 17.6 GB while `source-21dea3c0` alone holds 57.95 GB; the pass-3
Windows full-lib log had been truncated at 169 KB by the volume loss and must be re-run.

## What was reclaimed (this effort's own superseded staging only)

```
Remove-Item -Recurse -Force C:\Users\sook\ferryx-pane-completion\source   (24.21 GB)
```

Justification: `source` lives under `ferryx-pane-completion\`, the pane-liveness staging root this
dispatch owns, and it is superseded — it has no candidate-added
`src-tauri\src\daemon\split_journal.rs` (verified `Test-Path` = False), so it predates the
composed candidate. `source-21dea3c0` is the active pass-3 tree and was kept.

## Deliberately NOT touched

The eight long-lived `cargo.exe` processes (pids 4716, 10060, 16168, 16196, 18524, 19780, 20164,
27036, created 2026-10-03 17:49 through 2026-10-04 10:46) whose parents are all `rustup.exe` and
which are **not** descendants of this dispatch's runner. They hold no CPU (each < 1 s cumulative)
and their commands target a different workspace's `Cargo.toml`, so they are another session's
processes. They were left running untouched, per the boundary against killing foreign work.
