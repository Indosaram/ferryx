# Backend performance mass-ulw

Started: 2026-08-22
Notepad: `/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/ulw-be-20260823-001715.XXXXXX.md.Bb2GK2ccqe`
Final report: `BACKEND_PERFORMANCE_SWEEP.md`

## Topology
Run A `backend-perf-audit-20260822`: 6 parallel `quick` audits → `unspecified-low` synthesize → `quick` verify-audit.

Lanes (write one markdown each under `docs/backend-perf/`):
- audit-terminal
- audit-worktree
- audit-remote
- audit-daemon-session
- audit-browser-ipc
- audit-notification

Run B built from PRIORITIZED.md. Implementation via category routing (Gemini). No `model` + `category`.
